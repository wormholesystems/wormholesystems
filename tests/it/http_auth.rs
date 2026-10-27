//! The sign-in endpoints as the browser sees them.

use crate::common;

use axum::http::StatusCode;
use common::app::{app, get, session_cookie};
use common::world;
use sqlx::PgPool;

#[sqlx::test]
async fn logout_clears_the_cookie_on_the_path_it_was_set_on(pool: PgPool) {
    let w = world(&pool).await;
    let cookie = session_cookie(&pool, w.owner).await;

    let out = get(app(&pool), "/auth/logout", Some(&cookie)).await;

    assert_eq!(out.status, StatusCode::SEE_OTHER);
    let cleared = out
        .set_cookies()
        .into_iter()
        .find(|c| c.starts_with("ws_session="))
        .expect("the session cookie is cleared");
    // Without the path the browser scopes the removal to /auth and keeps the real cookie.
    assert!(cleared.contains("Path=/"), "{cleared}");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    let sessions: i64 = sqlx::query_scalar("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}
