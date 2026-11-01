//! Putting things on a map, for the tests that need a chain to work on rather than the
//! placing itself.

use sqlx::PgPool;
use wormholesystems::maps::connection::{AddConnection, add_connection};
use wormholesystems::maps::map::{UpdateMap, update_map};
use wormholesystems::maps::signatures::{AddSignature, add_signature};
use wormholesystems::maps::solar_system::{AddSystem, add_system};
use wormholesystems::maps::{Actor, ConnectionType, MapConnection, SignatureGroup};

/// Place `system` at the origin; returns its `map_solar_systems` id.
pub(crate) async fn place(pool: &PgPool, actor: Actor, map_id: i64, system: i64) -> i64 {
    place_at(pool, actor, map_id, system, 0.0).await
}

/// Place `system` `x` along the top row, for the tests that care where it sits.
pub(crate) async fn place_at(pool: &PgPool, actor: Actor, map_id: i64, system: i64, x: f64) -> i64 {
    add_system(
        pool,
        actor,
        AddSystem {
            map_id,
            solar_system_id: system,
            x,
            y: 0.0,
            alias: None,
        },
    )
    .await
    .expect("add_system")
    .id
}

/// A wormhole between two placements, nothing known about it yet.
pub(crate) async fn connect(
    pool: &PgPool,
    actor: Actor,
    map_id: i64,
    from: i64,
    to: i64,
) -> MapConnection {
    connect_kind(pool, actor, map_id, from, to, ConnectionType::Wormhole).await
}

/// An edge of the given kind between two placements.
pub(crate) async fn connect_kind(
    pool: &PgPool,
    actor: Actor,
    map_id: i64,
    from: i64,
    to: i64,
    kind: ConnectionType,
) -> MapConnection {
    add_connection(
        pool,
        actor,
        AddConnection {
            map_id,
            from_system: from,
            to_system: to,
            kind,
            size: None,
        },
    )
    .await
    .expect("add_connection")
}

/// Scan an unnamed wormhole signature in `system`; returns its id.
pub(crate) async fn scan(pool: &PgPool, actor: Actor, map_id: i64, system: i64, sig: &str) -> i64 {
    add_signature(
        pool,
        actor,
        AddSignature {
            map_id,
            solar_system_id: system,
            signature_id: sig.into(),
            group: SignatureGroup::Wormhole,
            ..Default::default()
        },
    )
    .await
    .expect("add_signature")
    .id
}

/// A map that draws the far side of every hole it knows about. Ghosts are never placed by
/// hand in the tests, any more than they are in the app: the map raises them, and the
/// tests say so by scanning.
pub(crate) async fn ghosting(pool: &PgPool, actor: Actor, map_id: i64) {
    update_map(
        pool,
        actor,
        UpdateMap {
            map_id,
            ghost_unlinked_wormholes: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("update_map");
}

/// The node the map raised for this scan.
pub(crate) async fn ghost_for(pool: &PgPool, map_id: i64, sig: i64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "select case when f.solar_system_id is null then f.id else t.id end
         from signatures s
         join map_connections c on c.id = s.connection_id
         join map_solar_systems f on f.id = c.from_system
         join map_solar_systems t on t.id = c.to_system
         where s.id = $1 and s.map_id = $2",
    )
    .bind(sig)
    .bind(map_id)
    .fetch_one(pool)
    .await
    .expect("query signatures")
}
