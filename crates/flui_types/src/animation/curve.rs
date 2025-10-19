//! Animation curves for interpolation.

use std::f32::consts::PI;

/// A mapping from the unit interval to the unit interval.
///
/// A curve must map `t=0.0` to `0.0` and `t=1.0` to `1.0`.
/// Similar to Flutter's `Curve`.
///
/// # Examples
///
/// ```
/// use flui_types::animation::Curve;
///
/// struct MyCurve;
///
/// impl Curve for MyCurve {
///     fn transform(&self, t: f32) -> f32 {
///         t * t // quadratic ease-in
///     }
/// }
///
/// let curve = MyCurve;
/// assert_eq!(curve.transform(0.0), 0.0);
/// assert_eq!(curve.transform(0.5), 0.25);
/// assert_eq!(curve.transform(1.0), 1.0);
/// ```
pub trait Curve {
    /// Returns the value of the curve at point `t`.
    ///
    /// The value of `t` must be between 0.0 and 1.0, inclusive.
    fn transform(&self, t: f32) -> f32;

    /// Returns a new curve that is the reversed curve of this one.
    fn flipped(&self) -> FlippedCurve<Self>
    where
        Self: Sized + Clone,
    {
        FlippedCurve { curve: self.clone() }
    }
}

/// A parametric curve in 2D space.
///
/// Similar to Flutter's `ParametricCurve<T>`.
pub trait ParametricCurve<T> {
    /// Returns the value of the curve at point `t`.
    fn transform(&self, t: f32) -> T;
}

/// A curve that maps a value in the unit interval to a 2D point.
///
/// Similar to Flutter's `Curve2D`.
pub trait Curve2D {
    /// Returns the point on the curve at parameter `t`.
    fn transform(&self, t: f32) -> Curve2DSample;
}

/// A sample point on a 2D curve.
///
/// Similar to Flutter's `Curve2DSample`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Curve2DSample {
    /// The value of the curve at this point.
    pub value: f32,
    /// The derivative (slope) of the curve at this point.
    pub derivative: f32,
}

impl Curve2DSample {
    /// Creates a new 2D curve sample.
    #[inline]
    #[must_use]
    pub const fn new(value: f32, derivative: f32) -> Self {
        Self { value, derivative }
    }
}

// ============================================================================
// Standard Curves
// ============================================================================

/// A linear curve.
///
/// The identity function that maps `t` to `t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Linear;

impl Curve for Linear {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        t.clamp(0.0, 1.0)
    }
}

/// A sawtooth curve that repeats.
///
/// Similar to Flutter's `SawTooth`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SawTooth {
    /// The number of repetitions of the sawtooth pattern.
    pub count: u32,
}

impl SawTooth {
    /// Creates a new sawtooth curve with the given count.
    #[inline]
    #[must_use]
    pub const fn new(count: u32) -> Self {
        Self { count }
    }
}

impl Curve for SawTooth {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        (t * self.count as f32).fract()
    }
}

/// A curve that is 0.0 until [begin], then curved from 0.0 to 1.0 at [begin]
/// and [end], then 1.0 after [end].
///
/// Similar to Flutter's `Interval`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interval<C: Curve = Linear> {
    /// The start of the interval (0.0 to 1.0).
    pub begin: f32,
    /// The end of the interval (0.0 to 1.0).
    pub end: f32,
    /// The curve to apply within the interval.
    pub curve: C,
}

impl<C: Curve> Interval<C> {
    /// Creates a new interval curve.
    #[inline]
    #[must_use]
    pub fn new(begin: f32, end: f32, curve: C) -> Self {
        assert!((0.0..=1.0).contains(&begin), "begin must be in range [0.0, 1.0]");
        assert!((0.0..=1.0).contains(&end), "end must be in range [0.0, 1.0]");
        assert!(end >= begin, "end must be >= begin");
        Self { begin, end, curve }
    }
}

impl Interval<Linear> {
    /// Creates a new interval curve with a linear curve.
    #[inline]
    #[must_use]
    pub fn linear(begin: f32, end: f32) -> Self {
        Self::new(begin, end, Linear)
    }
}

impl<C: Curve> Curve for Interval<C> {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        if t < self.begin {
            0.0
        } else if t > self.end {
            1.0
        } else if (self.end - self.begin).abs() < 1e-6 {
            if t < self.end { 0.0 } else { 1.0 }
        } else {
            let local_t = (t - self.begin) / (self.end - self.begin);
            self.curve.transform(local_t)
        }
    }
}

/// A curve that is 0.0 until [threshold], then jumps to 1.0.
///
/// Similar to Flutter's `Threshold`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Threshold {
    /// The threshold at which the curve jumps to 1.0.
    pub threshold: f32,
}

impl Threshold {
    /// Creates a new threshold curve.
    #[inline]
    #[must_use]
    pub fn new(threshold: f32) -> Self {
        assert!((0.0..=1.0).contains(&threshold), "threshold must be in range [0.0, 1.0]");
        Self { threshold }
    }
}

impl Curve for Threshold {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < self.threshold { 0.0 } else { 1.0 }
    }
}

// ============================================================================
// Cubic Curves
// ============================================================================

/// A cubic polynomial curve.
///
/// Similar to Flutter's `Cubic`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cubic {
    /// The x coordinate of the first control point.
    pub a: f32,
    /// The y coordinate of the first control point.
    pub b: f32,
    /// The x coordinate of the second control point.
    pub c: f32,
    /// The y coordinate of the second control point.
    pub d: f32,
}

impl Cubic {
    /// Creates a new cubic curve.
    pub const fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self { a, b, c, d }
    }

    /// Evaluates the cubic bezier curve at t.
    fn evaluate_cubic(&self, t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
        let t2 = t * t;
        let t3 = t2 * t;
        let one_minus_t = 1.0 - t;
        let one_minus_t2 = one_minus_t * one_minus_t;
        let one_minus_t3 = one_minus_t2 * one_minus_t;

        one_minus_t3 * p0 + 3.0 * one_minus_t2 * t * p1 + 3.0 * one_minus_t * t2 * p2 + t3 * p3
    }
}

impl Curve for Cubic {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        // Binary search to find t value that gives us the desired x
        let mut start = 0.0;
        let mut end = 1.0;

        for _ in 0..8 {
            let mid = (start + end) / 2.0;
            let x = self.evaluate_cubic(mid, 0.0, self.a, self.c, 1.0);

            if (x - t).abs() < 1e-6 {
                return self.evaluate_cubic(mid, 0.0, self.b, self.d, 1.0);
            }

            if x < t {
                start = mid;
            } else {
                end = mid;
            }
        }

        let mid = (start + end) / 2.0;
        self.evaluate_cubic(mid, 0.0, self.b, self.d, 1.0)
    }
}

// ============================================================================
// Elastic Curves
// ============================================================================

/// An oscillating curve that grows in magnitude while overshooting its bounds.
///
/// Similar to Flutter's `ElasticInCurve`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticInCurve {
    /// The period of oscillation.
    pub period: f32,
}

impl ElasticInCurve {
    /// Creates a new elastic-in curve with the given period.
    pub const fn new(period: f32) -> Self {
        Self { period }
    }
}

impl Default for ElasticInCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticInCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let s = self.period / 4.0;
        let t = t - 1.0;
        -((2.0_f32).powf(10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin())
    }
}

/// An oscillating curve that shrinks in magnitude while overshooting its bounds.
///
/// Similar to Flutter's `ElasticOutCurve`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticOutCurve {
    /// The period of oscillation.
    pub period: f32,
}

impl ElasticOutCurve {
    /// Creates a new elastic-out curve with the given period.
    pub const fn new(period: f32) -> Self {
        Self { period }
    }
}

impl Default for ElasticOutCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticOutCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let s = self.period / 4.0;
        (2.0_f32).powf(-10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin() + 1.0
    }
}

/// An oscillating curve that grows and then shrinks in magnitude while
/// overshooting its bounds.
///
/// Similar to Flutter's `ElasticInOutCurve`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticInOutCurve {
    /// The period of oscillation.
    pub period: f32,
}

impl ElasticInOutCurve {
    /// Creates a new elastic-in-out curve with the given period.
    pub const fn new(period: f32) -> Self {
        Self { period }
    }
}

impl Default for ElasticInOutCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticInOutCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let s = self.period / 4.0;
        let t = 2.0 * t - 1.0;

        if t < 0.0 {
            -0.5 * ((2.0_f32).powf(10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin())
        } else {
            0.5 * ((2.0_f32).powf(-10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin()) + 1.0
        }
    }
}

// ============================================================================
// Catmull-Rom Curves
// ============================================================================

/// A Catmull-Rom curve passing through a set of points.
///
/// Similar to Flutter's `CatmullRomCurve`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatmullRomCurve {
    /// The control points of the curve.
    pub points: Vec<(f32, f32)>,
    /// The tension parameter (0.0 = no tension, 0.5 = Catmull-Rom, 1.0 = tight).
    pub tension: f32,
}

impl CatmullRomCurve {
    /// Creates a new Catmull-Rom curve.
    #[inline]
    #[must_use]
    pub fn new(points: Vec<(f32, f32)>, tension: f32) -> Self {
        assert!(points.len() >= 2, "Must have at least 2 points");
        Self { points, tension }
    }

    /// Creates a Catmull-Rom curve with default tension (0.0).
    #[inline]
    #[must_use]
    pub fn with_points(points: Vec<(f32, f32)>) -> Self {
        Self::new(points, 0.0)
    }
}

impl Curve for CatmullRomCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        if self.points.len() == 1 {
            return self.points[0].1;
        }

        // Find the segment
        let segment_count = self.points.len() - 1;
        let t_scaled = t * segment_count as f32;
        let segment = (t_scaled.floor() as usize).min(segment_count - 1);
        let local_t = t_scaled - segment as f32;

        // Get the 4 control points for this segment
        let p0 = if segment > 0 { self.points[segment - 1] } else { self.points[0] };
        let p1 = self.points[segment];
        let p2 = self.points[segment + 1];
        let p3 = if segment + 2 < self.points.len() {
            self.points[segment + 2]
        } else {
            self.points[segment + 1]
        };

        // Catmull-Rom interpolation
        let t2 = local_t * local_t;
        let t3 = t2 * local_t;

        let v0 = (p2.1 - p0.1) * (1.0 - self.tension) * 0.5;
        let v1 = (p3.1 - p1.1) * (1.0 - self.tension) * 0.5;

        (2.0 * p1.1 - 2.0 * p2.1 + v0 + v1) * t3
            + (-3.0 * p1.1 + 3.0 * p2.1 - 2.0 * v0 - v1) * t2
            + v0 * local_t
            + p1.1
    }
}

/// A Catmull-Rom spline.
///
/// Similar to Flutter's `CatmullRomSpline`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatmullRomSpline {
    /// The control points of the spline.
    pub points: Vec<Curve2DSample>,
}

impl CatmullRomSpline {
    /// Creates a new Catmull-Rom spline.
    #[inline]
    #[must_use]
    pub fn new(points: Vec<Curve2DSample>) -> Self {
        assert!(points.len() >= 2, "Must have at least 2 points");
        Self { points }
    }
}

impl Curve2D for CatmullRomSpline {
    fn transform(&self, t: f32) -> Curve2DSample {
        let t = t.clamp(0.0, 1.0);

        if self.points.len() == 1 {
            return self.points[0];
        }

        // Find the segment
        let segment_count = self.points.len() - 1;
        let t_scaled = t * segment_count as f32;
        let segment = (t_scaled.floor() as usize).min(segment_count - 1);
        let local_t = t_scaled - segment as f32;

        // Get the 4 control points for this segment
        let p0 = if segment > 0 { self.points[segment - 1] } else { self.points[0] };
        let p1 = self.points[segment];
        let p2 = self.points[segment + 1];
        let p3 = if segment + 2 < self.points.len() {
            self.points[segment + 2]
        } else {
            self.points[segment + 1]
        };

        // Catmull-Rom interpolation for both value and derivative
        let t2 = local_t * local_t;
        let t3 = t2 * local_t;

        let v0_val = (p2.value - p0.value) * 0.5;
        let v1_val = (p3.value - p1.value) * 0.5;

        let value = (2.0 * p1.value - 2.0 * p2.value + v0_val + v1_val) * t3
            + (-3.0 * p1.value + 3.0 * p2.value - 2.0 * v0_val - v1_val) * t2
            + v0_val * local_t
            + p1.value;

        let v0_der = (p2.derivative - p0.derivative) * 0.5;
        let v1_der = (p3.derivative - p1.derivative) * 0.5;

        let derivative = (2.0 * p1.derivative - 2.0 * p2.derivative + v0_der + v1_der) * t3
            + (-3.0 * p1.derivative + 3.0 * p2.derivative - 2.0 * v0_der - v1_der) * t2
            + v0_der * local_t
            + p1.derivative;

        Curve2DSample::new(value, derivative)
    }
}

// ============================================================================
// Curve Modifiers
// ============================================================================

/// A curve that is the flipped version of another curve.
///
/// Flipping swaps the output: `transform(t)` becomes `1.0 - transform(t)`.
///
/// Similar to Flutter's `FlippedCurve`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FlippedCurve<C: Curve> {
    /// The curve to flip.
    pub curve: C,
}

impl<C: Curve> FlippedCurve<C> {
    /// Creates a new flipped curve.
    #[inline]
    #[must_use]
    pub const fn new(curve: C) -> Self {
        Self { curve }
    }
}

impl<C: Curve> Curve for FlippedCurve<C> {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        1.0 - self.curve.transform(t)
    }
}

/// A curve that is the reversed version of another curve.
///
/// Reversing swaps the input: `transform(t)` becomes `transform(1.0 - t)`.
///
/// Similar to Flutter's `ReverseCurve` (but Flutter doesn't have this as a separate type).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReverseCurve<C: Curve> {
    /// The curve to reverse.
    pub curve: C,
}

impl<C: Curve> ReverseCurve<C> {
    /// Creates a new reversed curve.
    #[inline]
    #[must_use]
    pub const fn new(curve: C) -> Self {
        Self { curve }
    }
}

impl<C: Curve> Curve for ReverseCurve<C> {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        self.curve.transform(1.0 - t)
    }
}

// ============================================================================
// Predefined Curves
// ============================================================================

/// A collection of commonly used curves.
///
/// Similar to Flutter's `Curves` class.
pub struct Curves;

#[allow(non_upper_case_globals)]
impl Curves {
    /// A linear curve (the identity function).
    pub const Linear: Linear = Linear;

    /// A cubic ease-in curve.
    pub const EaseIn: Cubic = Cubic::new(0.42, 0.0, 1.0, 1.0);

    /// A cubic ease-out curve.
    pub const EaseOut: Cubic = Cubic::new(0.0, 0.0, 0.58, 1.0);

    /// A cubic ease-in-out curve.
    pub const EaseInOut: Cubic = Cubic::new(0.42, 0.0, 0.58, 1.0);

    /// A curve that is fast at the beginning and slow at the end.
    pub const FastOutSlowIn: Cubic = Cubic::new(0.4, 0.0, 0.2, 1.0);

    /// A curve that starts slowly and ends quickly.
    pub const SlowOutFastIn: Cubic = Cubic::new(0.0, 0.0, 0.2, 1.0);

    /// A curve that starts quickly, slows down, and then ends quickly.
    pub const EaseInOutCubic: Cubic = Cubic::new(0.645, 0.045, 0.355, 1.0);

    /// A curve that starts slowly and ends at full speed.
    pub const EaseInSine: Cubic = Cubic::new(0.47, 0.0, 0.745, 0.715);

    /// A curve that starts at full speed and ends slowly.
    pub const EaseOutSine: Cubic = Cubic::new(0.39, 0.575, 0.565, 1.0);

    /// A curve that starts slowly, speeds up, and then ends slowly.
    pub const EaseInOutSine: Cubic = Cubic::new(0.445, 0.05, 0.55, 0.95);

    /// A curve that starts slowly and accelerates exponentially.
    pub const EaseInExpo: Cubic = Cubic::new(0.95, 0.05, 0.795, 0.035);

    /// A curve that starts quickly and decelerates exponentially.
    pub const EaseOutExpo: Cubic = Cubic::new(0.19, 1.0, 0.22, 1.0);

    /// A curve that accelerates and decelerates exponentially.
    pub const EaseInOutExpo: Cubic = Cubic::new(1.0, 0.0, 0.0, 1.0);

    /// A curve that starts slowly and accelerates sharply.
    pub const EaseInCirc: Cubic = Cubic::new(0.6, 0.04, 0.98, 0.335);

    /// A curve that starts quickly and decelerates sharply.
    pub const EaseOutCirc: Cubic = Cubic::new(0.075, 0.82, 0.165, 1.0);

    /// A curve that accelerates and decelerates sharply.
    pub const EaseInOutCirc: Cubic = Cubic::new(0.785, 0.135, 0.15, 0.86);

    /// A curve that starts slowly and overshoots at the end.
    pub const EaseInBack: Cubic = Cubic::new(0.6, -0.28, 0.735, 0.045);

    /// A curve that starts by overshooting and then settles.
    pub const EaseOutBack: Cubic = Cubic::new(0.175, 0.885, 0.32, 1.275);

    /// A curve that overshoots both at the start and at the end.
    pub const EaseInOutBack: Cubic = Cubic::new(0.68, -0.55, 0.265, 1.55);

    /// An elastic ease-in curve.
    pub const ElasticIn: ElasticInCurve = ElasticInCurve::new(0.4);

    /// An elastic ease-out curve.
    pub const ElasticOut: ElasticOutCurve = ElasticOutCurve::new(0.4);

    /// An elastic ease-in-out curve.
    pub const ElasticInOut: ElasticInOutCurve = ElasticInOutCurve::new(0.4);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_curve() {
        let curve = Linear;
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_sawtooth_curve() {
        let curve = SawTooth::new(2);
        assert_eq!(curve.transform(0.0), 0.0);
        assert!((curve.transform(0.25) - 0.5).abs() < 1e-6);
        assert!((curve.transform(0.5) - 0.0).abs() < 1e-6);
        assert!((curve.transform(0.75) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_interval_curve() {
        let curve = Interval::linear(0.2, 0.8);
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.2), 0.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(0.8), 1.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_threshold_curve() {
        let curve = Threshold::new(0.5);
        assert_eq!(curve.transform(0.0), 0.0);
        assert_eq!(curve.transform(0.4), 0.0);
        assert_eq!(curve.transform(0.5), 1.0);
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_cubic_curve() {
        let curve = Cubic::new(0.42, 0.0, 1.0, 1.0); // ease-in
        assert!((curve.transform(0.0) - 0.0).abs() < 1e-4);
        assert!(curve.transform(0.5) < 0.5); // ease-in should be slower at start
        assert!((curve.transform(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_elastic_in_curve() {
        let curve = ElasticInCurve::default();
        assert!((curve.transform(0.0) - 0.0).abs() < 1e-2); // Elastic curves have some overshoot
        // Elastic in curve oscillates and can have large values near t=1.0
        let val = curve.transform(1.0);
        assert!(val.abs() < 2.0); // Just check it's bounded
    }

    #[test]
    fn test_elastic_out_curve() {
        let curve = ElasticOutCurve::default();
        assert!(curve.transform(0.0).abs() < 1e-6);
        assert!((curve.transform(1.0) - 1.0).abs() < 0.1); // Should settle near 1.0
    }

    #[test]
    fn test_flipped_curve() {
        let curve = FlippedCurve::new(Linear);
        assert_eq!(curve.transform(0.0), 1.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(1.0), 0.0);
    }

    #[test]
    fn test_reverse_curve() {
        let curve = ReverseCurve::new(Threshold::new(0.3));
        assert_eq!(curve.transform(0.0), 1.0); // reverse of 1.0 at t=1.0
        assert_eq!(curve.transform(0.8), 0.0); // reverse of 0.0 at t=0.2
    }

    #[test]
    fn test_catmull_rom_curve() {
        let points = vec![(0.0, 0.0), (0.5, 0.8), (1.0, 1.0)];
        let curve = CatmullRomCurve::with_points(points);

        assert_eq!(curve.transform(0.0), 0.0);
        assert!((curve.transform(1.0) - 1.0).abs() < 0.01); // Catmull-Rom can overshoot slightly
    }

    #[test]
    fn test_curves_constants() {
        assert_eq!(Curves::Linear.transform(0.5), 0.5);
        assert!(Curves::EaseIn.transform(0.5) < 0.5);
        assert!(Curves::EaseOut.transform(0.5) > 0.5);
    }

    #[test]
    fn test_curve2d_sample() {
        let sample = Curve2DSample::new(0.5, 1.0);
        assert_eq!(sample.value, 0.5);
        assert_eq!(sample.derivative, 1.0);
    }

    #[test]
    fn test_catmull_rom_spline() {
        let points = vec![
            Curve2DSample::new(0.0, 0.0),
            Curve2DSample::new(1.0, 1.0),
        ];
        let spline = CatmullRomSpline::new(points);

        let result = spline.transform(0.5);
        assert!(result.value >= 0.0 && result.value <= 1.0);
    }
}
