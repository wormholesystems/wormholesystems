//! Access tokens held in memory between polls.
//!
//! The tracking poller wants a character's token every five seconds, and a token lives
//! twenty minutes, so loading it from the store each time is nearly always a wasted read.
//! [`TokenCache`] keeps the last one per character and only goes back to the store when it
//! is about to expire (which is also when it gets refreshed), or, now and then, when it
//! lacks a scope the caller wants, so a scope granted on a later login is picked up.
//!
//! Each character's slot is locked while it is read or refreshed. Two users of the same
//! character's token (the two poller tiers, a waypoint, the structure sync) therefore never
//! refresh it side by side, which spent two refreshes on one token and raced their saves.
//! That only holds while they all go through the one cache, which is why it lives in
//! [`AppState`](crate::app::AppState).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::Result;
use super::scopes::Scope;
use super::sso::{REFRESH_LEEWAY, Sso};
use super::token::{Token, TokenStore};

/// How often a cached token missing a wanted scope is checked against the store again.
const SCOPE_RECHECK: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct TokenCache {
    slots: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<Option<Cached>>>>>,
}

struct Cached {
    token: Token,
    loaded_at: Instant,
}

// By hand, so the tokens it holds stay out of logs.
impl std::fmt::Debug for TokenCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenCache").finish_non_exhaustive()
    }
}

impl TokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// `character_id`'s token, valid for at least [`REFRESH_LEEWAY`]. `wanted` are the
    /// scopes the caller is about to check for; it does not fail when one is missing, it
    /// only decides whether a cached token without it is worth checking again.
    pub async fn token<S: TokenStore>(
        &self,
        sso: &Sso,
        store: &S,
        character_id: i64,
        wanted: &[Scope],
    ) -> Result<Token> {
        let slot = Arc::clone(
            self.slots
                .lock()
                .expect("token cache lock")
                .entry(character_id)
                .or_default(),
        );
        let mut slot = slot.lock().await;
        if let Some(cached) = slot.as_ref() {
            let expiring = cached.token.expires_within(REFRESH_LEEWAY);
            let lacking = wanted.iter().any(|s| !cached.token.has_scope(*s))
                && cached.loaded_at.elapsed() >= SCOPE_RECHECK;
            if !expiring && !lacking {
                return Ok(cached.token.clone());
            }
        }
        // Back to the store, which also picks up a token somebody else refreshed or a new
        // login replaced since we cached ours.
        let token = sso.stored_token(store, character_id).await?;
        *slot = Some(Cached {
            token: token.clone(),
            loaded_at: Instant::now(),
        });
        Ok(token)
    }

    /// Drop `character_id`'s cached token, so the next caller loads the one a fresh login
    /// just stored instead of carrying on with the old one.
    pub async fn forget(&self, character_id: i64) {
        let slot = self
            .slots
            .lock()
            .expect("token cache lock")
            .get(&character_id)
            .cloned();
        if let Some(slot) = slot {
            *slot.lock().await = None;
        }
    }

    /// Forget characters no longer being polled. A slot somebody holds right now stays, so
    /// a second caller cannot get a fresh one and refresh beside them.
    pub fn retain(&self, keep: impl Fn(i64) -> bool) {
        self.slots
            .lock()
            .expect("token cache lock")
            .retain(|id, slot| keep(*id) || Arc::strong_count(slot) > 1);
    }
}
