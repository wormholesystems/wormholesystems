//! Which map snapshot is the newest. Refreshes run in the background and can come back in
//! any order, so each is numbered as it starts and only one newer than what is shown gets
//! shown. A change (a paste, a jump, an edit) counts as newer than every refresh started
//! before it is done: those may have read the map before the change landed.

#[derive(Debug, Default)]
pub(crate) struct Freshness {
    started: u64,
    shown: u64,
}

impl Freshness {
    /// Number a refresh as it starts.
    pub(crate) fn start(&mut self) -> u64 {
        self.started += 1;
        self.started
    }

    /// Whether the reply to refresh `n` may be shown, and if so, that it is.
    pub(crate) fn show(&mut self, n: u64) -> bool {
        if n <= self.shown {
            return false;
        }
        self.shown = n;
        true
    }

    /// The map changed, or what it shows now came after every refresh started so far.
    pub(crate) fn changed(&mut self) {
        self.shown = self.start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_reply_is_dropped() {
        let mut fresh = Freshness::default();
        let first = fresh.start();
        let second = fresh.start();
        assert!(fresh.show(second));
        assert!(!fresh.show(first));
    }

    #[test]
    fn replies_in_order_are_all_shown() {
        let mut fresh = Freshness::default();
        let first = fresh.start();
        let second = fresh.start();
        assert!(fresh.show(first));
        assert!(fresh.show(second));
    }

    #[test]
    fn a_change_outranks_refreshes_started_before_it() {
        let mut fresh = Freshness::default();
        let before = fresh.start();
        fresh.changed();
        assert!(!fresh.show(before));
        let after = fresh.start();
        assert!(fresh.show(after));
    }
}
