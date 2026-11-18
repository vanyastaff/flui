//! The one schedule the platform layer backs off on: a wait that doubles with
//! each attempt in a row that came to nothing, from a first wait up to a
//! ceiling, and starts over once an attempt succeeds.
//!
//! Each caller arms the wait as a deadline of its loop, never a poll: the UI
//! runtime's retry of a frame the surface refused or that failed (the first
//! wait one display period, the ceiling `FAILURE_RETRY_CAP`), the Wayland
//! host's wait on a frame callback the compositor does not answer
//! (`CALLBACK_GRACE` to `MAX_CALLBACK_GRACE`), and the Linux settings watch's
//! retries of a portal read that can heal and of a session bus that does not
//! answer, which it counts so a failure that never heals costs a bounded
//! number of attempts (`READ_RETRIES`, `CONNECT_RETRIES`). The other hosts
//! pace their frames by a signal that always comes (a blocking present,
//! AppKit's display pass, a display link, the choreographer, the browser's
//! animation frame) and wait on nothing a surface can withhold.

use std::time::Duration;

/// A doubling wait from `first` up to `ceiling`.
///
/// ```
/// use std::time::Duration;
/// use flui_platform::Backoff;
///
/// let backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(8));
/// let waits: Vec<_> = (0..6).map(|misses| backoff.delay(misses).as_secs()).collect();
/// assert_eq!(waits, [1, 2, 4, 8, 8, 8]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Backoff {
    first: Duration,
    ceiling: Duration,
}

impl Backoff {
    /// The schedule that waits `first` after no miss and doubles per miss in
    /// a row, never past `ceiling`; a `ceiling` below `first` caps every wait
    /// at `ceiling`.
    #[must_use]
    pub const fn new(first: Duration, ceiling: Duration) -> Self {
        Self { first, ceiling }
    }

    /// The wait after `misses` attempts in a row came to nothing: `first`
    /// doubled `misses` times, at most the ceiling. Any count, however large,
    /// answers the ceiling without overflow.
    #[must_use]
    pub fn delay(self, misses: u32) -> Duration {
        let factor = 1_u32.checked_shl(misses).unwrap_or(u32::MAX);
        self.first
            .checked_mul(factor)
            .map_or(self.ceiling, |wait| wait.min(self.ceiling))
    }
}
