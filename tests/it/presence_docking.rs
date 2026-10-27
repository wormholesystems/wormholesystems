//! A docked pilot is docked whether or not we can name where: stations come from the SDE,
//! structures only once a pilot with the structures permission has docked in one.

use crate::common;

use std::time::{Duration, SystemTime};

use common::{SYS_A, world};
use sqlx::PgPool;
use wormholesystems::db::PgTokenStore;
use wormholesystems::esi::entities::Structure;
use wormholesystems::esi::{Token, TokenStore};
use wormholesystems::maps::Actor;
use wormholesystems::maps::presence::{MapCharacter, characters_on};
use wormholesystems::structures;

const STATION: i64 = 60003760;
const STRUCTURE: i64 = 1035466617946;

/// The character is online in SYS_A, tracked on the map, docked wherever the ids say.
async fn docked(
    pool: &PgPool,
    map_id: i64,
    actor: Actor,
    station: Option<i64>,
    structure: Option<i64>,
) {
    PgTokenStore::new(pool.clone())
        .save(
            actor.character_id,
            &Token {
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                expires_at: SystemTime::now() + Duration::from_mins(20),
                scopes: vec!["esi-location.read_location.v1".into()],
            },
        )
        .await
        .expect("create token store");
    sqlx::query(
        "insert into character_status (character_id, online, solar_system_id, station_id, structure_id)
         values ($1, true, $2, $3, $4)
         on conflict (character_id) do update
         set station_id = excluded.station_id, structure_id = excluded.structure_id",
    )
    .bind(actor.character_id)
    .bind(SYS_A)
    .bind(station)
    .bind(structure)
    .execute(pool)
    .await
    .expect("insert character_status");
    sqlx::query(
        "insert into map_user_settings (map_id, user_id, tracking_allowed) values ($1, $2, true)
         on conflict (map_id, user_id) do update set tracking_allowed = true",
    )
    .bind(map_id)
    .bind(actor.user_id)
    .execute(pool)
    .await
    .expect("insert map_user_settings");
}

async fn me(pool: &PgPool, map_id: i64, actor: Actor) -> MapCharacter {
    characters_on(pool, map_id, actor.user_id)
        .await
        .expect("characters_on")
        .into_iter()
        .find(|c| c.character_id == actor.character_id)
        .expect("character is listed")
}

#[tokio::test]
async fn a_station_is_named_from_the_sde() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    for statement in [
        "insert into categories (id, name) values (3, 'Station')",
        "insert into groups (id, category_id, name) values (15, 3, 'Station')",
        "insert into types (id, group_id, name) values (52678, 15, 'Caldari Station')",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "insert into stations (id, solar_system_id, type_id, name)
         values ($1, $2, 52678, 'Jita IV - Moon 4 - Caldari Navy Assembly Plant')",
    )
    .bind(STATION)
    .bind(SYS_A)
    .execute(&pool)
    .await
    .unwrap();
    docked(&pool, w.map_id, w.owner, Some(STATION), None).await;

    let pilot = me(&pool, w.map_id, w.owner).await;

    assert!(pilot.is_docked);
    assert_eq!(
        pilot.docked_at.as_deref(),
        Some("Jita IV - Moon 4 - Caldari Navy Assembly Plant")
    );
}

#[tokio::test]
async fn an_unknown_structure_is_still_docked_and_named_once_fetched() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    docked(&pool, w.map_id, w.owner, None, Some(STRUCTURE)).await;

    let pilot = me(&pool, w.map_id, w.owner).await;
    assert!(pilot.is_docked);
    assert_eq!(pilot.docked_at, None);
    assert!(!structures::is_named(&pool, STRUCTURE).await.unwrap());

    // A type the SDE does not know is dropped rather than failing the row.
    structures::store(
        &pool,
        STRUCTURE,
        &Structure {
            name: "Jita - Trade Hub".into(),
            owner_id: 2001,
            solar_system_id: SYS_A,
            type_id: Some(35833),
        },
    )
    .await
    .unwrap();

    assert!(structures::is_named(&pool, STRUCTURE).await.unwrap());
    assert_eq!(
        me(&pool, w.map_id, w.owner).await.docked_at.as_deref(),
        Some("Jita - Trade Hub")
    );
}

#[tokio::test]
async fn the_sync_asks_about_new_structures_first_and_leaves_refused_ones_a_day() {
    let (pool, _db) = common::db::fresh().await;
    common::seed_universe(&pool).await;
    let (named, refused, fresh, new) = (1, 2, 3, 4);
    let structure = Structure {
        name: "Somewhere".into(),
        // A corporation we have never heard of.
        owner_id: 98000001,
        solar_system_id: SYS_A,
        type_id: None,
    };
    structures::store(&pool, named, &structure).await.unwrap();
    structures::store(&pool, fresh, &structure).await.unwrap();
    structures::note_refused(&pool, refused).await.unwrap();
    // Named over a week ago, so due again.
    sqlx::query("update structures set updated_at = now() - interval '8 days' where id = $1")
        .bind(named)
        .execute(&pool)
        .await
        .unwrap();

    let ids = [named, refused, fresh, new];
    assert_eq!(
        structures::needing_names(&pool, &ids, 10).await.unwrap(),
        vec![new, named]
    );
    assert_eq!(
        structures::needing_names(&pool, &ids, 1).await.unwrap(),
        vec![new]
    );

    // Refused yesterday: due again, and refusing again keeps a name it already had.
    sqlx::query("update structures set updated_at = now() - interval '25 hours'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        structures::needing_names(&pool, &ids, 10)
            .await
            .unwrap()
            .contains(&refused)
    );
    structures::note_refused(&pool, named).await.unwrap();
    assert!(structures::is_named(&pool, named).await.unwrap());
}
