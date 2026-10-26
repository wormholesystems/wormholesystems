//! The daily threat rules, per wormhole system over 90 days: count kills per organisation
//! (victim and attackers, alliance over corporation, each org once per killmail), keep
//! those active on >= 5 distinct days, take the top 10. Summed kills set the level:
//! >= 50 critical, >= 15 high, else unknown.

use std::collections::HashMap;
use std::time::Duration;

use sqlx::PgPool;

use crate::entities::{self, EntityKind};
use crate::esi::EsiClient;

const ANALYSIS_WINDOW_DAYS: i32 = 90;
const MIN_ACTIVE_DAYS: i64 = 5;
const TOP_ORGS: i64 = 10;
const HOSTILE_THRESHOLD: i64 = 50;
const ACTIVE_THRESHOLD: i64 = 15;

/// The legacy thresholds: summed top-org kills decide the level.
pub fn threat_level(total_kills: i64) -> crate::maps::ThreatLevel {
    use crate::maps::ThreatLevel;
    if total_kills >= HOSTILE_THRESHOLD {
        ThreatLevel::Critical
    } else if total_kills >= ACTIVE_THRESHOLD {
        ThreatLevel::High
    } else {
        ThreatLevel::Unknown
    }
}

/// The daily pass: recompute the threats, then drop killmails past retention.
pub(super) fn start(pool: PgPool, esi: EsiClient) {
    crate::background::spawn_every("threat analysis", Duration::from_hours(24), move || {
        let (pool, esi) = (pool.clone(), esi.clone());
        async move {
            let analyzed = analyze(&pool, &esi).await.map_err(|err| err.to_string());
            if let Err(err) = purge(&pool).await {
                tracing::error!("killmail purge failed: {err}");
            }
            analyzed
        }
    });
}

/// Advisory lock key for [`analyze`], which replaces the whole table and so cannot overlap
/// with itself. Arbitrary, only has to be unique among this application's locks.
const THREAT_ANALYSIS_LOCK: i64 = 0x7B12_0001;

struct OrgStat {
    solar_system_id: i64,
    entity_type: String,
    entity_id: i64,
    kills: i64,
}

/// Recompute threat for every wormhole system (full replacement).
pub async fn analyze(pool: &PgPool, esi: &EsiClient) -> Result<(), Box<dyn std::error::Error>> {
    let stats = org_stats(pool).await?;
    let names = org_names(pool, esi, &stats).await?;
    replace_threats(pool, &stats, &names).await?;
    Ok(())
}

/// The top organisations active in each wormhole system over the analysis window.
async fn org_stats(pool: &PgPool) -> sqlx::Result<Vec<OrgStat>> {
    let rows = sqlx::query!(
        r#"with orgs as (
               select k.solar_system_id, (o->>'id')::bigint as entity_id,
                      o->>'kind' as entity_type, k.id as killmail_id, date(k.time) as day
               from killmails k
               cross join lateral jsonb_array_elements(k.orgs) o
               where k.time >= now() - make_interval(days => $1)
                 and k.solar_system_id in (select solar_system_id from wormhole_systems)
           ),
           stats as (
               select solar_system_id, entity_type, entity_id,
                      count(distinct killmail_id) as kills, count(distinct day) as active_days
               from orgs group by 1, 2, 3
           ),
           ranked as (
               select *, row_number() over (
                   partition by solar_system_id order by kills desc, entity_id
               ) as rn
               from stats where active_days >= $2
           )
           -- `!` throughout: these come out of the CTEs above, which the planner cannot
           -- prove non-null, but a row only exists here because an org produced it.
           select solar_system_id as "solar_system_id!", entity_type as "entity_type!",
                  entity_id as "entity_id!", kills::bigint as "kills!"
           from ranked where rn <= $3"#,
        ANALYSIS_WINDOW_DAYS,
        MIN_ACTIVE_DAYS,
        TOP_ORGS,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| OrgStat {
            solar_system_id: r.solar_system_id,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            kills: r.kills,
        })
        .collect())
}

/// Name what we can, fetching and keeping the rest so the next run need not ask again.
async fn org_names(
    pool: &PgPool,
    esi: &EsiClient,
    stats: &[OrgStat],
) -> sqlx::Result<HashMap<(String, i64), String>> {
    let ids_of = |kind: &str| -> Vec<i64> {
        let mut ids: Vec<i64> = stats
            .iter()
            .filter(|s| s.entity_type == kind)
            .map(|s| s.entity_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let (alliances, corporations) = (ids_of("alliance"), ids_of("corporation"));
    entities::ensure(pool, esi, EntityKind::Alliance, &alliances).await;
    entities::ensure(pool, esi, EntityKind::Corporation, &corporations).await;
    let names = sqlx::query!(
        r#"select 'alliance' as "kind!", id as "id!", name as "name!" from alliances where id = any($1)
           union all
           select 'corporation', id, name from corporations where id = any($2)"#,
        &alliances,
        &corporations,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| ((r.kind, r.id), r.name))
    .collect();
    Ok(names)
}

#[expect(
    clippy::as_conversions,
    reason = "`expr as Type` is sqlx's parameter type override, which the macro expands to a cast"
)]
async fn replace_threats(
    pool: &PgPool,
    stats: &[OrgStat],
    names: &HashMap<(String, i64), String>,
) -> sqlx::Result<()> {
    // Full replacement in one transaction, and only one at a time: the backfill asks for an
    // analysis when it finishes, which can land on top of the daily one. Two replacements
    // interleaving delete each other's rows and then collide on the unique key.
    let mut tx = pool.begin().await?;
    sqlx::query!("select pg_advisory_xact_lock($1)", THREAT_ANALYSIS_LOCK)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("delete from wormhole_system_threats")
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        "update wormhole_systems set threat_level = 'unknown'::threat_level,
                threat_analyzed_at = now()"
    )
    .execute(&mut *tx)
    .await?;

    // Both written in one statement each, from parallel arrays.
    let mut totals: HashMap<i64, i64> = HashMap::new();
    let mut systems = Vec::with_capacity(stats.len());
    let mut entity_ids = Vec::with_capacity(stats.len());
    let mut entity_types = Vec::with_capacity(stats.len());
    let mut entity_names = Vec::with_capacity(stats.len());
    let mut kills = Vec::with_capacity(stats.len());
    for s in stats {
        *totals.entry(s.solar_system_id).or_default() += s.kills;
        systems.push(s.solar_system_id);
        entity_ids.push(s.entity_id);
        entity_types.push(s.entity_type.clone());
        let name = names.get(&(s.entity_type.clone(), s.entity_id));
        entity_names.push(name.map_or("Unknown entity", String::as_str).to_owned());
        kills.push(i32::try_from(s.kills).unwrap_or(i32::MAX));
    }
    sqlx::query!(
        "insert into wormhole_system_threats (solar_system_id, entity_id, entity_type, name, kills)
         select * from unnest($1::bigint[], $2::bigint[], $3::text[], $4::text[], $5::int[])",
        &systems,
        &entity_ids,
        &entity_types,
        &entity_names,
        &kills,
    )
    .execute(&mut *tx)
    .await?;

    let (levelled, levels): (Vec<i64>, Vec<crate::maps::ThreatLevel>) = totals
        .iter()
        .map(|(system, total)| (*system, threat_level(*total)))
        .unzip();
    sqlx::query!(
        "update wormhole_systems w set threat_level = l.level
         from unnest($1::bigint[], $2::threat_level[]) as l(solar_system_id, level)
         where w.solar_system_id = l.solar_system_id",
        &levelled,
        &levels as &[crate::maps::ThreatLevel],
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn purge(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "delete from killmails where time < now() - make_interval(days => $1)",
        super::RETENTION_DAYS
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_match_legacy() {
        use crate::maps::ThreatLevel;
        assert_eq!(threat_level(0), ThreatLevel::Unknown);
        assert_eq!(threat_level(14), ThreatLevel::Unknown);
        assert_eq!(threat_level(15), ThreatLevel::High);
        assert_eq!(threat_level(49), ThreatLevel::High);
        assert_eq!(threat_level(50), ThreatLevel::Critical);
    }
}
