//! Naming ids from ESI, and remembering the ones ESI no longer knows so they are not asked
//! for on every pass.

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use sqlx::PgPool;
use wormholesystems::entities::{EntityKind, ensure, unresolved};
use wormholesystems::esi::EsiClient;

const LIVE_CORP: i64 = 98000001;
const DEAD_CORP: i64 = 98000002;

/// A stand-in ESI: one corporation exists, every other id is a 404, and every request is
/// counted so a test can say how often the resolver came asking.
async fn stub_esi() -> (EsiClient, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/corporations/{id}", get(corporation))
        .with_state(Arc::clone(&hits));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let base = format!(
        "http://{}",
        listener.local_addr().expect("read listener address")
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });
    (
        EsiClient::with_config(reqwest::Client::new(), base, "2025-08-26"),
        hits,
    )
}

async fn corporation(
    State(hits): State<Arc<AtomicUsize>>,
    Path(id): Path<i64>,
) -> (StatusCode, String) {
    hits.fetch_add(1, Ordering::SeqCst);
    if id == LIVE_CORP {
        (
            StatusCode::OK,
            r#"{"name":"Live Corp","ticker":"LIVE","member_count":1,"ceo_id":1,"creator_id":1,"tax_rate":0.1}"#.into(),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            r#"{"error":"Corporation not found"}"#.into(),
        )
    }
}

async fn noted(pool: &PgPool, kind: &str, id: i64) -> bool {
    sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from unresolvable_entities where kind = $1 and id = $2)",
    )
    .bind(kind)
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("query unresolvable_entities")
}

#[sqlx::test]
async fn a_404_is_remembered_and_the_id_is_not_asked_for_again(pool: PgPool) {
    let (esi, hits) = stub_esi().await;

    ensure(
        &pool,
        &esi,
        EntityKind::Corporation,
        &[LIVE_CORP, DEAD_CORP],
    )
    .await;
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert!(noted(&pool, "corporation", DEAD_CORP).await);
    assert!(!noted(&pool, "corporation", LIVE_CORP).await);

    let wanted: HashSet<i64> = [LIVE_CORP, DEAD_CORP].into_iter().collect();
    assert!(
        unresolved(&pool, EntityKind::Corporation, &wanted)
            .await
            .is_empty(),
        "the live one is fresh and the dead one is noted"
    );

    ensure(
        &pool,
        &esi,
        EntityKind::Corporation,
        &[LIVE_CORP, DEAD_CORP],
    )
    .await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        2,
        "the second pass asks for neither"
    );
}

#[sqlx::test]
async fn a_stale_note_is_retried_and_dropped_once_the_id_resolves(pool: PgPool) {
    let (esi, hits) = stub_esi().await;
    sqlx::query(
        "insert into unresolvable_entities (kind, id, noted_at)
         values ('corporation', $1, now() - interval '31 days')",
    )
    .bind(LIVE_CORP)
    .execute(&pool)
    .await
    .unwrap();

    ensure(&pool, &esi, EntityKind::Corporation, &[LIVE_CORP]).await;
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "a month on, it is asked about again"
    );
    assert!(
        !noted(&pool, "corporation", LIVE_CORP).await,
        "and cleared now that it resolved"
    );
}

#[sqlx::test]
async fn other_failures_are_not_mistaken_for_a_missing_entity(pool: PgPool) {
    // Nothing listens here, so every request fails before ESI could say anything.
    let esi = EsiClient::with_config(reqwest::Client::new(), "http://127.0.0.1:9", "2025-08-26");

    ensure(&pool, &esi, EntityKind::Corporation, &[DEAD_CORP]).await;
    assert!(!noted(&pool, "corporation", DEAD_CORP).await);
}
