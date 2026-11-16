//! How long the Win32 clipboard keeps trying to open a clipboard another
//! process holds, how long a request may wait for its turn, and how often
//! its thread may start again (ADR-0337 §2).
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

/// The longest a request waits for its turn: one read of a late renderer
/// (Win32 gives up on it after thirty seconds) and the patience of the
/// requests around it, with room to spare. Past it the caller has long
/// stopped waiting for a clipboard answer.
pub(crate) const LONGEST_QUEUED: Duration = Duration::from_secs(40);

/// How many times the clipboard thread may start its loop again within
/// [`RESTART_WINDOW`] before it stops serving.
pub(crate) const MAX_RESTARTS: usize = 4;

/// The window [`MAX_RESTARTS`] counts restarts in.
pub(crate) const RESTART_WINDOW: Duration = Duration::from_secs(10);

/// Whether a request accepted at `accepted` has waited past
/// [`LONGEST_QUEUED`] for its turn at `now`.
pub(crate) fn queued_too_long(accepted: Instant, now: Instant) -> bool {
    now.saturating_duration_since(accepted) > LONGEST_QUEUED
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

/// The restarts the clipboard thread has left: a failure that recurs at once
/// would otherwise spin a core and log without end.
#[derive(Debug)]
pub(crate) struct RestartBudget {
    /// The instants of the restarts within the last window, oldest first.
    recent: Vec<Instant>,
}

impl RestartBudget {
    pub(crate) fn new() -> Self {
        Self {
            recent: Vec::with_capacity(MAX_RESTARTS),
        }
    }

    /// The thread failed at `now`: `true` when it may start again, `false`
    /// when [`MAX_RESTARTS`] restarts already fell within [`RESTART_WINDOW`].
    pub(crate) fn restart(&mut self, now: Instant) -> bool {
        self.recent
            .retain(|at| now.saturating_duration_since(*at) < RESTART_WINDOW);
        if self.recent.len() >= MAX_RESTARTS {
            return false;
        }
        self.recent.push(now);
        true
    }
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
        let behind_a_render = accepted + Duration::from_secs(30);
        assert!(!queued_too_long(accepted, behind_a_render));
        let mut attempts = OpenAttempts::new(behind_a_render);
        assert!(
            attempts.failed(behind_a_render).is_some(),
            "a request that waited its turn still tries again"
        );
        assert!(queued_too_long(
            accepted,
            accepted + LONGEST_QUEUED + Duration::from_millis(1)
        ));
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

    /// The thread may start again a few times within the window, then not,
    /// and again once the window has passed the earlier restarts.
    #[test]
    fn restarts_are_counted_within_a_window() {
        let start = Instant::now();
        let mut budget = RestartBudget::new();
        for at in 0..MAX_RESTARTS {
            let at = u64::try_from(at).expect("few restarts");
            assert!(budget.restart(start + Duration::from_millis(at)));
        }
        assert!(
            !budget.restart(start + Duration::from_millis(10)),
            "a failure that recurs at once ends the restarts"
        );
        assert!(
            budget.restart(start + RESTART_WINDOW + Duration::from_secs(1)),
            "restarts spread over time are allowed"
        );
    }
}
