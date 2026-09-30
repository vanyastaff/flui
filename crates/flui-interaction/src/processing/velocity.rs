//! Velocity estimation for gesture recognition
//!
//! This module provides pointer velocity estimation. Three tracker flavours
//! are provided:
//!
//! - [`VelocityTracker`] — least-squares polynomial regression on a 20-sample
//!   circular buffer. This is the default and the one the core gesture
//!   pipeline uses.
//! - [`IosFlingVelocityTracker`] — iOS `UIScrollView` fling approximation:
//!   weighted average of three adjacent 2-point velocities. Use this when you
//!   want the initial fling velocity that matches native iOS scroll physics.
//! - [`MacosFlingVelocityTracker`] — same algorithm as iOS, with different
//!   weights (matches `NSScrollView`).
//!
//! # Algorithm
//!
//! All trackers keep a 20-slot circular buffer of `(time, position)` samples
//! and walk backwards from the newest sample, stopping when either the
//! horizon (100 ms) is exceeded or the gap between consecutive samples
//! exceeds 40 ms (the pointer is considered stationary). For the least-
//! squares flavour, the surviving samples are fed to `LeastSquaresSolver`
//! which fits a quadratic polynomial in time and reports its derivative at
//! `t = 0` as the velocity.
//!
//! Confidence is the product of the R² fit quality of the x and y
//! polynomials; a perfect linear swipe gives 1.0, a noisy curve gives
//! something close to 0.0.
//!
//! # Example
//!
//! ```rust
//! use web_time::{Duration, Instant};
//!
//! use flui_interaction::processing::VelocityTracker;
//! use flui_foundation::geometry::Offset;
//! use flui_interaction::PointerDeviceKind;
//!
//! let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
//! let start = Instant::now();
//! for i in 0..10 {
//!     tracker.add_position(
//!         start + Duration::from_millis(i * 10),
//!         Offset::new((i as f64 * 10.0), 0.0),
//!     );
//! }
//!
//! // Velocity is the linear coefficient of the quadratic fit,
//! // scaled to px/s.
//! let _estimate = tracker.get_velocity_estimate();
//! // Fling velocity is the same estimate gated by a min-speed
//! // threshold (~50 px/s on either axis).
//! let _fling = tracker.get_fling_velocity(false);
//! ```

use web_time::{Duration, Instant};

pub use crate::device_kind::PointerDeviceKind;
pub use crate::velocity::{Velocity, VelocityEstimate};
use flui_foundation::geometry::Offset;

use super::lsq_solver::{MAX_SAMPLES, solve_two};

// ============================================================================
// Constants
// ============================================================================

/// If no sample has been added for this long, the pointer is considered
/// stopped and the velocity is reported as zero with confidence 1.0.
const ASSUME_POINTER_STOPPED: Duration = Duration::from_millis(40);

/// Maximum age of samples to consider when fitting.
const HORIZON: Duration = Duration::from_millis(100);

/// Minimum number of contiguous samples needed to attempt a least-squares fit.
const MIN_SAMPLE_SIZE: usize = 3;

/// Number of samples to keep in the circular buffer. Also the upper bound for
/// the shared `LeastSquaresSolver` scratch buffer.
const HISTORY_SIZE: usize = MAX_SAMPLES;

/// Polynomial degree for the least-squares fit. Quadratic.
const POLYNOMIAL_DEGREE: usize = 2;

/// Speed below which a release is not a fling, in px/s.
///
/// A fling is normally a speed check combined with a slop check on the
/// up/down offset; this crate applies the speed half on its own, through
/// [`fling_velocity_or_zero`].
const MIN_FLING_SPEED_PX_S: f64 = 50.0;

/// Gate `velocity` to [`Velocity::ZERO`] unless it is a fling, or `allow_slow`
/// waives the gate.
///
/// Every tracker exposes this as `get_fling_velocity`; one body keeps the
/// threshold, and the axes it is measured on, from drifting between them.
fn fling_velocity_or_zero(velocity: Velocity, allow_slow: bool) -> Velocity {
    if allow_slow {
        return velocity;
    }
    if velocity.pixels_per_second.dx.abs() < MIN_FLING_SPEED_PX_S
        && velocity.pixels_per_second.dy.abs() < MIN_FLING_SPEED_PX_S
    {
        return Velocity::ZERO;
    }
    velocity
}

/// The velocity an estimate carries, or [`Velocity::ZERO`] when there is no
/// estimate or it reports no motion.
///
/// Every tracker exposes this as `get_velocity`; one body keeps the
/// missing-estimate and zero-motion cases answering alike.
fn velocity_from_estimate(estimate: Option<VelocityEstimate>) -> Velocity {
    match estimate {
        Some(est) if est.pixels_per_second != Offset::ZERO => Velocity::new(est.pixels_per_second),
        _ => Velocity::ZERO,
    }
}

// ============================================================================
// PointAtTime
// ============================================================================

/// One position sample. The slot in the circular buffer is `Option<_PointAtTime>`
/// so an unwritten slot is distinguishable from `(Offset::ZERO, Instant::EPOCH)`.
#[derive(Debug, Clone, Copy)]
struct PointAtTime {
    /// When this sample was recorded.
    time: Instant,
    /// Position at this time.
    position: Offset<f64>,
}

// ============================================================================
// VelocityTracker
// ============================================================================

/// Computes a pointer's velocity from a stream of `(time, position)` samples.
///
/// Uses a polynomial least-squares fit. Adding samples is
/// O(1); computing a velocity is O(N) where N ≤ 20, with the inner loop
/// running through fixed-size stack-allocated scratch buffers in
/// `LeastSquaresSolver`.
#[derive(Debug, Clone)]
pub struct VelocityTracker {
    /// Pointer device kind. Recorded even though the algorithm is currently
    /// device-independent.
    kind: PointerDeviceKind,

    /// Circular buffer of samples. Empty slots are `None` so we can
    /// distinguish "slot not yet written" from a sample at `Instant::EPOCH`.
    samples: [Option<PointAtTime>; HISTORY_SIZE],

    /// Index of the most recently written sample. Wraps around modulo
    /// `HISTORY_SIZE`.
    index: usize,

    /// When the most recent sample was added. Used to detect "the pointer
    /// has been still for 40 ms or more" — the signal that the velocity is
    /// zero.
    since_last_sample: Option<Instant>,

    /// Memoized result of the buffer-pure part of [`Self::get_velocity_estimate`]
    /// (the backward walk plus the least-squares fit). Invalidated whenever
    /// the sample buffer changes ([`Self::add_position`] / [`Self::reset`]).
    ///
    /// Only the buffer-pure computation is cached; the time-dependent
    /// "stationary for 40 ms" gate is re-evaluated on every call, so a cached
    /// fit is returned only while the pointer is still moving. This collapses
    /// the repeated queries on the drag-end path — most visibly
    /// [`super::InputPredictor::predict`], which asks for both the velocity
    /// and the estimate in one call — from two O(N) QR solves to one.
    cached_estimate: Option<VelocityEstimate>,
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerDeviceKind::Touch)
    }
}

impl VelocityTracker {
    /// Construct a new velocity tracker for the given pointer device kind.
    ///
    /// The parameter is kept even though the algorithm doesn't yet branch on
    /// it, so the field is in place for future device-specific tuning (mouse
    /// vs touch vs stylus).
    #[must_use]
    pub fn with_kind(kind: PointerDeviceKind) -> Self {
        Self {
            kind,
            samples: [None; HISTORY_SIZE],
            index: 0,
            since_last_sample: None,
            cached_estimate: None,
        }
    }

    /// The kind of pointer this tracker is for.
    #[inline]
    pub fn kind(&self) -> PointerDeviceKind {
        self.kind
    }

    /// Record a position at the given time.
    ///
    /// O(1). The samples are stored in a 20-slot circular buffer; older
    /// samples are silently overwritten.
    ///
    /// The `time` parameter is the *logical* timestamp used for the
    /// velocity fit (it can come from a synthetic test clock, a high-
    /// resolution pointer-event clock, or a frame-time source). The
    /// "stationary for 40 ms" gate uses `Instant::now()` so the check
    /// always reflects wall-clock time, regardless of how the caller
    /// generates the logical timestamps.
    pub fn add_position(&mut self, time: Instant, position: Offset<f64>) {
        // Reject non-finite coordinates: NaN/Inf would poison the least-squares
        // fit (NaN comparisons defeat the singular-matrix guard) and propagate
        // into every downstream velocity. Pointer streams are untrusted input.
        if !position.dx.is_finite() || !position.dy.is_finite() {
            return;
        }
        // Mark "now" as the latest activity. Used by get_velocity_estimate()
        // to short-circuit when the pointer has been still for >= 40 ms.
        // We use the real wall clock here, NOT the `time` argument, so a
        // caller that supplies synthetic timestamps (tests, frame-time
        // extrapolation, replay logs) still gets the correct stationary
        // signal.
        self.since_last_sample = Some(Instant::now());

        // The sample buffer is about to change, so any memoized fit is stale.
        self.cached_estimate = None;

        // Advance the write index, wrapping at HISTORY_SIZE.
        self.index = (self.index + 1) % HISTORY_SIZE;
        self.samples[self.index] = Some(PointAtTime { time, position });
    }

    /// Reset the tracker, discarding all samples.
    pub fn reset(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
        self.since_last_sample = None;
        self.cached_estimate = None;
    }

    /// Number of samples currently stored.
    #[inline]
    pub fn sample_count(&self) -> usize {
        self.samples.iter().filter(|s| s.is_some()).count()
    }

    /// Returns `true` when the tracker has at least `MIN_SAMPLE_SIZE` (3)
    /// contiguous samples since the last stationary signal — enough data
    /// to attempt a least-squares fit.
    #[inline]
    pub fn has_sufficient_data(&self) -> bool {
        self.estimate_sample_count() >= MIN_SAMPLE_SIZE
    }

    /// The most recent velocity estimate, including the polynomial-fit
    /// confidence and the time/position span it was computed over.
    ///
    /// Returns `None` if the tracker has no samples at all.
    ///
    /// Takes `&mut self` because the buffer-pure part of the result is
    /// memoized (see the private `compute_estimate`); the cache is reused until
    /// the next [`Self::add_position`] / [`Self::reset`]. The "stationary for
    /// 40 ms" gate below is time-dependent and re-checked every call, so a
    /// cached fit is only ever returned while the pointer is still moving.
    pub fn get_velocity_estimate(&mut self) -> Option<VelocityEstimate> {
        // Pointer has been still for >= 40 ms → velocity is exactly zero with
        // perfect confidence. A fully-populated VelocityEstimate is returned
        // so callers can still ask for `duration` / `offset`.
        // Time-dependent, so never cached.
        if let Some(last) = self.since_last_sample
            && last.elapsed() >= ASSUME_POINTER_STOPPED
        {
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                Duration::ZERO,
                1.0,
            ));
        }

        // Reuse the memoized fit if the sample buffer hasn't changed since it
        // was computed. `VelocityEstimate` is `Copy`, so this is a cheap read.
        if let Some(cached) = self.cached_estimate {
            return Some(cached);
        }

        let estimate = self.compute_estimate()?;
        self.cached_estimate = Some(estimate);
        Some(estimate)
    }

    /// Compute the buffer-pure part of the velocity estimate: walk the
    /// circular buffer back from the newest sample and run the least-squares
    /// fit. Returns `None` only when the buffer is empty.
    ///
    /// This is a pure function of the sample buffer — it does not consult the
    /// time-dependent stationary gate, nor the memo cache — which is exactly
    /// what makes the cache in [`Self::get_velocity_estimate`] sound. O(N)
    /// where N ≤ `HISTORY_SIZE` (the buffer is bounded at 20 samples).
    fn compute_estimate(&self) -> Option<VelocityEstimate> {
        let newest = self.samples[self.index]?;
        // Walk backwards through the circular buffer, collecting samples that
        // represent continuous motion: age <= HORIZON and gap between
        // adjacent samples <= ASSUME_POINTER_STOPPED.
        let mut previous = newest;
        let mut oldest = newest;
        let mut xs = [0.0f64; HISTORY_SIZE];
        let mut ys = [0.0f64; HISTORY_SIZE];
        let mut ts = [0.0f64; HISTORY_SIZE];
        let mut ws = [0.0f64; HISTORY_SIZE];
        let mut n: usize = 0;
        let mut cursor = self.index;

        // Bound the walk at one full lap — anything beyond that is stale.
        for _ in 0..HISTORY_SIZE {
            let Some(sample) = self.samples[cursor] else {
                break;
            };

            // age is in ms; delta is the gap from the previously-iterated
            // sample, also in ms.
            let age_ms = newest
                .time
                .checked_duration_since(sample.time)
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let delta_ms = previous
                .time
                .checked_duration_since(sample.time)
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            previous = sample;

            // Stop the walk if the sample is past the horizon or the gap from
            // the previous one is too large — the pointer was stationary
            // between them.
            if age_ms > HORIZON.as_secs_f64() * 1000.0
                || delta_ms > ASSUME_POINTER_STOPPED.as_secs_f64() * 1000.0
            {
                break;
            }

            oldest = sample;
            ts[n] = -age_ms; // Negative: we go back from the newest sample.
            xs[n] = sample.position.dx;
            ys[n] = sample.position.dy;
            ws[n] = 1.0; // Uniform weights.
            n += 1;

            // Step the cursor one slot backwards through the circular buffer.
            cursor = if cursor == 0 {
                HISTORY_SIZE - 1
            } else {
                cursor - 1
            };
        }

        // We were unable to gather enough samples to fit. Report zero
        // velocity with confidence 1.0 and the span we did see.
        if n < MIN_SAMPLE_SIZE {
            // A zero velocity here means no fling at all, and the two ways to
            // reach one are indistinguishable from the outside: this one (the
            // contiguous run was cut short, so a delivery gap crossed
            // ASSUME_POINTER_STOPPED) and the degenerate-span one below.
            // Naming which fired is the difference between a diagnosis and a
            // guess when a gesture dies on a loaded machine.
            tracing::trace!(
                target: "flui.velocity",
                contiguous_samples = n,
                span_ms = newest
                    .time
                    .saturating_duration_since(oldest.time)
                    .as_secs_f64()
                    * 1000.0,
                reason = "too_few_contiguous_samples",
                "velocity estimate: no fling"
            );
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                newest.time.saturating_duration_since(oldest.time),
                1.0,
            ));
        }

        // Guard: if the total time window is effectively zero (all samples at
        // the same timestamp, which happens when pointer events arrive within a
        // single OS timer tick — common in headless tests with coarse clocks on
        // Windows), the least-squares system is singular and would produce NaN.
        // Report zero velocity with zero confidence so callers can still decide
        // whether to spring back based on position (e.g. overscroll), rather
        // than silently corrupting the simulation with NaN.
        let total_span_ms = ts[..n]
            .iter()
            .map(|&t| -t) // ts are stored as negative age_ms; max(-ts) = total age
            .fold(0.0_f64, f64::max);
        if total_span_ms < 1e-6 {
            // Enough samples, but they all carry one timestamp — the batched
            // drain. See the note at the sibling return above for why this is
            // reported rather than left silent.
            tracing::trace!(
                target: "flui.velocity",
                contiguous_samples = n,
                span_ms = total_span_ms,
                reason = "degenerate_time_span",
                "velocity estimate: no fling"
            );
            return Some(VelocityEstimate::new(
                newest.position - oldest.position,
                Offset::ZERO,
                newest.time.saturating_duration_since(oldest.time),
                0.0,
            ));
        }

        // Fit a quadratic in milliseconds; velocity in px/ms is the linear
        // coefficient. Scale to px/s (× 1000). The x and y fits share the same
        // sample times and weights, so `solve_two` computes the QR
        // factorization once for both.
        let (x_fit, y_fit) = solve_two(&ts[..n], &ws[..n], &xs[..n], &ys[..n], POLYNOMIAL_DEGREE);

        match (x_fit, y_fit) {
            (Some(xf), Some(yf)) => Some(VelocityEstimate::new(
                newest.position - oldest.position,
                Offset::new(xf.coefficients[1] * 1000.0, yf.coefficients[1] * 1000.0),
                newest.time.saturating_duration_since(oldest.time),
                xf.confidence * yf.confidence,
            )),
            // Numerical failure on one axis — keep going with zero on that
            // axis and the other axis's confidence. Rare; happens on
            // degenerate data.
            _ => Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                newest.time.saturating_duration_since(oldest.time),
                0.0,
            )),
        }
    }

    /// The most recent velocity as a [`Velocity`].
    ///
    /// Cheap wrapper over [`Self::get_velocity_estimate`] that returns
    /// [`Velocity::ZERO`] when the estimate is missing or its velocity is
    /// zero. This is the canonical call site for "fling this view" in a drag-end
    /// callback.
    ///
    /// `&mut self` for the same memoization reason as
    /// [`Self::get_velocity_estimate`], which this delegates to.
    pub fn get_velocity(&mut self) -> Velocity {
        velocity_from_estimate(self.get_velocity_estimate())
    }

    /// Velocity for fling detection.
    ///
    /// When `allow_slow` is `false` (the typical case), the result is
    /// [`Velocity::ZERO`] for any motion under ~50 px/s — the usual fling
    /// threshold (a fling also requires the offset between the up and down
    /// events to exceed a slop, combined with a non-trivial velocity). When
    /// `allow_slow` is `true`, the raw estimate is returned even at very
    /// low speeds — useful for snap-back animations and small-list
    /// micro-scrolls.
    ///
    /// `&mut self` for the same memoization reason as [`Self::get_velocity`].
    pub fn get_fling_velocity(&mut self, allow_slow: bool) -> Velocity {
        fling_velocity_or_zero(self.get_velocity(), allow_slow)
    }

    /// Alias for [`Self::get_velocity_estimate`].
    ///
    /// `&mut self` for the same memoization reason as
    /// [`Self::get_velocity_estimate`], which this delegates to.
    #[inline]
    pub fn estimate(&mut self) -> Option<VelocityEstimate> {
        self.get_velocity_estimate()
    }

    /// Construct a touch-kind tracker. Equivalent to [`Self::default`].
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimate of contiguous samples in the walk window. O(HISTORY_SIZE).
    fn estimate_sample_count(&self) -> usize {
        let Some(newest) = self.samples[self.index] else {
            return 0;
        };
        let mut previous = newest;
        let mut n = 0usize;
        let mut cursor = self.index;
        for _ in 0..HISTORY_SIZE {
            let Some(sample) = self.samples[cursor] else {
                break;
            };
            let age_ms = newest
                .time
                .checked_duration_since(sample.time)
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let delta_ms = previous
                .time
                .checked_duration_since(sample.time)
                .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            previous = sample;
            if age_ms > HORIZON.as_secs_f64() * 1000.0
                || delta_ms > ASSUME_POINTER_STOPPED.as_secs_f64() * 1000.0
            {
                break;
            }
            n += 1;
            cursor = if cursor == 0 {
                HISTORY_SIZE - 1
            } else {
                cursor - 1
            };
        }
        n
    }
}

// ============================================================================
// IosFlingVelocityTracker
// ============================================================================

/// Velocity tracker that matches iOS `UIScrollView` fling estimation.
///
/// Uses a weighted average of three adjacent 2-point velocities (offsets
/// `-2`, `-1`, `0` in the circular buffer) with weights `0.6 / 0.35 / 0.05`.
/// The fit is intentionally crude — it's what `UIScrollView` reports to its
/// delegate in `scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)`,
/// and the gesture pipeline uses it to seed the `Scrollable`'s fling
/// simulation.
///
/// The 20-slot history is larger than the 4 used by the maths — the extra
/// slots keep the `VelocityEstimate.offset` (computed as `newest - oldest`)
/// large enough to be recognised as a fling by the drag recognizers.
#[derive(Debug, Clone)]
pub struct IosFlingVelocityTracker {
    inner: VelocityTracker,
    /// Weights applied to the 2-point velocities at offsets (-2, -1, 0).
    weights: [f64; 3],
}

impl Default for IosFlingVelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerDeviceKind::Touch)
    }
}

impl IosFlingVelocityTracker {
    /// Construct an iOS-flavour tracker for the given pointer kind.
    #[must_use]
    pub fn with_kind(kind: PointerDeviceKind) -> Self {
        Self {
            inner: VelocityTracker::with_kind(kind),
            weights: [0.6, 0.35, 0.05],
        }
    }

    /// Record a position. O(1). Mirrors `IOSScrollViewFlingVelocityTracker.addPosition`.
    pub fn add_position(&mut self, time: Instant, position: Offset<f64>) {
        self.inner.add_position(time, position);
    }

    /// Reset all samples.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// The pointer kind this tracker is configured for.
    #[inline]
    pub fn kind(&self) -> PointerDeviceKind {
        self.inner.kind()
    }

    /// Velocity estimate. The `pixels_per_second` is the weighted sum of
    /// 2-point velocities; the `confidence` is always 1.0 (the algorithm
    /// makes no claim about fit quality); `duration` and `offset` are
    /// computed from the newest and oldest non-null samples.
    pub fn get_velocity_estimate(&self) -> Option<VelocityEstimate> {
        // Stationary? Report zero with confidence 1.0.
        if let Some(last) = self.inner.since_last_sample
            && last.elapsed() >= ASSUME_POINTER_STOPPED
        {
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                Duration::ZERO,
                1.0,
            ));
        }

        let estimated_velocity = self.estimated_velocity();
        let newest = self.inner.samples[self.inner.index]?;
        // Walk forward through the buffer to find the oldest non-null sample.
        let mut oldest: Option<PointAtTime> = None;
        for i in 1..=HISTORY_SIZE {
            let slot = self.inner.samples[(self.inner.index + i) % HISTORY_SIZE];
            if let Some(s) = slot {
                oldest = Some(s);
                break;
            }
        }
        let oldest = oldest.expect("newest was Some, so at least one slot is non-null");

        Some(VelocityEstimate::new(
            newest.position - oldest.position,
            estimated_velocity,
            newest.time.saturating_duration_since(oldest.time),
            1.0,
        ))
    }

    /// Velocity for fling detection. Same semantics as
    /// [`VelocityTracker::get_fling_velocity`].
    pub fn get_fling_velocity(&self, allow_slow: bool) -> Velocity {
        fling_velocity_or_zero(self.get_velocity(), allow_slow)
    }

    /// The raw weighted-average velocity, regardless of the
    /// "stationary for 40 ms" gate.
    fn estimated_velocity(&self) -> Offset<f64> {
        let v = |offset: isize| self.two_sample_velocity_at_f64(offset);
        let dx = v(-2).0 * self.weights[0] + v(-1).0 * self.weights[1] + v(0).0 * self.weights[2];
        let dy = v(-2).1 * self.weights[0] + v(-1).1 * self.weights[1] + v(0).1 * self.weights[2];
        Offset::new(dx, dy)
    }

    /// The 2-point velocity at the given offset from the newest sample,
    /// returned in `(dx, dy)` f64 form for precision arithmetic.
    /// `offset = 0` is the most recent pair, `-1` is the one before, etc.
    fn two_sample_velocity_at_f64(&self, offset: isize) -> (f64, f64) {
        let end_idx =
            (self.inner.index as isize + offset).rem_euclid(HISTORY_SIZE as isize) as usize;
        let start_idx = (end_idx as isize - 1).rem_euclid(HISTORY_SIZE as isize) as usize;
        let (Some(end), Some(start)) = (self.inner.samples[end_idx], self.inner.samples[start_idx])
        else {
            return (0.0, 0.0);
        };
        // dt is in microseconds; convert to milliseconds for the divisor so
        // we preserve precision.
        let dt_us = end.time.saturating_duration_since(start.time).as_micros();
        if dt_us == 0 {
            return (0.0, 0.0);
        }
        let dt_ms = dt_us as f64 / 1000.0;
        // (end - start) is in pixels; divide by dt_ms to get px/ms; × 1000 = px/s.
        let dx_px_s = (end.position.dx - start.position.dx) * 1000.0 / dt_ms;
        let dy_px_s = (end.position.dy - start.position.dy) * 1000.0 / dt_ms;
        (dx_px_s, dy_px_s)
    }

    /// Alias for [`Self::get_velocity_estimate`].
    #[inline]
    pub fn estimate(&self) -> Option<VelocityEstimate> {
        self.get_velocity_estimate()
    }

    /// Velocity as a [`Velocity`]. [`Velocity::ZERO`] when the estimate is
    /// missing or its velocity is zero.
    pub fn get_velocity(&self) -> Velocity {
        velocity_from_estimate(self.get_velocity_estimate())
    }
}

// ============================================================================
// MacosFlingVelocityTracker
// ============================================================================

/// Velocity tracker matching macOS `NSScrollView` fling estimation.
///
/// Same algorithm as [`IosFlingVelocityTracker`] with weights
/// `0.15 / 0.65 / 0.2` (the macOS delegate weights from
/// `scrollViewWillEndDragging(_:withVelocity:targetContentOffset:)`).
#[derive(Debug, Clone)]
pub struct MacosFlingVelocityTracker {
    inner: IosFlingVelocityTracker,
}

impl Default for MacosFlingVelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerDeviceKind::Touch)
    }
}

impl MacosFlingVelocityTracker {
    /// Construct a macOS-flavour tracker for the given pointer kind.
    #[must_use]
    pub fn with_kind(kind: PointerDeviceKind) -> Self {
        let mut inner = IosFlingVelocityTracker::with_kind(kind);
        inner.weights = [0.15, 0.65, 0.2];
        Self { inner }
    }

    /// Record a position.
    pub fn add_position(&mut self, time: Instant, position: Offset<f64>) {
        self.inner.add_position(time, position);
    }

    /// Reset all samples.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// The pointer kind this tracker is configured for.
    #[inline]
    pub fn kind(&self) -> PointerDeviceKind {
        self.inner.kind()
    }

    /// Velocity estimate using the macOS weights.
    pub fn get_velocity_estimate(&self) -> Option<VelocityEstimate> {
        self.inner.get_velocity_estimate()
    }

    /// Velocity as a [`Velocity`].
    pub fn get_velocity(&self) -> Velocity {
        self.inner.get_velocity()
    }

    /// Velocity for fling detection.
    pub fn get_fling_velocity(&self, allow_slow: bool) -> Velocity {
        self.inner.get_fling_velocity(allow_slow)
    }
}

// ============================================================================
// ImpulseVelocityTracker
// ============================================================================

/// Velocity tracker using Android's impulse strategy — the platform default
/// since Android 8.1 (`ImpulseVelocityTrackerStrategy` in AOSP
/// `frameworks/native/libs/input/VelocityTracker.cpp`).
///
/// The impulse model treats the touch surface as a physical object the
/// finger does work on, and recovers the release velocity from the
/// accumulated kinetic energy:
///
/// ```text
/// w   += (v_i − v(w)) · |v_i|        per sample interval, first interval ×0.5
/// v(w) = sign(w) · √(2·|w|)
/// ```
///
/// Compared to the quadratic least-squares fit, each interval's contribution
/// is weighted by the velocity *change* it represents, so a sharp
/// deceleration right before lift-off discounts older samples instead of
/// being averaged away — flings track the finger's final intent, which is
/// why AOSP made it the default. Use this tracker for Android-feel scroll
/// and fling; use [`VelocityTracker`] (least-squares) for the default fit.
///
/// Sample window and stationary gates are shared with the other trackers
/// (100 ms horizon, 40 ms assume-stopped).
#[derive(Debug, Clone)]
pub struct ImpulseVelocityTracker {
    inner: VelocityTracker,
}

impl Default for ImpulseVelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerDeviceKind::Touch)
    }
}

impl ImpulseVelocityTracker {
    /// Construct an impulse tracker for the given pointer kind.
    #[must_use]
    pub fn with_kind(kind: PointerDeviceKind) -> Self {
        Self {
            inner: VelocityTracker::with_kind(kind),
        }
    }

    /// Record a position.
    pub fn add_position(&mut self, time: Instant, position: Offset<f64>) {
        self.inner.add_position(time, position);
    }

    /// Reset all samples.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    /// The pointer kind this tracker is configured for.
    #[inline]
    pub fn kind(&self) -> PointerDeviceKind {
        self.inner.kind()
    }

    /// AOSP: kinetic energy back to velocity, preserving direction.
    /// `v = sign(w) · √2 · √|w|` (mass cancels).
    #[inline]
    fn kinetic_energy_to_velocity(work: f64) -> f64 {
        core::f64::consts::SQRT_2 * work.abs().sqrt() * work.signum()
    }

    /// Impulse velocity over one axis. `positions`/`times` are chronological
    /// (oldest first); both slices have the same length ≥ 2 and strictly
    /// increasing times (enforced by the caller's sample walk).
    fn impulse_axis(positions: &[f64], dts: &[f64]) -> f64 {
        let mut work = 0.0_f64;
        for i in 0..positions.len() - 1 {
            let v_prev = Self::kinetic_energy_to_velocity(work);
            let v_curr = (positions[i + 1] - positions[i]) / dts[i];
            work += (v_curr - v_prev) * v_curr.abs();
            if i == 0 {
                // Boundary condition (AOSP "approach 2"): with no information
                // before the window, assume the finger started from rest —
                // halve the first interval's contribution.
                work *= 0.5;
            }
        }
        Self::kinetic_energy_to_velocity(work)
    }

    /// Velocity estimate via the impulse strategy.
    ///
    /// Returns `None` when no samples have been recorded; a zero estimate
    /// when the pointer is stationary (40 ms without a sample) or fewer than
    /// two samples fall inside the window.
    pub fn get_velocity_estimate(&self) -> Option<VelocityEstimate> {
        // Stationary gate, same contract as the other trackers.
        if let Some(last) = self.inner.since_last_sample
            && last.elapsed() >= ASSUME_POINTER_STOPPED
        {
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                Duration::ZERO,
                1.0,
            ));
        }

        let newest = self.inner.samples[self.inner.index]?;

        // Walk backwards through the window (100 ms horizon / 40 ms gap),
        // collecting chronological samples for the impulse integration.
        let mut chron: [Option<PointAtTime>; HISTORY_SIZE] = [None; HISTORY_SIZE];
        let mut n = 0usize;
        let mut cursor = self.inner.index;
        let mut previous = newest;
        for _ in 0..HISTORY_SIZE {
            let Some(sample) = self.inner.samples[cursor] else {
                break;
            };
            let age = newest.time.saturating_duration_since(sample.time);
            let delta = previous.time.saturating_duration_since(sample.time);
            if age > HORIZON || delta > ASSUME_POINTER_STOPPED {
                break;
            }
            previous = sample;
            chron[n] = Some(sample);
            n += 1;
            cursor = if cursor == 0 {
                HISTORY_SIZE - 1
            } else {
                cursor - 1
            };
        }

        let oldest = chron[n.saturating_sub(1)].unwrap_or(newest);
        if n < 2 {
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                Duration::ZERO,
                1.0,
            ));
        }

        // Reverse into chronological order and strip zero-dt duplicates
        // (the walk guarantees monotone times, but identical timestamps can
        // occur on coarse clocks and would divide by zero).
        let mut xs: [f64; HISTORY_SIZE] = [0.0; HISTORY_SIZE];
        let mut ys: [f64; HISTORY_SIZE] = [0.0; HISTORY_SIZE];
        let mut dts: [f64; HISTORY_SIZE] = [0.0; HISTORY_SIZE];
        let mut m = 0usize;
        let mut last_time: Option<Instant> = None;
        for i in (0..n).rev() {
            // Invariant: slots 0..n were written by the walk above.
            let s = chron[i].expect("walk wrote chron[0..n]; i < n");
            if let Some(prev_time) = last_time {
                let dt = s.time.saturating_duration_since(prev_time).as_secs_f64();
                if dt <= 0.0 {
                    // Same-timestamp duplicate: keep the newer position only.
                    xs[m - 1] = s.position.dx;
                    ys[m - 1] = s.position.dy;
                    continue;
                }
                dts[m - 1] = dt;
            }
            xs[m] = s.position.dx;
            ys[m] = s.position.dy;
            last_time = Some(s.time);
            m += 1;
        }
        if m < 2 {
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                Duration::ZERO,
                1.0,
            ));
        }

        let vx = Self::impulse_axis(&xs[..m], &dts[..m - 1]);
        let vy = Self::impulse_axis(&ys[..m], &dts[..m - 1]);

        Some(VelocityEstimate::new(
            newest.position - oldest.position,
            Offset::new(vx, vy),
            newest.time.saturating_duration_since(oldest.time),
            // The impulse model makes no fit-quality claim (AOSP reports the
            // value unconditionally).
            1.0,
        ))
    }

    /// Velocity as a [`Velocity`].
    pub fn get_velocity(&self) -> Velocity {
        velocity_from_estimate(self.get_velocity_estimate())
    }

    /// Velocity for fling detection. Same semantics as
    /// [`VelocityTracker::get_fling_velocity`].
    pub fn get_fling_velocity(&self, allow_slow: bool) -> Velocity {
        fling_velocity_or_zero(self.get_velocity(), allow_slow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_swipe_x(
        duration_ms: u64,
        samples: usize,
        slope_px_per_s: f64,
    ) -> Vec<(Instant, Offset<f64>)> {
        let start = Instant::now();
        let dt = Duration::from_millis(duration_ms / samples as u64);
        (0..samples)
            .map(|i| {
                let t = start + dt * i as u32;
                let pos = Offset::new(slope_px_per_s * (i as f64 * dt.as_secs_f64()), 0.0);
                (t, pos)
            })
            .collect()
    }

    #[test]
    fn impulse_recovers_constant_velocity_exactly() {
        // For uniform motion the impulse model is exact: the first interval
        // contributes w = ½v², every later interval contributes zero, and
        // v = √(2w) returns the original speed.
        let mut tracker = ImpulseVelocityTracker::with_kind(PointerDeviceKind::Touch);
        for (t, p) in linear_swipe_x(90, 10, 1000.0) {
            tracker.add_position(t, p);
        }
        let v = tracker.get_velocity().pixels_per_second.dx;
        assert!(
            (v - 1000.0).abs() < 10.0,
            "constant 1000 px/s must be recovered exactly, got {v}"
        );
    }

    proptest::proptest! {}

    // ── Dead-fling witnesses ─────────────────────────────────────────────
    //
    // Both zero-velocity exits below are reachable on a loaded machine and
    // are indistinguishable from the outside — a gesture that moved the
    // content but produced no fling. These assert that each one SAYS which
    // it was, because that distinction is the whole diagnostic value.
}
