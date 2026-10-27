//! Seeded reference data over HTTP: revalidated by ETag, so a re-seed reaches browsers on
//! their next load instead of a day later.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::app::{app, get, send};
use common::catalog::seed_catalog;
use sqlx::PgPool;

#[sqlx::test]
async fn the_signature_catalog_revalidates_by_etag(pool: PgPool) {
    seed_catalog(&pool).await;

    let first = get(app(&pool), "/api/signature-types", None).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.headers[header::CACHE_CONTROL], "public, no-cache");
    let etag = first.headers[header::ETAG].clone();

    let revalidate = |tag: &str| {
        Request::builder()
            .uri("/api/signature-types")
            .header(header::IF_NONE_MATCH, tag)
            .body(Body::empty())
            .expect("build request")
    };
    let unchanged = send(app(&pool), revalidate(etag.to_str().unwrap())).await;
    assert_eq!(unchanged.status, StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.headers[header::ETAG], etag);

    let stale = send(app(&pool), revalidate("\"something-older\"")).await;
    assert_eq!(stale.status, StatusCode::OK);
    assert_eq!(stale.body, first.body);
}
