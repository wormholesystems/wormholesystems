//! Which characters a desktop app is mapping jumps for right now.
//!
//! The desktop app reads the game's own logs, so it sees a jump the moment it happens; the
//! website only sees it on the next ESI poll. With both open, both map the same jump and race
//! each other. So the desktop app says which characters it is watching, every
//! [`HEARTBEAT`], and the website leaves those characters' jumps to it.
//!
//! Kept in memory: a restart forgets every lease, and the next heartbeat brings it back. Until
//! then the website maps those jumps itself, which is how it worked before.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;
use tokio::time::Instant;

use crate::realtime::{UserEvent, UserHub};

/// How often the desktop app says it is still there.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// How long one heartbeat holds a character: long enough to miss one, short enough that a
/// desktop app that crashed hands its characters back soon.
const LEASE: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy)]
struct Lease {
    user_id: i64,
    map_id: i64,
    until: Instant,
}

static LEASES: LazyLock<Mutex<HashMap<i64, Lease>>> = LazyLock::new(Mutex::default);

/// What the desktop app is doing, sent every [`HEARTBEAT`] and once more when it stops.
#[derive(Debug, Clone, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct DesktopHeartbeat {
    /// The map it maps jumps onto.
    pub map_id: i64,
    /// The characters it is watching, by name: the game's logs know no ids. Names that are
    /// not on the signed-in account are ignored.
    pub characters: Vec<String>,
    /// `false` when it stops watching them: quit, signed out, tracking turned off.
    pub active: bool,
}

/// The map a desktop app is mapping this character's jumps onto, if one is.
pub fn mapping_on(character_id: i64) -> Option<i64> {
    let leases = LEASES.lock().expect("desktop leases poisoned");
    leases
        .get(&character_id)
        .filter(|lease| lease.until > Instant::now())
        .map(|lease| lease.map_id)
}

/// Take or give back the characters a heartbeat names. The user's open tabs hear about every
/// character that changed hands, and again when a lease lapses without a word.
pub async fn heartbeat(
    pool: &PgPool,
    users: &UserHub,
    user_id: i64,
    beat: DesktopHeartbeat,
) -> Result<(), sqlx::Error> {
    let names: Vec<String> = beat.characters.iter().map(|n| n.to_lowercase()).collect();
    let characters = sqlx::query_scalar!(
        "select id from characters where user_id = $1 and lower(name) = any($2)",
        user_id,
        &names,
    )
    .fetch_all(pool)
    .await?;

    let now = Instant::now();
    let mut changed = Vec::new();
    {
        let mut leases = LEASES.lock().expect("desktop leases poisoned");
        for &character_id in &characters {
            let held = leases
                .get(&character_id)
                .filter(|lease| lease.until > now)
                .map(|lease| lease.map_id);
            if beat.active {
                leases.insert(
                    character_id,
                    Lease {
                        user_id,
                        map_id: beat.map_id,
                        until: now + LEASE,
                    },
                );
                if held != Some(beat.map_id) {
                    changed.push(character_id);
                }
            } else if leases.remove(&character_id).is_some() && held.is_some() {
                changed.push(character_id);
            }
        }
    }
    for &character_id in &changed {
        users.publish(user_id, UserEvent::CharacterStatusChanged { character_id });
    }
    if beat.active {
        for character_id in characters {
            tokio::spawn(lapse(users.clone(), character_id, now + LEASE));
        }
    }
    Ok(())
}

/// Once a lease has run out with no heartbeat to renew it, drop it and tell its user, so
/// the website takes the character's jumps back.
async fn lapse(users: UserHub, character_id: i64, until: Instant) {
    tokio::time::sleep_until(until).await;
    let lapsed = {
        let mut leases = LEASES.lock().expect("desktop leases poisoned");
        match leases.get(&character_id) {
            Some(lease) if lease.until <= until => leases.remove(&character_id),
            _ => None,
        }
    };
    if let Some(lease) = lapsed {
        users.publish(
            lease.user_id,
            UserEvent::CharacterStatusChanged { character_id },
        );
    }
}
