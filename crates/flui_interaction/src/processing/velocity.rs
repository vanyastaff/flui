//! Velocity estimation using least squares polynomial regression
//!
//! This module provides accurate velocity estimation for gesture recognition,
//! following Flutter's `PolynomialFitLeastSquaresVelocityTracker` approach.
//!
//! # Algorithm
//!
//! Instead of simple linear velocity calculation, we use weighted least squares
//! polynomial regression to estimate velocity. This provides:
//!
//! - Better accuracy with noisy touch input
//! - Smoother velocity curves
//! - More accurate fling detection
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::velocity::VelocityTracker;
//!
//! let mut tracker = VelocityTracker::new();
//!
//! // Add samples as pointer moves
//! tracker.add_position(Instant::now(), Offset::new(0.0, 0.0));
//! // ... more samples ...
//! tracker.add_position(Instant::now(), Offset::new(100.0, 50.0));
//!
//! // Get estimated velocity
//! let velocity = tracker.velocity();
//! println!("Velocity: {} px/s", velocity.magnitude());
//! ```

use flui_types::geometry::Offset;
use std::time::{Duration, Instant};

// ============================================================================
// Constants
// ============================================================================

/// Maximum age of samples to consider (100ms)
const HORIZON: Duration = Duration::from_millis(100);

/// Minimum number of samples needed for velocity estimation
const MIN_SAMPLES: usize = 2;

/// Maximum samples to keep in the tracker
const MAX_SAMPLES: usize = 20;

/// Polynomial degree for least squares fit (2 = quadratic)
const POLYNOMIAL_DEGREE: usize = 2;

/// Minimum duration to compute velocity (1ms)
const MIN_DURATION: Duration = Duration::from_micros(1000);

// ============================================================================
// Velocity struct
// ============================================================================

/// Velocity in 2D space, measured in pixels per second.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Velocity {
    /// Velocity vector in pixels per second.
    pub pixels_per_second: Offset,
}

impl Velocity {
    /// Zero velocity.
    pub const ZERO: Self = Self {
        pixels_per_second: Offset::ZERO,
    };

    /// Create a new velocity from pixels per second.
    #[inline]
    pub const fn new(pixels_per_second: Offset) -> Self {
        Self { pixels_per_second }
    }

    /// Create zero velocity.
    #[inline]
    pub const fn zero() -> Self {
        Self::ZERO
    }

    /// Get the magnitude (speed) in pixels per second.
    #[inline]
    pub fn magnitude(&self) -> f32 {
        self.pixels_per_second.distance()
    }

    /// Get the direction as a unit vector, or None if velocity is zero.
    pub fn direction(&self) -> Option<Offset> {
        let mag = self.magnitude();
        if mag < f32::EPSILON {
            None
        } else {
            Some(Offset::new(
                self.pixels_per_second.dx / mag,
                self.pixels_per_second.dy / mag,
            ))
        }
    }

    /// Clamp velocity magnitude to a maximum value.
    pub fn clamp_magnitude(self, max: f32) -> Self {
        let mag = self.magnitude();
        if mag <= max {
            self
        } else {
            let scale = max / mag;
            Self {
                pixels_per_second: Offset::new(
                    self.pixels_per_second.dx * scale,
                    self.pixels_per_second.dy * scale,
                ),
            }
        }
    }
}

impl From<Offset> for Velocity {
    #[inline]
    fn from(pixels_per_second: Offset) -> Self {
        Self { pixels_per_second }
    }
}

impl From<Velocity> for Offset {
    #[inline]
    fn from(velocity: Velocity) -> Self {
        velocity.pixels_per_second
    }
}

// ============================================================================
// PositionSample
// ============================================================================

/// A single position sample with timestamp.
#[derive(Debug, Clone, Copy)]
struct PositionSample {
    /// When this sample was recorded.
    time: Instant,
    /// Position at this time.
    position: Offset,
}

// ============================================================================
// VelocityTracker
// ============================================================================

/// Tracks pointer positions and estimates velocity using polynomial regression.
///
/// This tracker uses weighted least squares polynomial fitting to estimate
/// velocity, providing more accurate results than simple linear calculation,
/// especially with noisy touch input.
///
/// # Algorithm
///
/// 1. Maintains a window of recent position samples (last 100ms)
/// 2. Applies exponential time-based weighting (recent samples weighted more)
/// 3. Fits a quadratic polynomial to x(t) and y(t) separately
/// 4. Velocity is the derivative of the polynomial at t=0 (current time)
///
/// # Example
///
/// ```rust,ignore
/// let mut tracker = VelocityTracker::new();
///
/// // Simulate a horizontal swipe
/// let start = Instant::now();
/// for i in 0..10 {
///     let t = start + Duration::from_millis(i * 10);
///     tracker.add_position(t, Offset::new(i as f32 * 10.0, 0.0));
/// }
///
/// let velocity = tracker.velocity();
/// // velocity.pixels_per_second.dx ≈ 1000.0 (100px in 100ms = 1000px/s)
/// ```
#[derive(Debug, Clone)]
pub struct VelocityTracker {
    /// Ring buffer of position samples.
    samples: Vec<PositionSample>,
    /// Strategy for velocity estimation.
    strategy: VelocityEstimationStrategy,
}

/// Strategy for velocity estimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VelocityEstimationStrategy {
    /// Least squares polynomial regression (most accurate, Flutter-style).
    #[default]
    LeastSquaresPolynomial,
    /// Simple linear regression (faster, less accurate).
    LinearRegression,
    /// Two-sample velocity (fastest, least accurate).
    TwoSample,
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl VelocityTracker {
    /// Create a new velocity tracker with default polynomial strategy.
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(MAX_SAMPLES),
            strategy: VelocityEstimationStrategy::default(),
        }
    }

    /// Create a velocity tracker with a specific estimation strategy.
    pub fn with_strategy(strategy: VelocityEstimationStrategy) -> Self {
        Self {
            samples: Vec::with_capacity(MAX_SAMPLES),
            strategy,
        }
    }

    /// Add a position sample.
    pub fn add_position(&mut self, time: Instant, position: Offset) {
        // Remove samples older than HORIZON
        let cutoff = time.checked_sub(HORIZON).unwrap_or(time);
        self.samples.retain(|s| s.time >= cutoff);

        // Add new sample
        self.samples.push(PositionSample { time, position });

        // Limit total samples
        if self.samples.len() > MAX_SAMPLES {
            self.samples.remove(0);
        }
    }

    /// Get the estimated velocity.
    ///
    /// Returns `Velocity::ZERO` if there aren't enough samples.
    pub fn velocity(&self) -> Velocity {
        if self.samples.len() < MIN_SAMPLES {
            return Velocity::ZERO;
        }

        match self.strategy {
            VelocityEstimationStrategy::LeastSquaresPolynomial => self.polynomial_velocity(),
            VelocityEstimationStrategy::LinearRegression => self.linear_velocity(),
            VelocityEstimationStrategy::TwoSample => self.two_sample_velocity(),
        }
    }

    /// Reset the tracker, clearing all samples.
    pub fn reset(&mut self) {
        self.samples.clear();
    }

    /// Returns the number of samples currently stored.
    #[inline]
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Returns true if there are enough samples to estimate velocity.
    #[inline]
    pub fn has_sufficient_data(&self) -> bool {
        self.samples.len() >= MIN_SAMPLES
    }

    // ========================================================================
    // Private: Velocity Estimation Methods
    // ========================================================================

    /// Least squares polynomial regression (Flutter-style).
    fn polynomial_velocity(&self) -> Velocity {
        let n = self.samples.len();
        if n < MIN_SAMPLES {
            return Velocity::ZERO;
        }

        // Reference time is the last sample
        let ref_time = self.samples[n - 1].time;

        // Convert samples to relative time (seconds before ref_time)
        let mut times: Vec<f64> = Vec::with_capacity(n);
        let mut x_positions: Vec<f64> = Vec::with_capacity(n);
        let mut y_positions: Vec<f64> = Vec::with_capacity(n);
        let mut weights: Vec<f64> = Vec::with_capacity(n);

        for sample in &self.samples {
            // Time in seconds (negative, going back from ref_time)
            let dt = ref_time.duration_since(sample.time).as_secs_f64();
            let t = -dt; // Negative because we're going back in time

            // Exponential time-based weighting (more recent = higher weight)
            // Weight decays with e^(-dt / tau) where tau = 50ms
            let weight = (-dt / 0.05).exp();

            times.push(t);
            x_positions.push(sample.position.dx as f64);
            y_positions.push(sample.position.dy as f64);
            weights.push(weight);
        }

        // Fit polynomial and get velocity (derivative at t=0)
        let vx = polynomial_fit_velocity(&times, &x_positions, &weights);
        let vy = polynomial_fit_velocity(&times, &y_positions, &weights);

        Velocity::new(Offset::new(vx as f32, vy as f32))
    }

    /// Simple linear regression.
    fn linear_velocity(&self) -> Velocity {
        let n = self.samples.len();
        if n < MIN_SAMPLES {
            return Velocity::ZERO;
        }

        let ref_time = self.samples[n - 1].time;

        // Compute means
        let mut sum_t = 0.0f64;
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;

        for sample in &self.samples {
            let dt = ref_time.duration_since(sample.time).as_secs_f64();
            sum_t += -dt;
            sum_x += sample.position.dx as f64;
            sum_y += sample.position.dy as f64;
        }

        let mean_t = sum_t / n as f64;
        let mean_x = sum_x / n as f64;
        let mean_y = sum_y / n as f64;

        // Compute slope (velocity)
        let mut num_x = 0.0f64;
        let mut num_y = 0.0f64;
        let mut denom = 0.0f64;

        for sample in &self.samples {
            let dt = ref_time.duration_since(sample.time).as_secs_f64();
            let t = -dt - mean_t;
            let x = sample.position.dx as f64 - mean_x;
            let y = sample.position.dy as f64 - mean_y;

            num_x += t * x;
            num_y += t * y;
            denom += t * t;
        }

        if denom.abs() < f64::EPSILON {
            return Velocity::ZERO;
        }

        let vx = num_x / denom;
        let vy = num_y / denom;

        Velocity::new(Offset::new(vx as f32, vy as f32))
    }

    /// Simple two-sample velocity (fastest).
    fn two_sample_velocity(&self) -> Velocity {
        let n = self.samples.len();
        if n < 2 {
            return Velocity::ZERO;
        }

        let oldest = &self.samples[0];
        let newest = &self.samples[n - 1];

        let dt = newest.time.duration_since(oldest.time);
        if dt < MIN_DURATION {
            return Velocity::ZERO;
        }

        let dt_secs = dt.as_secs_f32();
        let delta = newest.position - oldest.position;

        Velocity::new(Offset::new(delta.dx / dt_secs, delta.dy / dt_secs))
    }
}

// ============================================================================
// Polynomial Fitting
// ============================================================================

/// Fit a weighted polynomial and return the velocity (first derivative at t=0).
///
/// Uses weighted least squares to fit a polynomial of degree POLYNOMIAL_DEGREE
/// to the data, then returns the first derivative evaluated at t=0.
fn polynomial_fit_velocity(times: &[f64], values: &[f64], weights: &[f64]) -> f64 {
    let n = times.len();
    if n < MIN_SAMPLES {
        return 0.0;
    }

    // For quadratic fit: y = a + b*t + c*t²
    // Velocity at t=0 is b (the linear coefficient)
    //
    // Using normal equations: (X'WX) * coeffs = X'W * y
    // Where X is the Vandermonde matrix, W is diagonal weight matrix

    let degree = POLYNOMIAL_DEGREE.min(n - 1);

    // Build the weighted normal equations
    // For efficiency with small degree, we compute directly

    if degree == 1 {
        // Linear fit: y = a + b*t, velocity = b
        let mut sw = 0.0f64;
        let mut swt = 0.0f64;
        let mut swtt = 0.0f64;
        let mut swy = 0.0f64;
        let mut swty = 0.0f64;

        for i in 0..n {
            let w = weights[i];
            let t = times[i];
            let y = values[i];

            sw += w;
            swt += w * t;
            swtt += w * t * t;
            swy += w * y;
            swty += w * t * y;
        }

        let det = sw * swtt - swt * swt;
        if det.abs() < f64::EPSILON {
            return 0.0;
        }

        // b = (sw * swty - swt * swy) / det
        (sw * swty - swt * swy) / det
    } else {
        // Quadratic fit: y = a + b*t + c*t², velocity = b
        let mut m = [[0.0f64; 3]; 3]; // Normal matrix
        let mut v = [0.0f64; 3]; // Right-hand side

        for i in 0..n {
            let w = weights[i];
            let t = times[i];
            let y = values[i];

            let t2 = t * t;
            let t3 = t2 * t;
            let t4 = t2 * t2;

            m[0][0] += w;
            m[0][1] += w * t;
            m[0][2] += w * t2;
            m[1][1] += w * t2;
            m[1][2] += w * t3;
            m[2][2] += w * t4;

            v[0] += w * y;
            v[1] += w * t * y;
            v[2] += w * t2 * y;
        }

        // Symmetric matrix
        m[1][0] = m[0][1];
        m[2][0] = m[0][2];
        m[2][1] = m[1][2];

        // Solve using Gaussian elimination with partial pivoting
        solve_3x3(&m, &v).map(|coeffs| coeffs[1]).unwrap_or(0.0)
    }
}

/// Solve a 3x3 linear system using Gaussian elimination.
#[allow(clippy::needless_range_loop)] // Matrix operations require explicit indexing
fn solve_3x3(a: &[[f64; 3]; 3], b: &[f64; 3]) -> Option<[f64; 3]> {
    let mut a = *a;
    let mut b = *b;

    // Forward elimination with partial pivoting
    for i in 0..3 {
        // Find pivot
        let mut max_row = i;
        let mut max_val = a[i][i].abs();
        for k in (i + 1)..3 {
            if a[k][i].abs() > max_val {
                max_val = a[k][i].abs();
                max_row = k;
            }
        }

        if max_val < f64::EPSILON {
            return None; // Singular matrix
        }

        // Swap rows
        if max_row != i {
            a.swap(i, max_row);
            b.swap(i, max_row);
        }

        // Eliminate
        for k in (i + 1)..3 {
            let factor = a[k][i] / a[i][i];
            for j in i..3 {
                a[k][j] -= factor * a[i][j];
            }
            b[k] -= factor * b[i];
        }
    }

    // Back substitution
    let mut x = [0.0f64; 3];
    for i in (0..3).rev() {
        let mut sum = b[i];
        for j in (i + 1)..3 {
            sum -= a[i][j] * x[j];
        }
        if a[i][i].abs() < f64::EPSILON {
            return None;
        }
        x[i] = sum / a[i][i];
    }

    Some(x)
}

// ============================================================================
// VelocityEstimate (for prediction)
// ============================================================================

/// Velocity estimate with confidence information.
#[derive(Debug, Clone, Copy)]
pub struct VelocityEstimate {
    /// Estimated velocity.
    pub velocity: Velocity,
    /// Confidence in the estimate (0.0 - 1.0).
    pub confidence: f32,
    /// Duration of data used for estimation.
    pub duration: Duration,
    /// Number of samples used.
    pub sample_count: usize,
}

impl VelocityEstimate {
    /// Check if this estimate is reliable enough for fling detection.
    pub fn is_reliable(&self) -> bool {
        self.confidence > 0.5 && self.sample_count >= 3
    }
}

impl VelocityTracker {
    /// Get a velocity estimate with confidence information.
    pub fn estimate(&self) -> VelocityEstimate {
        let velocity = self.velocity();
        let n = self.samples.len();

        let (duration, confidence) = if n < 2 {
            (Duration::ZERO, 0.0)
        } else {
            let first = self.samples[0].time;
            let last = self.samples[n - 1].time;
            let dur = last.duration_since(first);

            // Confidence based on sample count and duration
            let count_factor = (n as f32 / 5.0).min(1.0);
            let duration_factor = (dur.as_secs_f32() / 0.05).min(1.0);
            let conf = count_factor * duration_factor;

            (dur, conf)
        };

        VelocityEstimate {
            velocity,
            confidence,
            duration,
            sample_count: n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_zero() {
        assert_eq!(Velocity::zero(), Velocity::ZERO);
        assert_eq!(Velocity::ZERO.magnitude(), 0.0);
    }

    #[test]
    fn test_velocity_magnitude() {
        let v = Velocity::new(Offset::new(3.0, 4.0));
        assert!((v.magnitude() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_velocity_direction() {
        let v = Velocity::new(Offset::new(10.0, 0.0));
        let dir = v.direction().unwrap();
        assert!((dir.dx - 1.0).abs() < 0.001);
        assert!(dir.dy.abs() < 0.001);

        assert!(Velocity::ZERO.direction().is_none());
    }

    #[test]
    fn test_velocity_clamp() {
        let v = Velocity::new(Offset::new(1000.0, 0.0));
        let clamped = v.clamp_magnitude(500.0);
        assert!((clamped.magnitude() - 500.0).abs() < 0.001);
    }

    #[test]
    fn test_tracker_empty() {
        let tracker = VelocityTracker::new();
        assert_eq!(tracker.velocity(), Velocity::ZERO);
        assert!(!tracker.has_sufficient_data());
    }

    #[test]
    fn test_tracker_single_sample() {
        let mut tracker = VelocityTracker::new();
        tracker.add_position(Instant::now(), Offset::new(0.0, 0.0));
        assert_eq!(tracker.velocity(), Velocity::ZERO);
        assert!(!tracker.has_sufficient_data());
    }

    #[test]
    fn test_tracker_horizontal_motion() {
        let mut tracker = VelocityTracker::new();
        let start = Instant::now();

        // 100 pixels in 100ms = 1000 px/s
        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(i as f32 * 10.0, 0.0));
        }

        let velocity = tracker.velocity();
        // Should be approximately 1000 px/s horizontal
        assert!(velocity.pixels_per_second.dx > 800.0);
        assert!(velocity.pixels_per_second.dx < 1200.0);
        assert!(velocity.pixels_per_second.dy.abs() < 100.0);
    }

    #[test]
    fn test_tracker_vertical_motion() {
        let mut tracker = VelocityTracker::new();
        let start = Instant::now();

        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(0.0, i as f32 * 10.0));
        }

        let velocity = tracker.velocity();
        assert!(velocity.pixels_per_second.dx.abs() < 100.0);
        assert!(velocity.pixels_per_second.dy > 800.0);
    }

    #[test]
    fn test_tracker_diagonal_motion() {
        let mut tracker = VelocityTracker::new();
        let start = Instant::now();

        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(i as f32 * 10.0, i as f32 * 10.0));
        }

        let velocity = tracker.velocity();
        // Both components should be around 1000 px/s
        assert!(velocity.pixels_per_second.dx > 800.0);
        assert!(velocity.pixels_per_second.dy > 800.0);
    }

    #[test]
    fn test_tracker_reset() {
        let mut tracker = VelocityTracker::new();
        tracker.add_position(Instant::now(), Offset::new(0.0, 0.0));
        tracker.add_position(Instant::now(), Offset::new(10.0, 0.0));

        assert!(tracker.has_sufficient_data());
        tracker.reset();
        assert!(!tracker.has_sufficient_data());
    }

    #[test]
    fn test_tracker_strategies() {
        let start = Instant::now();
        let samples: Vec<_> = (0..10)
            .map(|i| {
                (
                    start + Duration::from_millis(i * 10),
                    Offset::new(i as f32 * 10.0, 0.0),
                )
            })
            .collect();

        for strategy in [
            VelocityEstimationStrategy::LeastSquaresPolynomial,
            VelocityEstimationStrategy::LinearRegression,
            VelocityEstimationStrategy::TwoSample,
        ] {
            let mut tracker = VelocityTracker::with_strategy(strategy);
            for (t, pos) in &samples {
                tracker.add_position(*t, *pos);
            }

            let velocity = tracker.velocity();
            // All strategies should give roughly 1000 px/s
            assert!(
                velocity.pixels_per_second.dx > 500.0,
                "{:?} gave {}",
                strategy,
                velocity.pixels_per_second.dx
            );
        }
    }

    #[test]
    fn test_velocity_estimate() {
        let mut tracker = VelocityTracker::new();
        let start = Instant::now();

        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(i as f32 * 10.0, 0.0));
        }

        let estimate = tracker.estimate();
        assert!(estimate.is_reliable());
        assert_eq!(estimate.sample_count, 10);
        assert!(estimate.confidence > 0.5);
    }

    #[test]
    fn test_old_samples_removed() {
        let mut tracker = VelocityTracker::new();
        let start = Instant::now();

        // Add old sample
        tracker.add_position(start, Offset::ZERO);

        // Add recent samples (200ms later, beyond HORIZON)
        let recent = start + Duration::from_millis(200);
        for i in 0..5 {
            let t = recent + Duration::from_millis(i * 10);
            tracker.add_position(t, Offset::new(i as f32 * 10.0, 0.0));
        }

        // Old sample should be removed, only 5 recent samples remain
        assert_eq!(tracker.sample_count(), 5);
    }
}
