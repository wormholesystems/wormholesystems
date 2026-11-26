//! Tabs: a map split over several canvases, each a map of its own. What is drawn on one
//! and scanned on one is that tab's alone; who may see it, and a system's intel, is the map's.

use crate::common;

use common::map::{connect, ghosting, place_at, scan};
use common::{SYS_A, SYS_B, SYS_C, member_with_role, world};
use sqlx::PgPool;
use wormholesystems::maps::history::{MapIdBody, undo};
use wormholesystems::maps::map::{GetMap, UpdateMap, get_map, list_maps, update_map};
use wormholesystems::maps::signatures::list_signatures;
use wormholesystems::maps::solar_system::{
    ClearMap, SetAlias, SetHome, SetOccupier, SetPinned, SetStatus, clear_map, set_alias, set_home,
    set_occupier, set_pinned, set_status,
};
use wormholesystems::maps::tabs::{
    CreateTab, DeleteTab, MAX_TABS, RenameTab, ReorderTabs, create_tab, delete_tab, rename_tab,
    reorder_tabs,
};
use wormholesystems::maps::{Actor, MapError, MapSystemView, Role, SystemStatus};

async fn tab(pool: &PgPool, actor: Actor, map_id: i64, name: &str) -> i64 {
    create_tab(
        pool,
        actor,
        CreateTab {
            map_id,
            name: name.into(),
        },
    )
    .await
    .expect("create_tab")
    .id
}

async fn view(pool: &PgPool, actor: Actor, map_id: i64) -> wormholesystems::maps::MapView {
    get_map(pool, actor, GetMap { map_id })
        .await
        .expect("get_map")
}

/// The scanner ids a tab holds for a system, in order.
async fn scans(pool: &PgPool, actor: Actor, map_id: i64, system: i64) -> Vec<String> {
    let mut ids: Vec<String> = list_signatures(pool, actor, map_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|s| s.solar_system_id == system)
        .map(|s| s.signature_id)
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_map_is_its_own_first_tab_and_managers_add_more() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let member = member_with_role(&pool, w.owner, w.map_id, 1002, 2002, Role::Member).await;

    let v = view(&pool, w.owner, w.map_id).await;
    assert_eq!(v.root_id, w.map_id);
    assert_eq!(v.tabs.len(), 1);
    assert!(v.tabs[0].is_root);
    assert_eq!(v.tabs[0].name, "Main");

    assert!(matches!(
        create_tab(
            &pool,
            member,
            CreateTab {
                map_id: w.map_id,
                name: "Thera".into()
            }
        )
        .await,
        Err(MapError::Forbidden),
    ));
    assert!(matches!(
        create_tab(
            &pool,
            w.owner,
            CreateTab {
                map_id: w.map_id,
                name: "  ".into()
            }
        )
        .await,
        Err(MapError::Validation(_)),
    ));

    let thera = tab(&pool, w.owner, w.map_id, " Thera ").await;
    // Asked of a tab, the new one is still the map's: tabs have no tabs.
    let turnur = tab(&pool, w.owner, thera, "Turnur").await;
    rename_tab(
        &pool,
        w.owner,
        RenameTab {
            map_id: turnur,
            name: "Turnur exits".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        reorder_tabs(
            &pool,
            w.owner,
            ReorderTabs {
                map_id: w.map_id,
                tab_ids: vec![turnur, w.map_id]
            }
        )
        .await,
        Err(MapError::Validation(_)),
    ));
    let order = reorder_tabs(
        &pool,
        w.owner,
        ReorderTabs {
            map_id: w.map_id,
            tab_ids: vec![turnur, w.map_id, thera],
        },
    )
    .await
    .unwrap();
    let names: Vec<&str> = order.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Turnur exits", "Main", "Thera"]);

    // The same bar whichever tab is read, and each tab says which map it belongs to.
    let v = view(&pool, w.owner, thera).await;
    assert_eq!(v.root_id, w.map_id);
    assert_eq!(v.tabs.len(), 3);

    // A tab is part of its map, not another map in anybody's list.
    assert_eq!(list_maps(&pool, w.owner.user_id).await.unwrap().len(), 1);

    for n in 3..MAX_TABS {
        tab(&pool, w.owner, w.map_id, &format!("Tab {n}")).await;
    }
    assert!(matches!(
        create_tab(
            &pool,
            w.owner,
            CreateTab {
                map_id: w.map_id,
                name: "One too many".into()
            }
        )
        .await,
        Err(MapError::Conflict(_)),
    ));
}

#[tokio::test]
async fn a_tab_is_edited_on_its_own() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let member = member_with_role(&pool, w.owner, w.map_id, 1002, 2002, Role::Member).await;
    let stranger_user = common::new_user(&pool).await;
    common::add_character(&pool, stranger_user, 1003, 2003, None).await;
    let stranger = Actor {
        user_id: stranger_user,
        character_id: 1003,
    };
    let thera = tab(&pool, w.owner, w.map_id, "Thera").await;

    // The same system on both tabs is two placements: a name, a pin and a home each.
    let on_main = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    // A grant on the map is a grant on its tabs; no grant is no tab.
    let on_thera = place_at(&pool, member, thera, SYS_A, 400.0).await;
    assert!(matches!(
        get_map(&pool, stranger, GetMap { map_id: thera }).await,
        Err(MapError::NotFound),
    ));
    let exit = place_at(&pool, w.owner, thera, SYS_B, 600.0).await;
    connect(&pool, w.owner, thera, on_thera, exit).await;

    set_alias(
        &pool,
        w.owner,
        SetAlias {
            map_id: thera,
            map_solar_system_id: on_thera,
            alias: Some("Hub".into()),
        },
    )
    .await
    .unwrap();
    set_pinned(
        &pool,
        w.owner,
        SetPinned {
            map_id: thera,
            map_solar_system_id: on_thera,
            value: true,
        },
    )
    .await
    .unwrap();
    set_home(
        &pool,
        w.owner,
        SetHome {
            map_id: w.map_id,
            map_solar_system_id: on_main,
            value: true,
        },
    )
    .await
    .unwrap();

    let main = view(&pool, w.owner, w.map_id).await;
    let other = view(&pool, w.owner, thera).await;
    let MapSystemView::System {
        alias,
        is_pinned,
        is_home,
        ..
    } = &main.systems[0]
    else {
        panic!("a system");
    };
    assert_eq!(
        (alias.as_deref(), *is_pinned, *is_home),
        (None, false, true)
    );
    let hub = other
        .systems
        .iter()
        .find(|s| s.id() == on_thera)
        .expect("placed on the tab");
    let MapSystemView::System {
        alias,
        is_pinned,
        is_home,
        ..
    } = hub
    else {
        panic!("a system");
    };
    assert_eq!(
        (alias.as_deref(), *is_pinned, *is_home),
        (Some("Hub"), true, false)
    );

    // Each tab reads what the others draw, for the routes and the links between them.
    assert_eq!(main.connections.len(), 0);
    assert_eq!(main.elsewhere.len(), 1);
    assert_eq!(main.elsewhere[0].tab_id, thera);
    assert_eq!(main.elsewhere[0].systems.len(), 2);
    assert_eq!(main.elsewhere[0].connections.len(), 1);
    assert_eq!(main.elsewhere[0].connections[0].from_solar_system_id, SYS_A);
    assert_eq!(other.elsewhere[0].systems.len(), 1);

    // Clearing and undoing are the tab's own: the other tab does not move.
    clear_map(&pool, w.owner, ClearMap { map_id: thera })
        .await
        .unwrap();
    assert_eq!(view(&pool, w.owner, thera).await.systems.len(), 1);
    assert_eq!(view(&pool, w.owner, w.map_id).await.systems.len(), 1);
    undo(&pool, w.owner, MapIdBody { map_id: thera })
        .await
        .unwrap();
    assert_eq!(view(&pool, w.owner, thera).await.systems.len(), 2);
    assert!(matches!(
        undo(&pool, w.owner, MapIdBody { map_id: w.map_id }).await,
        Ok(())
    ));
    // That undid the home mark on Main, the last thing done there, and nothing on Thera.
    assert_eq!(view(&pool, w.owner, thera).await.systems.len(), 2);
}

/// The status and the occupier a tab shows for its first system.
async fn intel(pool: &PgPool, actor: Actor, map_id: i64) -> (SystemStatus, Option<String>) {
    let view = view(pool, actor, map_id).await;
    let MapSystemView::System {
        status,
        occupying_group,
        ..
    } = &view.systems[0]
    else {
        panic!("a system");
    };
    (*status, occupying_group.clone())
}

async fn occupy(pool: &PgPool, actor: Actor, map_id: i64, placement: i64, by: Option<&str>) {
    set_occupier(
        pool,
        actor,
        SetOccupier {
            map_id,
            map_solar_system_id: placement,
            occupier: by.map(Into::into),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn intel_is_all_that_tabs_share_about_a_system() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let thera = tab(&pool, w.owner, w.map_id, "Thera").await;
    place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    let on_thera = place_at(&pool, w.owner, thera, SYS_A, 400.0).await;

    // A scan stays on the tab it was made on, in either direction.
    scan(&pool, w.owner, w.map_id, SYS_A, "ABC-123").await;
    assert_eq!(
        scans(&pool, w.owner, thera, SYS_A).await,
        Vec::<String>::new()
    );
    scan(&pool, w.owner, thera, SYS_A, "XYZ-789").await;
    assert_eq!(scans(&pool, w.owner, w.map_id, SYS_A).await, ["ABC-123"]);

    // What is known about the system itself is the map's: set on one tab, it shows on
    // the other.
    set_status(
        &pool,
        w.owner,
        SetStatus {
            map_id: thera,
            map_solar_system_id: on_thera,
            status: SystemStatus::Hostile,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        intel(&pool, w.owner, w.map_id).await,
        (SystemStatus::Hostile, None)
    );
    occupy(&pool, w.owner, thera, on_thera, Some("Hard Knocks")).await;
    assert_eq!(
        intel(&pool, w.owner, w.map_id).await,
        (SystemStatus::Hostile, Some("Hard Knocks".into()))
    );

    // A tab made afterwards knows it too, before it draws the system and once it does.
    let turnur = tab(&pool, w.owner, w.map_id, "Turnur").await;
    place_at(&pool, w.owner, turnur, SYS_A, 0.0).await;
    assert_eq!(
        intel(&pool, w.owner, turnur).await,
        (SystemStatus::Hostile, Some("Hard Knocks".into()))
    );

    // Undone where it was set, it is gone everywhere.
    undo(&pool, w.owner, MapIdBody { map_id: thera })
        .await
        .unwrap();
    assert_eq!(
        intel(&pool, w.owner, w.map_id).await,
        (SystemStatus::Hostile, None)
    );
}

#[tokio::test]
async fn each_tab_draws_the_unmapped_holes_scanned_on_it() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    ghosting(&pool, w.owner, w.map_id).await;
    // The setting is the map's, and a tab made afterwards starts with it.
    let thera = tab(&pool, w.owner, w.map_id, "Thera").await;
    place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    place_at(&pool, w.owner, thera, SYS_A, 400.0).await;

    // The tab it was scanned on draws the hole. The other knows nothing of it.
    scan(&pool, w.owner, thera, SYS_A, "ABC-123").await;
    let other = view(&pool, w.owner, thera).await;
    assert_eq!(other.systems.len(), 2);
    assert_eq!(other.connections.len(), 1);
    let main = view(&pool, w.owner, w.map_id).await;
    assert_eq!(main.systems.len(), 1);
    // Only that the same system is over there, and what a route can take: not the ghost.
    assert_eq!(main.elsewhere[0].systems, [SYS_A]);
    assert!(main.elsewhere[0].connections.is_empty());

    // The same signature scanned here is this tab's own hole.
    scan(&pool, w.owner, w.map_id, SYS_A, "ABC-123").await;
    assert_eq!(view(&pool, w.owner, w.map_id).await.systems.len(), 2);
    assert_eq!(view(&pool, w.owner, thera).await.systems.len(), 2);

    // Turning the setting off on the map reaches the tab's row too.
    update_map(
        &pool,
        w.owner,
        UpdateMap {
            map_id: thera,
            ghost_unlinked_wormholes: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        !view(&pool, w.owner, w.map_id)
            .await
            .map
            .ghost_unlinked_wormholes
    );
    assert!(
        !view(&pool, w.owner, thera)
            .await
            .map
            .ghost_unlinked_wormholes
    );
}

#[tokio::test]
async fn deleting_a_tab_takes_what_is_drawn_on_it_and_nothing_else() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let member = member_with_role(&pool, w.owner, w.map_id, 1002, 2002, Role::Member).await;
    let thera = tab(&pool, w.owner, w.map_id, "Thera").await;
    place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    place_at(&pool, w.owner, thera, SYS_A, 0.0).await;
    place_at(&pool, w.owner, thera, SYS_C, 200.0).await;

    assert!(matches!(
        delete_tab(&pool, member, DeleteTab { map_id: thera }).await,
        Err(MapError::Forbidden),
    ));
    // The first tab is the map.
    assert!(matches!(
        delete_tab(&pool, w.owner, DeleteTab { map_id: w.map_id }).await,
        Err(MapError::Conflict(_)),
    ));
    delete_tab(&pool, w.owner, DeleteTab { map_id: thera })
        .await
        .unwrap();

    let v = view(&pool, w.owner, w.map_id).await;
    assert_eq!(v.tabs.len(), 1);
    assert_eq!(v.systems.len(), 1);
    assert!(v.elsewhere.is_empty());
    assert!(matches!(
        get_map(&pool, w.owner, GetMap { map_id: thera }).await,
        Err(MapError::NotFound),
    ));
}
