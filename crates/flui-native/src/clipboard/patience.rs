//! How long the Win32 clipboard keeps trying to open a clipboard another
//! process holds, and how often (ADR-0337 §2).
//!
//! `OpenClipboard` fails while any other window holds the clipboard open,
//! and clipboard managers, remote-desktop clients (`rdpclip.exe`) and
//! clipboard history open it right after every change to read what was
//! written. Win32 offers no wait for the clipboard to close, so a request
//! tries again on a schedule: first after [`FIRST_DELAY`], each delay twice
//! the one before up to [`LONGEST_DELAY`], the last attempt at the moment its
//! [`PATIENCE`] runs out, counted from when the request was accepted, so a
//! request queued behind others waits no longer in all. A request that
//! reaches its turn after its patience ran out still gets one attempt.
//!
//! The schedule is pure: the clipboard's thread hands it the clock, and a
//! test drives it with instants of its own.

#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

/// How long a request keeps trying, from its acceptance. Chromium tries five
/// times 5 ms apart and Firefox three times 3 ms apart, both on their UI
/// thread, where a longer wait would freeze the window; off the owner thread
/// a copy can wait for a remote-desktop client that reads it over a slow
/// link.
pub(crate) const PATIENCE: Duration = Duration::from_millis(500);

/// The delay after the first failed attempt.
pub(crate) const FIRST_DELAY: Duration = Duration::from_millis(4);

/// The longest delay between two attempts.
pub(crate) const LONGEST_DELAY: Duration = Duration::from_millis(64);

/// One request's attempts to open the clipboard.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenAttempts {
    /// When the request was accepted; `None` once its patience runs out
    /// past what `Instant` can hold, which leaves it unbounded by nothing
    /// but the attempts themselves (never, on a real clock).
    deadline: Option<Instant>,
    failures: u32,
}

impl OpenAttempts {
    /// The attempts of a request accepted at `accepted`.
    pub(crate) fn new(accepted: Instant) -> Self {
        Self {
            deadline: accepted.checked_add(PATIENCE),
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
    /// instants after its acceptance, in milliseconds.
    fn schedule() -> Vec<u128> {
        let accepted = Instant::now();
        let mut attempts = OpenAttempts::new(accepted);
        let mut now = accepted;
        let mut tried = vec![0];
        while let Some(next) = attempts.failed(now) {
            assert!(next > now, "every retry is later than the attempt before");
            now = next;
            tried.push(now.duration_since(accepted).as_millis());
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

    /// A request that reaches its turn after its patience ran out (it
    /// waited behind a slow read) still makes its one attempt, then is
    /// refused at once.
    #[test]
    fn a_request_past_its_patience_makes_one_attempt() {
        let accepted = Instant::now();
        let mut attempts = OpenAttempts::new(accepted);
        let late = accepted + PATIENCE + Duration::from_secs(30);
        assert_eq!(attempts.failed(late), None);
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
