//! The skyhook mirror: what a sync does to the table, and what a row reads as.

mod common;

use chrono::{Duration, Utc};
use common::{SYS_A, SYS_B, seed_universe};
use sqlx::PgPool;
use wormholesystems::esi::skyhooks::{RaidableSkyhook, TheftWindow};
use wormholesystems::skyhooks::{PlanetKind, list, store};

/// A planet the SDE knows about, so the sync has something to hang a skyhook on.
async fn seed_planet(pool: &PgPool, id: i64, system: i64, index: i32, type_name: &str) {
    sqlx::query(
        "insert into categories (id, name, published) values (7, 'Celestial', true)
         on conflict do nothing",
    )
    .execute(pool)
    .await
    .expect("insert categories");
    sqlx::query(
        "insert into groups (id, category_id, name, published) values (7, 7, 'Planet', true)
         on conflict do nothing",
    )
    .execute(pool)
    .await
    .expect("insert groups");
    let type_id = 2000 + i64::from(index) + system % 7;
    sqlx::query(
        "insert into types (id, group_id, name, published) values ($1, 7, $2, true)
         on conflict (id) do nothing",
    )
    .bind(type_id)
    .bind(type_name)
    .execute(pool)
    .await
    .expect("insert types");
    sqlx::query(
        "insert into planets (id, solar_system_id, type_id, celestial_index) values ($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(system)
    .bind(type_id)
    .bind(index)
    .execute(pool)
    .await
    .expect("insert planets");
}

/// A skyhook whose window runs from `starts_in` for two hours, as ESI reports them.
fn skyhook(planet_id: i64, system: i64, starts_in: Duration) -> RaidableSkyhook {
    let start = Utc::now() + starts_in;
    RaidableSkyhook {
        planet_id,
        solar_system_id: system,
        theft_vulnerability: TheftWindow {
            start,
            end: start + Duration::hours(2),
        },
    }
}

#[sqlx::test]
async fn a_sync_mirrors_esi_rather_than_accumulating(pool: PgPool) {
    seed_universe(&pool).await;
    seed_planet(&pool, 40000001, SYS_A, 3, "Planet (Lava)").await;
    seed_planet(&pool, 40000002, SYS_B, 6, "Planet (Ice)").await;

    let stored = store(
        &pool,
        &[
            skyhook(40000001, SYS_A, Duration::minutes(-10)),
            skyhook(40000002, SYS_B, Duration::minutes(30)),
        ],
    )
    .await
    .unwrap();
    assert_eq!(stored, 2);
    assert_eq!(list(&pool).await.unwrap().len(), 2);

    // The second sync no longer mentions the first skyhook: its window has passed, so it
    // stops being a row rather than lingering as history.
    let stored = store(&pool, &[skyhook(40000002, SYS_B, Duration::minutes(30))])
        .await
        .unwrap();
    assert_eq!(stored, 1);
    let listed = list(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].planet_id, 40000002);
}

#[sqlx::test]
async fn a_moved_window_updates_in_place(pool: PgPool) {
    seed_universe(&pool).await;
    seed_planet(&pool, 40000001, SYS_A, 3, "Planet (Lava)").await;

    store(&pool, &[skyhook(40000001, SYS_A, Duration::minutes(10))])
        .await
        .unwrap();
    let first = list(&pool).await.unwrap()[0].vulnerable_from;

    store(&pool, &[skyhook(40000001, SYS_A, Duration::minutes(90))])
        .await
        .unwrap();
    let listed = list(&pool).await.unwrap();
    assert_eq!(listed.len(), 1, "the same planet is one row, not two");
    assert!(listed[0].vulnerable_from > first);
}

#[sqlx::test]
async fn a_planet_the_sde_has_never_heard_of_is_skipped_without_failing(pool: PgPool) {
    seed_universe(&pool).await;
    seed_planet(&pool, 40000001, SYS_A, 3, "Planet (Lava)").await;

    // A content patch can add planets before the next SDE seed. That must not take the
    // whole sync down with it.
    let stored = store(
        &pool,
        &[
            skyhook(40000001, SYS_A, Duration::minutes(5)),
            skyhook(49999999, SYS_A, Duration::minutes(5)),
        ],
    )
    .await
    .unwrap();
    assert_eq!(stored, 1);
    assert_eq!(list(&pool).await.unwrap().len(), 1);
}

#[sqlx::test]
async fn a_row_reads_as_it_does_in_the_overview(pool: PgPool) {
    seed_universe(&pool).await;
    seed_planet(&pool, 40000001, SYS_A, 4, "Planet (Lava)").await;
    store(&pool, &[skyhook(40000001, SYS_A, Duration::minutes(-5))])
        .await
        .unwrap();

    let row = &list(&pool).await.unwrap()[0];
    // Built from the celestial index, because the SDE names almost no planets.
    assert_eq!(row.planet_name, "Jita IV");
    assert_eq!(row.planet_kind, PlanetKind::Lava);
    assert_eq!(row.system_name, "Jita");
    assert_eq!(row.region, "Test Region");
}

#[sqlx::test]
async fn a_window_that_has_closed_is_not_listed(pool: PgPool) {
    seed_universe(&pool).await;
    seed_planet(&pool, 40000001, SYS_A, 3, "Planet (Lava)").await;

    // Still in the table (ESI last said so), but there is nothing left to raid.
    store(&pool, &[skyhook(40000001, SYS_A, Duration::hours(-3))])
        .await
        .unwrap();
    assert!(list(&pool).await.unwrap().is_empty());
}
