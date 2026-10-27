//! The per-map navigation watchlist: CRUD, idempotent add, role gating.

use crate::common;

use common::{SYS_A, SYS_B, SYS_C, member_with_role, world};
use wormholesystems::maps::watchlist::{
    AddWatchlistEntry, RemoveWatchlistEntry, SetWatchlistPinned, add_watchlist_entry,
    list_watchlist, remove_watchlist_entry, set_watchlist_pinned,
};
use wormholesystems::maps::{MapError, Role};

/// A new map arrives with the trade hubs already watched and pinned, so the navigation
/// panel can answer "how far from Jita" before anyone configures anything. The fixture
/// universe only contains one of the five, which is also the point: hubs missing from the
/// database are skipped rather than failing the creation.
#[tokio::test]
async fn new_maps_start_with_the_trade_hubs() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let entries = list_watchlist(&pool, w.owner, w.map_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].solar_system_id, SYS_A); // Jita
    assert!(entries[0].is_pinned);
}

#[tokio::test]
async fn crud_and_gating() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let viewer = member_with_role(&pool, w.owner, w.map_id, 1002, 2002, Role::Viewer).await;
    // Jita is seeded onto every new map, so the CRUD here works on systems that are not.

    // Viewers can read but not mutate.
    let err = add_watchlist_entry(
        &pool,
        viewer,
        AddWatchlistEntry {
            map_id: w.map_id,
            solar_system_id: SYS_B,
        },
    )
    .await;
    assert!(matches!(err, Err(MapError::Forbidden)));

    let entry = add_watchlist_entry(
        &pool,
        w.owner,
        AddWatchlistEntry {
            map_id: w.map_id,
            solar_system_id: SYS_B,
        },
    )
    .await
    .unwrap();
    assert!(!entry.is_pinned);

    // Adding the same system again is idempotent (same row).
    let again = add_watchlist_entry(
        &pool,
        w.owner,
        AddWatchlistEntry {
            map_id: w.map_id,
            solar_system_id: SYS_B,
        },
    )
    .await
    .unwrap();
    assert_eq!(again.id, entry.id);

    add_watchlist_entry(
        &pool,
        w.owner,
        AddWatchlistEntry {
            map_id: w.map_id,
            solar_system_id: SYS_C,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        list_watchlist(&pool, viewer, w.map_id).await.unwrap().len(),
        3
    );

    let pinned = set_watchlist_pinned(
        &pool,
        w.owner,
        SetWatchlistPinned {
            map_id: w.map_id,
            entry_id: entry.id,
            value: true,
        },
    )
    .await
    .unwrap();
    assert!(pinned.is_pinned);

    remove_watchlist_entry(
        &pool,
        w.owner,
        RemoveWatchlistEntry {
            map_id: w.map_id,
            entry_id: entry.id,
        },
    )
    .await
    .unwrap();
    let remaining = list_watchlist(&pool, w.owner, w.map_id).await.unwrap();
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[1].solar_system_id, SYS_C);

    // Unknown entry → NotFound.
    let err = remove_watchlist_entry(
        &pool,
        w.owner,
        RemoveWatchlistEntry {
            map_id: w.map_id,
            entry_id: entry.id,
        },
    )
    .await;
    assert!(matches!(err, Err(MapError::NotFound)));
}
