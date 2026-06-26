//! Monotonic time source for deadline-driven gesture recognition.
//!
//! Production reads the OS clock ([`SystemClock`] = `Instant::now()`); a headless
//! frame driver swaps in a [`ManualClock`] it advances explicitly, so a deadline
//! (e.g. a long-press timeout) elapses deterministically with no wall-clock
//! sleep. The [`GestureArena`](crate::arena::GestureArena) owns the clock and
//! recognizers read it via `RecognizerBase::now()`, so the production and
//! headless paths run the *same* code — only the time source differs.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A monotonic time source — the single authority a deadline-driven recognizer
/// reads `now()` from.
///
/// The default ([`SystemClock`]) is the OS clock; a headless frame driver uses
/// [`ManualClock`] to advance a virtual timeline deterministically. Injected once
/// at arena construction (mirroring how an `AnimationController` holds its
/// `Scheduler`), so per-call signatures stay unchanged.
pub trait MonotonicClock: Send + Sync + fmt::Debug {
    /// The current instant on this clock's timeline. Must be non-decreasing
    /// across calls.
    fn now(&self) -> Instant;
}

/// The real OS clock — `Instant::now()`. The default; production is unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl MonotonicClock for SystemClock {
    #[inline]
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A virtual clock advanced explicitly by a frame driver.
///
/// `now()` is `base + elapsed`; [`advance`](Self::advance) moves the timeline
/// forward. Clones share the same timeline (the elapsed counter is `Arc`-backed),
/// so a driver holding one handle and the arena holding another observe a single
/// clock. `now()` returns a real [`Instant`] on the virtual timeline, so types
/// that already store an `Instant` (e.g. a recognizer's captured down-time) need
/// no change — only their source does.
#[derive(Debug, Clone)]
pub struct ManualClock {
    base: Instant,
    elapsed: Arc<Mutex<Duration>>,
}

impl ManualClock {
    /// A virtual clock starting at the construction instant with zero elapsed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: Arc::new(Mutex::new(Duration::ZERO)),
        }
    }

    /// Move the virtual timeline forward by `dt`.
    pub fn advance(&self, dt: Duration) {
        *self.elapsed.lock() += dt;
    }

    /// The elapsed virtual time since construction.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        *self.elapsed.lock()
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for ManualClock {
    #[inline]
    fn now(&self) -> Instant {
        self.base + *self.elapsed.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances_deterministically() {
        let clock = ManualClock::new();
        let t0 = clock.now();
        clock.advance(Duration::from_millis(500));
        let t1 = clock.now();
        assert_eq!(t1.duration_since(t0), Duration::from_millis(500));
        // Clones share the timeline.
        let other = clock.clone();
        clock.advance(Duration::from_millis(100));
        assert_eq!(other.elapsed(), Duration::from_millis(600));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let clock = SystemClock;
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }
}
