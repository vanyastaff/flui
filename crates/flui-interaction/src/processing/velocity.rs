//! Velocity estimation for gesture recognition
//!
//! One sample buffer supports four explicitly selected release estimators:
//!
//! - [`VelocityTracker`] — least-squares polynomial regression on a 20-sample
//!   circular buffer. This is the default and the one the core gesture
//!   pipeline uses.
//! - [`VelocityEstimator::Impulse`] — velocity-change integration.
//! - [`VelocityEstimator::Ios`] and [`VelocityEstimator::Macos`] — weighted
//!   averages of three adjacent interval velocities, with different weights.
//!
//! All defaults use least squares. Applications select another algorithm in
//! [`GestureSettings`](crate::settings::GestureSettings); no OS policy is read.
//!
//! # Algorithm
//!
//! The tracker keeps a 20-slot circular buffer of `(time, position)` samples.
//! Least squares and impulse walk backwards from the newest sample, stopping when either the
//! horizon (100 ms) is exceeded or the gap between consecutive samples
//! exceeds 40 ms (the pointer is considered stationary). For the least-
//! squares flavour, the surviving samples are fed to `LeastSquaresSolver`
//! which fits a quadratic polynomial in time and reports its derivative at
//! `t = 0` as the velocity. When the samples carry fewer than three distinct
//! timestamps (moves drained in one frame share one) the quadratic is
//! rank-deficient and a straight line is fitted instead, so a moving pointer
//! never reports zero velocity just because its samples were batched.
//!
//! Confidence is the product of the R² fit quality of the x and y
//! polynomials; a perfect linear swipe gives 1.0, a noisy curve gives
//! something close to 0.0. Weighted estimators use the three newest interval
//! velocities and report confidence 1.0 without making a fit-quality claim.
//!
//! # Numerical contract
//!
//! For any sequence of finite samples — duplicate or out-of-order
//! timestamps, sub-microsecond spacing, coordinates anywhere in the `f64`
//! range — every tracker publishes a finite estimate whose speed is at most
//! [`DEFAULT_MAX_FLING_VELOCITY`], in the direction of the measured motion.
//! A gesture with a stricter configured maximum clamps further through
//! [`GestureSettings::clamp_fling_velocity`](crate::settings::GestureSettings::clamp_fling_velocity).
//!
//! # Stop detection
//!
//! The "pointer stopped" gate compares the newest sample's time with the
//! time of the query. [`VelocityTracker::estimate_at`] and
//! [`VelocityTracker::velocity_at`] take that time from the caller — the
//! same clock that stamped the samples, so virtual clocks, replays and
//! tests are deterministic.
//!
//! # Example
//!
//! ```rust
//! use web_time::{Duration, Instant};
//!
//! use flui_interaction::processing::VelocityTracker;
//! use flui_foundation::geometry::Offset;
//! use flui_interaction::PointerKind;
//!
//! let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
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
//! let query = start + Duration::from_millis(90);
//! let _estimate = tracker.estimate_at(query);
//! let _velocity = tracker.velocity_at(query);
//! ```

use web_time::{Duration, Instant};

use crate::events::PointerKind;
use crate::settings::DEFAULT_MAX_FLING_VELOCITY;
pub use crate::velocity::{Velocity, VelocityEstimate};
use flui_foundation::geometry::Offset;

use super::lsq_solver::{MAX_SAMPLES, PolynomialFit, solve_two};

/// The algorithm used to estimate release velocity from admitted pointer history.
///
/// This is an authored interaction policy. Choosing an algorithm does not read
/// OS preferences or promise an exact match to a native scroll view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum VelocityEstimator {
    /// Quadratic least-squares regression, falling back to a line when needed.
    #[default]
    LeastSquares,
    /// Integrate changes in velocity as impulse work over the sample window.
    Impulse,
    /// Weight the three newest interval velocities by 0.6, 0.35 and 0.05.
    Ios,
    /// Weight the three newest interval velocities by 0.15, 0.65 and 0.2.
    Macos,
}

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

/// Polynomial degree for the least-squares fit. Quadratic, falling back to
/// linear when the sample times cannot determine a quadratic.
const POLYNOMIAL_DEGREE: usize = 2;

/// The distance between two instants, whichever is later.
fn time_between(a: Instant, b: Instant) -> Duration {
    if a >= b {
        a.duration_since(b)
    } else {
        b.duration_since(a)
    }
}

/// Bound a measured velocity: finite, at most [`DEFAULT_MAX_FLING_VELOCITY`],
/// direction kept. A NaN component (no direction to keep) gives zero.
fn bounded(pixels_per_second: Offset<f64>) -> Offset<f64> {
    Velocity::new(pixels_per_second)
        .clamp_magnitude(0.0, DEFAULT_MAX_FLING_VELOCITY)
        .pixels_per_second
}

/// `newest - oldest`, saturated to the finite range: two coordinates of
/// opposite sign near `f64::MAX` would otherwise subtract to infinity.
fn finite_offset(newest: Offset<f64>, oldest: Offset<f64>) -> Offset<f64> {
    let saturate = |v: f64| v.clamp(f64::MIN, f64::MAX);
    let delta = newest - oldest;
    Offset::new(saturate(delta.dx), saturate(delta.dy))
}

/// `time - reference` in milliseconds, negative when `time` is earlier.
fn signed_ms(time: Instant, reference: Instant) -> f64 {
    match time.checked_duration_since(reference) {
        Some(later) => later.as_secs_f64() * 1000.0,
        None => -(reference.duration_since(time).as_secs_f64() * 1000.0),
    }
}

/// The estimate reported once the pointer is known to have stopped: zero
/// velocity with full confidence.
const STOPPED: VelocityEstimate =
    VelocityEstimate::new(Offset::ZERO, Offset::ZERO, Duration::ZERO, 1.0);

/// The velocity an estimate carries, or [`Velocity::ZERO`] when there is no
/// estimate or it reports no motion.
///
/// Missing-estimate and zero-motion cases answer alike.
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
    kind: PointerKind,

    estimator: VelocityEstimator,

    /// Circular buffer of samples. Empty slots are `None` so we can
    /// distinguish "slot not yet written" from a sample at `Instant::EPOCH`.
    samples: [Option<PointAtTime>; HISTORY_SIZE],

    /// Index of the most recently written sample. Wraps around modulo
    /// `HISTORY_SIZE`.
    index: usize,

    /// Memoized result of the buffer-pure part of [`Self::estimate_at`]
    /// (the selected estimator). Invalidated whenever
    /// the sample buffer changes ([`Self::add_position`] / [`Self::reset`]).
    ///
    /// Only the buffer-pure computation is cached; the time-dependent
    /// "stationary for 40 ms" gate is re-evaluated on every call, so a cached
    /// fit is returned only while the pointer is still moving. This collapses
    /// repeated velocity and estimate queries over unchanged samples from
    /// two O(N) QR solves to one.
    cached_fit: Option<VelocityEstimate>,
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::with_kind(PointerKind::Touch)
    }
}

impl VelocityTracker {
    /// Construct a new velocity tracker for the given pointer device kind.
    ///
    /// Uses least squares; `kind` records the admitted source independently
    /// of the authored algorithm choice.
    #[must_use]
    pub fn with_kind(kind: PointerKind) -> Self {
        Self::with_estimator(kind, VelocityEstimator::LeastSquares)
    }

    /// Construct a tracker with an explicit release-velocity policy.
    ///
    /// The policy stays fixed until this tracker is replaced; resetting samples
    /// does not change it. Queries share the same sample-clock stop gate.
    #[must_use]
    pub fn with_estimator(kind: PointerKind, estimator: VelocityEstimator) -> Self {
        Self {
            kind,
            estimator,
            samples: [None; HISTORY_SIZE],
            index: 0,
            cached_fit: None,
        }
    }

    /// The kind of pointer this tracker is for.
    #[inline]
    pub fn kind(&self) -> PointerKind {
        self.kind
    }

    /// Record a position at the given time.
    ///
    /// O(1). The samples are stored in a 20-slot circular buffer; older
    /// samples are silently overwritten.
    ///
    /// The `time` parameter is the timestamp used for the velocity fit and
    /// for [`Self::estimate_at`]'s stop gate: the pointer event's time, or any
    /// other clock the caller also queries with. Samples need not arrive in
    /// time order. A non-finite position is ignored.
    pub fn add_position(&mut self, time: Instant, position: Offset<f64>) {
        // Reject non-finite coordinates: NaN/Inf would poison the least-squares
        // fit (NaN comparisons defeat the singular-matrix guard) and propagate
        // into every downstream velocity. Pointer streams are untrusted input.
        if !position.dx.is_finite() || !position.dy.is_finite() {
            return;
        }
        // The sample buffer is about to change, so any memoized fit is stale.
        self.cached_fit = None;

        // Advance the write index, wrapping at HISTORY_SIZE.
        self.index = (self.index + 1) % HISTORY_SIZE;
        self.samples[self.index] = Some(PointAtTime { time, position });
    }

    /// Reset the tracker, discarding all samples.
    pub fn reset(&mut self) {
        self.samples = [None; HISTORY_SIZE];
        self.index = 0;
        self.cached_fit = None;
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

    /// The velocity estimate as of `now`, on the clock that stamped the
    /// samples.
    ///
    /// When `now` is 40 ms or more after the newest sample's time, the
    /// pointer has stopped and the estimate is zero with full confidence.
    /// A `now` before the newest sample (a query racing a late sample) is
    /// treated as "still moving". Returns `None` if the tracker has no
    /// samples. The result is finite and its speed is at most
    /// [`DEFAULT_MAX_FLING_VELOCITY`].
    ///
    /// Takes `&mut self` to cache the buffer-pure estimate until the next
    /// sample or reset. The stop gate is checked on every query.
    #[must_use]
    pub fn estimate_at(&mut self, now: Instant) -> Option<VelocityEstimate> {
        let newest = self.samples[self.index]?;
        let stopped = now
            .checked_duration_since(newest.time)
            .is_some_and(|gap| gap >= ASSUME_POINTER_STOPPED);
        self.estimate_unless_stopped(stopped)
    }

    /// The velocity as of `now`, on the clock that stamped the samples:
    /// [`Self::estimate_at`]'s velocity, or [`Velocity::ZERO`] without an
    /// estimate.
    #[must_use]
    pub fn velocity_at(&mut self, now: Instant) -> Velocity {
        velocity_from_estimate(self.estimate_at(now))
    }

    /// The stop-gated, memoized estimate on the caller's sample clock.
    fn estimate_unless_stopped(&mut self, stopped: bool) -> Option<VelocityEstimate> {
        // A stopped pointer has exactly zero velocity with perfect confidence.
        // Time-dependent, so never cached.
        if stopped {
            return Some(STOPPED);
        }

        // Reuse the memoized fit if the sample buffer hasn't changed since it
        // was computed. `VelocityEstimate` is `Copy`, so this is a cheap read.
        if let Some(cached) = self.cached_fit {
            return Some(cached);
        }

        let fit = match self.estimator {
            VelocityEstimator::LeastSquares => self.compute_estimate(),
            VelocityEstimator::Impulse => self.compute_impulse(),
            VelocityEstimator::Ios => self.compute_weighted([0.6, 0.35, 0.05]),
            VelocityEstimator::Macos => self.compute_weighted([0.15, 0.65, 0.2]),
        }?;
        self.cached_fit = Some(fit);
        Some(fit)
    }

    /// Walk the circular buffer back from the newest sample while the samples
    /// represent continuous motion: within [`HORIZON`] of the newest sample
    /// and no more than [`ASSUME_POINTER_STOPPED`] from their neighbour.
    /// Distances are absolute, so an out-of-order sample is measured by how
    /// far it is in time, not dropped at the boundary. `visit` receives each
    /// accepted sample with its signed time relative to the newest one, in
    /// milliseconds. Returns the newest sample, the oldest accepted one and
    /// how many were accepted.
    fn walk_window(
        &self,
        mut visit: impl FnMut(PointAtTime, f64),
    ) -> Option<(PointAtTime, PointAtTime, usize)> {
        let newest = self.samples[self.index]?;
        let mut previous = newest;
        let mut oldest = newest;
        let mut n = 0usize;
        let mut cursor = self.index;
        // Bound the walk at one full lap — anything beyond that is stale.
        for _ in 0..HISTORY_SIZE {
            let Some(sample) = self.samples[cursor] else {
                break;
            };
            let age = time_between(sample.time, newest.time);
            let gap = time_between(previous.time, sample.time);
            previous = sample;
            if age > HORIZON || gap > ASSUME_POINTER_STOPPED {
                break;
            }
            oldest = sample;
            visit(sample, signed_ms(sample.time, newest.time));
            n += 1;
            cursor = if cursor == 0 {
                HISTORY_SIZE - 1
            } else {
                cursor - 1
            };
        }
        Some((newest, oldest, n))
    }

    /// Compute the buffer-pure part of the velocity estimate: walk the
    /// circular buffer back from the newest sample and run the least-squares
    /// fit. Returns `None` only when the buffer is empty.
    ///
    /// This is a pure function of the sample buffer — it does not consult the
    /// time-dependent stationary gate, nor the memo cache — which is exactly
    /// what makes the cache in [`Self::estimate_at`] sound. O(N)
    /// where N ≤ `HISTORY_SIZE` (the buffer is bounded at 20 samples).
    #[inline(never)]
    fn compute_estimate(&self) -> Option<VelocityEstimate> {
        let mut xs = [0.0f64; HISTORY_SIZE];
        let mut ys = [0.0f64; HISTORY_SIZE];
        let mut ts = [0.0f64; HISTORY_SIZE];
        let mut n: usize = 0;
        let mut earliest: Option<Instant> = None;
        let mut latest: Option<Instant> = None;
        let (newest, oldest, _) = self.walk_window(|sample, t_ms| {
            earliest = Some(earliest.map_or(sample.time, |t| t.min(sample.time)));
            latest = Some(latest.map_or(sample.time, |t| t.max(sample.time)));
            ts[n] = t_ms;
            xs[n] = sample.position.dx;
            ys[n] = sample.position.dy;
            n += 1;
        })?;
        let ws = [1.0f64; HISTORY_SIZE]; // Uniform weights.
        let offset = finite_offset(newest.position, oldest.position);
        let duration = time_between(newest.time, oldest.time);

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
                span_ms = duration.as_secs_f64() * 1000.0,
                reason = "too_few_contiguous_samples",
                "velocity estimate: no fling"
            );
            return Some(VelocityEstimate::new(
                Offset::ZERO,
                Offset::ZERO,
                duration,
                1.0,
            ));
        }

        // Guard: if the total time window is effectively zero (all samples at
        // the same timestamp, which happens when pointer events arrive within a
        // single OS timer tick — common in headless tests with coarse clocks on
        // Windows), no polynomial in time can be fitted. Report zero velocity
        // with zero confidence so callers can still decide whether to spring
        // back based on position (e.g. overscroll), rather than silently
        // corrupting the simulation with NaN.
        let span = match (earliest, latest) {
            (Some(earliest), Some(latest)) => latest.duration_since(earliest),
            _ => Duration::ZERO,
        };
        let total_span_ms = span.as_secs_f64() * 1000.0;
        if span.is_zero() {
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
            return Some(VelocityEstimate::new(offset, Offset::ZERO, duration, 0.0));
        }

        // Fit a quadratic in milliseconds; velocity in px/ms is the linear
        // coefficient. Scale to px/s (× 1000). The x and y fits share the same
        // sample times and weights, so `solve_two` computes the QR
        // factorization once for both. Samples with only two distinct times
        // cannot determine a quadratic; a line still fits them.
        let fit = |degree| match solve_two(&ts[..n], &ws[..n], &xs[..n], &ys[..n], degree) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };
        let Some((x_fit, y_fit)) = fit(POLYNOMIAL_DEGREE).or_else(|| fit(1)) else {
            // Neither fit is determined: the times are distinct but closer than
            // the solver can separate. Same answer as the degenerate span.
            tracing::trace!(
                target: "flui.velocity",
                contiguous_samples = n,
                span_ms = total_span_ms,
                reason = "unsolvable_time_span",
                "velocity estimate: no fling"
            );
            return Some(VelocityEstimate::new(offset, Offset::ZERO, duration, 0.0));
        };
        let slope = |fit: &PolynomialFit| fit.coefficients[1] * 1000.0;
        Some(VelocityEstimate::new(
            offset,
            bounded(Offset::new(slope(&x_fit), slope(&y_fit))),
            duration,
            x_fit.confidence * y_fit.confidence,
        ))
    }

    /// Construct a touch-kind tracker. Equivalent to [`Self::default`].
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of contiguous samples in the walk window. O(HISTORY_SIZE).
    fn estimate_sample_count(&self) -> usize {
        self.walk_window(|_, _| {}).map_or(0, |(_, _, n)| n)
    }
}

impl VelocityTracker {
    /// Velocity estimate. The `pixels_per_second` is the weighted sum of
    /// 2-point velocities; the `confidence` is always 1.0 (the algorithm
    /// makes no claim about fit quality); `duration` and `offset` are
    /// computed from the newest and oldest eligible samples.
    fn compute_weighted(&self, weights: [f64; 3]) -> Option<VelocityEstimate> {
        let (newest, oldest, eligible_samples) = self.walk_window(|_, _| {})?;
        let estimated_velocity =
            bounded(self.estimated_weighted_velocity(weights, eligible_samples));

        Some(VelocityEstimate::new(
            finite_offset(newest.position, oldest.position),
            estimated_velocity,
            time_between(newest.time, oldest.time),
            1.0,
        ))
    }

    /// The raw weighted-average velocity, regardless of the
    /// "stationary for 40 ms" gate. Each two-point velocity is bounded
    /// before weighting, so the sum cannot overflow.
    fn estimated_weighted_velocity(
        &self,
        weights: [f64; 3],
        eligible_samples: usize,
    ) -> Offset<f64> {
        let v = |offset: isize, required_samples| {
            if eligible_samples < required_samples {
                return Offset::ZERO;
            }
            let (dx, dy) = self.two_sample_velocity_at_f64(offset);
            bounded(Offset::new(dx, dy))
        };
        let (a, b, c) = (v(-2, 4), v(-1, 3), v(0, 2));
        let dx = a.dx * weights[0] + b.dx * weights[1] + c.dx * weights[2];
        let dy = a.dy * weights[0] + b.dy * weights[1] + c.dy * weights[2];
        Offset::new(dx, dy)
    }

    /// The 2-point velocity at the given offset from the newest sample,
    /// returned in `(dx, dy)` f64 form for precision arithmetic.
    /// `offset = 0` is the most recent pair, `-1` is the one before, etc.
    fn two_sample_velocity_at_f64(&self, offset: isize) -> (f64, f64) {
        let end_idx = (self.index as isize + offset).rem_euclid(HISTORY_SIZE as isize) as usize;
        let start_idx = (end_idx as isize - 1).rem_euclid(HISTORY_SIZE as isize) as usize;
        let (Some(end), Some(start)) = (self.samples[end_idx], self.samples[start_idx]) else {
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
}

impl VelocityTracker {
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
            // Bounded per interval, so the squared term cannot overflow; the
            // published vector is bounded again as a whole.
            let v_curr = ((positions[i + 1] - positions[i]) / dts[i])
                .clamp(-DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MAX_FLING_VELOCITY);
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

    /// Buffer-pure impulse estimate; the caller applies the query-clock gate.
    fn compute_impulse(&self) -> Option<VelocityEstimate> {
        let newest = self.samples[self.index]?;

        // Walk backwards through the window (100 ms horizon / 40 ms gap),
        // collecting chronological samples for the impulse integration.
        let mut chron: [Option<PointAtTime>; HISTORY_SIZE] = [None; HISTORY_SIZE];
        let mut n = 0usize;
        let mut cursor = self.index;
        let mut previous = newest;
        for _ in 0..HISTORY_SIZE {
            let Some(sample) = self.samples[cursor] else {
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
            let s = chron[i].expect("BUG: walk wrote chron[0..n] and i < n");
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
            finite_offset(newest.position, oldest.position),
            bounded(Offset::new(vx, vy)),
            newest.time.saturating_duration_since(oldest.time),
            // The impulse model makes no fit-quality claim (AOSP reports the
            // value unconditionally).
            1.0,
        ))
    }
}
