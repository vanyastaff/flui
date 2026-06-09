//! Velocity estimation for gesture recognition
//!
//! This module provides pointer velocity estimation that mirrors Flutter's
//! [`velocity_tracker.dart`](https://api.flutter.dev/flutter/gestures/VelocityTracker-class.html)
//! API. Three tracker flavours are provided:
//!
//! - [`VelocityTracker`] — least-squares polynomial regression on a 20-sample
//!   circular buffer, identical algorithm to Flutter's
//!   `PolynomialFitLeastSquaresVelocityTracker`. This is the default and the
//!   only one Flutter's core gesture pipeline uses.
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
//! use std::time::{Duration, Instant};
//!
//! use flui_interaction::processing::VelocityTracker;
//! use flui_types::geometry::{Offset, Pixels};
//! use flui_types::gestures::PointerDeviceKind;
//!
//! let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
//! let start = Instant::now();
//! for i in 0..10 {
//!     tracker.add_position(
//!         start + Duration::from_millis(i * 10),
//!         Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)),
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

use std::time::{Duration, Instant};

use flui_types::geometry::{Offset, Pixels};
pub use flui_types::gestures::{PointerDeviceKind, Velocity, VelocityEstimate};

use super::lsq_solver::{LeastSquaresSolver, MAX_SAMPLES};

// ============================================================================
// Constants (Flutter parity — see velocity_tracker.dart lines 142-145)
// ============================================================================

/// If no sample has been added for this long, the pointer is considered
/// stopped and the velocity is reported as zero with confidence 1.0.
///
/// Flutter: `_assumePointerMoveStoppedMilliseconds = 40`.
const ASSUME_POINTER_STOPPED: Duration = Duration::from_millis(40);

/// Maximum age of samples to consider when fitting.
///
/// Flutter: `_horizonMilliseconds = 100`.
const HORIZON: Duration = Duration::from_millis(100);

/// Minimum number of contiguous samples needed to attempt a least-squares fit.
///
/// Flutter: `_minSampleSize = 3`.
const MIN_SAMPLE_SIZE: usize = 3;

/// Number of samples to keep in the circular buffer.
///
/// Flutter: `_historySize = 20`. We also use this as the upper bound for the
/// shared `LeastSquaresSolver` scratch buffer.
const HISTORY_SIZE: usize = MAX_SAMPLES;

/// Polynomial degree for the least-squares fit. Quadratic — same as Flutter.
const POLYNOMIAL_DEGREE: usize = 2;

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
    position: Offset<Pixels>,
}

// ============================================================================
// VelocityTracker
// ============================================================================

/// Computes a pointer's velocity from a stream of `(time, position)` samples.
///
/// Mirrors Flutter's `VelocityTracker` (the
/// `PolynomialFitLeastSquaresVelocityTracker` strategy). Adding samples is
/// O(1); computing a velocity is O(N) where N ≤ 20, with the inner loop
/// running through fixed-size stack-allocated scratch buffers in
/// `LeastSquaresSolver`.
#[derive(Debug, Clone)]
pub struct VelocityTracker {
    /// Pointer device kind. Recorded for parity with Flutter even though the
    /// algorithm is currently device-independent.
    kind: PointerDeviceKind,

    /// Circular buffer of samples. Empty slots are `None` so we can
    /// distinguish "slot not yet written" from a sample at `Instant::EPOCH`.
    samples: [Option<PointAtTime>; HISTORY_SIZE],

    /// Index of the most recently written sample. Wraps around modulo
    /// `HISTORY_SIZE`.
    index: usize,

    /// When the most recent sample was added. Used to detect "the pointer
    /// has been still for 40 ms or more" — the canonical Flutter signal that
    /// the velocity is zero.
    since_last_sample: Option<Instant>,
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerDeviceKind::Touch)
    }
}

impl VelocityTracker {
    /// Construct a new velocity tracker for the given pointer device kind.
    ///
    /// Flutter's `VelocityTracker.withKind(this.kind)` constructor — we keep
    /// the parameter even though the algorithm doesn't yet branch on it, so
    /// downstream code can match Flutter's API shape and the field is in
    /// place for future device-specific tuning (mouse vs touch vs stylus).
    #[must_use]
    pub fn with_kind(kind: PointerDeviceKind) -> Self {
        Self {
            kind,
            samples: [None; HISTORY_SIZE],
            index: 0,
            since_last_sample: None,
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
    pub fn add_position(&mut self, time: Instant, position: Offset<Pixels>) {
        // Reject non-finite coordinates: NaN/Inf would poison the least-squares
        // fit (NaN comparisons defeat the singular-matrix guard) and propagate
        // into every downstream velocity. Pointer streams are untrusted input.
        if !position.dx.0.is_finite() || !position.dy.0.is_finite() {
            return;
        }
        // Mark "now" as the latest activity. Used by get_velocity_estimate()
        // to short-circuit when the pointer has been still for >= 40 ms.
        // We use the real wall clock here, NOT the `time` argument, so a
        // caller that supplies synthetic timestamps (tests, frame-time
        // extrapolation, replay logs) still gets the correct stationary
        // signal.
        self.since_last_sample = Some(Instant::now());

        // Advance the write index, wrapping at HISTORY_SIZE.
        self.index = (self.index + 1) % HISTORY_SIZE;
        self.samples[self.index] = Some(PointAtTime { time, position });
    }

    /// Reset the tracker, discarding all samples.
    pub fn reset(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
        self.since_last_sample = None;
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
    /// This is the Rust port of Flutter's `getVelocityEstimate()`.
    pub fn get_velocity_estimate(&self) -> Option<VelocityEstimate> {
        // Pointer has been still for >= 40 ms → velocity is exactly zero with
        // perfect confidence. Flutter returns a fully-populated
        // VelocityEstimate so callers can still ask for `duration` / `offset`.
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
            xs[n] = sample.position.dx.get() as f64;
            ys[n] = sample.position.dy.get() as f64;
            ws[n] = 1.0; // Uniform weights — Flutter's `PolynomialFitLeastSquares`.
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
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                newest.time.saturating_duration_since(oldest.time),
                1.0,
            ));
        }

        // Fit a quadratic in milliseconds; velocity in px/ms is the linear
        // coefficient. Scale to px/s (× 1000).
        let x_fit = LeastSquaresSolver::new(&ts[..n], &xs[..n], &ws[..n]).solve(POLYNOMIAL_DEGREE);
        let y_fit = LeastSquaresSolver::new(&ts[..n], &ys[..n], &ws[..n]).solve(POLYNOMIAL_DEGREE);

        match (x_fit, y_fit) {
            (Some(xf), Some(yf)) => Some(VelocityEstimate::new(
                newest.position - oldest.position,
                Offset::new(
                    Pixels((xf.coefficients[1] * 1000.0) as f32),
                    Pixels((yf.coefficients[1] * 1000.0) as f32),
                ),
                newest.time.saturating_duration_since(oldest.time),
                (xf.confidence * yf.confidence) as f32,
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
    /// zero. This is the canonical call site for "fling this view" — Flutter
    /// uses `getVelocity()` in the drag-end callback for the same purpose.
    pub fn get_velocity(&self) -> Velocity {
        match self.get_velocity_estimate() {
            Some(est) if est.pixels_per_second != Offset::ZERO => {
                Velocity::new(est.pixels_per_second)
            }
            _ => Velocity::ZERO,
        }
    }

    /// Velocity for fling detection.
    ///
    /// When `allow_slow` is `false` (the typical case), the result is
    /// [`Velocity::ZERO`] for any motion under ~50 px/s — the threshold
    /// Flutter's `VerticalDragGestureRecognizer.isFlingGesture` checks
    /// against (it requires the offset between the up and down events to
    /// exceed a slop, combined with a non-trivial velocity). When
    /// `allow_slow` is `true`, the raw estimate is returned even at very
    /// low speeds — useful for snap-back animations and small-list
    /// micro-scrolls.
    pub fn get_fling_velocity(&self, allow_slow: bool) -> Velocity {
        let velocity = self.get_velocity();
        if allow_slow {
            return velocity;
        }
        // ~50 px/s threshold. Below that, the gesture is not a fling.
        const MIN_FLING_SPEED_PX_S: f32 = 50.0;
        if velocity.pixels_per_second.dx.get().abs() < MIN_FLING_SPEED_PX_S
            && velocity.pixels_per_second.dy.get().abs() < MIN_FLING_SPEED_PX_S
        {
            return Velocity::ZERO;
        }
        velocity
    }

    /// Flutter-port alias for [`Self::get_velocity_estimate`].
    #[inline]
    pub fn estimate(&self) -> Option<VelocityEstimate> {
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
/// The 20-slot history is larger than the 4 used by the maths — Flutter
/// keeps the extra slots so the `VelocityEstimate.offset` (computed as
/// `newest - oldest`) is large enough to be recognised as a fling by
/// `VerticalDragGestureRecognizer.isFlingGesture`.
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
    pub fn add_position(&mut self, time: Instant, position: Offset<Pixels>) {
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
        // Stationary? Flutter's contract: report zero with confidence 1.0.
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
        let velocity = self.get_velocity();
        if allow_slow {
            return velocity;
        }
        const MIN_FLING_SPEED: f32 = 50.0;
        if velocity.pixels_per_second.dx.get().abs() < MIN_FLING_SPEED
            && velocity.pixels_per_second.dy.get().abs() < MIN_FLING_SPEED
        {
            return Velocity::ZERO;
        }
        velocity
    }

    /// The raw weighted-average velocity, regardless of the
    /// "stationary for 40 ms" gate.
    fn estimated_velocity(&self) -> Offset<Pixels> {
        // We do the weighted sum in f64 for precision (weights are 0.6 /
        // 0.35 / 0.05 and would lose bits through f32), then convert at
        // the end. `Pixels` is a `#[repr(transparent)]` newtype around f32.
        let v = |offset: isize| self.two_sample_velocity_at_f64(offset);
        let dx = v(-2).0 * self.weights[0] + v(-1).0 * self.weights[1] + v(0).0 * self.weights[2];
        let dy = v(-2).1 * self.weights[0] + v(-1).1 * self.weights[1] + v(0).1 * self.weights[2];
        Offset::new(Pixels(dx as f32), Pixels(dy as f32))
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
        // we preserve precision the way Flutter does.
        let dt_us = end.time.saturating_duration_since(start.time).as_micros();
        if dt_us == 0 {
            return (0.0, 0.0);
        }
        let dt_ms = dt_us as f64 / 1000.0;
        // (end - start) is in pixels; divide by dt_ms to get px/ms; × 1000 = px/s.
        let dx_px_s = (end.position.dx.get() - start.position.dx.get()) as f64 * 1000.0 / dt_ms;
        let dy_px_s = (end.position.dy.get() - start.position.dy.get()) as f64 * 1000.0 / dt_ms;
        (dx_px_s, dy_px_s)
    }

    /// Flutter-port alias for [`Self::get_velocity_estimate`].
    #[inline]
    pub fn estimate(&self) -> Option<VelocityEstimate> {
        self.get_velocity_estimate()
    }

    /// Velocity as a [`Velocity`]. [`Velocity::ZERO`] when the estimate is
    /// missing or its velocity is zero.
    pub fn get_velocity(&self) -> Velocity {
        match self.get_velocity_estimate() {
            Some(est) if est.pixels_per_second != Offset::ZERO => {
                Velocity::new(est.pixels_per_second)
            }
            _ => Velocity::ZERO,
        }
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
    pub fn add_position(&mut self, time: Instant, position: Offset<Pixels>) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_swipe_x(
        duration_ms: u64,
        samples: usize,
        slope_px_per_s: f32,
    ) -> Vec<(Instant, Offset<Pixels>)> {
        let start = Instant::now();
        let dt = Duration::from_millis(duration_ms / samples as u64);
        (0..samples)
            .map(|i| {
                let t = start + dt * i as u32;
                let pos = Offset::new(
                    Pixels(slope_px_per_s * (i as f32 * dt.as_secs_f32())),
                    Pixels(0.0),
                );
                (t, pos)
            })
            .collect()
    }

    #[test]
    fn zero_velocity_is_zero() {
        let v = Velocity::ZERO;
        assert_eq!(v, Velocity::ZERO);
        assert_eq!(v.magnitude(), 0.0);
    }

    #[test]
    fn magnitude_and_direction() {
        let v = Velocity::new(Offset::new(Pixels(3.0), Pixels(4.0)));
        assert!((v.magnitude() - 5.0).abs() < 0.001);
    }

    #[test]
    fn empty_tracker_has_no_estimate() {
        let tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        assert_eq!(tracker.get_velocity(), Velocity::ZERO);
        assert!(!tracker.has_sufficient_data());
    }

    #[test]
    fn single_sample_returns_zero() {
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        tracker.add_position(Instant::now(), Offset::new(Pixels(0.0), Pixels(0.0)));
        assert_eq!(tracker.get_velocity(), Velocity::ZERO);
    }

    #[test]
    fn non_finite_position_is_rejected() {
        // NaN/Inf coordinates (untrusted pointer input) must not poison the
        // estimate. Feeding them is a no-op; a following valid swipe still
        // produces a finite velocity.
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        let start = Instant::now();
        tracker.add_position(start, Offset::new(Pixels(f32::NAN), Pixels(0.0)));
        tracker.add_position(start, Offset::new(Pixels(0.0), Pixels(f32::INFINITY)));
        for i in 0..5 {
            let t = start + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)));
        }
        assert!(
            tracker.get_velocity().magnitude().is_finite(),
            "velocity must stay finite after NaN/Inf input"
        );
    }

    #[test]
    fn horizontal_swipe_matches_slope() {
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        // 1000 px/s over 100 ms, 10 samples → slope 1000 px/s, dx ≈ 1000.
        for (t, p) in linear_swipe_x(100, 10, 1000.0) {
            tracker.add_position(t, p);
        }
        let v = tracker.get_velocity();
        assert!(
            v.pixels_per_second.dx.get() > 800.0,
            "expected dx > 800, got {}",
            v.pixels_per_second.dx.get()
        );
        assert!(v.pixels_per_second.dx.get() < 1200.0);
        assert!(v.pixels_per_second.dy.get().abs() < 100.0);
    }

    #[test]
    fn vertical_swipe_matches_slope() {
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        let start = Instant::now();
        for i in 0..10 {
            tracker.add_position(
                start + Duration::from_millis(i * 10),
                Offset::new(Pixels(0.0), Pixels(i as f32 * 10.0)),
            );
        }
        let v = tracker.get_velocity();
        assert!(v.pixels_per_second.dx.get().abs() < 100.0);
        assert!(v.pixels_per_second.dy.get() > 800.0);
    }

    #[test]
    fn stationary_pointer_reports_zero() {
        // 40 ms after the last sample, the tracker must report zero.
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        let start = Instant::now();
        for i in 0..10 {
            tracker.add_position(
                start + Duration::from_millis(i * 10),
                Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)),
            );
        }
        std::thread::sleep(ASSUME_POINTER_STOPPED + Duration::from_millis(20));
        let estimate = tracker.get_velocity_estimate().expect("non-empty");
        assert_eq!(estimate.pixels_per_second, Offset::ZERO);
        assert_eq!(estimate.confidence, 1.0);
    }

    #[test]
    fn fling_velocity_threshold() {
        // 100 px/s swipe → below the 50 px/s-per-axis fling threshold? No,
        // 100 > 50. So it should be a fling.
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        for (t, p) in linear_swipe_x(100, 10, 100.0) {
            tracker.add_position(t, p);
        }
        let fling = tracker.get_fling_velocity(false);
        assert!(fling != Velocity::ZERO);

        // 20 px/s swipe → well below threshold, must be zero.
        let mut tracker2 = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        for (t, p) in linear_swipe_x(100, 10, 20.0) {
            tracker2.add_position(t, p);
        }
        let no_fling = tracker2.get_fling_velocity(false);
        assert_eq!(no_fling, Velocity::ZERO);

        // allow_slow lifts the threshold.
        let slow_ok = tracker2.get_fling_velocity(true);
        assert!(slow_ok != Velocity::ZERO);
    }

    #[test]
    fn ios_fling_matches_weighted_average() {
        let mut tracker = IosFlingVelocityTracker::with_kind(PointerDeviceKind::Touch);
        // 10 samples at 10 ms intervals, dx ramping by 10 px each step.
        // → per-step velocity 1000 px/s for every adjacent pair, so all
        // three weighted 2-point velocities are 1000 px/s.
        let start = Instant::now();
        for i in 0..10 {
            tracker.add_position(
                start + Duration::from_millis(i * 10),
                Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)),
            );
        }
        let v = tracker.get_velocity();
        assert!(
            (v.pixels_per_second.dx.get() - 1000.0).abs() < 1.0,
            "expected ~1000 px/s, got {}",
            v.pixels_per_second.dx.get()
        );
    }

    #[test]
    fn reset_clears_state() {
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        // Three samples separated by 10 ms — enough for has_sufficient_data.
        let start = Instant::now();
        for i in 0..3 {
            tracker.add_position(
                start + Duration::from_millis(i * 10),
                Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)),
            );
        }
        assert!(tracker.has_sufficient_data());
        tracker.reset();
        assert!(!tracker.has_sufficient_data());
        assert_eq!(tracker.sample_count(), 0);
    }

    #[test]
    fn velocity_estimate_has_high_confidence_for_linear_data() {
        let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
        for (t, p) in linear_swipe_x(100, 10, 1000.0) {
            tracker.add_position(t, p);
        }
        let est = tracker.get_velocity_estimate().expect("non-empty");
        assert!(
            est.confidence > 0.9,
            "confidence {} should be > 0.9 for linear data",
            est.confidence
        );
    }

    #[test]
    fn kind_is_recorded() {
        let t = VelocityTracker::with_kind(PointerDeviceKind::Stylus);
        assert_eq!(t.kind(), PointerDeviceKind::Stylus);
    }

    proptest::proptest! {
        /// Finite positions at monotonic times never produce a non-finite
        /// velocity, and the estimate's confidence stays in [0, 1].
        #[test]
        fn velocity_finite_and_confidence_bounded(
            xs in proptest::collection::vec(-1e4f32..1e4, 2..=20),
        ) {
            let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
            let start = Instant::now();
            for (i, &x) in xs.iter().enumerate() {
                tracker.add_position(
                    start + Duration::from_millis(i as u64 * 8),
                    Offset::new(Pixels(x), Pixels(0.0)),
                );
            }
            proptest::prop_assert!(
                tracker.get_velocity().magnitude().is_finite(),
                "velocity must be finite for finite input"
            );
            if let Some(est) = tracker.get_velocity_estimate() {
                proptest::prop_assert!(
                    (0.0..=1.0).contains(&est.confidence),
                    "confidence {} out of [0,1]",
                    est.confidence
                );
            }
        }
    }
}
