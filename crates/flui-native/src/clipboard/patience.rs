//! How long the Win32 clipboard keeps trying to open a clipboard another
//! process holds, and how long a request may wait for its turn
//! (ADR-0337 §2).
//!
//! `OpenClipboard` fails while any other window holds the clipboard open,
//! and clipboard managers, remote-desktop clients (`rdpclip.exe`) and
//! clipboard history open it right after every change to read what was
//! written. Win32 offers no wait for the clipboard to close, so a request
//! tries again on a schedule: first after [`FIRST_DELAY`], each delay twice
//! the one before up to [`LONGEST_DELAY`], the last attempt at the moment its
//! [`PATIENCE`] runs out, counted from when the request's turn came, so a
//! request queued behind a slow one keeps its full patience. Its time in the
//! queue has a bound of its own, [`LONGEST_QUEUED`]: a request still waiting
//! for its turn past it is refused `Timeout` without an attempt.
//!
//! The schedule is pure: the clipboard's thread hands it the clock, and a
//! test drives it with instants of its own.

#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

/// How long a request keeps trying, from its turn. Chromium tries five
/// times 5 ms apart and Firefox three times 3 ms apart, both on their UI
/// thread, where a longer wait would freeze the window; off the owner thread
/// a copy can wait for a remote-desktop client that reads it over a slow
/// link.
pub(crate) const PATIENCE: Duration = Duration::from_millis(500);

/// The delay after the first failed attempt.
pub(crate) const FIRST_DELAY: Duration = Duration::from_millis(4);

/// The longest delay between two attempts.
pub(crate) const LONGEST_DELAY: Duration = Duration::from_millis(64);

/// The longest a request waits for its turn before it is refused
/// `Timeout`. It bounds the wait for a request's turn, not its answer: a
/// request whose turn comes just inside it still makes its attempts and its
/// own call to Win32, which a late or hung owner can keep thirty seconds,
/// so a request answers within `LONGEST_QUEUED`, [`PATIENCE`] and thirty
/// seconds at worst.
pub(crate) const LONGEST_QUEUED: Duration = Duration::from_secs(40);

/// A request waiting for its turn, since it was accepted: the only state
/// [`LONGEST_QUEUED`] is measured in. Its turn consumes it, so a request
/// attempting to open the clipboard is never measured against the bound
/// again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Waiting {
    accepted: Instant,
}

impl Waiting {
    /// A request accepted at `accepted`.
    pub(crate) const fn since(accepted: Instant) -> Self {
        Self { accepted }
    }

    /// The request's turn came at `now`: its attempts, whose patience counts
    /// from `now`, or `None` when it waited past [`LONGEST_QUEUED`] and is
    /// refused `Timeout` without an attempt.
    pub(crate) fn turn(self, now: Instant) -> Option<OpenAttempts> {
        (now.saturating_duration_since(self.accepted) <= LONGEST_QUEUED)
            .then(|| OpenAttempts::new(now))
    }
}

/// One request's attempts to open the clipboard.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenAttempts {
    /// When the request's patience runs out; `None` past what `Instant` can
    /// hold (never, on a real clock).
    deadline: Option<Instant>,
    failures: u32,
}

impl OpenAttempts {
    /// The attempts of a request whose turn came at `turn`.
    pub(crate) fn new(turn: Instant) -> Self {
        Self {
            deadline: turn.checked_add(PATIENCE),
            failures: 0,
        }
    }

    /// The attempt made at `now` failed: when to try again, or `None` when
    /// the request's patience has run out and it is refused.
    pub(crate) fn failed(&mut self, now: Instant) -> Option<Instant> {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return None;
        }
        let delay = delay_after(self.failures);
        self.failures = self.failures.saturating_add(1);
        let next = now.checked_add(delay)?;
        Some(self.deadline.map_or(next, |deadline| next.min(deadline)))
    }
}

/// The delay after `failures` earlier failures and this one: doubling from
/// [`FIRST_DELAY`], never past [`LONGEST_DELAY`].
fn delay_after(failures: u32) -> Duration {
    1_u32
        .checked_shl(failures)
        .and_then(|factor| FIRST_DELAY.checked_mul(factor))
        .map_or(LONGEST_DELAY, |delay| delay.min(LONGEST_DELAY))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The attempts a request makes against a clipboard held for good: the
    /// instants after its turn, in milliseconds.
    fn schedule() -> Vec<u128> {
        let turn = Instant::now();
        let mut attempts = OpenAttempts::new(turn);
        let mut now = turn;
        let mut tried = vec![0];
        while let Some(next) = attempts.failed(now) {
            assert!(next > now, "every retry is later than the attempt before");
            now = next;
            tried.push(now.duration_since(turn).as_millis());
        }
        tried
    }

    /// The delays double from the first to the longest, and the last attempt
    /// is the one at the patience's end, after which the request is refused.
    #[test]
    fn a_held_clipboard_is_tried_on_a_doubling_schedule_until_the_patience_ends() {
        let tried = schedule();
        assert_eq!(&tried[..6], &[0, 4, 12, 28, 60, 124]);
        assert_eq!(tried.last(), Some(&PATIENCE.as_millis()));
        assert!(
            tried
                .windows(2)
                .all(|pair| pair[1] - pair[0] <= LONGEST_DELAY.as_millis()),
            "no wait is longer than the longest delay: {tried:?}"
        );
        assert_eq!(tried.len(), 12, "{tried:?}");
    }

    /// A request's time in the queue is bounded apart from its patience: a
    /// request that waited behind a late renderer keeps its patience, and
    /// one that waited past the queue's bound is refused.
    #[test]
    fn the_queue_has_a_bound_of_its_own() {
        let accepted = Instant::now();
        let waiting = Waiting::since(accepted);
        let behind_a_render = accepted + LONGEST_QUEUED;
        let mut attempts = waiting
            .turn(behind_a_render)
            .expect("a turn inside the bound makes its attempts");
        assert_eq!(
            attempts.failed(behind_a_render + PATIENCE / 2),
            Some(behind_a_render + PATIENCE / 2 + FIRST_DELAY),
            "a request whose turn came keeps its patience past the bound"
        );
        assert!(
            waiting
                .turn(accepted + LONGEST_QUEUED + Duration::from_millis(1))
                .is_none(),
            "a turn past the bound is refused"
        );
    }

    /// The delay saturates at the longest however many attempts failed,
    /// with no overflow in the doubling.
    #[test]
    fn the_delay_saturates_without_overflow() {
        for failures in [5, 31, 32, 64, u32::MAX] {
            assert_eq!(delay_after(failures), LONGEST_DELAY, "{failures}");
        }
        assert_eq!(delay_after(0), FIRST_DELAY);
    }
}
