//! Tabs: a map split over several canvases.
//!
//! A tab is a map in its own right: a row in `maps` that names another as its parent. Its
//! systems, connections, signatures and history hang off its own id, so everything that
//! edits one map (placing, pinning, naming, cleaning, undoing) works on one tab and never
//! reaches into another. The parent is the first tab, so a map nobody has split is one
//! row, as it always was.
//!
//! What ties the tabs of a map together is deliberately little: who may see them and how
//! (grants, sharing and each viewer's settings are the parent's), the routes, which run
//! over every tab's connections, and a system's intel: its status, occupier and notes
//! ([`share_intel`]). Beyond that a tab only shows which of its systems another tab draws
//! too: signatures, the holes they raise and aliases are each tab's own.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::access::require_role;
use super::command::Tx;
use super::error::{MapError, Result};
use super::{Actor, ConnectionType, MassStatus, Role, TimeStatus};
use crate::realtime::MapEvent;

/// More than fits in the bar is more than anyone keeps track of.
pub const MAX_TABS: i64 = 12;
const MAX_NAME_CHARS: usize = 24;

/// One tab of a map. `id` is a map id like any other: it is what the tab's systems and
/// connections are read and written under.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct MapTab {
    pub id: i64,
    pub name: String,
    /// The map itself, which is its own first tab and goes only when the map does.
    pub is_root: bool,
}

/// What another tab draws, as little of it as the routes and the links between tabs need.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct TabNetwork {
    pub tab_id: i64,
    /// The solar systems it draws, each once.
    pub systems: Vec<i64>,
    pub connections: Vec<TabEdge>,
}

/// A connection on another tab, by the systems it joins rather than by placements this
/// tab has never heard of.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct TabEdge {
    pub from_solar_system_id: i64,
    pub to_solar_system_id: i64,
    pub kind: ConnectionType,
    pub mass_status: Option<MassStatus>,
    pub time_status: Option<TimeStatus>,
}

/// The map a tab belongs to: itself, when it is not a tab.
pub async fn root_of(exec: impl sqlx::PgExecutor<'_>, map_id: i64) -> Result<i64> {
    sqlx::query_scalar!("select map_root($1)", map_id)
        .fetch_one(exec)
        .await?
        .ok_or(MapError::NotFound)
}

/// The map and each of its tabs, whichever of them was asked about.
pub async fn family_ids(exec: impl sqlx::PgExecutor<'_>, map_id: i64) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar!(
        r#"select m.id as "id!" from maps m
           where coalesce(m.parent_id, m.id) = map_root($1)
           order by m.tab_order, m.id"#,
        map_id,
    )
    .fetch_all(exec)
    .await?)
}

/// A map's tabs in bar order, whichever of them was asked about. Never empty.
pub async fn list_tabs(exec: impl sqlx::PgExecutor<'_>, map_id: i64) -> Result<Vec<MapTab>> {
    Ok(sqlx::query_as!(
        MapTab,
        r#"select m.id as "id!", m.tab_name as "name!", (m.parent_id is null) as "is_root!"
           from maps m
           where coalesce(m.parent_id, m.id) = map_root($1)
           order by m.tab_order, m.id"#,
        map_id,
    )
    .fetch_all(exec)
    .await?)
}

/// What the other tabs of a map draw. Ghosts are left out: a hole nobody has been through
/// leads nowhere a route could take, and names no system to link to.
pub async fn elsewhere(pool: &PgPool, map_id: i64) -> Result<Vec<TabNetwork>> {
    let systems = sqlx::query!(
        r#"select distinct s.map_id, s.solar_system_id as "solar_system_id!"
           from map_solar_systems s
           join maps m on m.id = s.map_id
           where coalesce(m.parent_id, m.id) = map_root($1) and s.map_id <> $1
             and s.solar_system_id is not null
           order by s.map_id, s.solar_system_id"#,
        map_id,
    )
    .fetch_all(pool)
    .await?;
    let edges = sqlx::query!(
        r#"select c.map_id, f.solar_system_id as "from!", t.solar_system_id as "to!",
                  c.type as kind, c.mass_status, c.time_status
           from map_connections c
           join maps m on m.id = c.map_id
           join map_solar_systems f on f.id = c.from_system
           join map_solar_systems t on t.id = c.to_system
           where coalesce(m.parent_id, m.id) = map_root($1) and c.map_id <> $1
             and f.solar_system_id is not null and t.solar_system_id is not null
           order by c.id"#,
        map_id,
    )
    .fetch_all(pool)
    .await?;

    let mut tabs: Vec<TabNetwork> = family_ids(pool, map_id)
        .await?
        .into_iter()
        .filter(|id| *id != map_id)
        .map(|tab_id| TabNetwork {
            tab_id,
            systems: Vec::new(),
            connections: Vec::new(),
        })
        .collect();
    for row in systems {
        if let Some(tab) = tabs.iter_mut().find(|t| t.tab_id == row.map_id) {
            tab.systems.push(row.solar_system_id);
        }
    }
    for row in edges {
        if let Some(tab) = tabs.iter_mut().find(|t| t.tab_id == row.map_id) {
            tab.connections.push(TabEdge {
                from_solar_system_id: row.from,
                to_solar_system_id: row.to,
                kind: row.kind,
                mass_status: row.mass_status,
                time_status: row.time_status,
            });
        }
    }
    Ok(tabs)
}

/// A system's intel (status, occupier, notes) is the one thing known about it that every
/// tab of the map agrees on: after a tab wrote the intel of these systems, write it to
/// the others. Returns the events for the placements it changed there.
///
/// A details row outlives a placement, so a tab is told whether or not it draws the
/// system right now.
pub(super) async fn share_intel(
    tx: &mut Tx<'_>,
    map_id: i64,
    solar_system_ids: &[i64],
) -> Result<Vec<MapEvent>> {
    let changed = sqlx::query!(
        r#"with shared as (
               insert into map_solar_system_details
                   (map_id, solar_system_id, status, occupying_group, notes)
               select m.id, d.solar_system_id, d.status, d.occupying_group, d.notes
               from map_solar_system_details d
               join maps m on coalesce(m.parent_id, m.id) = map_root($1) and m.id <> $1
               where d.map_id = $1 and d.solar_system_id = any($2)
               on conflict (map_id, solar_system_id) do update
                   set status = excluded.status, occupying_group = excluded.occupying_group,
                       notes = excluded.notes, updated_at = now()
                   where (map_solar_system_details.status,
                          map_solar_system_details.occupying_group,
                          map_solar_system_details.notes)
                       is distinct from
                         (excluded.status, excluded.occupying_group, excluded.notes)
               returning map_id, solar_system_id
           )
           select s.map_id as "map_id!", s.id as "id!"
           from shared c
           join map_solar_systems s
             on s.map_id = c.map_id and s.solar_system_id = c.solar_system_id"#,
        map_id,
        solar_system_ids,
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(changed
        .into_iter()
        .map(|p| MapEvent::SystemDetailsChanged {
            map_id: p.map_id,
            map_solar_system_id: p.id,
        })
        .collect())
}

/// [`share_intel`] for the system behind one placement.
pub(super) async fn share_placement_intel(
    tx: &mut Tx<'_>,
    map_id: i64,
    map_solar_system_id: i64,
) -> Result<Vec<MapEvent>> {
    let system = sqlx::query_scalar!(
        "select solar_system_id from map_solar_systems where id = $1 and map_id = $2",
        map_solar_system_id,
        map_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    match system {
        Some(system) => share_intel(tx, map_id, &[system]).await,
        None => Ok(Vec::new()),
    }
}

fn clean_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(MapError::Validation("a tab needs a name".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(MapError::Validation(format!(
            "a tab name is at most {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(name)
}

/// Tell everyone with the map open that its tabs changed. Said on the map itself, which
/// every tab's viewers hear.
fn announce(root: i64) {
    crate::realtime::hub().publish(MapEvent::MapUpdated { map_id: root });
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct CreateTab {
    /// The map, or any of its tabs.
    pub map_id: i64,
    pub name: String,
}

/// Add an empty tab at the end of the bar. Manager+. It starts with the map's settings and
/// nothing on it.
pub async fn create_tab(pool: &PgPool, actor: Actor, cmd: CreateTab) -> Result<MapTab> {
    let name = clean_name(&cmd.name)?;
    require_role(pool, cmd.map_id, actor.user_id, Role::Manager).await?;
    let mut tx = pool.begin().await?;
    // The map's row is the lock: two tabs added at once take turns, so the limit holds.
    let root = sqlx::query_scalar!(
        "select id from maps where id = map_root($1) for update",
        cmd.map_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(MapError::NotFound)?;
    let count = sqlx::query_scalar!(
        r#"select count(*) as "count!" from maps where id = $1 or parent_id = $1"#,
        root,
    )
    .fetch_one(&mut *tx)
    .await?;
    if count >= MAX_TABS {
        return Err(MapError::Conflict(format!(
            "a map has at most {MAX_TABS} tabs"
        )));
    }
    // The name and the settings are copied from the map by trigger.
    let tab = sqlx::query_as!(
        MapTab,
        r#"insert into maps (name, parent_id, tab_name, tab_order)
           select '', $1, $2, coalesce(max(tab_order), -1) + 1
           from maps where id = $1 or parent_id = $1
           returning id as "id!", tab_name as "name!", false as "is_root!""#,
        root,
        name,
    )
    .fetch_one(&mut *tx)
    .await?;
    // The tabs agree on each system's intel, and a new one starts out agreeing.
    sqlx::query!(
        "insert into map_solar_system_details
             (map_id, solar_system_id, status, occupying_group, notes)
         select $1, solar_system_id, status, occupying_group, notes
         from map_solar_system_details
         where map_id = $2",
        tab.id,
        root,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    announce(root);
    Ok(tab)
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct RenameTab {
    /// The tab.
    pub map_id: i64,
    pub name: String,
}

/// Rename a tab. Manager+.
pub async fn rename_tab(pool: &PgPool, actor: Actor, cmd: RenameTab) -> Result<MapTab> {
    let name = clean_name(&cmd.name)?;
    require_role(pool, cmd.map_id, actor.user_id, Role::Manager).await?;
    let tab = sqlx::query_as!(
        MapTab,
        r#"update maps set tab_name = $2 where id = $1
           returning id as "id!", tab_name as "name!", (parent_id is null) as "is_root!""#,
        cmd.map_id,
        name,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(MapError::NotFound)?;
    announce(root_of(pool, cmd.map_id).await?);
    Ok(tab)
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct ReorderTabs {
    /// The map, or any of its tabs.
    pub map_id: i64,
    /// Every tab of the map, in the order the bar should show them.
    pub tab_ids: Vec<i64>,
}

/// Put the tabs in a new order. Manager+. The list has to name each of the map's tabs
/// once: a tab created meanwhile by somebody else would otherwise land wherever its old
/// number happened to fall.
pub async fn reorder_tabs(pool: &PgPool, actor: Actor, cmd: ReorderTabs) -> Result<Vec<MapTab>> {
    require_role(pool, cmd.map_id, actor.user_id, Role::Manager).await?;
    let mut tx = pool.begin().await?;
    let mut current = family_ids(&mut *tx, cmd.map_id).await?;
    let mut wanted = cmd.tab_ids.clone();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err(MapError::Validation(
            "the order has to name each of the map's tabs once".into(),
        ));
    }
    let orders: Vec<i32> = (0..).take(cmd.tab_ids.len()).collect();
    sqlx::query!(
        "update maps m set tab_order = o.tab_order
         from unnest($1::bigint[], $2::int[]) as o(id, tab_order)
         where m.id = o.id",
        &cmd.tab_ids,
        &orders,
    )
    .execute(&mut *tx)
    .await?;
    let tabs = list_tabs(&mut *tx, cmd.map_id).await?;
    tx.commit().await?;
    announce(root_of(pool, cmd.map_id).await?);
    Ok(tabs)
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct DeleteTab {
    /// The tab.
    pub map_id: i64,
}

/// Delete a tab with everything drawn on it: its systems, connections, signatures and
/// history. Manager+. The map's own first tab is the map, and goes only with it.
pub async fn delete_tab(pool: &PgPool, actor: Actor, cmd: DeleteTab) -> Result<()> {
    require_role(pool, cmd.map_id, actor.user_id, Role::Manager).await?;
    let root = root_of(pool, cmd.map_id).await?;
    if root == cmd.map_id {
        return Err(MapError::Conflict(
            "the first tab is the map itself and goes only with it".into(),
        ));
    }
    sqlx::query!(
        "delete from maps where id = $1 and parent_id is not null",
        cmd.map_id,
    )
    .execute(pool)
    .await?;
    announce(root);
    Ok(())
}
