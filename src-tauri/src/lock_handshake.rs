//! Lock handshake (§6.2): Rust asks the UI to flush, then waits for
//! the ack or a cap before the keys drop. Separate from AppState so
//! it can be tested without a vault.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Manual and idle locks: a live UI acks in milliseconds; a dead one
/// must never veto locking.
pub const LOCK_ACK_WAIT: Duration = Duration::from_secs(5);
/// Screen lock and similar OS signals: the unlocked window matters
/// more, so wait less.
pub const LOCK_ACK_WAIT_OS: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Shared {
    generation: u64,
    pending: Option<u64>,
    last_ack: Option<(u64, bool)>,
    /// Content-free: "the last lock dropped unsaved edits".
    notice: bool,
}

#[derive(Default)]
pub struct LockHandshake {
    shared: Mutex<Shared>,
    cv: Condvar,
}

impl LockHandshake {
    fn guard(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start a handshake. None if one is already pending.
    pub fn begin(&self) -> Option<u64> {
        let mut s = self.guard();
        if s.pending.is_some() {
            return None;
        }
        s.generation += 1;
        s.pending = Some(s.generation);
        Some(s.generation)
    }

    pub fn pending(&self) -> Option<u64> {
        self.guard().pending
    }

    /// Only the ack for the pending generation counts; stale acks
    /// are ignored. `ok` is false when the UI could not flush.
    pub fn acknowledge(&self, generation: u64, ok: bool) -> bool {
        let mut s = self.guard();
        if s.pending != Some(generation) {
            return false;
        }
        s.pending = None;
        s.last_ack = Some((generation, ok));
        self.cv.notify_all();
        true
    }

    /// A direct lock supersedes any pending handshake.
    pub fn cancel(&self) {
        self.guard().pending = None;
        self.cv.notify_all();
    }

    /// Wait for the ack. Returns the UI's `ok`; a timeout or a
    /// cancel counts as ok (a dead UI must not veto, and there is
    /// no failure to report). A timed-out handshake is cleared so
    /// a late ack cannot touch a later wait.
    pub fn wait(&self, generation: u64, cap: Duration) -> bool {
        let deadline = Instant::now() + cap;
        let mut s = self.guard();
        while s.pending == Some(generation) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            s = self
                .cv
                .wait_timeout(s, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if s.pending == Some(generation) {
            s.pending = None;
            self.cv.notify_all();
        }
        match s.last_ack {
            Some((g, ok)) if g == generation => ok,
            _ => true,
        }
    }

    pub fn flag_notice(&self) {
        self.guard().notice = true;
    }

    pub fn take_notice(&self) -> bool {
        std::mem::take(&mut self.guard().notice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_begin_is_refused_until_acked() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        assert!(hs.begin().is_none());
        assert_eq!(hs.pending(), Some(g));
        assert!(hs.acknowledge(g, true));
        assert_ne!(hs.begin().unwrap(), g);
    }

    #[test]
    fn stale_ack_is_ignored() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        assert!(!hs.acknowledge(g + 99, true));
        assert_eq!(hs.pending(), Some(g));
    }

    #[test]
    fn ack_wakes_waiter_with_the_uis_verdict() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        std::thread::scope(|s| {
            let h = &hs;
            let waiter = s.spawn(move || h.wait(g, Duration::from_secs(5)));
            std::thread::sleep(Duration::from_millis(50));
            assert!(hs.acknowledge(g, false));
            assert!(!waiter.join().unwrap());
        });
    }

    #[test]
    fn joiner_gets_the_same_verdict() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        assert!(hs.begin().is_none());
        let joined = hs.pending().unwrap();
        std::thread::scope(|s| {
            let h = &hs;
            let a = s.spawn(move || h.wait(joined, Duration::from_secs(5)));
            let b = s.spawn(move || h.wait(joined, Duration::from_secs(5)));
            std::thread::sleep(Duration::from_millis(50));
            hs.acknowledge(g, false);
            assert!(!a.join().unwrap());
            assert!(!b.join().unwrap());
        });
    }

    #[test]
    fn timeout_counts_as_ok_and_clears_the_handshake() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        assert!(hs.wait(g, Duration::from_millis(50)));
        assert_eq!(hs.pending(), None);
        assert!(!hs.acknowledge(g, false), "late ack must be ignored");
    }

    #[test]
    fn cancel_wakes_waiter_and_clears() {
        let hs = LockHandshake::default();
        let g = hs.begin().unwrap();
        std::thread::scope(|s| {
            let h = &hs;
            let waiter = s.spawn(move || h.wait(g, Duration::from_secs(5)));
            std::thread::sleep(Duration::from_millis(50));
            hs.cancel();
            assert!(waiter.join().unwrap());
        });
        assert_eq!(hs.pending(), None);
    }

    #[test]
    fn notice_is_taken_once() {
        let hs = LockHandshake::default();
        assert!(!hs.take_notice());
        hs.flag_notice();
        assert!(hs.take_notice());
        assert!(!hs.take_notice());
    }
}
