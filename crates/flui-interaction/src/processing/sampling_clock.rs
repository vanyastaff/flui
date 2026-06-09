//! Frame-paced clock for pointer-event resampling.
//!
//! Flutter's `PointerEventResampler.sample(...)` is caller-paced: the
//! caller passes a `(sampleTime, nextSampleTime)` pair on every frame
//! tick. The resampler interpolates queued events between those two
//! instants. To keep the call sites consistent and to surface the
//! "what is the input vs display refresh rate" decision at one
//! canonical location, we wrap that pacing contract in a
//! [`SamplingClock`].
//!
//! # Why this exists
//!
//! - The caller (the `flui_engine` frame loop, a custom shell, a test)
//!   chooses *one* sample frequency, not per-resampler. Centralising
//!   the cadence in a clock means: resampler, velocity tracker, and
//!   predictor all read the same wall-clock anchor, no drift between
//!   consumers.
//! - Mismatched input/display rates (e.g. 120Hz touch sensor against
//!   a 60Hz display) need a deliberate up- or down-sampling step. The
//!   clock is the place that policy lives.
//! - Tests need a deterministic, monotonic clock that does not depend
//!   on `std::time::Instant::now()` (which can jump under
//!   `cargo test` sharding). [`SamplingClock::Manual`] exists for that.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::processing::sampling_clock::SamplingClock;
//! use std::time::Duration;
//!
//! // 120Hz input clock on a 60Hz display: up-sample to display rate.
//! let clock = SamplingClock::Fixed {
//!     period: Duration::from_micros(8_333), // ~120Hz
//! };
//!
//! let (now, next) = clock.tick();
//! resampler.sample(now, next, |event| dispatch(event));
//! ```
//!
//! Flutter reference: `gestures/resampler.dart` (caller-paced sampling
//! loop) and `scheduler/ticker.dart` (frame-tick clock).

use std::time::{Duration, Instant};

/// Default sampling period: 60 Hz (Flutter's `kDefaultSamplePeriod`).
///
/// Matches Flutter's [`kDefaultSamplePeriod`](https://api.flutter.dev/flutter/scheduler/kDefaultSamplePeriod-constant.html)
/// (16,667 µs). Touch sensors commonly run at 120 Hz, displays at 60 Hz;
/// the resampler bridges the two.
pub const DEFAULT_SAMPLE_PERIOD: Duration = Duration::from_micros(16_667);

/// Wall-clock-paced sampling policy.
///
/// Holds no state of its own — `tick()` returns the `(now, next)` pair
/// the caller hands to the resampler. The three variants cover the
/// real call sites (real-time engine loop, test loop, deterministic
/// replay).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum SamplingClock {
    /// Fixed-cadence wall-clock sampling.
    ///
    /// `now` is `Instant::now()`; `next` is `now + period`. The clock
    /// does not align to any external phase, so consecutive calls
    /// produce monotonically increasing `now` values (modulo
    /// `Instant`'s own monotonicity guarantee) and a constant stride.
    Fixed {
        /// Time between samples. Must be > 0; the constructor
        /// normalises a 0-period to [`DEFAULT_SAMPLE_PERIOD`].
        period: Duration,
    },

    /// Caller-supplied monotonic clock, used by tests and replay.
    ///
    /// `tick()` returns `(*now, *now + period)` and advances `*now` by
    /// `period`. Caller is responsible for keeping the underlying
    /// value monotonic; the resampler assumes `next > now` strictly.
    Manual {
        /// Sample period. Must be > 0.
        period: Duration,
    },
}

impl SamplingClock {
    /// Default 60 Hz [`SamplingClock::Fixed`] clock.
    #[inline]
    #[must_use]
    pub const fn default_fixed() -> Self {
        Self::Fixed {
            period: DEFAULT_SAMPLE_PERIOD,
        }
    }

    /// Returns the configured sample period.
    ///
    /// A 0-period is normalised to [`DEFAULT_SAMPLE_PERIOD`] — the
    /// resampler rejects `next <= now` and a 0-period would always
    /// violate that invariant.
    #[inline]
    #[must_use]
    pub fn period(&self) -> Duration {
        let raw = match *self {
            Self::Fixed { period } | Self::Manual { period } => period,
        };
        if raw.is_zero() {
            DEFAULT_SAMPLE_PERIOD
        } else {
            raw
        }
    }

    /// Compute `(now, next)` for a [`SamplingClock::Fixed`] clock.
    ///
    /// Returns `None` for [`SamplingClock::Manual`], whose ticks
    /// require a mutable `*mut Instant` parameter — use
    /// [`Self::tick_manual`] for that variant.
    pub fn tick(&self) -> Option<(Instant, Instant)> {
        match *self {
            Self::Fixed { period } => {
                let period = if period.is_zero() {
                    DEFAULT_SAMPLE_PERIOD
                } else {
                    period
                };
                let now = Instant::now();
                // `Instant::checked_add` saturates on overflow; for
                // realistic sample periods (~ms) on a real-time wall
                // clock, overflow is unreachable in practice.
                let next = now.checked_add(period).unwrap_or(now);
                Some((now, next))
            }
            Self::Manual { .. } => None,
        }
    }

    /// Advance a manual clock by one period.
    ///
    /// Returns `(now, next)` where `next = *now + period` and
    /// `*now = next`. The caller owns the backing storage; this
    /// method does not allocate.
    ///
    /// # Panics
    ///
    /// Panics if `*now + period` overflows `Instant` arithmetic.
    /// `Instant` is `u64` seconds + `u32` nanos on every supported
    /// platform, so overflow is unreachable in practice (~584 years
    /// of monotonic time from the system boot). The check is
    /// defensive — if you can construct this overflow you have
    /// either booted a 585-year-old system or are testing
    /// pathological input.
    #[must_use]
    pub fn tick_manual(&self, now: &mut Instant) -> (Instant, Instant) {
        debug_assert!(
            matches!(self, Self::Manual { .. }),
            "tick_manual called on non-Manual clock; use tick() instead"
        );
        let period = self.period();
        let cur = *now;
        let next = cur
            .checked_add(period)
            .expect("manual clock overflow: ~584 years of monotonic Instant");
        *now = next;
        (cur, next)
    }
}

impl Default for SamplingClock {
    fn default() -> Self {
        Self::default_fixed()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_period_is_60hz() {
        // 60Hz = 16.667ms; check rounded to the microsecond.
        assert_eq!(DEFAULT_SAMPLE_PERIOD, Duration::from_micros(16_667));
    }

    #[test]
    fn default_fixed_has_expected_period() {
        let clock = SamplingClock::default_fixed();
        assert_eq!(clock.period(), DEFAULT_SAMPLE_PERIOD);
    }

    #[test]
    fn zero_period_normalised() {
        // A caller passing Duration::ZERO does not get a divide-by-zero
        // or a 0-stride loop; the clock falls back to DEFAULT.
        let clock = SamplingClock::Fixed {
            period: Duration::ZERO,
        };
        assert_eq!(clock.period(), DEFAULT_SAMPLE_PERIOD);
    }

    #[test]
    fn manual_zero_period_normalised() {
        let clock = SamplingClock::Manual {
            period: Duration::ZERO,
        };
        assert_eq!(clock.period(), DEFAULT_SAMPLE_PERIOD);
    }

    #[test]
    fn fixed_tick_returns_advancing_pair() {
        let clock = SamplingClock::Fixed {
            period: Duration::from_millis(8),
        };
        let (now1, next1) = clock.tick().expect("Fixed tick is Some");
        let (now2, next2) = clock.tick().expect("Fixed tick is Some");
        // Wall clock is monotonic: now2 >= now1.
        assert!(now2 >= now1, "now1={:?} now2={:?}", now1, now2);
        // next = now + period exactly (within 1µs of measurement).
        let stride1 = next1.duration_since(now1);
        let stride2 = next2.duration_since(now2);
        assert!(
            (stride1.as_micros() as i128 - 8_000).abs() <= 1_000,
            "stride1={:?}",
            stride1
        );
        assert!(
            (stride2.as_micros() as i128 - 8_000).abs() <= 1_000,
            "stride2={:?}",
            stride2
        );
    }

    #[test]
    fn manual_tick_advances_backing_storage() {
        let clock = SamplingClock::Manual {
            period: Duration::from_millis(4),
        };
        // Anchor at a known instant by re-using Instant::now() once;
        // subsequent ticks must be deterministic.
        let mut now = Instant::now();
        let (t0, n0) = clock.tick_manual(&mut now);
        let (t1, n1) = clock.tick_manual(&mut now);
        let (t2, n2) = clock.tick_manual(&mut now);

        assert_eq!(n0.duration_since(t0), Duration::from_millis(4));
        assert_eq!(t1, n0);
        assert_eq!(n1, t1 + Duration::from_millis(4));
        assert_eq!(t2, n1);
        assert_eq!(n2, t2 + Duration::from_millis(4));
    }

    #[test]
    fn tick_returns_none_for_manual() {
        // The ergonomic contract: a Manual clock's `tick()` is `None`
        // — caller must use `tick_manual` to advance state.
        let clock = SamplingClock::Manual {
            period: Duration::from_millis(16),
        };
        assert!(clock.tick().is_none());
    }

    #[test]
    fn default_trait_uses_default_fixed() {
        let clock = SamplingClock::default();
        assert_eq!(clock.period(), DEFAULT_SAMPLE_PERIOD);
    }
}
