// ============================================================================
// Shared retry backoff (deadline-paced, never slept on)
// ============================================================================
//
// One exponential-backoff implementation, parameterised by the label its log
// lines carry. Two callers share it today: device-loss recovery
// (`device_recovery.rs`) and surface-recreation retry (`surface_lifecycle.rs`).
// Both are "an expensive synchronous rebuild failed; pace the next attempt on a
// deadline the platform's own idle wait carries, and never block the frame
// thread waiting for it".
//
// This module exists because the two callers would otherwise be two copies of
// the same counters-and-deadline logic — the drift class the repository's
// "one fact, one place" rule exists to prevent. The *policy* (what a failure
// means, what a success resets, whether a hard cap is ever reached) is
// identical between them; only the label differs, so the label is the one
// parameter.

use web_time::Instant;

/// Exponential backoff for a synchronous rebuild retry loop: paces how often
/// an ATTEMPT is made while the failure persists, growing from one frame
/// interval up to a capped ceiling and resetting on the first success.
///
/// Deliberately never gives up permanently (no attempt-count ceiling): the
/// conditions this paces — a lost GPU device, a surface whose native window
/// was not yet available — are expected to clear eventually, so a hard cap
/// would turn a recoverable condition into a dead app. An UNBOUNDED,
/// un-backed-off retry loop is equally wrong (each attempt can rebuild a full
/// GPU stack), so this paces the ATTEMPT itself, not just the log line.
///
/// This is a DEADLINE, not a sleep: [`Self::next_attempt_at`] reports the
/// earliest instant an attempt is allowed, and the caller's only obligation
/// is to skip attempting before it — never to block the calling thread
/// waiting for it. A `thread::sleep` sized to this backoff's interval
/// (climbing to a full second at the cap) blocks input dispatch and lifecycle
/// delivery on every backend's event-loop thread, so each caller wires
/// [`Self::next_attempt_at`] into its platform's own wake-deadline mechanism
/// instead. **A deadline wired into such a hook is necessary but not
/// sufficient on its own**: it must also appear in the owning frame closure's
/// `dirty` predicate or the wake it actuates reaches `WakeAction::Skip` and
/// returns before this backoff is consulted again.
///
/// `Send + Sync`: captured behind an `Arc` by a platform callback, whose
/// closure the backends require to be `Send`. State lives behind one
/// `parking_lot::Mutex` rather than several independent atomics — read/written
/// at most once per frame wake, and a single lock rules out the counters and
/// the deadline ever being updated out of step with each other.
///
/// The mutex has two callers (the frame closure and the wake-deadline hook
/// closure), both on this platform's single event-loop thread, never
/// concurrently — which is what makes holding a `parking_lot::Mutex`
/// (non-reentrant) safe with no real contention. It is still deliberately
/// never held across a `tracing` call (whose subscriber may perform I/O).
pub(super) struct RetryBackoff {
    state: parking_lot::Mutex<RetryBackoffState>,
    /// The subject named in this backoff's log lines, e.g. `"GPU device
    /// recovery"` or `"wgpu surface recreation"`. A `&'static str` rather
    /// than a `String` because every caller passes a literal and the field is
    /// read on the frame path.
    label: &'static str,
}

struct RetryBackoffState {
    /// Consecutive failures since the last success (or since construction).
    consecutive_failures: u32,
    /// Whether the "backoff reached its cap" error has already been logged
    /// once this losing streak — re-armed on the next success.
    cap_logged: bool,
    /// The earliest instant the next attempt is allowed. `None` before the
    /// first failure of a streak (attempt immediately) or right after a
    /// success.
    next_attempt_at: Option<Instant>,
}

impl RetryBackoff {
    /// The base interval: roughly one frame at 60 Hz. A retry cadence, not
    /// a pacing constant — it deliberately does NOT track the display (a
    /// subject that just failed is not presenting anything to pace).
    pub(super) const BASE: std::time::Duration = std::time::Duration::from_millis(16);
    /// The ceiling: "on the order of a second", per the retry policy both
    /// callers document.
    pub(super) const CAP: std::time::Duration = std::time::Duration::from_secs(1);
    /// `BASE << SHIFT_CAP >= CAP` already holds well before this shift is
    /// reached, so capping the shift itself (rather than only the final
    /// `.min(CAP)`) avoids ever computing `1u32 << n` for an unboundedly long
    /// losing streak.
    pub(super) const SHIFT_CAP: u32 = 6;

    pub(super) fn new(label: &'static str) -> Self {
        Self {
            state: parking_lot::Mutex::new(RetryBackoffState {
                consecutive_failures: 0,
                cap_logged: false,
                next_attempt_at: None,
            }),
            label,
        }
    }

    /// The earliest instant the next attempt is allowed, if a failure has
    /// armed one. `None` when ready right now (never failed, or the last
    /// outcome was a success).
    pub(super) fn next_attempt_at(&self) -> Option<Instant> {
        self.state.lock().next_attempt_at
    }

    /// Record a failed attempt at `now`, arm the next deadline, and return it.
    ///
    /// Logs the first failure of a losing streak at `error`, every subsequent
    /// one at `debug`, and re-emits `error` exactly once more when the backoff
    /// reaches [`Self::CAP`] — a permanently failing rebuild says so once more
    /// at that point, not on every attempt after it.
    pub(super) fn record_failure(&self, error: &flui_engine::EngineError, now: Instant) -> Instant {
        /// Which line to log, decided while the state lock is held (it
        /// reads/mutates `cap_logged`); the actual `tracing` call happens
        /// AFTER the guard drops.
        enum LogKind {
            FirstFailure,
            ReachedCap,
            Retrying,
        }

        let (deadline, interval, log_kind) = {
            let mut state = self.state.lock();
            let shift = state.consecutive_failures.min(Self::SHIFT_CAP);
            let interval = (Self::BASE * (1u32 << shift)).min(Self::CAP);
            let at_cap = interval >= Self::CAP;
            let is_first = state.consecutive_failures == 0;
            state.consecutive_failures += 1;
            let deadline = now + interval;
            state.next_attempt_at = Some(deadline);

            let log_kind = if is_first {
                LogKind::FirstFailure
            } else if at_cap && !state.cap_logged {
                state.cap_logged = true;
                LogKind::ReachedCap
            } else {
                LogKind::Retrying
            };
            (deadline, interval, log_kind)
            // `state` (the `MutexGuard`) drops here, before any `tracing`
            // call: the mutex's other caller is a wake-deadline hook closure,
            // and holding it across a subscriber's possible I/O is undesirable
            // even though no reentrant call is reachable today.
        };

        match log_kind {
            LogKind::FirstFailure => {
                tracing::error!(
                    subject = self.label,
                    error = ?error,
                    "rebuild failed; retrying with backoff"
                );
            }
            LogKind::ReachedCap => {
                tracing::error!(
                    subject = self.label,
                    error = ?error,
                    backoff = ?interval,
                    "rebuild still failing at the backoff cap; retries continue silently from here"
                );
            }
            LogKind::Retrying => {
                tracing::debug!(
                    subject = self.label,
                    error = ?error,
                    backoff = ?interval,
                    "rebuild failed; retrying"
                );
            }
        }
        deadline
    }

    /// Reset the backoff after a successful rebuild.
    pub(super) fn record_success(&self) {
        let mut state = self.state.lock();
        state.consecutive_failures = 0;
        state.cap_logged = false;
        state.next_attempt_at = None;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod retry_backoff_tests {
    use std::time::Duration;

    use flui_engine::EngineError;
    use web_time::Instant;

    use super::RetryBackoff;

    fn scripted_error() -> EngineError {
        EngineError::SurfaceCreation(Box::new(std::io::Error::other("scripted retry failure")))
    }

    /// A fresh backoff has no opinion: an attempt may be made immediately.
    #[test]
    fn a_fresh_backoff_arms_no_deadline() {
        let backoff = RetryBackoff::new("test subject");
        assert!(
            backoff.next_attempt_at().is_none(),
            "a backoff that has never failed must not gate an attempt"
        );
    }

    /// The first failure arms a deadline one `BASE` interval out, and a second
    /// failure arms a strictly later one — the cadence grows.
    #[test]
    fn consecutive_failures_grow_the_armed_deadline() {
        let backoff = RetryBackoff::new("test subject");
        let now = Instant::now();

        let first = backoff.record_failure(&scripted_error(), now);
        assert_eq!(first - now, RetryBackoff::BASE);
        assert_eq!(backoff.next_attempt_at(), Some(first));

        let second = backoff.record_failure(&scripted_error(), now);
        assert!(
            second > first,
            "the second failure must arm a later deadline than the first"
        );
    }

    /// The interval is capped at `CAP`: a long losing streak never arms a
    /// deadline more than `CAP` out, and the cap is reached within the shift
    /// bound.
    #[test]
    fn the_interval_is_capped() {
        let backoff = RetryBackoff::new("test subject");
        let now = Instant::now();

        let mut last = now;
        for _ in 0..(RetryBackoff::SHIFT_CAP + 5) {
            last = backoff.record_failure(&scripted_error(), now);
        }
        assert!(
            last - now <= RetryBackoff::CAP,
            "even a long losing streak must not exceed CAP, got {:?}",
            last - now
        );
        assert_eq!(
            last - now,
            RetryBackoff::CAP,
            "the streak must actually reach CAP, not stall below it"
        );
    }

    /// A success clears the deadline and the failure count: the next failure
    /// starts a fresh streak at `BASE`.
    #[test]
    fn a_success_resets_to_a_fresh_backoff() {
        let backoff = RetryBackoff::new("test subject");
        let now = Instant::now();
        let _ = backoff.record_failure(&scripted_error(), now);
        let _ = backoff.record_failure(&scripted_error(), now);

        backoff.record_success();
        assert!(
            backoff.next_attempt_at().is_none(),
            "a success must clear the armed deadline"
        );

        let after_success = backoff.record_failure(&scripted_error(), now);
        assert_eq!(
            after_success - now,
            RetryBackoff::BASE,
            "the first failure of a fresh streak must be back at BASE, not carry the old count"
        );
    }

    /// The cap log is re-armed by a success: a second losing streak reaches
    /// the cap line again.
    #[test]
    fn a_second_streak_can_reach_the_cap_again() {
        let backoff = RetryBackoff::new("test subject");
        let now = Instant::now();
        for _ in 0..=RetryBackoff::SHIFT_CAP {
            let _ = backoff.record_failure(&scripted_error(), now);
        }
        // The cap line has been logged once; a success re-arms it.
        backoff.record_success();
        assert!(
            !backoff.state.lock().cap_logged,
            "success re-arms the cap log"
        );

        for _ in 0..=RetryBackoff::SHIFT_CAP {
            let _ = backoff.record_failure(&scripted_error(), now);
        }
        assert!(
            backoff.state.lock().cap_logged,
            "a second losing streak must be able to reach the cap line again"
        );
    }

    /// `CAP` is not below the shift cap's reach: `BASE << SHIFT_CAP` is at
    /// least `CAP`, so the cap is reachable without an unbounded shift.
    #[test]
    fn base_shifted_by_the_shift_cap_reaches_the_cap() {
        assert!(
            RetryBackoff::BASE * (1u32 << RetryBackoff::SHIFT_CAP) >= RetryBackoff::CAP,
            "the shift cap must be high enough for BASE << SHIFT_CAP to reach CAP"
        );
        // And sanity-check the duration type is the one the runners use.
        let _: Duration = RetryBackoff::CAP;
    }
}
