//! Which ESI permissions each of a user's characters is missing, for the warning beside the
//! portrait in the navigation bar.

use std::time::{Duration, SystemTime};

use sqlx::PgPool;
use wormholesystems::db::PgTokenStore;
use wormholesystems::esi::jwt::Claims;
use wormholesystems::esi::{Scope, Token, TokenStore};
use wormholesystems::identity::characters::list;
use wormholesystems::maps::Actor;
use wormholesystems::session::{Entity, persist_identity};

fn claims(character_id: i64, owner_hash: &str) -> Claims {
    Claims {
        character_id,
        name: format!("Pilot {character_id}"),
        owner_hash: owner_hash.into(),
        scopes: vec![],
    }
}

fn corp() -> Entity {
    Entity {
        id: 2001,
        name: "Corp".into(),
        ticker: "CRP".into(),
    }
}

fn token(scopes: &[Scope]) -> Token {
    Token {
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        expires_at: SystemTime::now() + Duration::from_mins(20),
        scopes: scopes.iter().map(|s| s.as_str().to_string()).collect(),
    }
}

async fn missing(pool: &PgPool, actor: Actor, character_id: i64) -> Vec<String> {
    list(pool, actor)
        .await
        .expect("list characters")
        .into_iter()
        .find(|c| c.character_id == character_id)
        .expect("character is listed")
        .missing_scopes
}

#[sqlx::test]
async fn each_character_says_which_permissions_it_has_not_granted(pool: PgPool) {
    let user = persist_identity(&pool, &claims(100, "h1"), corp(), None, None)
        .await
        .unwrap();
    persist_identity(&pool, &claims(200, "h2"), corp(), None, Some(user))
        .await
        .unwrap();
    persist_identity(&pool, &claims(300, "h3"), corp(), None, Some(user))
        .await
        .unwrap();
    let actor = Actor {
        user_id: user,
        character_id: 100,
    };
    let store = PgTokenStore::new(pool.clone());

    // Everything granted.
    store.save(100, &token(&Scope::ALL)).await.unwrap();
    // Location unticked at EVE's consent screen.
    store
        .save(200, &token(&[Scope::ReadOnline, Scope::WriteWaypoint]))
        .await
        .unwrap();
    // 300 has no token at all, so it has granted nothing.

    assert_eq!(missing(&pool, actor, 100).await, Vec::<String>::new());
    assert_eq!(
        missing(&pool, actor, 200).await,
        [
            "esi-location.read_location.v1",
            "esi-location.read_ship_type.v1"
        ]
    );
    assert_eq!(
        missing(&pool, actor, 300).await.len(),
        Scope::REQUIRED.len()
    );

    // Granting the rest clears it.
    store.save(200, &token(&Scope::ALL)).await.unwrap();
    assert_eq!(missing(&pool, actor, 200).await, Vec::<String>::new());

    // Optional permissions are never reported missing.
    store.save(300, &token(&Scope::REQUIRED)).await.unwrap();
    assert_eq!(missing(&pool, actor, 300).await, Vec::<String>::new());
}
