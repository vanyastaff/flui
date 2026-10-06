//! Animation curves for interpolation.

use crate::animation::{Retirement, Terminal};

use smallvec::SmallVec;
use std::f64::consts::PI;
use std::fmt;
use std::sync::Arc;

/// An easing function: maps animation progress `t` to eased progress.
///
/// # Contract
///
/// Every curve in this crate follows one input policy, and an implementation
/// outside it should too:
///
/// - `transform` is defined on all of `f64`. For `t` in `[0, 1]` the result
///   is finite, `transform(0.0) == 0.0` and `transform(1.0) == 1.0` exactly.
///   Inside the interval the result may leave `[0, 1]` (an overshooting
///   curve such as [`Curves::EaseOutBack`] or [`ElasticOutCurve`]).
/// - A finite `t` outside `[0, 1]`, and `±∞`, is clamped: the result is the
///   value at the nearest end. Curves do not extrapolate.
/// - `transform(f64::NAN)` returns NaN. A curve never disguises a broken
///   input as valid progress; the caller decides what a non-finite sample
///   means.
/// - Monotonicity belongs to the individual curve and is stated in its
///   documentation.
///
/// Combinators keep the policy: [`FlippedCurve`] preserves both the
/// endpoints and NaN.
///
/// # Examples
///
/// ```
/// use flui_animation::curve::Curve;
///
/// struct MyCurve;
///
/// impl Curve for MyCurve {
///     fn transform(&self, t: f64) -> f64 {
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
    /// Returns the eased progress at progress `t`, following the input
    /// policy in the trait documentation.
    fn transform(&self, t: f64) -> f64;

    /// Returns a new curve that is the flipped version of this one.
    ///
    /// Flipping rotates the curve 180°: `transform(t)` becomes
    /// `1.0 - transform(1.0 - t)`, turning an ease-in into an ease-out. A curve
    /// symmetric about the centre (like [`Linear`]) is its own flip.
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

/// Why a curve parameter was rejected.
///
/// Returned by the `try_new` constructors and, as the error message, by
/// serde decoding (feature `serde`); the panicking `new` constructors panic
/// with the same text.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum CurveError {
    /// The parameter is NaN or infinite.
    #[error("curve parameter `{parameter}` is not finite")]
    NonFinite {
        /// The constructor argument that was rejected.
        parameter: &'static str,
    },
    /// The parameter is finite but outside the range the curve admits.
    #[error("curve parameter `{parameter}` = {value} is outside {allowed}")]
    OutOfRange {
        /// The constructor argument that was rejected.
        parameter: &'static str,
        /// The rejected value.
        value: f64,
        /// The admitted range, in interval notation.
        allowed: &'static str,
    },
    /// An [`Interval`] whose `begin` lies after its `end`.
    #[error("interval begin {begin} is after end {end}")]
    IntervalReversed {
        /// The rejected start of the interval.
        begin: f64,
        /// The rejected end of the interval.
        end: f64,
    },
}

/// A parametric curve in 2D space.
pub trait ParametricCurve<T> {
    /// Returns the value of the curve at point `t`.
    fn transform(&self, t: f64) -> T;
}

/// A curve that maps a value in the unit interval to a 2D point.
pub trait Curve2D {
    /// Returns the point on the curve at parameter `t`.
    fn transform(&self, t: f64) -> Curve2DSample;
}

/// A sample point on a 2D curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Curve2DSample {
    /// The value of the curve at this point.
    pub value: f64,
    /// The derivative (slope) of the curve at this point.
    pub derivative: f64,
}

impl Curve2DSample {
    /// Creates a new 2D curve sample.
    #[inline]
    #[must_use]
    pub const fn new(value: f64, derivative: f64) -> Self {
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
    fn transform(&self, t: f64) -> f64 {
        t.clamp(0.0, 1.0)
    }
}

/// A sawtooth curve that repeats.
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
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        (t * self.count as f64).fract()
    }
}

/// A curve that is 0.0 until `begin`, then curved from 0.0 to 1.0 at `begin`
/// and `end`, then 1.0 after `end`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interval<C: Curve + Copy = Linear> {
    /// The start of the interval (0.0 to 1.0).
    begin: f64,
    /// The end of the interval (0.0 to 1.0).
    end: f64,
    /// The curve to apply within the interval.
    curve: C,
}

impl<C: Curve + Copy> Interval<C> {
    /// Creates a new interval curve.
    #[inline]
    #[must_use]
    pub fn new(begin: f64, end: f64, curve: C) -> Self {
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

    /// Creates a new interval curve, rejecting invalid bounds.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for invalid bounds.
    pub fn try_new(begin: f64, end: f64, curve: C) -> Result<Self, CurveError> {
        Ok(Self { begin, end, curve })
    }
}

impl Interval<Linear> {
    /// Creates a new interval curve with a linear curve.
    #[inline]
    #[must_use]
    pub fn linear(begin: f64, end: f64) -> Self {
        Self::new(begin, end, Linear)
    }
}

impl<C: Curve + Copy> Curve for Interval<C> {
    fn transform(&self, t: f64) -> f64 {
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
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Threshold {
    /// The threshold at which the curve jumps to 1.0.
    pub threshold: f64,
}

impl Threshold {
    /// Creates a new threshold curve.
    #[inline]
    #[must_use]
    pub fn new(threshold: f64) -> Self {
        assert!(
            (0.0..=1.0).contains(&threshold),
            "threshold must be in range [0.0, 1.0]"
        );
        Self { threshold }
    }
}

impl Curve for Threshold {
    #[inline]
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        if t < self.threshold { 0.0 } else { 1.0 }
    }
}

// ============================================================================
// Cubic Curves
// ============================================================================

/// A cubic polynomial curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cubic {
    /// The x coordinate of the first control point.
    a: f64,
    /// The y coordinate of the first control point.
    b: f64,
    /// The x coordinate of the second control point.
    c: f64,
    /// The y coordinate of the second control point.
    d: f64,
}

impl Cubic {
    /// Creates a new cubic curve.
    #[must_use]
    pub const fn new(a: f64, b: f64, c: f64, d: f64) -> Self {
        Self { a, b, c, d }
    }

    /// Creates a new cubic curve, rejecting invalid control points.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for invalid control points.
    pub fn try_new(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError> {
        Ok(Self::new(x1, y1, x2, y2))
    }
}

/// Evaluates the cubic bezier curve at t.
#[inline]
fn evaluate_cubic(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    let one_minus_t = 1.0 - t;
    let one_minus_t2 = one_minus_t * one_minus_t;
    let one_minus_t3 = one_minus_t2 * one_minus_t;

    one_minus_t3 * p0 + 3.0 * one_minus_t2 * t * p1 + 3.0 * one_minus_t * t2 * p2 + t3 * p3
}

/// Derivative with respect to `t` of [`evaluate_cubic`].
#[inline]
fn evaluate_cubic_derivative(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let one_minus_t = 1.0 - t;
    3.0 * (one_minus_t * one_minus_t * (p1 - p0)
        + 2.0 * one_minus_t * t * (p2 - p1)
        + t * t * (p3 - p2))
}

/// Absolute solver tolerance for the cubic-bezier x-inversion.
const CUBIC_SOLVE_EPSILON: f64 = 1e-6;

impl Cubic {
    /// Solves `x(s) = x` for the bezier parameter `s` over `[0, 1]`.
    ///
    /// Uses Newton-Raphson (quadratic convergence — typically 2-4 iterations),
    /// falling back to bisection where the curve is too flat for Newton to make
    /// progress. This is the standard WebKit `UnitBezier` solver and replaces a
    /// plain 8-iteration bisection: same result within tolerance, fewer
    /// iterations on the dominant per-frame curve path.
    fn solve_x(&self, x: f64) -> f64 {
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
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
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
            s = f64::midpoint(lo, hi);
        }
        s
    }
}

impl Curve for Cubic {
    fn transform(&self, t: f64) -> f64 {
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
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThreePointCubic {
    /// First control point of the first segment (tangent at `(0, 0)`).
    a1: (f64, f64),
    /// Second control point of the first segment (tangent into `midpoint`).
    b1: (f64, f64),
    /// The shared point both segments pass through.
    ///
    /// `midpoint.0` must lie strictly inside `(0, 1)` — both segment widths
    /// are used as divisors.
    midpoint: (f64, f64),
    /// First control point of the second segment (tangent out of `midpoint`).
    a2: (f64, f64),
    /// Second control point of the second segment (tangent at `(1, 1)`).
    b2: (f64, f64),
}

impl ThreePointCubic {
    /// Creates a three-point cubic from the control points of both segments.
    ///
    /// The two implied end points `(0,0)` and `(1,1)` are fixed and not
    /// passed.
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
        a1: (f64, f64),
        b1: (f64, f64),
        midpoint: (f64, f64),
        a2: (f64, f64),
        b2: (f64, f64),
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

    /// Creates a three-point cubic, rejecting invalid control points.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for invalid control points.
    pub fn try_new(
        a1: (f64, f64),
        b1: (f64, f64),
        midpoint: (f64, f64),
        a2: (f64, f64),
        b2: (f64, f64),
    ) -> Result<Self, CurveError> {
        Ok(Self {
            a1,
            b1,
            midpoint,
            a2,
            b2,
        })
    }
}

impl Curve for ThreePointCubic {
    fn transform(&self, t: f64) -> f64 {
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Split<B: Curve = Linear, E: Curve = Cubic> {
    #[cfg_attr(feature = "serde", serde(rename = "split"))]
    boundary: f64,
    begin_curve: Terminal<B>,
    end_curve: Terminal<E>,
}

/// Decode owns each completed field until the complete split is admitted.
/// Failed/partial decoding retains opaque curves so its error stays authoritative.
#[cfg(feature = "serde")]
struct DecodedCurve<T>(Option<T>);

#[cfg(feature = "serde")]
impl<T> DecodedCurve<T> {
    fn commit(mut self) -> Terminal<T> {
        Terminal::new(self.0.take().expect("BUG: decoded curve is committed once"))
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for DecodedCurve<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(|value| Self(Some(value)))
    }
}

#[cfg(feature = "serde")]
impl<T> Drop for DecodedCurve<T> {
    fn drop(&mut self) {
        std::mem::forget(self.0.take());
    }
}

#[cfg(feature = "serde")]
impl<'de, B, E> serde::Deserialize<'de> for Split<B, E>
where
    B: Curve + serde::Deserialize<'de>,
    E: Curve + serde::Deserialize<'de>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename = "Split")]
        struct Fields<B, E> {
            split: f64,
            begin_curve: DecodedCurve<B>,
            end_curve: DecodedCurve<E>,
        }
        let fields = <Fields<B, E> as serde::Deserialize>::deserialize(deserializer)?;
        if !(0.0..=1.0).contains(&fields.split) {
            // Preserve the validation error without invoking opaque curve
            // destruction on the rejected aggregate.
            return Err(serde::de::Error::custom(
                "split must be in range [0.0, 1.0]",
            ));
        }
        Ok(Self {
            boundary: fields.split,
            begin_curve: fields.begin_curve.commit(),
            end_curve: fields.end_curve.commit(),
        })
    }
}

impl<B: Curve, E: Curve> Drop for Split<B, E> {
    fn drop(&mut self) {
        let begin = self.begin_curve.withdraw();
        let end = self.end_curve.withdraw();
        let mut retirement = Retirement::new();
        retirement.retire(begin);
        retirement.retire(end);
        retirement.finish();
    }
}

impl Split<Linear, Cubic> {
    /// Creates a split curve with the default halves: linear before `split`,
    /// `Curves::EaseOutCubic` after.
    #[must_use]
    pub fn new(split: f64) -> Self {
        Self::with_curves(split, Linear, Curves::EaseOutCubic)
    }
}

impl<B: Curve, E: Curve> Split<B, E> {
    /// The checked progress value separating the curves, in `[0, 1]`.
    #[must_use]
    pub fn split(&self) -> f64 {
        self.boundary
    }

    /// The curve sampled before the split.
    #[must_use]
    pub fn begin_curve(&self) -> &B {
        self.begin_curve.get()
    }

    /// The curve sampled at and after the split.
    #[must_use]
    pub fn end_curve(&self) -> &E {
        self.end_curve.get()
    }

    /// Creates a split curve with explicit segment curves.
    #[must_use]
    pub fn with_curves(split: f64, begin_curve: B, end_curve: E) -> Self {
        let begin_curve = Terminal::new(begin_curve);
        let end_curve = Terminal::new(end_curve);
        assert!(
            (0.0..=1.0).contains(&split),
            "split must be in range [0.0, 1.0]"
        );
        Self {
            boundary: split,
            begin_curve,
            end_curve,
        }
    }
}

impl<B: Curve, E: Curve> Curve for Split<B, E> {
    #[expect(clippy::float_cmp)] // Intentional exact comparisons at the split boundary
    fn transform(&self, t: f64) -> f64 {
        if t.is_nan() {
            return 0.0;
        }
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        if t == self.boundary {
            return self.boundary;
        }
        if t < self.boundary {
            // `t < split` implies `split > 0`, so the division is safe.
            let progress = t / self.boundary;
            self.boundary * self.begin_curve.transform(progress)
        } else {
            // `t > split` implies `split < 1`, so the division is safe.
            let progress = (t - self.boundary) / (1.0 - self.boundary);
            self.boundary + (1.0 - self.boundary) * self.end_curve.transform(progress)
        }
    }
}

// ============================================================================
// Elastic Curves
// ============================================================================

/// An oscillating curve that grows in magnitude while overshooting its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticInCurve {
    /// The period of oscillation.
    period: f64,
}

impl ElasticInCurve {
    /// Creates a new elastic-in curve with the given period.
    #[must_use]
    pub const fn new(period: f64) -> Self {
        Self { period }
    }

    /// Creates the curve, rejecting an invalid period.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for an invalid period.
    pub fn try_new(period: f64) -> Result<Self, CurveError> {
        Ok(Self::new(period))
    }
}

impl Default for ElasticInCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticInCurve {
    #[expect(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f64) -> f64 {
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
        -((2.0_f64).powf(10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin())
    }
}

/// An oscillating curve that shrinks in magnitude while overshooting its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticOutCurve {
    /// The period of oscillation.
    period: f64,
}

impl ElasticOutCurve {
    /// Creates a new elastic-out curve with the given period.
    #[must_use]
    pub const fn new(period: f64) -> Self {
        Self { period }
    }

    /// Creates the curve, rejecting an invalid period.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for an invalid period.
    pub fn try_new(period: f64) -> Result<Self, CurveError> {
        Ok(Self::new(period))
    }
}

impl Default for ElasticOutCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticOutCurve {
    #[expect(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        // Guarantee exact boundary values per Curve contract
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }
        let s = self.period / 4.0;
        (2.0_f64).powf(-10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin() + 1.0
    }
}

/// An oscillating curve that grows and then shrinks in magnitude while
/// overshooting its bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ElasticInOutCurve {
    /// The period of oscillation.
    period: f64,
}

impl ElasticInOutCurve {
    /// Creates a new elastic-in-out curve with the given period.
    #[must_use]
    pub const fn new(period: f64) -> Self {
        Self { period }
    }

    /// Creates the curve, rejecting an invalid period.
    ///
    /// # Errors
    ///
    /// Returns a [`CurveError`] for an invalid period.
    pub fn try_new(period: f64) -> Result<Self, CurveError> {
        Ok(Self::new(period))
    }
}

impl Default for ElasticInOutCurve {
    fn default() -> Self {
        Self::new(0.4)
    }
}

impl Curve for ElasticInOutCurve {
    #[expect(clippy::float_cmp)] // Intentional exact comparison after clamp
    fn transform(&self, t: f64) -> f64 {
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
            -0.5 * ((2.0_f64).powf(10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin())
        } else {
            0.5 * ((2.0_f64).powf(-10.0 * t) * ((t - s) * (2.0 * PI) / self.period).sin()) + 1.0
        }
    }
}

// ============================================================================
// Bounce Curves
// ============================================================================

/// A bounce curve that bounces at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceOutCurve;

impl Curve for BounceOutCurve {
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        bounce_out(t)
    }
}

/// A bounce curve that bounces at the beginning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInCurve;

impl Curve for BounceInCurve {
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        1.0 - bounce_out(1.0 - t)
    }
}

/// A bounce curve that bounces at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BounceInOutCurve;

impl Curve for BounceInOutCurve {
    fn transform(&self, t: f64) -> f64 {
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
fn bounce_out(t: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecelerateCurve;

impl Curve for DecelerateCurve {
    #[inline]
    fn transform(&self, t: f64) -> f64 {
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
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatmullRomCurve {
    /// The control points of the curve.
    /// Stack-allocated for up to 8 points, heap-allocated for more.
    pub points: SmallVec<[(f64, f64); 8]>,
    /// The tension parameter (0.0 = no tension, 0.5 = Catmull-Rom, 1.0 = tight).
    pub tension: f64,
}

impl CatmullRomCurve {
    /// Creates a new Catmull-Rom curve.
    #[inline]
    #[must_use]
    pub fn new(points: impl Into<SmallVec<[(f64, f64); 8]>>, tension: f64) -> Self {
        let points = points.into();
        assert!(points.len() >= 2, "Must have at least 2 points");
        Self { points, tension }
    }

    /// Creates a Catmull-Rom curve with default tension (0.0).
    #[inline]
    #[must_use]
    pub fn with_points(points: impl Into<SmallVec<[(f64, f64); 8]>>) -> Self {
        Self::new(points, 0.0)
    }
}

impl Curve for CatmullRomCurve {
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);

        if self.points.len() == 1 {
            return self.points[0].1;
        }

        // Find the segment
        let segment_count = self.points.len() - 1;
        let t_scaled = t * segment_count as f64;
        let segment = (t_scaled.floor() as usize).min(segment_count - 1);
        let local_t = t_scaled - segment as f64;

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
    fn transform(&self, t: f64) -> Curve2DSample {
        let t = t.clamp(0.0, 1.0);

        if self.points.len() == 1 {
            return self.points[0];
        }

        // Find the segment
        let segment_count = self.points.len() - 1;
        let t_scaled = t * segment_count as f64;
        let segment = (t_scaled.floor() as usize).min(segment_count - 1);
        let local_t = t_scaled - segment as f64;

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

/// A curve that is the flipped version of another curve: the 180° rotation
/// `1.0 - curve.transform(1.0 - t)`, which turns an ease-in into an ease-out
/// while still mapping `0.0 → 0.0` and `1.0 → 1.0`.
///
/// It is the 180° rotation, not the vertical mirror `1.0 - curve.transform(t)`
/// — a mirror would invert every consumer expecting a rotation (a
/// `CurvedAnimation` reverse-curve default, most visibly).
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
    fn transform(&self, t: f64) -> f64 {
        1.0 - self.curve.transform(1.0 - t)
    }
}

/// A curve that is the reversed version of another curve.
///
/// Reversing swaps the input: `transform(t)` becomes `transform(1.0 - t)`.
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
    fn transform(&self, t: f64) -> f64 {
        self.curve.transform(1.0 - t)
    }
}

// ============================================================================
// Predefined Curves
// ============================================================================

/// A collection of commonly used curves.
#[derive(Debug)]
pub struct Curves;

#[expect(non_upper_case_globals)]
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
/// `PartialEq`/`Eq` are reference equality (`Arc::ptr_eq`) — `Curve` carries
/// no structural-equality bound, so this is the only comparison available for
/// an erased `dyn Curve`. Identity comparison is what lets an
/// implicit-animation staleness check compare a repeated curve handle as
/// unchanged. Two `ArcCurve`s built from
/// separate `ArcCurve::new(...)` calls compare unequal even when they wrap the
/// same curve *value* — callers who want a stable comparison across rebuilds
/// must reuse the same `ArcCurve` handle (clone it), not reconstruct it.
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

impl PartialEq for ArcCurve {
    /// Reference equality (`Arc::ptr_eq`) — see the type doc's *why*.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ArcCurve {}

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
    fn transform(&self, t: f64) -> f64 {
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
    fn transform(&self, t: f64) -> f64 {
        (**self).transform(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                let t = i as f64 / 1000.0;
                let s = cubic.solve_x(t);
                let x = evaluate_cubic(s, 0.0, cubic.a, cubic.c, 1.0);
                assert!((x - t).abs() < 1e-3, "x({s})={x} != t={t}");
            }
        }
    }
}
