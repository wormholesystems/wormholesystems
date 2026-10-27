//! The import routes over HTTP: who is turned away before the file is read, and a file that
//! makes it through.

use crate::common;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use common::app::{app, get, request_json, send, session_cookie};
use common::{member_with_role, world};
use serde_json::json;
use sqlx::PgPool;
use wormholesystems::maps::Role;

/// A request whose body fails the moment anything reads it. A route that answers it with
/// anything but a body error never touched the body.
fn unreadable(path: &str, cookie: Option<&str>) -> Request<Body> {
    let body = Body::from_stream(futures_util::stream::once(async {
        Err::<Bytes, _>(std::io::Error::other("the body was read"))
    }));
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder.body(body).expect("build request")
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stranger_is_turned_away_before_the_file_is_read(pool: PgPool) {
    let w = world(&pool).await;
    let import = format!("/api/maps/{}/transfer/import", w.map_id);

    let anonymous = send(app(&pool), unreadable(&import, None)).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let anonymous_new = send(
        app(&pool),
        unreadable("/api/maps/transfer/import-new", None),
    )
    .await;
    assert_eq!(anonymous_new.status, StatusCode::UNAUTHORIZED);

    let member = member_with_role(&pool, w.owner, w.map_id, 1003, 2003, Role::Member).await;
    let cookie = session_cookie(&pool, member).await;
    let not_a_manager = send(app(&pool), unreadable(&import, Some(&cookie))).await;
    assert_eq!(not_a_manager.status, StatusCode::FORBIDDEN);

    // The owner gets as far as reading it.
    let cookie = session_cookie(&pool, w.owner).await;
    let owner = send(app(&pool), unreadable(&import, Some(&cookie))).await;
    assert_eq!(owner.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_exported_file_imports_back(pool: PgPool) {
    let w = world(&pool).await;
    let cookie = session_cookie(&pool, w.owner).await;

    let export = get(
        app(&pool),
        &format!(
            "/api/maps/{}/transfer/export?sections=settings,routes",
            w.map_id
        ),
        Some(&cookie),
    )
    .await;
    assert_eq!(export.status, StatusCode::OK);
    let content = export.body.to_string();

    let merged = request_json(
        app(&pool),
        "POST",
        &format!("/api/maps/{}/transfer/import", w.map_id),
        Some(&cookie),
        json!({ "sections": ["routes"], "content": content }),
    )
    .await;
    assert_eq!(merged.status, StatusCode::OK, "{}", merged.body);

    let created = request_json(
        app(&pool),
        "POST",
        "/api/maps/transfer/import-new",
        Some(&cookie),
        json!({ "name": "Copy", "sections": ["settings", "routes"], "content": content }),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.body);
    assert_eq!(created.body["name"], "Copy");

    let bad_file = request_json(
        app(&pool),
        "POST",
        "/api/maps/transfer/import-new",
        Some(&cookie),
        json!({ "sections": ["settings"], "content": "not a map" }),
    )
    .await;
    assert_eq!(bad_file.status, StatusCode::BAD_REQUEST);
}
