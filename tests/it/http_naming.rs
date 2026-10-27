//! Naming over HTTP: the map's strategy suggests aliases and names bookmarks, so every client
//! gets the same answer. A highsec system (alias `1`) is connected to a C5 (alias `11`).

use crate::common;

use common::app::{app, get, request_json, session_cookie};
use common::catalog::{C247, seed_catalog, seed_wormhole_system};
use common::map::connect;
use common::{SYS_A, SYS_B, World, world};
use serde_json::json;
use sqlx::PgPool;
use wormholesystems::maps::NamingStrategyKind;
use wormholesystems::maps::SignatureGroup;
use wormholesystems::maps::map::{NamingUpdate, UpdateMap, update_map};
use wormholesystems::maps::signatures::{
    AddSignature, LinkSignature, add_signature, link_signature,
};
use wormholesystems::maps::solar_system::{AddSystem, add_system};

const C5: i64 = 31000001;

struct Chain {
    w: World,
    cookie: String,
    jita: i64,
    c5: i64,
    hole: i64,
}

async fn place(pool: &PgPool, w: &World, system: i64, alias: &str) -> i64 {
    add_system(
        pool,
        w.owner,
        AddSystem {
            map_id: w.map_id,
            solar_system_id: system,
            x: 0.0,
            y: 0.0,
            alias: Some(alias.into()),
        },
    )
    .await
    .expect("add_system")
    .id
}

async fn chain(pool: &PgPool) -> Chain {
    let w = world(pool).await;
    seed_catalog(pool).await;
    seed_wormhole_system(pool, C5, 5).await;
    let cookie = session_cookie(pool, w.owner).await;
    let jita = place(pool, &w, SYS_A, "1").await;
    let c5 = place(pool, &w, C5, "11").await;
    let hole = connect(pool, w.owner, w.map_id, jita, c5).await.id;
    Chain {
        w,
        cookie,
        jita,
        c5,
        hole,
    }
}

#[tokio::test]
async fn suggests_the_next_alias_in_the_chain() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    // Out of `1` into highsec: `11` is taken, so `12`.
    let base = format!("/api/maps/{}/naming/alias", c.w.map_id);
    let path = format!("{base}?origin={}&target_system={SYS_B}", c.jita);
    let next = get(app(&pool), &path, Some(&c.cookie)).await;
    assert_eq!(next.body["alias"], "12");
    // A typed hole out of the C5, known only by the class it leads to.
    let path = format!("{base}?origin={}&target_class=3", c.c5);
    let deeper = get(app(&pool), &path, Some(&c.cookie)).await;
    assert_eq!(deeper.body["alias"], "111");
}

#[tokio::test]
async fn names_each_end_of_a_connection() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    let scanned = add_signature(
        &pool,
        c.w.owner,
        AddSignature {
            map_id: c.w.map_id,
            solar_system_id: SYS_A,
            signature_id: "HOM-111".into(),
            group: SignatureGroup::Wormhole,
            signature_type_id: Some(C247),
            ..Default::default()
        },
    )
    .await
    .expect("add_signature");
    link_signature(
        &pool,
        c.w.owner,
        LinkSignature {
            map_id: c.w.map_id,
            signature_pk: scanned.id,
            connection_id: c.hole,
        },
    )
    .await
    .expect("link_signature");

    let path = format!("/api/maps/{}/connections/{}/bookmarks", c.w.map_id, c.hole);
    let ends = get(app(&pool), &path, Some(&c.cookie)).await;
    // Standing in highsec, the way into the C5 carries the signature scanned there. From the
    // C5, `1` is an ancestor of `11`, so it is the way back.
    assert_eq!(
        ends.body,
        json!([
            { "node": c.jita, "text": "*1 HS" },
            { "node": c.c5, "text": "11 HOM C5" },
        ])
    );
}

#[tokio::test]
async fn names_an_unflown_hole_and_suggests_its_alias() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    let path = format!("/api/maps/{}/naming/bookmark", c.w.map_id);
    let body = json!({
        "near": c.jita,
        "suggest_alias": true,
        "signature_id": "ABC-123",
        "signature_type_id": C247,
    });
    let named = request_json(app(&pool), "POST", &path, Some(&c.cookie), body).await;
    // Nothing is known beyond what the type promises: a C3, the next child of `1`.
    assert_eq!(
        named.body,
        json!({ "text": "12 ABC C3", "suggested_alias": "12" })
    );

    // An alias given outright wins, and a null one names it without.
    for (alias, text) in [(json!("X"), "X ABC C3"), (json!(null), "ABC C3")] {
        let body = json!({
            "near": c.jita,
            "alias": alias,
            "suggest_alias": true,
            "signature_id": "ABC-123",
            "signature_type_id": C247,
        });
        let named = request_json(app(&pool), "POST", &path, Some(&c.cookie), body).await;
        assert_eq!(named.body, json!({ "text": text, "suggested_alias": null }));
    }
}

#[tokio::test]
async fn a_stranger_learns_nothing_about_a_private_map() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    let path = format!("/api/maps/{}/naming/alias?origin={}", c.w.map_id, c.jita);
    let refused = get(app(&pool), &path, None).await;
    assert_eq!(refused.status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn previews_settings_being_edited() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    let body = json!({
        "alias_scheme": "alphabetical",
        "ignored_alias": "HOME",
        "bookmark_wormhole": "{alias} [{wh}]",
        "bookmark_kspace": "",
        "bookmark_return": "",
    });
    let shown = request_json(
        app(&pool),
        "POST",
        "/api/naming/preview",
        Some(&c.cookie),
        body,
    )
    .await;
    assert_eq!(shown.body["aliases"], json!(["A", "B", "C", "AA"]));
    assert_eq!(shown.body["wormhole"], "1a [H296]");
    assert_eq!(shown.body["return"], "*1 ABC C5");
}

#[tokio::test]
async fn a_map_on_the_siblings_strategy_numbers_each_systems_holes() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    sqlx::query("update maps set naming_strategy = 'siblings' where id = $1")
        .bind(c.w.map_id)
        .execute(&pool)
        .await
        .expect("switch strategy");
    // Home is what tells the C5's parent from its children.
    sqlx::query("update map_solar_systems set is_home = true where id = $1")
        .bind(c.jita)
        .execute(&pool)
        .await
        .expect("set home");

    // The C5 (`11` from the chain days) has no holes of its own yet: its first is `1`,
    // with nothing of its parent's in front, and its parent's `1` not counted.
    let path = format!(
        "/api/maps/{}/naming/alias?origin={}&target_class=3",
        c.w.map_id, c.c5
    );
    let first = get(app(&pool), &path, Some(&c.cookie)).await;
    assert_eq!(first.body["alias"], "1");

    let body = json!({
        "alias_scheme": "numeric",
        "strategy": "siblings",
        "ignored_alias": "HOME",
        "bookmark_wormhole": "",
        "bookmark_kspace": "",
        "bookmark_return": "",
    });
    let shown = request_json(
        app(&pool),
        "POST",
        "/api/naming/preview",
        Some(&c.cookie),
        body,
    )
    .await;
    assert_eq!(shown.body["aliases"], json!(["1", "2", "3", "1"]));
}

#[tokio::test]
async fn a_claimed_alias_is_written_so_the_next_hole_takes_the_one_after() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    // Two holes scanned in highsec on a map that draws their far sides: two nameless
    // ghosts.
    update_map(
        &pool,
        c.w.owner,
        UpdateMap {
            map_id: c.w.map_id,
            ghost_unlinked_wormholes: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("update_map");
    let mut ghosts = Vec::new();
    for sig in ["AAA-111", "BBB-222"] {
        let scanned = add_signature(
            &pool,
            c.w.owner,
            AddSignature {
                map_id: c.w.map_id,
                solar_system_id: SYS_A,
                signature_id: sig.into(),
                group: SignatureGroup::Wormhole,
                ..Default::default()
            },
        )
        .await
        .expect("add_signature");
        let ghost: i64 = sqlx::query_scalar(
            "select id from map_solar_systems where raised_by_signature_id = $1",
        )
        .bind(scanned.id)
        .fetch_one(&pool)
        .await
        .expect("the scan raised a ghost");
        ghosts.push(ghost);
    }
    let (a, b) = (ghosts[0], ghosts[1]);

    let path = format!("/api/maps/{}/naming/bookmark", c.w.map_id);
    let copy = |destination: i64| {
        request_json(
            app(&pool),
            "POST",
            &path,
            Some(&c.cookie),
            json!({ "near": c.jita, "destination": destination, "suggest_alias": true,
                    "claim_alias": true, "signature_type_id": C247 }),
        )
    };
    let (first, second) = tokio::join!(copy(a), copy(b));
    let mut names = [
        first.body["suggested_alias"].as_str().unwrap().to_string(),
        second.body["suggested_alias"].as_str().unwrap().to_string(),
    ];
    names.sort();
    // At the same moment, and still one each.
    assert_eq!(names, ["12", "13"]);
    let written: Vec<Option<String>> =
        sqlx::query_scalar("select alias from map_solar_systems where id = any($1) order by alias")
            .bind(vec![a, b])
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(written, [Some("12".into()), Some("13".into())]);
}

#[tokio::test]
async fn saving_naming_without_a_strategy_keeps_the_maps_own() {
    let (pool, _db) = common::db::fresh().await;
    let c = chain(&pool).await;
    sqlx::query("update maps set naming_strategy = 'siblings' where id = $1")
        .bind(c.w.map_id)
        .execute(&pool)
        .await
        .unwrap();
    let map = update_map(
        &pool,
        c.w.owner,
        UpdateMap {
            map_id: c.w.map_id,
            // What a client from before the choice sends.
            naming: Some(NamingUpdate {
                strategy: None,
                alias_scheme: wormholesystems::maps::AliasScheme::Alphabetical,
                ignored_alias: "HOME".into(),
                bookmark_wormhole: "{alias} {sig}".into(),
                bookmark_kspace: "{alias} {sig}".into(),
                bookmark_return: "*{alias}".into(),
            }),
            ..Default::default()
        },
    )
    .await
    .expect("update_map");
    assert_eq!(map.naming.strategy, NamingStrategyKind::Siblings);
}
