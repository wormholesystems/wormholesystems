//! Building the chain from a tracked jump: what one jump creates, and that it comes back
//! out as a single step.

use crate::common;

use common::map::place_at;
use common::{SYS_A, SYS_B, SYS_C, member_with_role, world};
use sqlx::PgPool;
use wormholesystems::maps::history::{MapIdBody, list_history, redo, undo};
use wormholesystems::maps::map::{GetMap, get_map};
use wormholesystems::maps::signatures::{AddSignature, add_signature, list_signatures};
use wormholesystems::maps::tracking::{TrackJump, track_jump};
use wormholesystems::maps::{Actor, MapError, Role, SignatureGroup, WormholeSize};

async fn scan(pool: &PgPool, actor: Actor, map_id: i64, sys: i64, id: &str) -> i64 {
    add_signature(
        pool,
        actor,
        AddSignature {
            map_id,
            solar_system_id: sys,
            signature_id: id.into(),
            group: SignatureGroup::Unknown,
            ..Default::default()
        },
    )
    .await
    .expect("add_signature")
    .id
}

fn jump(map_id: i64, from: i64, to: i64) -> TrackJump {
    TrackJump {
        map_id,
        from_map_solar_system_id: from,
        to_solar_system_id: to,
        x: 200.0,
        y: 0.0,
        signature_pk: None,
        alias: None,
        size: None,
        mass_status: None,
        time_status: None,
    }
}

async fn systems(pool: &PgPool, actor: Actor, map_id: i64) -> Vec<i64> {
    get_map(pool, actor, GetMap { map_id })
        .await
        .expect("get_map")
        .systems
        .iter()
        .filter_map(wormholesystems::maps::MapSystemView::solar_system_id)
        .collect()
}

async fn connections(pool: &PgPool, actor: Actor, map_id: i64) -> usize {
    get_map(pool, actor, GetMap { map_id })
        .await
        .expect("get_map")
        .connections
        .len()
}

#[tokio::test]
async fn a_jump_places_the_system_connects_it_and_links_the_signature() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    let sig = scan(&pool, w.owner, w.map_id, SYS_A, "ABC-123").await;

    track_jump(
        &pool,
        w.owner,
        TrackJump {
            signature_pk: Some(sig),
            alias: Some("D2".into()),
            size: Some(WormholeSize::Medium),
            ..jump(w.map_id, a, SYS_B)
        },
    )
    .await
    .unwrap();

    let view = get_map(&pool, w.owner, GetMap { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(view.systems.len(), 2);
    let placed = view
        .systems
        .iter()
        .find(|s| s.solar_system_id() == Some(SYS_B))
        .unwrap();
    assert_eq!(placed.alias(), Some("D2"));
    assert_eq!(view.connections.len(), 1);
    assert_eq!(view.connections[0].size, Some(WormholeSize::Medium));

    // The signature is now the hole: promoted out of `unknown` and tied to the edge.
    let sigs = list_signatures(&pool, w.owner, w.map_id).await.unwrap();
    let linked = sigs.iter().find(|s| s.id == sig).unwrap();
    assert_eq!(linked.group, SignatureGroup::Wormhole);
    assert_eq!(linked.connection_id, Some(view.connections[0].id));
}

async fn alias_of(pool: &PgPool, actor: Actor, map_id: i64, sys: i64) -> Option<String> {
    get_map(pool, actor, GetMap { map_id })
        .await
        .expect("get_map")
        .systems
        .iter()
        .find(|s| s.solar_system_id() == Some(sys))
        .and_then(|s| s.alias().map(str::to_string))
}

#[tokio::test]
async fn undoing_a_jump_into_a_placed_system_puts_its_old_name_back() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    place_at(&pool, w.owner, w.map_id, SYS_B, 200.0).await;

    track_jump(
        &pool,
        w.owner,
        TrackJump {
            alias: Some("D2".into()),
            ..jump(w.map_id, a, SYS_B)
        },
    )
    .await
    .unwrap();
    assert_eq!(
        alias_of(&pool, w.owner, w.map_id, SYS_B).await.as_deref(),
        Some("D2")
    );

    undo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    // The system was there before the jump, so it stays, under the name it had.
    assert_eq!(systems(&pool, w.owner, w.map_id).await, vec![SYS_A, SYS_B]);
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 0);
    assert_eq!(alias_of(&pool, w.owner, w.map_id, SYS_B).await, None);

    redo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(
        alias_of(&pool, w.owner, w.map_id, SYS_B).await.as_deref(),
        Some("D2")
    );
}

#[tokio::test]
async fn the_whole_jump_is_one_undo() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    let sig = scan(&pool, w.owner, w.map_id, SYS_A, "ABC-123").await;
    track_jump(
        &pool,
        w.owner,
        TrackJump {
            signature_pk: Some(sig),
            ..jump(w.map_id, a, SYS_B)
        },
    )
    .await
    .unwrap();

    // Three things happened, but the history holds one step for them.
    let history = list_history(&pool, w.owner, w.map_id).await.unwrap();
    assert_eq!(history.entries[0].kind, "tracking.jumped");

    undo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(systems(&pool, w.owner, w.map_id).await, vec![SYS_A]);
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 0);
    let sigs = list_signatures(&pool, w.owner, w.map_id).await.unwrap();
    let restored = sigs.iter().find(|s| s.id == sig).unwrap();
    assert_eq!(
        restored.group,
        SignatureGroup::Unknown,
        "the signature goes back to how it was scanned"
    );
    assert_eq!(restored.connection_id, None);

    redo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 1);
    let sigs = list_signatures(&pool, w.owner, w.map_id).await.unwrap();
    assert_eq!(
        sigs.iter().find(|s| s.id == sig).unwrap().group,
        SignatureGroup::Wormhole
    );
}

#[tokio::test]
async fn jumping_into_a_system_already_on_the_map_connects_rather_than_duplicates() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    place_at(&pool, w.owner, w.map_id, SYS_B, 300.0).await;

    track_jump(&pool, w.owner, jump(w.map_id, a, SYS_B))
        .await
        .unwrap();

    assert_eq!(systems(&pool, w.owner, w.map_id).await.len(), 2);
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 1);
}

#[tokio::test]
async fn jumping_a_connection_that_is_already_mapped_only_links_the_signature() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    let sig = scan(&pool, w.owner, w.map_id, SYS_A, "ABC-123").await;
    track_jump(&pool, w.owner, jump(w.map_id, a, SYS_B))
        .await
        .unwrap();
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 1);

    // Flying it again with the signature named adds nothing, it just says which hole it is.
    track_jump(
        &pool,
        w.owner,
        TrackJump {
            signature_pk: Some(sig),
            ..jump(w.map_id, a, SYS_B)
        },
    )
    .await
    .unwrap();
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 1);
    assert_eq!(systems(&pool, w.owner, w.map_id).await.len(), 2);

    let sigs = list_signatures(&pool, w.owner, w.map_id).await.unwrap();
    assert!(
        sigs.iter()
            .find(|s| s.id == sig)
            .unwrap()
            .connection_id
            .is_some()
    );
    assert_eq!(
        list_history(&pool, w.owner, w.map_id)
            .await
            .unwrap()
            .entries[0]
            .kind,
        "tracking.linked"
    );

    // And undoing that unpicks only the link.
    undo(&pool, w.owner, MapIdBody { map_id: w.map_id })
        .await
        .unwrap();
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 1);
    let sigs = list_signatures(&pool, w.owner, w.map_id).await.unwrap();
    assert_eq!(
        sigs.iter().find(|s| s.id == sig).unwrap().connection_id,
        None
    );
}

#[tokio::test]
async fn flying_a_stargate_builds_nothing() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    // A stargate needs a type, which needs a group and a category; only the FKs matter.
    for statement in [
        "insert into categories (id, name, published) values (6, 'Ship', true) on conflict do nothing",
        "insert into groups (id, category_id, name, published) values (10, 6, 'Stargate', true)
         on conflict do nothing",
        "insert into types (id, group_id, name, published) values (16, 10, 'Stargate', true)
         on conflict do nothing",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "insert into stargates
             (id, solar_system_id, destination_system_id, destination_stargate_id, type_id)
         values (1, $1, $2, 2, 16)",
    )
    .bind(SYS_A)
    .bind(SYS_C)
    .execute(&pool)
    .await
    .unwrap();

    let err = track_jump(&pool, w.owner, jump(w.map_id, a, SYS_C)).await;
    assert!(matches!(err, Err(MapError::Conflict(_))));
    assert_eq!(connections(&pool, w.owner, w.map_id).await, 0);
}

#[tokio::test]
async fn a_viewer_cannot_build_the_chain() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;
    let viewer = member_with_role(&pool, w.owner, w.map_id, 1002, 2002, Role::Viewer).await;

    let err = track_jump(&pool, viewer, jump(w.map_id, a, SYS_B)).await;
    assert!(matches!(err, Err(MapError::Forbidden)));
}

/// The mass side of the same jump. `record_transit` runs whether or not anyone is looking
/// at the map, so a jump through an unmapped hole leaves a pending row; the connection the
/// prompt eventually creates has to pick it up, or the mass is silently lost.
#[tokio::test]
async fn the_connection_claims_the_transit_recorded_before_it_existed() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let a = place_at(&pool, w.owner, w.map_id, SYS_A, 0.0).await;

    sqlx::query(
        "insert into categories (id, name, published) values (6, 'Ship', true)
         on conflict do nothing",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into groups (id, category_id, name, published) values (25, 6, 'Frigate', true)
         on conflict do nothing",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("insert into types (id, group_id, name, published, mass) values (587, 25, 'Rifter', true, 1067000)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "insert into character_status (character_id, online, ship_type_id) values ($1, true, 587)
         on conflict (character_id) do update set ship_type_id = 587",
    )
    .bind(w.owner.character_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into map_user_settings (map_id, user_id, tracking_allowed) values ($1, $2, true)
         on conflict (map_id, user_id) do update set tracking_allowed = true",
    )
    .bind(w.map_id)
    .bind(w.owner.user_id)
    .execute(&pool)
    .await
    .unwrap();

    // The pilot flies through before the hole is on the map.
    wormholesystems::maps::jumps::record_transit(&pool, w.owner.character_id, SYS_A, SYS_B)
        .await
        .unwrap();
    let pending: i64 =
        sqlx::query_scalar("select count(*) from map_connection_jumps where connection_id is null")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, 1);

    track_jump(&pool, w.owner, jump(w.map_id, a, SYS_B))
        .await
        .unwrap();

    let claimed: i64 = sqlx::query_scalar(
        "select count(*) from map_connection_jumps where connection_id is not null",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(claimed, 1);
}

/// A desktop app watching a character takes that character's jumps off the website until
/// it says it stopped, and only for the account it is signed in to.
#[tokio::test]
async fn a_desktop_app_holds_the_characters_it_watches() {
    use wormholesystems::identity::characters;
    use wormholesystems::pilots::desktop::{DesktopHeartbeat, heartbeat};
    use wormholesystems::realtime::{UserEvent, UserHub};

    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let name: String = sqlx::query_scalar("select name from characters where id = $1")
        .bind(w.owner.character_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let users = UserHub::new();
    let mut heard = users.subscribe(w.owner.user_id);
    let mine = |list: Vec<characters::CharacterRef>| {
        list.into_iter()
            .find(|c| c.character_id == w.owner.character_id)
            .unwrap()
            .desktop_map_id
    };
    assert_eq!(mine(characters::list(&pool, w.owner).await.unwrap()), None);

    let beat = |active| DesktopHeartbeat {
        map_id: w.map_id,
        characters: vec![name.to_uppercase()],
        active,
    };
    heartbeat(&pool, &users, w.owner.user_id, beat(true))
        .await
        .unwrap();
    assert_eq!(
        mine(characters::list(&pool, w.owner).await.unwrap()),
        Some(w.map_id)
    );
    assert!(matches!(
        heard.try_recv(),
        Ok(UserEvent::CharacterStatusChanged { character_id }) if character_id == w.owner.character_id
    ));

    // Renewing it is not news.
    heartbeat(&pool, &users, w.owner.user_id, beat(true))
        .await
        .unwrap();
    assert!(heard.try_recv().is_err());

    // Somebody else's account cannot let it go.
    let other = member_with_role(&pool, w.owner, w.map_id, 1003, 2003, Role::Member).await;
    heartbeat(&pool, &users, other.user_id, beat(false))
        .await
        .unwrap();
    assert_eq!(
        mine(characters::list(&pool, w.owner).await.unwrap()),
        Some(w.map_id)
    );

    heartbeat(&pool, &users, w.owner.user_id, beat(false))
        .await
        .unwrap();
    assert_eq!(mine(characters::list(&pool, w.owner).await.unwrap()), None);
    assert!(heard.try_recv().is_ok());
}
