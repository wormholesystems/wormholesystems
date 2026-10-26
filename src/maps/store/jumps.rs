//! Writes to `map_connection_jumps`: the jump log a connection's mass estimate is read from.
//!
//! A connection's jumps go with it by the foreign key, so whatever snapshots a connection
//! snapshots its jumps too, and puts them back after it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::maps::command::Tx;
use crate::maps::error::Result;

/// A whole jump as it stands, keys included: what a removal snapshots and an undo puts back
/// under the same id. Serialized into the undo history, so the field names are a stored
/// format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JumpRow {
    pub id: i64,
    pub connection_id: Option<i64>,
    pub character_id: Option<i64>,
    pub from_solar_system_id: i64,
    pub to_solar_system_id: i64,
    pub ship_type_id: Option<i64>,
    pub ship_name: Option<String>,
    pub mass: i64,
    pub is_manual: bool,
    pub created_at: DateTime<Utc>,
}

/// These jumps as they stand, for a snapshot.
pub(crate) async fn snapshot(tx: &mut Tx<'_>, map_id: i64, ids: &[i64]) -> Result<Vec<JumpRow>> {
    Ok(sqlx::query_as!(
        JumpRow,
        "select id, connection_id, character_id, from_solar_system_id, to_solar_system_id,
                ship_type_id, ship_name, mass, is_manual, created_at
         from map_connection_jumps where map_id = $1 and id = any($2)
         order by id",
        map_id,
        ids,
    )
    .fetch_all(&mut **tx)
    .await?)
}

/// Every jump through these connections, for a snapshot of what goes with them.
pub(crate) async fn snapshot_through(
    tx: &mut Tx<'_>,
    map_id: i64,
    connection_ids: &[i64],
) -> Result<Vec<JumpRow>> {
    Ok(sqlx::query_as!(
        JumpRow,
        "select id, connection_id, character_id, from_solar_system_id, to_solar_system_id,
                ship_type_id, ship_name, mass, is_manual, created_at
         from map_connection_jumps where map_id = $1 and connection_id = any($2)
         order by id",
        map_id,
        connection_ids,
    )
    .fetch_all(&mut **tx)
    .await?)
}

/// Put snapshotted jumps back under their own ids, in one statement: undoing a cleared map
/// can bring back thousands. A row that is somehow back already is left alone, and a pilot
/// whose character has gone since is forgotten, as the foreign key would have done had the
/// jump stayed.
pub(crate) async fn reinsert(tx: &mut Tx<'_>, map_id: i64, rows: &[JumpRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let connection_ids: Vec<Option<i64>> = rows.iter().map(|r| r.connection_id).collect();
    let character_ids: Vec<Option<i64>> = rows.iter().map(|r| r.character_id).collect();
    let from_ids: Vec<i64> = rows.iter().map(|r| r.from_solar_system_id).collect();
    let to_ids: Vec<i64> = rows.iter().map(|r| r.to_solar_system_id).collect();
    let ship_type_ids: Vec<Option<i64>> = rows.iter().map(|r| r.ship_type_id).collect();
    let ship_names: Vec<Option<String>> = rows.iter().map(|r| r.ship_name.clone()).collect();
    let masses: Vec<i64> = rows.iter().map(|r| r.mass).collect();
    let manual: Vec<bool> = rows.iter().map(|r| r.is_manual).collect();
    let created: Vec<DateTime<Utc>> = rows.iter().map(|r| r.created_at).collect();
    #[expect(
        clippy::as_conversions,
        reason = "sqlx's `as` type override for a typed parameter, not a value conversion"
    )]
    sqlx::query!(
        "insert into map_connection_jumps
             (id, map_id, connection_id, character_id, from_solar_system_id,
              to_solar_system_id, ship_type_id, ship_name, mass, is_manual, created_at)
         overriding system value
         select j.id, $1, j.connection_id, c.id, j.from_id, j.to_id, j.ship_type_id,
                j.ship_name, j.mass, j.is_manual, j.created_at
         from unnest($2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[],
                     $7::bigint[], $8::text[], $9::bigint[], $10::boolean[],
                     $11::timestamptz[])
              as j(id, connection_id, character_id, from_id, to_id, ship_type_id, ship_name,
                   mass, is_manual, created_at)
         left join characters c on c.id = j.character_id
         on conflict (id) do nothing",
        map_id,
        &ids,
        &connection_ids as &[Option<i64>],
        &character_ids as &[Option<i64>],
        &from_ids,
        &to_ids,
        &ship_type_ids as &[Option<i64>],
        &ship_names as &[Option<String>],
        &masses,
        &manual,
        &created,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}
