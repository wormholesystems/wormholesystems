//! Naming the structures pilots dock in. Public ones are named by [`crate::structures`];
//! a private one is visible only to characters on its docking list, so the one asked is the
//! pilot docked there, being inside is the proof of access. A structure nobody with the
//! permission has docked in stays unnamed, and the pilots card says so.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long to leave a docked pilot's structure alone, by why it was last left.
pub mod wait {
    use std::time::Duration;

    /// Named, or fresh enough: only checked again in case it goes stale.
    pub const NAMED: Duration = Duration::from_hours(1);
    /// The pilot has not granted the permission. As often as the token cache rechecks a
    /// missing scope, so granting it names the structure within a minute.
    pub const NO_PERMISSION: Duration = Duration::from_secs(60);
    /// ESI refused (403, not on the docking list) or does not know it (404).
    pub const REFUSED: Duration = Duration::from_hours(24);
    /// Anything else: ESI or the database had a bad minute.
    pub const FAILED: Duration = Duration::from_mins(5);
}

/// Which (structure, pilot) pairs are not worth asking about yet. Keyed by pilot too, since
/// a structure one pilot cannot see may be visible to the next one who docks.
#[derive(Default, Debug)]
pub struct Backoff {
    until: Mutex<HashMap<(i64, i64), Instant>>,
}

impl Backoff {
    pub fn due(&self, structure_id: i64, character_id: i64) -> bool {
        let until = self.until.lock().expect("structure backoff lock");
        until
            .get(&(structure_id, character_id))
            .is_none_or(|t| *t <= Instant::now())
    }

    pub fn wait(&self, structure_id: i64, character_id: i64, wait: Duration) {
        let now = Instant::now();
        let mut until = self.until.lock().expect("structure backoff lock");
        until.retain(|_, t| *t > now);
        until.insert((structure_id, character_id), now + wait);
    }
}
