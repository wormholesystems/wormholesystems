//! What a removal takes with it: every path out of the map goes through one cascade, so a
//! hole that goes takes its scans at both ends, whichever way it was taken off.

use crate::common;

use common::map::{connect, ghosting, place, scan};
use common::{SYS_A, SYS_B, world};
use sqlx::PgPool;
use wormholesystems::maps::Actor;
use wormholesystems::maps::connection::{
    CleanStaleConnections, RemoveConnection, clean_stale_connections, remove_connection,
};
use wormholesystems::maps::history::{MapIdBody, undo};
use wormholesystems::maps::map::{GetMap, get_map};
use wormholesystems::maps::signatures::{LinkSignature, link_signature, list_signatures};
use wormholesystems::maps::solar_system::{RemoveSystem, SetPinned, remove_system, set_pinned};
use wormholesystems::realtime::MapEvent;

/// Scan `sig` in `system` and link it to `connection_id`; returns the signature's id.
async fn scan_linked(
    pool: &PgPool,
    actor: Actor,
    map_id: i64,
    system: i64,
    sig: &str,
    connection_id: i64,
) -> i64 {
    let pk = scan(pool, actor, map_id, system, sig).await;
    link_signature(
        pool,
        actor,
        LinkSignature {
            map_id,
            signature_pk: pk,
            connection_id,
        },
    )
    .await
    .expect("link_signature");
    pk
}

/// Each signature on the map, as `(id, connection_id)`, in id order.
async fn links(pool: &PgPool, actor: Actor, map_id: i64) -> Vec<(i64, Option<i64>)> {
    let mut rows: Vec<(i64, Option<i64>)> = list_signatures(pool, actor, map_id)
        .await
        .expect("list_signatures")
        .into_iter()
        .map(|s| (s.id, s.connection_id))
        .collect();
    rows.sort_unstable();
    rows
}

async fn pin(pool: &PgPool, actor: Actor, map_id: i64, id: i64) {
    set_pinned(
        pool,
        actor,
        SetPinned {
            map_id,
            map_solar_system_id: id,
            value: true,
        },
    )
    .await
    .expect("set_pinned");
}

/// The stale clean was the path that only cut the edge: the scans stayed behind unlinked,
/// and on a map that draws unmapped holes each one grew a ghost in the hole's place.
#[tokio::test]
async fn cleaning_a_stale_hole_takes_its_scans_and_draws_no_ghost() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place(&pool, w.owner, w.map_id, SYS_A).await;
    let b = place(&pool, w.owner, w.map_id, SYS_B).await;
    // Both ends stay, so only the scans can be left behind.
    pin(&pool, w.owner, w.map_id, a).await;
    pin(&pool, w.owner, w.map_id, b).await;
    let hole = connect(&pool, w.owner, w.map_id, a, b).await.id;
    let near = scan_linked(&pool, w.owner, w.map_id, SYS_A, "ABC-123", hole).await;
    let far = scan_linked(&pool, w.owner, w.map_id, SYS_B, "XYZ-789", hole).await;
    ghosting(&pool, w.owner, w.map_id).await;
    // Two writes: marking it critical stamps the time, and the second backdates it.
    for sql in [
        "update map_connections set time_status = 'critical' where id = $1",
        "update map_connections set time_status_updated_at = now() - interval '2 hours'
         where id = $1",
    ] {
        sqlx::query(sql).bind(hole).execute(&pool).await.unwrap();
    }

    let mut events = wormholesystems::realtime::hub().subscribe(w.map_id);
    let cleaned =
        clean_stale_connections(&pool, w.owner, CleanStaleConnections { map_id: w.map_id })
            .await
            .unwrap();
    assert_eq!(cleaned, 1);

    let view = get_map(&pool, w.owner, GetMap { map_id: w.map_id })
        .await
        .unwrap();
    assert!(view.connections.is_empty());
    let mut ids: Vec<i64> = view
        .systems
        .iter()
        .map(wormholesystems::maps::MapSystemView::id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![a, b], "no ghost drawn for the hole that went");
    assert_eq!(links(&pool, w.owner, w.map_id).await, vec![]);

    // The signature panels hear about it, not only the map.
    let seen: Vec<MapEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    for solar_system_id in [SYS_A, SYS_B] {
        assert!(seen.contains(&MapEvent::SignatureChanged {
            map_id: w.map_id,
            solar_system_id,
        }));
    }

    // One undo brings the hole back with both scans on it.
    undo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(
        links(&pool, w.owner, w.map_id).await,
        vec![(near, Some(hole)), (far, Some(hole))]
    );
}

/// Signatures belong to a connection by its id, so of two holes between the same pair of
/// systems, cutting one leaves the other's scans alone.
#[tokio::test]
async fn two_holes_between_the_same_systems_keep_their_own_scans() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place(&pool, w.owner, w.map_id, SYS_A).await;
    let b = place(&pool, w.owner, w.map_id, SYS_B).await;
    let first = connect(&pool, w.owner, w.map_id, a, b).await.id;
    let second = connect(&pool, w.owner, w.map_id, a, b).await.id;
    scan_linked(&pool, w.owner, w.map_id, SYS_A, "ABC-123", first).await;
    scan_linked(&pool, w.owner, w.map_id, SYS_B, "XYZ-789", first).await;
    let near = scan_linked(&pool, w.owner, w.map_id, SYS_A, "DEF-456", second).await;
    let far = scan_linked(&pool, w.owner, w.map_id, SYS_B, "UVW-321", second).await;

    remove_connection(
        &pool,
        w.owner,
        RemoveConnection {
            map_id: w.map_id,
            connection_id: first,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        links(&pool, w.owner, w.map_id).await,
        vec![(near, Some(second)), (far, Some(second))]
    );
    let view = get_map(&pool, w.owner, GetMap { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(view.connections.len(), 1);
    assert_eq!(view.connections[0].id, second);
}

/// Removing a system takes the scan across each of its holes, and tells the system that
/// held it.
#[tokio::test]
async fn removing_a_system_tells_the_far_side_its_scan_went() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place(&pool, w.owner, w.map_id, SYS_A).await;
    let b = place(&pool, w.owner, w.map_id, SYS_B).await;
    let hole = connect(&pool, w.owner, w.map_id, a, b).await.id;
    scan_linked(&pool, w.owner, w.map_id, SYS_A, "ABC-123", hole).await;

    let mut events = wormholesystems::realtime::hub().subscribe(w.map_id);
    remove_system(
        &pool,
        w.owner,
        RemoveSystem {
            map_id: w.map_id,
            map_solar_system_id: b,
        },
    )
    .await
    .unwrap();

    assert_eq!(links(&pool, w.owner, w.map_id).await, vec![]);
    let seen: Vec<MapEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(seen.contains(&MapEvent::SignatureChanged {
        map_id: w.map_id,
        solar_system_id: SYS_A,
    }));
}
