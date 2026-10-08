//! Frame-paced clock for pointer-event resampling.
//!
//! `PointerEventResampler::sample(...)` is caller-paced: the
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
//!   the cadence in a clock means every resampler is paced alike.
//! - Mismatched input/display rates (e.g. 120Hz touch sensor against
//!   a 60Hz display) need a deliberate up- or down-sampling step. The
//!   clock is the place that policy lives.
//! - Tests need a deterministic, monotonic clock that does not depend
//!   on `std::time::Instant::now()` (which can jump under
//!   `cargo test` sharding). [`SamplingClock::Manual`] exists for that.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::processing::{PointerEventResampler, SamplingClock};
//! use flui_interaction::PointerId;
//! use std::time::Duration;
//!
//! // Pace sampling at the display's 60Hz cadence.
//! let clock = SamplingClock::Fixed {
//!     period: Duration::from_micros(16_667),
//! };
//!
//! let resampler = PointerEventResampler::new(PointerId::try_from(1_u64)?);
//! if let Some((now, next)) = clock.tick() {
//!     resampler.sample(now, next, |event| { /* dispatch the sampled event */ });
//! }
//! # Ok::<(), std::num::TryFromIntError>(())
//! ```

use web_time::{Duration, Instant};

/// Default sampling period: 60 Hz (16,667 µs).
///
/// Touch sensors commonly run at 120 Hz, displays at 60 Hz;
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
    /// [`SamplingClock::tick_manual`] returns `(*now, *now + period)` and
    /// advances `*now` by `period`. Caller is responsible for keeping the
    /// underlying value monotonic; the resampler assumes `next > now`
    /// strictly. Where no time store is supplied, [`SamplingClock::tick`]
    /// paces at `period` on the wall clock.
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

    /// Compute a `(now, now + period)` window anchored at the wall clock.
    ///
    /// Both variants answer: a [`SamplingClock::Manual`] clock owns no time
    /// store of its own, so a caller that cannot supply one (the gesture
    /// binding's `flush_pending_moves`) still gets a window paced at the
    /// manual period instead of none — which would hold every resampled move
    /// until the pointer lifts. Callers that own the manual time use
    /// [`Self::tick_manual`] (or pass explicit windows) for determinism.
    ///
    /// Returns `None` only if `now + period` overflows `Instant`.
    pub fn tick(&self) -> Option<(Instant, Instant)> {
        let now = Instant::now();
        // Propagate overflow as `None` (the caller falls back to direct
        // dispatch) rather than returning an invalid `(now, now)` window
        // that violates the documented `next > now` invariant.
        let next = now.checked_add(self.period())?;
        Some((now, next))
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
