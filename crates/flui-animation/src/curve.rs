//! Animation curves for interpolation.

use smallvec::SmallVec;
use std::f32::consts::PI;
use std::fmt;
use std::sync::Arc;

/// A mapping from the unit interval to the unit interval.
///
/// A curve must map `t=0.0` to `0.0` and `t=1.0` to `1.0`.
/// Similar to Flutter's `Curve`.
///
/// # Examples
///
/// ```
/// use flui_animation::curve::Curve;
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

    /// Returns a new curve that is the flipped version of this one.
    ///
    /// Flipping swaps the output: `transform(t)` becomes `1.0 - transform(t)`.
    #[must_use]
    fn flipped(self) -> FlippedCurve<Self>
    where
        Self: Sized,
    {
        FlippedCurve { curve: self }
    }

    /// Returns a new curve that is the reversed version of this one.
    ///
    /// Reversing swaps the input: `transform(t)` becomes `transform(1.0 - t)`.
    #[must_use]
    fn reversed(self) -> ReverseCurve<Self>
    where
        Self: Sized,
    {
        ReverseCurve { curve: self }
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

/// A curve that is 0.0 until `begin`, then curved from 0.0 to 1.0 at `begin`
/// and `end`, then 1.0 after `end`.
///
/// Similar to Flutter's `Interval`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interval<C: Curve + Copy = Linear> {
    /// The start of the interval (0.0 to 1.0).
    pub begin: f32,
    /// The end of the interval (0.0 to 1.0).
    pub end: f32,
    /// The curve to apply within the interval.
    pub curve: C,
}

impl<C: Curve + Copy> Interval<C> {
    /// Creates a new interval curve.
    #[inline]
    #[must_use]
    pub fn new(begin: f32, end: f32, curve: C) -> Self {
        assert!(
            (0.0..=1.0).contains(&begin),
            "begin must be in range [0.0, 1.0]"
        );
        assert!(
            (0.0..=1.0).contains(&end),
            "end must be in range [0.0, 1.0]"
        );
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

impl<C: Curve + Copy> Curve for Interval<C> {
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

/// A curve that is 0.0 until `threshold`, then jumps to 1.0.
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
        assert!(
            (0.0..=1.0).contains(&threshold),
            "threshold must be in range [0.0, 1.0]"
        );
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
    #[must_use]
    pub const fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self { a, b, c, d }
    }
}

/// Evaluates the cubic bezier curve at t.
#[inline]
fn evaluate_cubic(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let one_minus_t = 1.0 - t;
    let one_minus_t2 = one_minus_t * one_minus_t;
    let one_minus_t3 = one_minus_t2 * one_minus_t;

    one_minus_t3 * p0 + 3.0 * one_minus_t2 * t * p1 + 3.0 * one_minus_t * t2 * p2 + t3 * p3
}

/// Derivative with respect to `t` of [`evaluate_cubic`].
#[inline]
fn evaluate_cubic_derivative(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let one_minus_t = 1.0 - t;
    3.0 * (one_minus_t * one_minus_t * (p1 - p0)
        + 2.0 * one_minus_t * t * (p2 - p1)
        + t * t * (p3 - p2))
}

/// Absolute solver tolerance for the cubic-bezier x-inversion.
const CUBIC_SOLVE_EPSILON: f32 = 1e-6;

impl Cubic {
    /// Solves `x(s) = x` for the bezier parameter `s` over `[0, 1]`.
    ///
    /// Uses Newton-Raphson (quadratic convergence — typically 2-4 iterations),
    /// falling back to bisection where the curve is too flat for Newton to make
    /// progress. This is the standard WebKit `UnitBezier` solver and replaces a
    /// plain 8-iteration bisection: same result within tolerance, fewer
    /// iterations on the dominant per-frame curve path.
    fn solve_x(&self, x: f32) -> f32 {
        // Newton-Raphson from `x` as the initial guess (good because x(s) ≈ s).
        let mut s = x;
        for _ in 0..8 {
            let error = evaluate_cubic(s, 0.0, self.a, self.c, 1.0) - x;
            if error.abs() < CUBIC_SOLVE_EPSILON {
                return s;
            }
            let slope = evaluate_cubic_derivative(s, 0.0, self.a, self.c, 1.0);
            if slope.abs() < CUBIC_SOLVE_EPSILON {
                break; // too flat: Newton stalls, hand off to bisection
            }
            s -= error / slope;
        }

        // Bounded bisection fallback (worst case for near-flat segments).
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        let mut s = x.clamp(lo, hi);
        for _ in 0..32 {
            let estimate = evaluate_cubic(s, 0.0, self.a, self.c, 1.0);
            if (estimate - x).abs() < CUBIC_SOLVE_EPSILON {
                return s;
            }
            if estimate < x {
                lo = s;
            } else {
                hi = s;
            }
            s = f32::midpoint(lo, hi);
        }
        s
    }
}

impl Curve for Cubic {
    fn transform(&self, t: f32) -> f32 {
        // Rust's `clamp` propagates NaN, which would silently NaN both the
        // Newton loop and the bisection fallback; canonicalize to the left
        // endpoint instead of poisoning every downstream animation value.
        if t.is_nan() {
            return 0.0;
        }
        let t = t.clamp(0.0, 1.0);
        let s = self.solve_x(t);
        evaluate_cubic(s, 0.0, self.b, self.d, 1.0)
    }
}

/// Two cubic bezier segments joined at a shared `midpoint`.
///
/// The curve passes through `(0,0)`, `midpoint`, and `(1,1)`; each half is a
/// [`Cubic`] rescaled into its sub-rectangle. This is the building block for
/// the Material 3 emphasized easing set.
///
/// Similar to Flutter's `ThreePointCubic`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThreePointCubic {
    /// First control point of the first segment (tangent at `(0, 0)`).
    pub a1: (f32, f32),
    /// Second control point of the first segment (tangent into `midpoint`).
    pub b1: (f32, f32),
    /// The shared point both segments pass through.
    ///
    /// `midpoint.0` must lie strictly inside `(0, 1)` — both segment widths
    /// are used as divisors.
    pub midpoint: (f32, f32),
    /// First control point of the second segment (tangent out of `midpoint`).
    pub a2: (f32, f32),
    /// Second control point of the second segment (tangent at `(1, 1)`).
    pub b2: (f32, f32),
}

impl ThreePointCubic {
    /// Creates a three-point cubic from the control points of both segments.
    ///
    /// The two implied end points `(0,0)` and `(1,1)` are fixed and not
    /// passed. See Flutter's `ThreePointCubic` for the geometry.
    ///
    /// # Panics
    ///
    /// Panics when `midpoint` does not lie strictly inside the unit square:
    /// both segment widths (`midpoint.0`, `1 - midpoint.0`) and heights
    /// (`midpoint.1`, `1 - midpoint.1`) are used as divisors in
    /// [`Curve::transform`], so a midpoint on the boundary (or NaN) would
    /// silently evaluate to NaN/inf. For `const` constructions the panic is
    /// a compile error.
    #[must_use]
    pub const fn new(
        a1: (f32, f32),
        b1: (f32, f32),
        midpoint: (f32, f32),
        a2: (f32, f32),
        b2: (f32, f32),
    ) -> Self {
        assert!(
            midpoint.0 > 0.0 && midpoint.0 < 1.0 && midpoint.1 > 0.0 && midpoint.1 < 1.0,
            "ThreePointCubic midpoint must lie strictly inside the unit square: \
             both segments are rescaled by its distance to each edge"
        );
        Self {
            a1,
            b1,
            midpoint,
            a2,
            b2,
        }
    }
}

impl Curve for ThreePointCubic {
    fn transform(&self, t: f32) -> f32 {
        // NaN canonicalization mirrors `Cubic::transform`.
        if t.is_nan() {
            return 0.0;
        }
        let t = t.clamp(0.0, 1.0);
        let (mx, my) = self.midpoint;
        let first = t < mx;
        let scale_x = if first { mx } else { 1.0 - mx };
        let scale_y = if first { my } else { 1.0 - my };
        let scaled_t = (t - if first { 0.0 } else { mx }) / scale_x;
        if first {
            Cubic::new(
                self.a1.0 / scale_x,
                self.a1.1 / scale_y,
                self.b1.0 / scale_x,
                self.b1.1 / scale_y,
            )
            .transform(scaled_t)
                * scale_y
        } else {
            Cubic::new(
                (self.a2.0 - mx) / scale_x,
                (self.a2.1 - my) / scale_y,
                (self.b2.0 - mx) / scale_x,
                (self.b2.1 - my) / scale_y,
            )
            .transform(scaled_t)
                * scale_y
                + my
        }
    }
}

// ============================================================================
// Split Curve
// ============================================================================

/// A curve that progresses according to `begin_curve` until `split`, then
/// according to `end_curve`.
///
/// Useful when a widget must track the user's finger (linear) and then be
/// flung with an easing curve after release: `split` is the animation
/// progress at the moment of release.
///
/// Similar to Flutter's `Split`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Split<B: Curve = Linear, E: Curve = Cubic> {
    /// The progress value separating the two curves. In `[0, 1]`.
    pub split: f32,
    /// The curve used before `split`.
    pub begin_curve: B,
    /// The curve used at and after `split`.
    pub end_curve: E,
}

impl Split<Linear, Cubic> {
    /// Creates a split curve with Flutter's defaults: linear before `split`,
    /// `Curves::EaseOutCubic` after.
    #[must_use]
    pub fn new(split: f32) -> Self {
        Self::with_curves(split, Linear, Curves::EaseOutCubic)
    }
}

impl<B: Curve, E: Curve> Split<B, E> {
    /// Creates a split curve with explicit segment curves.
    #[must_use]
    pub fn with_curves(split: f32, begin_curve: B, end_curve: E) -> Self {
        assert!(
            (0.0..=1.0).contains(&split),
            "split must be in range [0.0, 1.0]"
        );
        Self {
            split,
            begin_curve,
            end_curve,
        }
    }
}

impl<B: Curve, E: Curve> Curve for Split<B, E> {
    #[allow(clippy::float_cmp)] // Intentional exact comparisons per the Flutter contract
    fn transform(&self, t: f32) -> f32 {
        if t.is_nan() {
            return 0.0;
        }
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        if t == self.split {
            return self.split;
        }
        if t < self.split {
            // `t < split` implies `split > 0`, so the division is safe.
            let progress = t / self.split;
            self.split * self.begin_curve.transform(progress)
        } else {
            // `t > split` implies `split < 1`, so the division is safe.
            let progress = (t - self.split) / (1.0 - self.split);
            self.split + (1.0 - self.split) * self.end_curve.transform(progress)
        }
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
    #[must_use]
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
    #[allow(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        // Guarantee exact boundary values per Curve contract
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
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
    #[must_use]
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
    #[allow(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        // Guarantee exact boundary values per Curve contract
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
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
    #[must_use]
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
    #[allow(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        // Guarantee exact boundary values per Curve contract
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
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
// Bounce Curves
// ============================================================================

/// A bounce curve that bounces at the end.
///
/// Similar to Flutter's `Curves.bounceOut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceOutCurve;

impl Curve for BounceOutCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        bounce_out(t)
    }
}

/// A bounce curve that bounces at the beginning.
///
/// Similar to Flutter's `Curves.bounceIn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInCurve;

impl Curve for BounceInCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        1.0 - bounce_out(1.0 - t)
    }
}

/// A bounce curve that bounces at both ends.
///
/// Similar to Flutter's `Curves.bounceInOut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInOutCurve;

impl Curve for BounceInOutCurve {
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            (1.0 - bounce_out(1.0 - t * 2.0)) * 0.5
        } else {
            bounce_out(t * 2.0 - 1.0) * 0.5 + 0.5
        }
    }
}

/// Helper function for bounce calculations.
#[inline]
fn bounce_out(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;

    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        N1 * t * t + 0.75
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        N1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / D1;
        N1 * t * t + 0.984_375
    }
}

// ============================================================================
// Decelerate Curve
// ============================================================================

/// A curve where the rate of change starts fast and then decelerates.
///
/// Similar to Flutter's `Curves.decelerate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecelerateCurve;

impl Curve for DecelerateCurve {
    #[inline]
    fn transform(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t) * (1.0 - t)
    }
}

// ============================================================================
// Catmull-Rom Curves
// ============================================================================

/// A Catmull-Rom curve passing through a set of points.
///
/// Uses stack allocation for up to 8 points to avoid heap allocations in common cases.
///
/// Similar to Flutter's `CatmullRomCurve`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatmullRomCurve {
    /// The control points of the curve.
    /// Stack-allocated for up to 8 points, heap-allocated for more.
    pub points: SmallVec<[(f32, f32); 8]>,
    /// The tension parameter (0.0 = no tension, 0.5 = Catmull-Rom, 1.0 = tight).
    pub tension: f32,
}

impl CatmullRomCurve {
    /// Creates a new Catmull-Rom curve.
    #[inline]
    #[must_use]
    pub fn new(points: impl Into<SmallVec<[(f32, f32); 8]>>, tension: f32) -> Self {
        let points = points.into();
        assert!(points.len() >= 2, "Must have at least 2 points");
        Self { points, tension }
    }

    /// Creates a Catmull-Rom curve with default tension (0.0).
    #[inline]
    #[must_use]
    pub fn with_points(points: impl Into<SmallVec<[(f32, f32); 8]>>) -> Self {
        Self::new(points, 0.0)
    }
}

impl Curve for CatmullRomCurve {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
        let p0 = if segment > 0 {
            self.points[segment - 1]
        } else {
            self.points[0]
        };
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
/// Uses stack allocation for up to 8 points to avoid heap allocations in common cases.
///
/// Similar to Flutter's `CatmullRomSpline`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatmullRomSpline {
    /// The control points of the spline.
    /// Stack-allocated for up to 8 points, heap-allocated for more.
    pub points: SmallVec<[Curve2DSample; 8]>,
}

impl CatmullRomSpline {
    /// Creates a new Catmull-Rom spline.
    #[inline]
    #[must_use]
    pub fn new(points: impl Into<SmallVec<[Curve2DSample; 8]>>) -> Self {
        let points = points.into();
        assert!(points.len() >= 2, "Must have at least 2 points");
        Self { points }
    }
}

impl Curve2D for CatmullRomSpline {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
        let p0 = if segment > 0 {
            self.points[segment - 1]
        } else {
            self.points[0]
        };
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
#[derive(Debug)]
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

    /// A bounce curve that bounces at the beginning.
    pub const BounceIn: BounceInCurve = BounceInCurve;

    /// A bounce curve that bounces at the end.
    pub const BounceOut: BounceOutCurve = BounceOutCurve;

    /// A bounce curve that bounces at both ends.
    pub const BounceInOut: BounceInOutCurve = BounceInOutCurve;

    /// A curve where the rate of change starts fast and then decelerates.
    pub const Decelerate: DecelerateCurve = DecelerateCurve;

    /// The CSS `ease` function: speeds up quickly, ends slowly.
    pub const Ease: Cubic = Cubic::new(0.25, 0.1, 0.25, 1.0);

    /// A quadratic ease-in (Penner `easeInQuad`).
    pub const EaseInQuad: Cubic = Cubic::new(0.55, 0.085, 0.68, 0.53);

    /// A cubic ease-in (Penner `easeInCubic`).
    pub const EaseInCubic: Cubic = Cubic::new(0.55, 0.055, 0.675, 0.19);

    /// A quartic ease-in (Penner `easeInQuart`).
    pub const EaseInQuart: Cubic = Cubic::new(0.895, 0.03, 0.685, 0.22);

    /// A quintic ease-in (Penner `easeInQuint`).
    pub const EaseInQuint: Cubic = Cubic::new(0.755, 0.05, 0.855, 0.06);

    /// A quadratic ease-out (Penner `easeOutQuad`).
    pub const EaseOutQuad: Cubic = Cubic::new(0.25, 0.46, 0.45, 0.94);

    /// A cubic ease-out (Penner `easeOutCubic`).
    pub const EaseOutCubic: Cubic = Cubic::new(0.215, 0.61, 0.355, 1.0);

    /// A quartic ease-out (Penner `easeOutQuart`).
    pub const EaseOutQuart: Cubic = Cubic::new(0.165, 0.84, 0.44, 1.0);

    /// A quintic ease-out (Penner `easeOutQuint`).
    pub const EaseOutQuint: Cubic = Cubic::new(0.23, 1.0, 0.32, 1.0);

    /// A quadratic ease-in-out (Penner `easeInOutQuad`).
    pub const EaseInOutQuad: Cubic = Cubic::new(0.455, 0.03, 0.515, 0.955);

    /// A quartic ease-in-out (Penner `easeInOutQuart`).
    pub const EaseInOutQuart: Cubic = Cubic::new(0.77, 0.0, 0.175, 1.0);

    /// A quintic ease-in-out (Penner `easeInOutQuint`).
    pub const EaseInOutQuint: Cubic = Cubic::new(0.86, 0.0, 0.07, 1.0);

    /// Starts nearly linear and ends with a strong ease-in; pairs with
    /// `LinearToEaseOut` for enter/exit transitions.
    pub const FastLinearToSlowEaseIn: Cubic = Cubic::new(0.18, 1.0, 0.04, 1.0);

    /// Starts nearly linear and ends with an ease-out; the exit counterpart
    /// of `FastLinearToSlowEaseIn`.
    pub const LinearToEaseOut: Cubic = Cubic::new(0.35, 0.91, 0.33, 0.97);

    /// Starts with an ease-in and ends nearly linear.
    pub const EaseInToLinear: Cubic = Cubic::new(0.67, 0.03, 0.65, 0.09);

    /// Fast at the edges, slow through the middle.
    pub const SlowMiddle: Cubic = Cubic::new(0.15, 0.85, 0.85, 0.15);

    /// Material 3 emphasized easing: the default M3 motion curve.
    pub const EaseInOutCubicEmphasized: ThreePointCubic = ThreePointCubic::new(
        (0.05, 0.0),
        (0.133_333, 0.06),
        (0.166_666, 0.4),
        (0.208_333, 0.82),
        (0.25, 1.0),
    );

    /// A strong ease-in followed by a long, gentle ease-out.
    pub const FastEaseInToSlowEaseOut: ThreePointCubic = ThreePointCubic::new(
        (0.056, 0.024),
        (0.108, 0.308_5),
        (0.198, 0.541),
        (0.365_5, 1.0),
        (0.546_5, 0.989),
    );
}

/// A reference-counted, type-erased curve handle.
///
/// Wraps any `impl Curve + Send + Sync + 'static` behind an `Arc` so that a
/// single, stable concrete type can be stored in widgets and animation
/// controllers, regardless of which specific curve is used.
///
/// `ArcCurve` implements `Curve + Clone + Send + Sync + Debug`, which satisfies
/// the full bound that [`CurvedAnimation`] places on its `C` type parameter.
/// The `Debug` output intentionally omits the inner curve's type name because
/// `Curve` does not require `Debug`; use a concrete named type when the type
/// name is load-bearing.
///
/// # Examples
///
/// ```
/// use flui_animation::curve::{ArcCurve, Curve, ElasticOutCurve};
///
/// let curve = ArcCurve::new(ElasticOutCurve::default());
/// assert_eq!(curve.transform(0.0), 0.0);
/// assert!((curve.transform(1.0) - 1.0).abs() < 1e-5);
/// ```
///
/// [`CurvedAnimation`]: crate::CurvedAnimation
#[derive(Clone)]
pub struct ArcCurve(Arc<dyn Curve + Send + Sync>);

impl ArcCurve {
    /// Wrap `curve` in a reference-counted erased handle.
    ///
    /// The `Arc` is cloned cheaply (reference-count bump), so `ArcCurve` can
    /// be stored in `Clone`-derived structs without duplicating the curve data.
    pub fn new(curve: impl Curve + Send + Sync + 'static) -> Self {
        Self(Arc::new(curve))
    }
}

impl Curve for ArcCurve {
    fn transform(&self, t: f32) -> f32 {
        self.0.transform(t)
    }
}

impl fmt::Debug for ArcCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArcCurve").finish_non_exhaustive()
    }
}

/// Blanket impl so `Arc<dyn Curve + Send + Sync>` can itself be used as a
/// `Curve` where object-safety is all that matters.  Note that this type alone
/// does not satisfy [`CurvedAnimation`]'s `C: Debug` bound; prefer [`ArcCurve`]
/// for that use-case.
///
/// [`CurvedAnimation`]: crate::CurvedAnimation
impl Curve for Arc<dyn Curve + Send + Sync> {
    fn transform(&self, t: f32) -> f32 {
        (**self).transform(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference solver: the plain 8-iteration bisection the Newton solver
    /// replaces. Used to prove the new `transform` matches the old behavior.
    fn reference_bisection_transform(cubic: &Cubic, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let (mut start, mut end) = (0.0_f32, 1.0_f32);
        for _ in 0..8 {
            let mid = f32::midpoint(start, end);
            let x = evaluate_cubic(mid, 0.0, cubic.a, cubic.c, 1.0);
            if (x - t).abs() < 1e-6 {
                return evaluate_cubic(mid, 0.0, cubic.b, cubic.d, 1.0);
            }
            if x < t {
                start = mid;
            } else {
                end = mid;
            }
        }
        evaluate_cubic(f32::midpoint(start, end), 0.0, cubic.b, cubic.d, 1.0)
    }

    #[test]
    fn cubic_solver_inverts_x() {
        // solve_x must invert bezier_x: bezier_x(solve_x(t)) ≈ t everywhere.
        for cubic in [
            Cubic::new(0.42, 0.0, 0.58, 1.0), // EaseInOut
            Cubic::new(0.25, 0.1, 0.25, 1.0), // Ease
            Cubic::new(0.42, 0.0, 1.0, 1.0),  // EaseIn
            Cubic::new(0.0, 0.0, 0.58, 1.0),  // EaseOut
        ] {
            for i in 0..=1000 {
                let t = i as f32 / 1000.0;
                let s = cubic.solve_x(t);
                let x = evaluate_cubic(s, 0.0, cubic.a, cubic.c, 1.0);
                assert!((x - t).abs() < 1e-3, "x({s})={x} != t={t}");
            }
        }
    }

    #[test]
    fn cubic_newton_matches_reference_bisection() {
        // The Newton solver produces the same curve as the old pure-bisection
        // one, only MORE precisely: the old stopped after 8 bisection steps
        // (parameter bracket 1/256), so its output carried up to ~0.5e-2 error
        // on steep segments, while Newton converges to 1e-6. The bound here is
        // that residual old imprecision — `cubic_solver_inverts_x` is the real
        // accuracy gate; this only proves there is no gross divergence.
        let cubic = Cubic::new(0.42, 0.0, 0.58, 1.0); // EaseInOut
        for i in 0..=1000 {
            let t = i as f32 / 1000.0;
            let new = cubic.transform(t);
            let old = reference_bisection_transform(&cubic, t);
            assert!((new - old).abs() < 1e-2, "t={t}: new={new} old={old}");
        }
    }

    #[test]
    fn cubic_endpoints_and_symmetry() {
        let ease_in_out = Cubic::new(0.42, 0.0, 0.58, 1.0);
        assert!((ease_in_out.transform(0.0)).abs() < 1e-4);
        assert!((ease_in_out.transform(1.0) - 1.0).abs() < 1e-4);
        // EaseInOut is symmetric about (0.5, 0.5).
        assert!((ease_in_out.transform(0.5) - 0.5).abs() < 1e-3);
    }

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
    fn cubic_nan_input_is_canonicalized() {
        // Rust's clamp propagates NaN; the solver must not.
        let c = Curves::EaseInOut;
        assert_eq!(c.transform(f32::NAN), 0.0);
        let tp = Curves::EaseInOutCubicEmphasized;
        assert_eq!(tp.transform(f32::NAN), 0.0);
        let split = Split::new(0.5);
        assert_eq!(split.transform(f32::NAN), 0.0);
    }

    #[test]
    fn penner_catalog_endpoints() {
        // Every new cubic catalog entry must satisfy the Curve contract at
        // the endpoints.
        for c in [
            Curves::Ease,
            Curves::EaseInQuad,
            Curves::EaseInCubic,
            Curves::EaseInQuart,
            Curves::EaseInQuint,
            Curves::EaseOutQuad,
            Curves::EaseOutCubic,
            Curves::EaseOutQuart,
            Curves::EaseOutQuint,
            Curves::EaseInOutQuad,
            Curves::EaseInOutQuart,
            Curves::EaseInOutQuint,
            Curves::FastLinearToSlowEaseIn,
            Curves::LinearToEaseOut,
            Curves::EaseInToLinear,
            Curves::SlowMiddle,
        ] {
            assert!(c.transform(0.0).abs() < 1e-4, "{c:?} must start at 0");
            assert!((c.transform(1.0) - 1.0).abs() < 1e-4, "{c:?} must end at 1");
        }
    }

    #[test]
    fn three_point_cubic_passes_through_midpoint() {
        let c = Curves::EaseInOutCubicEmphasized;
        assert!(c.transform(0.0).abs() < 1e-4);
        assert!((c.transform(1.0) - 1.0).abs() < 1e-4);
        // The curve must pass through its midpoint (M3 spec: (1/6, 0.4)).
        assert!((c.transform(0.166_666) - 0.4).abs() < 1e-2);

        let f = Curves::FastEaseInToSlowEaseOut;
        assert!(f.transform(0.0).abs() < 1e-4);
        assert!((f.transform(1.0) - 1.0).abs() < 1e-4);
        assert!((f.transform(0.198) - 0.541).abs() < 1e-2);
    }

    #[test]
    fn split_curve_contract() {
        let split = Split::new(0.5);
        // Endpoints and the split point itself are exact per the contract.
        assert_eq!(split.transform(0.0), 0.0);
        assert_eq!(split.transform(1.0), 1.0);
        assert_eq!(split.transform(0.5), 0.5);
        // Before the split the default begin curve is linear.
        assert!((split.transform(0.25) - 0.25).abs() < 1e-6);
        // After the split the ease-out segment runs ahead of linear.
        assert!(split.transform(0.75) > 0.75);

        // A custom begin curve is rescaled into [0, split].
        let custom = Split::with_curves(0.5, Threshold::new(0.5), Linear);
        assert_eq!(custom.transform(0.2), 0.0); // threshold not yet reached
        assert_eq!(custom.transform(0.3), 0.5); // threshold crossed -> split*1.0
    }

    #[test]
    fn test_curve2d_sample() {
        let sample = Curve2DSample::new(0.5, 1.0);
        assert_eq!(sample.value, 0.5);
        assert_eq!(sample.derivative, 1.0);
    }

    #[test]
    fn test_catmull_rom_spline() {
        let points = vec![Curve2DSample::new(0.0, 0.0), Curve2DSample::new(1.0, 1.0)];
        let spline = CatmullRomSpline::new(points);

        let result = spline.transform(0.5);
        assert!(result.value >= 0.0 && result.value <= 1.0);
    }

    #[test]
    fn test_bounce_out_curve() {
        let curve = BounceOutCurve;
        assert_eq!(curve.transform(0.0), 0.0);
        assert!(curve.transform(0.5) > 0.5); // bounces high
        assert!((curve.transform(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_bounce_in_curve() {
        let curve = BounceInCurve;
        assert!((curve.transform(0.0) - 0.0).abs() < 1e-6);
        assert!(curve.transform(0.5) < 0.5); // slow start due to bouncing
        assert!((curve.transform(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_bounce_in_out_curve() {
        let curve = BounceInOutCurve;
        assert!((curve.transform(0.0) - 0.0).abs() < 1e-6);
        assert!((curve.transform(0.5) - 0.5).abs() < 1e-6); // midpoint
        assert!((curve.transform(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_decelerate_curve() {
        let curve = DecelerateCurve;
        assert_eq!(curve.transform(0.0), 0.0);
        assert!(curve.transform(0.5) > 0.5); // fast start, slow end
        assert_eq!(curve.transform(1.0), 1.0);
    }

    #[test]
    fn test_curves_bounce_constants() {
        assert!((Curves::BounceIn.transform(1.0) - 1.0).abs() < 1e-6);
        assert!((Curves::BounceOut.transform(1.0) - 1.0).abs() < 1e-6);
        assert!((Curves::BounceInOut.transform(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(Curves::Decelerate.transform(1.0), 1.0);
    }

    #[test]
    fn test_curve_reversed_method() {
        let curve = Linear.reversed();
        assert_eq!(curve.transform(0.0), 1.0);
        assert_eq!(curve.transform(0.5), 0.5);
        assert_eq!(curve.transform(1.0), 0.0);
    }
}
