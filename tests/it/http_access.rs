//! HTTP-level coverage for the auth surfaces nothing exercised before: the session gate,
//! the body-vs-URL map id guard, the role gate, and the share-token read path.

use crate::common;

use axum::http::StatusCode;
use common::app::{app, get, request_json, session_cookie};
use common::{add_character, new_user, world};
use serde_json::json;
use wormholesystems::maps::Actor;

#[tokio::test]
async fn no_session_is_401_and_a_session_is_not() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;

    let anonymous = get(app(&pool), "/api/maps", None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let cookie = session_cookie(&pool, w.owner).await;
    let signed_in = get(app(&pool), "/api/maps", Some(&cookie)).await;
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_eq!(signed_in.body.as_array().map(std::vec::Vec::len), Some(1));
}

#[tokio::test]
async fn a_body_map_id_must_agree_with_the_url() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let cookie = session_cookie(&pool, w.owner).await;

    let mismatched = request_json(
        app(&pool),
        "POST",
        &format!("/api/maps/{}/systems/add", w.map_id),
        Some(&cookie),
        json!({ "map_id": w.map_id + 1, "solar_system_id": common::SYS_A, "x": 100.0, "y": 100.0, "alias": null }),
    )
    .await;
    assert_eq!(mismatched.status, StatusCode::BAD_REQUEST);

    let matched = request_json(
        app(&pool),
        "POST",
        &format!("/api/maps/{}/systems/add", w.map_id),
        Some(&cookie),
        json!({ "map_id": w.map_id, "solar_system_id": common::SYS_A, "x": 100.0, "y": 100.0, "alias": null }),
    )
    .await;
    assert_eq!(matched.status, StatusCode::OK);
}

#[tokio::test]
async fn an_outsider_gets_404_on_a_private_map_and_a_share_token_opens_it() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;

    let outsider_user = new_user(&pool).await;
    add_character(&pool, outsider_user, 1002, 2002, None).await;
    let outsider = Actor {
        user_id: outsider_user,
        character_id: 1002,
    };
    let cookie = session_cookie(&pool, outsider).await;

    // Not 403: a map you cannot see does not admit to existing.
    let path = format!("/api/maps/{}", w.map_id);
    let refused = get(app(&pool), &path, Some(&cookie)).await;
    assert_eq!(refused.status, StatusCode::NOT_FOUND);

    // A minted share token opens the same map read-only, session or not.
    let token = wormholesystems::maps::map::rotate_share_token(&pool, w.owner, w.map_id)
        .await
        .unwrap();
    let shared = get(app(&pool), &format!("{path}?share={token}"), None).await;
    assert_eq!(shared.status, StatusCode::OK);
    assert_eq!(shared.body["role"], "viewer");

    // The share cookie the share route leaves behind works the same way.
    let via_cookie = get(
        app(&pool),
        &path,
        Some(&format!("map_share_{}={}", w.map_id, token)),
    )
    .await;
    assert_eq!(via_cookie.status, StatusCode::OK);

    // A withdrawn token stops opening it.
    wormholesystems::maps::map::revoke_share_token(&pool, w.owner, w.map_id)
        .await
        .unwrap();
    let stale = get(app(&pool), &format!("{path}?share={token}"), None).await;
    assert_eq!(stale.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn alerts_are_manager_gated() {
    let (pool, _db) = common::db::fresh().await;
    let w = world(&pool).await;
    let member = common::member_with_role(
        &pool,
        w.owner,
        w.map_id,
        1003,
        2003,
        wormholesystems::maps::Role::Member,
    )
    .await;
    let member_cookie = session_cookie(&pool, member).await;
    let owner_cookie = session_cookie(&pool, w.owner).await;

    let path = format!("/api/maps/{}/alerts", w.map_id);
    let refused = get(app(&pool), &path, Some(&member_cookie)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let allowed = get(app(&pool), &path, Some(&owner_cookie)).await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert_eq!(allowed.body.as_array().map(std::vec::Vec::len), Some(0));
}
