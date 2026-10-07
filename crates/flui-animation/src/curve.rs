//! Animation curves for interpolation.

use crate::animation::{Retirement, Terminal};

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

    /// The derivative `d transform / dt` at progress `t`: how fast the
    /// eased progress moves per unit of progress.
    ///
    /// Same input policy as [`transform`](Self::transform): NaN gives NaN,
    /// and outside `[0, 1]` the curve is constant, so the slope is 0. Inside
    /// `[0, 1]` the result is finite: where the true derivative is infinite
    /// (a vertical tangent, a step) the curve reports a finite secant slope
    /// instead, and the default never returns a non-finite value.
    ///
    /// The default is a second-order finite difference with step
    /// `h = 1e-4`: central inside, one-sided
    /// `(−3f(t) + 4f(t ± h) − f(t ± 2h)) / 2h` within a step of either end.
    /// Its error is about `h²·|f'''|`; a non-finite difference reports 0.
    /// [`Linear`], [`Cubic`], [`Interval`], [`FlippedCurve`] and
    /// [`ArcCurve`] override it exactly.
    ///
    /// Keyframe tracks read it to match a cubic segment's velocity to a
    /// neighbouring curved segment.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{Curve, Curves};
    ///
    /// assert_eq!(Curves::Linear.slope(0.3), 1.0);
    /// assert_eq!(Curves::EaseIn.slope(2.0), 0.0);
    /// ```
    fn slope(&self, t: f64) -> f64 {
        difference_slope(|t| self.transform(t), t)
    }

    /// The value of a curve this crate defines, which lets [`ArcCurve`]
    /// compare it by value instead of by identity.
    ///
    /// Only this crate's curves and combinators override it; the type it
    /// returns cannot be named or built elsewhere, so an implementation
    /// outside the crate keeps the default `None`.
    #[doc(hidden)]
    fn builtin(&self) -> Option<builtin::BuiltinCurve> {
        None
    }
}

/// The step of [`Curve::slope`]'s default finite difference.
const SLOPE_STEP: f64 = 1e-4;

/// [`Curve::slope`]'s default: a second-order difference of `f` at `t`.
fn difference_slope(f: impl Fn(f64) -> f64, t: f64) -> f64 {
    if t.is_nan() {
        return t;
    }
    if !(0.0..=1.0).contains(&t) {
        return 0.0;
    }
    let h = SLOPE_STEP;
    let slope = if t - h < 0.0 {
        (-3.0 * f(t) + 4.0 * f(t + h) - f(t + 2.0 * h)) / (2.0 * h)
    } else if t + h > 1.0 {
        (3.0 * f(t) - 4.0 * f(t - h) + f(t - 2.0 * h)) / (2.0 * h)
    } else {
        (f(t + h) - f(t - h)) / (2.0 * h)
    };
    if slope.is_finite() { slope } else { 0.0 }
}

/// The slope outside `(0, 1)` and for NaN, or `None` for a `t` the curve's
/// own derivative handles (`[0, 1]`).
#[inline]
fn settled_slope(t: f64) -> Option<f64> {
    if t.is_nan() {
        Some(t)
    } else if (0.0..=1.0).contains(&t) {
        None
    } else {
        Some(0.0)
    }
}

/// The closed set of curves [`ArcCurve`] compares by value.
mod builtin {
    use super::{
        Cubic, Curve, ElasticInCurve, ElasticInOutCurve, ElasticOutCurve, Steps, ThreePointCubic,
        interval_slope, interval_transform,
    };
    use std::sync::Arc;

    /// A built-in curve's value, as returned by [`Curve::builtin`].
    ///
    /// Public only so the hidden trait method can name it; it is not
    /// reachable from outside the crate.
    #[derive(Debug, Clone, PartialEq)]
    pub struct BuiltinCurve(Arc<Builtin>);

    impl BuiltinCurve {
        pub(super) fn new(curve: Builtin) -> Self {
            Self(Arc::new(curve))
        }

        pub(super) fn transform(&self, t: f64) -> f64 {
            match &*self.0 {
                Builtin::Linear => super::Linear.transform(t),
                Builtin::Decelerate => super::DecelerateCurve.transform(t),
                Builtin::BounceIn => super::BounceInCurve.transform(t),
                Builtin::BounceOut => super::BounceOutCurve.transform(t),
                Builtin::BounceInOut => super::BounceInOutCurve.transform(t),
                Builtin::Cubic(curve) => curve.transform(t),
                Builtin::ThreePointCubic(curve) => curve.transform(t),
                Builtin::ElasticIn(curve) => curve.transform(t),
                Builtin::ElasticOut(curve) => curve.transform(t),
                Builtin::ElasticInOut(curve) => curve.transform(t),
                Builtin::Steps(curve) => curve.transform(t),
                Builtin::Interval { begin, end, curve } => {
                    interval_transform(*begin, *end, t, |local| curve.transform(local))
                }
                Builtin::Flipped(curve) => 1.0 - curve.transform(1.0 - t),
            }
        }

        pub(super) fn slope(&self, t: f64) -> f64 {
            match &*self.0 {
                Builtin::Linear => super::Linear.slope(t),
                Builtin::Decelerate => super::DecelerateCurve.slope(t),
                Builtin::BounceIn => super::BounceInCurve.slope(t),
                Builtin::BounceOut => super::BounceOutCurve.slope(t),
                Builtin::BounceInOut => super::BounceInOutCurve.slope(t),
                Builtin::Cubic(curve) => curve.slope(t),
                Builtin::ThreePointCubic(curve) => curve.slope(t),
                Builtin::ElasticIn(curve) => curve.slope(t),
                Builtin::ElasticOut(curve) => curve.slope(t),
                Builtin::ElasticInOut(curve) => curve.slope(t),
                Builtin::Steps(curve) => curve.slope(t),
                Builtin::Interval { begin, end, curve } => interval_slope(
                    *begin,
                    *end,
                    t,
                    |local| curve.slope(local),
                    |local| curve.transform(local),
                ),
                Builtin::Flipped(curve) => curve.slope(1.0 - t),
            }
        }
    }

    /// Every curve type the crate defines that has value equality; a
    /// combinator is listed when its inner curve is.
    #[derive(Debug, PartialEq)]
    #[expect(
        clippy::large_enum_variant,
        reason = "only ever allocated once, behind the Arc in BuiltinCurve"
    )]
    pub(super) enum Builtin {
        Linear,
        Decelerate,
        BounceIn,
        BounceOut,
        BounceInOut,
        Cubic(Cubic),
        ThreePointCubic(ThreePointCubic),
        ElasticIn(ElasticInCurve),
        ElasticOut(ElasticOutCurve),
        ElasticInOut(ElasticInOutCurve),
        Steps(Steps),
        Interval {
            begin: f64,
            end: f64,
            curve: BuiltinCurve,
        },
        Flipped(BuiltinCurve),
    }
}

use builtin::{Builtin, BuiltinCurve};

/// Implements [`Curve::builtin`] for a curve with no parameters or a
/// `Copy` value.
macro_rules! builtin_value {
    ($variant:ident) => {
        fn builtin(&self) -> Option<BuiltinCurve> {
            Some(BuiltinCurve::new(Builtin::$variant))
        }
    };
    ($variant:ident(self)) => {
        fn builtin(&self) -> Option<BuiltinCurve> {
            Some(BuiltinCurve::new(Builtin::$variant(*self)))
        }
    };
}

/// Why a curve parameter was rejected.
///
/// Returned by the `try_new` constructors and, as the error message, by
/// serde decoding (feature `serde`). The panicking `const fn new` constructors
/// cannot format a value in a `const` context, so their message states the
/// rule the arguments broke; the matching `try_new` names the parameter and
/// value.
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
    /// [`Steps`] with [`JumpAt::None`] and fewer than two steps: the curve
    /// would have no jump between its start and end values.
    #[error("steps with `JumpAt::None` need at least two steps")]
    TooFewSteps,
}

/// Propagates a validation error out of a `const fn` (`?` is not const).
macro_rules! check {
    ($result:expr) => {
        if let Err(error) = $result {
            return Err(error);
        }
    };
}

/// `value` is finite.
const fn finite(value: f64, parameter: &'static str) -> Result<(), CurveError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(CurveError::NonFinite { parameter })
    }
}

/// `value` is finite and inside `[low, high]`.
const fn within(
    value: f64,
    low: f64,
    high: f64,
    parameter: &'static str,
    allowed: &'static str,
) -> Result<(), CurveError> {
    check!(finite(value, parameter));
    if value < low || value > high {
        return Err(CurveError::OutOfRange {
            parameter,
            value,
            allowed,
        });
    }
    Ok(())
}

/// `value` is finite and inside the open interval `(0, 1)`.
const fn strictly_inside_unit(value: f64, parameter: &'static str) -> Result<(), CurveError> {
    check!(finite(value, parameter));
    if value <= 0.0 || value >= 1.0 {
        return Err(CurveError::OutOfRange {
            parameter,
            value,
            allowed: "(0, 1)",
        });
    }
    Ok(())
}

/// The input policy shared by every built-in curve: NaN passes through,
/// the ends are exact, anything outside `[0, 1]` clamps. `None` means `t` is
/// strictly inside `(0, 1)` and the curve evaluates its own shape.
#[inline]
fn settled(t: f64) -> Option<f64> {
    if t.is_nan() {
        Some(t)
    } else if t <= 0.0 {
        Some(0.0)
    } else if t >= 1.0 {
        Some(1.0)
    } else {
        None
    }
}

// ============================================================================
// Standard Curves
// ============================================================================
/// A linear curve.
///
/// The identity function on `[0, 1]`. Monotone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Linear;

impl Curve for Linear {
    builtin_value!(Linear);

    #[inline]
    fn transform(&self, t: f64) -> f64 {
        t.clamp(0.0, 1.0)
    }

    fn slope(&self, t: f64) -> f64 {
        settled_slope(t).unwrap_or(1.0)
    }
}

/// A curve that is 0 until `begin`, follows `curve` rescaled to
/// `[begin, end]`, and is 1 after `end`.
///
/// Lets several animations share one controller, each running in its own
/// slice of the controller's progress. An interval narrower than `1e-6` is a
/// step at `end`. Monotone when `curve` is.
///
/// # Examples
///
/// ```
/// use flui_animation::{Curve, Curves, Interval};
///
/// let late = Interval::new(0.5, 1.0, Curves::Linear);
/// assert_eq!(late.transform(0.25), 0.0);
/// assert_eq!(late.transform(0.75), 0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        try_from = "IntervalWire<C>",
        into = "IntervalWire<C>",
        bound(
            serialize = "C: serde::Serialize",
            deserialize = "C: serde::Deserialize<'de>"
        )
    )
)]
pub struct Interval<C: Curve + Copy = Linear> {
    begin: f64,
    end: f64,
    curve: C,
}

impl<C: Curve + Copy> Interval<C> {
    /// Admits `0 <= begin <= end <= 1`, both finite.
    const fn validate(begin: f64, end: f64) -> Result<(), CurveError> {
        check!(within(begin, 0.0, 1.0, "begin", "[0, 1]"));
        check!(within(end, 0.0, 1.0, "end", "[0, 1]"));
        if begin > end {
            return Err(CurveError::IntervalReversed { begin, end });
        }
        Ok(())
    }

    /// Creates an interval curve running `curve` over `[begin, end]`.
    ///
    /// # Panics
    ///
    /// Panics when [`Interval::try_new`] would return an error. In a `const`
    /// item the panic is a compile error.
    #[must_use]
    pub const fn new(begin: f64, end: f64, curve: C) -> Self {
        let interval = Self { begin, end, curve };
        match Self::validate(begin, end) {
            Ok(()) => interval,
            Err(_) => panic!(
                "Interval::new: begin and end must be finite with 0 <= begin <= end <= 1 \
                 (Interval::try_new reports which)"
            ),
        }
    }

    /// Creates an interval curve, rejecting invalid bounds.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when `begin` or `end` is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when either lies outside `[0, 1]`;
    /// - [`CurveError::IntervalReversed`] when `begin > end`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, Interval, Linear};
    ///
    /// assert!(Interval::try_new(0.2, 0.8, Linear).is_ok());
    /// assert!(matches!(
    ///     Interval::try_new(0.8, 0.2, Linear),
    ///     Err(CurveError::IntervalReversed { .. })
    /// ));
    /// ```
    pub fn try_new(begin: f64, end: f64, curve: C) -> Result<Self, CurveError> {
        Self::validate(begin, end)?;
        Ok(Self { begin, end, curve })
    }
}

impl Interval<Linear> {
    /// Creates a linear interval over `[begin, end]`.
    ///
    /// # Panics
    ///
    /// Panics on the bounds [`Interval::try_new`] rejects.
    #[must_use]
    pub const fn linear(begin: f64, end: f64) -> Self {
        Self::new(begin, end, Linear)
    }
}

impl<C: Curve + Copy> Curve for Interval<C> {
    fn transform(&self, t: f64) -> f64 {
        interval_transform(self.begin, self.end, t, |local| self.curve.transform(local))
    }

    fn slope(&self, t: f64) -> f64 {
        interval_slope(
            self.begin,
            self.end,
            t,
            |local| self.curve.slope(local),
            |local| self.curve.transform(local),
        )
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        let curve = self.curve.builtin()?;
        Some(BuiltinCurve::new(Builtin::Interval {
            begin: self.begin,
            end: self.end,
            curve,
        }))
    }
}

/// [`Interval`]'s derivative by the chain rule: 0 outside `[begin, end]`
/// and for a step, `slope(local) / (end − begin)` inside. Where that quotient
/// overflows (a steep inner curve in a narrow interval) the finite
/// difference of the interval's own shape stands in.
fn interval_slope(
    begin: f64,
    end: f64,
    t: f64,
    slope: impl FnOnce(f64) -> f64,
    transform: impl Fn(f64) -> f64,
) -> f64 {
    if let Some(settled) = settled_slope(t) {
        return settled;
    }
    if t < begin || t > end || end - begin < 1e-6 {
        return 0.0;
    }
    let chained = slope((t - begin) / (end - begin)) / (end - begin);
    if chained.is_finite() {
        chained
    } else {
        difference_slope(|t| interval_transform(begin, end, t, &transform), t)
    }
}

/// [`Interval`]'s shape over validated bounds, with `inner` the curve
/// evaluated on the local progress.
fn interval_transform(begin: f64, end: f64, t: f64, inner: impl FnOnce(f64) -> f64) -> f64 {
    if let Some(settled) = settled(t) {
        return settled;
    }
    if t < begin {
        0.0
    } else if t > end {
        1.0
    } else if end - begin < 1e-6 {
        if t < end { 0.0 } else { 1.0 }
    } else {
        inner((t - begin) / (end - begin))
    }
}

/// The serialized form of [`Interval`]: the field names it always had.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Interval")]
struct IntervalWire<C> {
    begin: f64,
    end: f64,
    curve: C,
}

#[cfg(feature = "serde")]
impl<C: Curve + Copy> From<Interval<C>> for IntervalWire<C> {
    fn from(interval: Interval<C>) -> Self {
        Self {
            begin: interval.begin,
            end: interval.end,
            curve: interval.curve,
        }
    }
}

#[cfg(feature = "serde")]
impl<C: Curve + Copy> TryFrom<IntervalWire<C>> for Interval<C> {
    type Error = CurveError;

    fn try_from(wire: IntervalWire<C>) -> Result<Self, CurveError> {
        Self::try_new(wire.begin, wire.end, wire.curve)
    }
}

// ============================================================================
// Cubic Curves
// ============================================================================

/// One coordinate of a unit cubic bezier, `p(s) = ((a·s + b)·s + c)·s`, with
/// end points 0 and 1.
#[derive(Clone, Copy)]
struct UnitBezier {
    a: f64,
    b: f64,
    c: f64,
}

impl UnitBezier {
    const fn new(p1: f64, p2: f64) -> Self {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        Self {
            a: 1.0 - c - b,
            b,
            c,
        }
    }

    #[inline]
    const fn at(self, s: f64) -> f64 {
        ((self.a * s + self.b) * s + self.c) * s
    }

    #[inline]
    fn slope(self, s: f64) -> f64 {
        (3.0 * self.a * s + 2.0 * self.b) * s + self.c
    }

    #[inline]
    fn curvature(self, s: f64) -> f64 {
        6.0 * self.a * s + 2.0 * self.b
    }
}

/// The largest `|y1|`, `|y2|` [`Cubic`] admits. Far past any easing's
/// overshoot, and small enough that the solver's coefficients and slope bound
/// stay finite.
const MAX_CUBIC_Y: f64 = 1e6;

/// The largest `|y|` a [`ThreePointCubic`] segment may reach once rescaled to
/// the unit square: the bezier coefficients `3·y` and `3·(y2 − y1)` stay
/// finite below it.
const MAX_SEGMENT_Y: f64 = 1e300;

/// Bezier parameters of the x lookup table.
const SAMPLE_PARAMETERS: [f64; 11] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

/// The output error the cubic solver stops at.
const OUTPUT_TOLERANCE: f64 = 1e-7;

/// Newton steps tried before bisection takes over.
const NEWTON_STEPS: usize = 4;

/// A Newton step this small means the next error is near its square:
/// stop iterating and prove the bound instead.
const NEWTON_SETTLED_STEP: f64 = 1e-5;

/// Below this `|x'(s)|` a Newton step is not trusted.
const MIN_NEWTON_SLOPE: f64 = 1e-7;

/// A CSS `cubic-bezier(x1, y1, x2, y2)` easing curve.
///
/// The bezier runs from `(0, 0)` through control points `(x1, y1)` and
/// `(x2, y2)` to `(1, 1)`. `x1` and `x2` lie in `[0, 1]` (CSS Easing 2
/// §2.2), so x is monotone in the bezier parameter and every progress value
/// has exactly one output; `y1` and `y2` lie in `[-1e6, 1e6]`, and outside
/// `[0, 1]` the curve overshoots. The curve is monotone when `y1` and `y2`
/// both lie in `[0, 1]`.
///
/// # Accuracy
///
/// The solver (the WebKit `UnitBezier` / Chromium `gfx::CubicBezier`
/// method: an 11-sample table, up to four Newton steps, then bisection)
/// stops on the **output**: it brackets the bezier parameter until the
/// bracket's width times the largest `|dy/ds|` is below `1e-7`, so the
/// result is within `1e-7` of the exact y(x) — including next to a vertical
/// tangent such as [`Curves::EaseInOutExpo`]'s, where a solver that stops on
/// the x residual is off by up to `9e-3`. Floating-point rounding in x
/// limits this only within about `1e-16 / x'(s)` of a point where `x'(s) = 0`.
///
/// # Serde
///
/// Serializes as `{ "a": x1, "b": y1, "c": x2, "d": y2 }`; decoding rejects
/// what [`Cubic::try_new`] rejects.
///
/// # Examples
///
/// ```
/// use flui_animation::{Cubic, Curve};
///
/// const EASE: Cubic = Cubic::new(0.25, 0.1, 0.25, 1.0);
/// assert!((EASE.transform(0.5) - 0.802_403_387_584_857).abs() < 1e-6);
/// ```
#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "CubicWire", into = "CubicWire"))]
pub struct Cubic {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x: UnitBezier,
    y: UnitBezier,
    /// Upper bound of `|dy/ds|` on `[0, 1]`; at least 1.
    y_slope_bound: f64,
    /// `x(SAMPLE_PARAMETERS[i])`.
    x_samples: [f64; SAMPLE_PARAMETERS.len()],
}

impl Cubic {
    /// Admits finite control points with `x1, x2 ∈ [0, 1]` and
    /// `y1, y2 ∈ [-MAX_CUBIC_Y, MAX_CUBIC_Y]`.
    const fn validate(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError> {
        check!(within(x1, 0.0, 1.0, "x1", "[0, 1]"));
        check!(within(y1, -MAX_CUBIC_Y, MAX_CUBIC_Y, "y1", "[-1e6, 1e6]"));
        check!(within(x2, 0.0, 1.0, "x2", "[0, 1]"));
        check!(within(y2, -MAX_CUBIC_Y, MAX_CUBIC_Y, "y2", "[-1e6, 1e6]"));
        Ok(Self::solved(x1, y1, x2, y2))
    }

    /// Precomputes the solver state for validated control points.
    const fn solved(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        let x = UnitBezier::new(x1, x2);
        // dy/ds is a quadratic bezier with control values 3·y1, 3·(y2 − y1),
        // 3·(1 − y2); its convex hull bounds it. The three sum to 3, so the
        // bound is at least 1.
        let y_slope_bound = 3.0 * y1.abs().max((y2 - y1).abs()).max((1.0 - y2).abs());
        let mut x_samples = [0.0; SAMPLE_PARAMETERS.len()];
        let mut i = 0;
        while i < SAMPLE_PARAMETERS.len() {
            x_samples[i] = x.at(SAMPLE_PARAMETERS[i]);
            i += 1;
        }
        Self {
            x1,
            y1,
            x2,
            y2,
            x,
            y: UnitBezier::new(y1, y2),
            y_slope_bound,
            x_samples,
        }
    }

    /// Creates a `cubic-bezier(x1, y1, x2, y2)` curve.
    ///
    /// # Panics
    ///
    /// Panics when [`Cubic::try_new`] would return an error. In a `const`
    /// item the panic is a compile error.
    #[must_use]
    pub const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        match Self::validate(x1, y1, x2, y2) {
            Ok(cubic) => cubic,
            Err(_) => panic!(
                "Cubic::new: x1 and x2 must be finite and in [0, 1], y1 and y2 in \
                 [-1e6, 1e6] (Cubic::try_new reports which)"
            ),
        }
    }

    /// Creates a `cubic-bezier(x1, y1, x2, y2)` curve, rejecting invalid
    /// control points.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when any argument is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when `x1` or `x2` lies outside `[0, 1]`, or
    ///   `y1` or `y2` outside `[-1e6, 1e6]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{Cubic, CurveError};
    ///
    /// assert!(Cubic::try_new(0.68, -0.55, 0.265, 1.55).is_ok());
    /// assert!(matches!(
    ///     Cubic::try_new(1.5, 0.0, 0.5, 1.0),
    ///     Err(CurveError::OutOfRange { parameter: "x1", .. })
    /// ));
    /// ```
    pub fn try_new(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError> {
        Self::validate(x1, y1, x2, y2)
    }

    /// Solves `x(s) = x` for the bezier parameter `s`, `0 < x < 1`.
    fn solve(&self, x: f64) -> f64 {
        // The table brackets the root: x(s) is non-decreasing.
        let upper = self.x_samples[1..]
            .iter()
            .position(|&sample| sample > x)
            .map_or(SAMPLE_PARAMETERS.len() - 1, |i| i + 1);
        let (mut lo, mut hi) = (SAMPLE_PARAMETERS[upper - 1], SAMPLE_PARAMETERS[upper]);
        let (x_lo, x_hi) = (self.x_samples[upper - 1], self.x_samples[upper]);
        let mut s = if x_hi > x_lo {
            lo + (x - x_lo) / (x_hi - x_lo) * (hi - lo)
        } else {
            f64::midpoint(lo, hi)
        };

        // The parameter distance that moves y by at most the tolerance.
        let reach = OUTPUT_TOLERANCE / self.y_slope_bound;
        for _ in 0..NEWTON_STEPS {
            let error = self.x.at(s) - x;
            if error < 0.0 {
                lo = s;
            } else if error > 0.0 {
                hi = s;
            } else {
                return s;
            }
            let slope = self.x.slope(s);
            if slope.abs() < MIN_NEWTON_SLOPE {
                break;
            }
            let next = s - error / slope;
            if !(lo..=hi).contains(&next) {
                break;
            }
            let step = (next - s).abs();
            s = next;
            if step < NEWTON_SETTLED_STEP {
                break;
            }
        }

        // Accept `s` only once the root is proven to lie within `reach` of
        // it; either way the two probes narrow the bracket.
        let below = (s - reach).max(lo);
        let above = (s + reach).min(hi);
        let x_below = self.x.at(below);
        let x_above = self.x.at(above);
        if x < x_below {
            hi = below;
        } else if x > x_above {
            lo = above;
        } else {
            return s;
        }

        // Bisection until the bracket alone bounds the output error.
        loop {
            let mid = f64::midpoint(lo, hi);
            if (hi - lo) * self.y_slope_bound < 2.0 * OUTPUT_TOLERANCE || mid <= lo || mid >= hi {
                return mid;
            }
            if self.x.at(mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }

    /// `dy/dx` at `x` where `x'` nearly vanishes inside the curve. About the
    /// stationary point `c` of `x'` the x polynomial is exactly
    /// `x(c) + p·u + a·u³` with `u = s − c` and `p = x'(c) ≥ 0`, so `u` is
    /// solved from the small offset `x − x(c)` to full relative precision and
    /// `x'(s) = p + 3a·u²` follows from it, rather than from an `s` rounding
    /// has moved onto the tangent. `None` when x has no such point (`a ≤ 0`).
    fn tangent_slope(&self, x: f64) -> Option<f64> {
        let cubed = self.x.a;
        if cubed <= 0.0 {
            return None;
        }
        let centre = (-self.x.b / (3.0 * cubed)).clamp(0.0, 1.0);
        let rate = self.x.slope(centre).max(0.0);
        let offset = x - self.x.at(centre);
        // `a·u³ + p·u` increases in `u`: bisect it over the parameter range.
        let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
        loop {
            let mid = f64::midpoint(lo, hi);
            if mid <= lo || mid >= hi {
                break;
            }
            if (cubed * mid).mul_add(mid, rate) * mid < offset {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let shift = f64::midpoint(lo, hi);
        Some(self.y.slope(centre + shift) / (3.0 * cubed * shift).mul_add(shift, rate))
    }
}

impl Curve for Cubic {
    builtin_value!(Cubic(self));

    /// `dy/dx = y'(s) / x'(s)` at the solved parameter `s` whenever `x'(s)` is
    /// nonzero and the quotient finite. At the ends `s` is exact, so however
    /// small `x'(s)` is it is the true derivative. Inside, `s` is known only to
    /// within the solver's reach `r`, which moves `x'` by up to
    /// `|x''|·r + 3|a|·r²`; an `x'(s)` below that is rounding, not slope, and
    /// the slope comes from the parameter re-solved about the stationary
    /// point of `x'` instead ([`Cubic::tangent_slope`]). Where `x'(s) = 0`
    /// exactly and `y'(s) = 0` too (a flat start such as
    /// `cubic-bezier(0, 0, …)`), the limit `y''(s) / x''(s)`; at a vertical
    /// tangent, or where the quotient overflows, the finite secant of the
    /// default difference.
    fn slope(&self, t: f64) -> f64 {
        if let Some(settled) = settled_slope(t) {
            return settled;
        }
        let interior = t > 0.0 && t < 1.0;
        let s = if t <= 0.0 {
            0.0
        } else if t >= 1.0 {
            1.0
        } else {
            self.solve(t)
        };
        let dx = self.x.slope(s);
        let dy = self.y.slope(s);
        let reach = OUTPUT_TOLERANCE / self.y_slope_bound;
        let x_error = self.x.curvature(s).abs() * reach + 3.0 * self.x.a.abs() * reach * reach;
        let ratio = if interior && dx.abs() <= x_error {
            self.tangent_slope(t).unwrap_or(f64::NAN)
        } else if dx != 0.0 {
            dy / dx
        } else if dy == 0.0 {
            self.y.curvature(s) / self.x.curvature(s)
        } else {
            f64::NAN
        };
        if ratio.is_finite() {
            return ratio;
        }
        difference_slope(|t| self.transform(t), t)
    }

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        self.y.at(self.solve(t))
    }
}

impl PartialEq for Cubic {
    /// Compares the control points; the solver state derives from them.
    fn eq(&self, other: &Self) -> bool {
        (self.x1, self.y1, self.x2, self.y2) == (other.x1, other.y1, other.x2, other.y2)
    }
}

impl fmt::Debug for Cubic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cubic")
            .field("x1", &self.x1)
            .field("y1", &self.y1)
            .field("x2", &self.x2)
            .field("y2", &self.y2)
            .finish_non_exhaustive()
    }
}

/// The serialized form of [`Cubic`]: the field names it always had.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "Cubic")]
struct CubicWire {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

#[cfg(feature = "serde")]
impl From<Cubic> for CubicWire {
    fn from(cubic: Cubic) -> Self {
        Self {
            a: cubic.x1,
            b: cubic.y1,
            c: cubic.x2,
            d: cubic.y2,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<CubicWire> for Cubic {
    type Error = CurveError;

    fn try_from(wire: CubicWire) -> Result<Self, CurveError> {
        Self::try_new(wire.a, wire.b, wire.c, wire.d)
    }
}

/// A rescaled [`ThreePointCubic`] segment y the cubic solver evaluates
/// without overflow.
const fn segment_y(y: f64) -> bool {
    y.is_finite() && y.abs() <= MAX_SEGMENT_Y
}

/// Two cubic bezier segments joined at a shared `midpoint`.
///
/// The curve passes through `(0, 0)`, `midpoint` and `(1, 1)`; each half is
/// a [`Cubic`] rescaled into its sub-rectangle. This is the building block
/// for the Material 3 emphasized easing set. Monotone when each segment's
/// control-point y values lie between its end points' y values.
///
/// # Serde
///
/// Serializes the five points as `a1`, `b1`, `midpoint`, `a2`, `b2`;
/// decoding rejects what [`ThreePointCubic::try_new`] rejects.
#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(try_from = "ThreePointCubicWire", into = "ThreePointCubicWire")
)]
pub struct ThreePointCubic {
    a1: (f64, f64),
    b1: (f64, f64),
    midpoint: (f64, f64),
    a2: (f64, f64),
    b2: (f64, f64),
    /// The first half, rescaled to the unit square.
    first: Cubic,
    /// The second half, rescaled to the unit square.
    second: Cubic,
}

impl ThreePointCubic {
    /// Admits finite points with `midpoint` strictly inside the unit square,
    /// `a1.x, b1.x ∈ [0, midpoint.x]`, `a2.x, b2.x ∈ [midpoint.x, 1]` and
    /// every control y in `[-1e6, 1e6]`.
    const fn validate(
        a1: (f64, f64),
        b1: (f64, f64),
        midpoint: (f64, f64),
        a2: (f64, f64),
        b2: (f64, f64),
    ) -> Result<Self, CurveError> {
        let (mx, my) = midpoint;
        check!(strictly_inside_unit(mx, "midpoint.x"));
        check!(strictly_inside_unit(my, "midpoint.y"));
        check!(within(a1.0, 0.0, mx, "a1.x", "[0, midpoint.x]"));
        check!(within(
            a1.1,
            -MAX_CUBIC_Y,
            MAX_CUBIC_Y,
            "a1.y",
            "[-1e6, 1e6]"
        ));
        check!(within(b1.0, 0.0, mx, "b1.x", "[0, midpoint.x]"));
        check!(within(
            b1.1,
            -MAX_CUBIC_Y,
            MAX_CUBIC_Y,
            "b1.y",
            "[-1e6, 1e6]"
        ));
        check!(within(a2.0, mx, 1.0, "a2.x", "[midpoint.x, 1]"));
        check!(within(
            a2.1,
            -MAX_CUBIC_Y,
            MAX_CUBIC_Y,
            "a2.y",
            "[-1e6, 1e6]"
        ));
        check!(within(b2.0, mx, 1.0, "b2.x", "[midpoint.x, 1]"));
        check!(within(
            b2.1,
            -MAX_CUBIC_Y,
            MAX_CUBIC_Y,
            "b2.y",
            "[-1e6, 1e6]"
        ));
        // Rescaling keeps x inside [0, 1] (IEEE division and subtraction are
        // monotone). With every y within MAX_CUBIC_Y, a rescaled y reaches
        // MAX_SEGMENT_Y only for a midpoint.y within about 1e-294 of 0 or 1.
        let (wide, high) = (1.0 - mx, 1.0 - my);
        let first = (a1.1 / my, b1.1 / my);
        let second = ((a2.1 - my) / high, (b2.1 - my) / high);
        if !(segment_y(first.0) && segment_y(first.1) && segment_y(second.0) && segment_y(second.1))
        {
            return Err(CurveError::OutOfRange {
                parameter: "midpoint.y",
                value: my,
                allowed: "(0, 1), far enough from both to rescale the control points",
            });
        }
        let first = Cubic::solved(a1.0 / mx, first.0, b1.0 / mx, first.1);
        let second = Cubic::solved((a2.0 - mx) / wide, second.0, (b2.0 - mx) / wide, second.1);
        Ok(Self {
            a1,
            b1,
            midpoint,
            a2,
            b2,
            first,
            second,
        })
    }

    /// Creates a three-point cubic from the control points of both segments.
    ///
    /// The two implied end points `(0, 0)` and `(1, 1)` are fixed and not
    /// passed.
    ///
    /// # Panics
    ///
    /// Panics when [`ThreePointCubic::try_new`] would return an error. In a
    /// `const` item the panic is a compile error.
    #[must_use]
    pub const fn new(
        a1: (f64, f64),
        b1: (f64, f64),
        midpoint: (f64, f64),
        a2: (f64, f64),
        b2: (f64, f64),
    ) -> Self {
        match Self::validate(a1, b1, midpoint, a2, b2) {
            Ok(curve) => curve,
            Err(_) => panic!(
                "ThreePointCubic::new: midpoint must lie strictly inside the unit square, \
                 a1.x and b1.x in [0, midpoint.x], a2.x and b2.x in [midpoint.x, 1], \
                 every control y in [-1e6, 1e6] (ThreePointCubic::try_new reports which)"
            ),
        }
    }

    /// Creates a three-point cubic, rejecting invalid control points.
    ///
    /// # Errors
    ///
    /// - [`CurveError::NonFinite`] when any coordinate is NaN or infinite;
    /// - [`CurveError::OutOfRange`] when `midpoint` is not strictly inside
    ///   the unit square, when a first-segment control point's x lies outside
    ///   `[0, midpoint.x]` or a second-segment one's outside
    ///   `[midpoint.x, 1]`, when a control y lies outside `[-1e6, 1e6]`, or
    ///   when `midpoint.y` is within about `1e-294` of 0 or 1, so that a
    ///   rescaled control point overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, ThreePointCubic};
    ///
    /// let on_edge =
    ///     ThreePointCubic::try_new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0));
    /// assert!(matches!(
    ///     on_edge,
    ///     Err(CurveError::OutOfRange { parameter: "midpoint.x", .. })
    /// ));
    /// ```
    pub fn try_new(
        a1: (f64, f64),
        b1: (f64, f64),
        midpoint: (f64, f64),
        a2: (f64, f64),
        b2: (f64, f64),
    ) -> Result<Self, CurveError> {
        Self::validate(a1, b1, midpoint, a2, b2)
    }
}

impl Curve for ThreePointCubic {
    builtin_value!(ThreePointCubic(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        let (mx, my) = self.midpoint;
        if t < mx {
            self.first.transform(t / mx) * my
        } else {
            self.second.transform((t - mx) / (1.0 - mx)) * (1.0 - my) + my
        }
    }
}

impl PartialEq for ThreePointCubic {
    /// Compares the five points; the segments derive from them.
    fn eq(&self, other: &Self) -> bool {
        (self.a1, self.b1, self.midpoint, self.a2, self.b2)
            == (other.a1, other.b1, other.midpoint, other.a2, other.b2)
    }
}

impl fmt::Debug for ThreePointCubic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreePointCubic")
            .field("a1", &self.a1)
            .field("b1", &self.b1)
            .field("midpoint", &self.midpoint)
            .field("a2", &self.a2)
            .field("b2", &self.b2)
            .finish_non_exhaustive()
    }
}

/// The serialized form of [`ThreePointCubic`]: the field names it always had.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "ThreePointCubic")]
struct ThreePointCubicWire {
    a1: (f64, f64),
    b1: (f64, f64),
    midpoint: (f64, f64),
    a2: (f64, f64),
    b2: (f64, f64),
}

#[cfg(feature = "serde")]
impl From<ThreePointCubic> for ThreePointCubicWire {
    fn from(curve: ThreePointCubic) -> Self {
        Self {
            a1: curve.a1,
            b1: curve.b1,
            midpoint: curve.midpoint,
            a2: curve.a2,
            b2: curve.b2,
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<ThreePointCubicWire> for ThreePointCubic {
    type Error = CurveError;

    fn try_from(wire: ThreePointCubicWire) -> Result<Self, CurveError> {
        Self::try_new(wire.a1, wire.b1, wire.midpoint, wire.a2, wire.b2)
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
        if let Err(error) = validate_split(fields.split) {
            // Preserve the validation error without invoking opaque curve
            // destruction on the rejected aggregate.
            return Err(serde::de::Error::custom(error));
        }
        Ok(Self {
            boundary: fields.split,
            begin_curve: fields.begin_curve.commit(),
            end_curve: fields.end_curve.commit(),
        })
    }
}

/// Admits a finite split point in `[0, 1]`.
const fn validate_split(split: f64) -> Result<(), CurveError> {
    within(split, 0.0, 1.0, "split", "[0, 1]")
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
    ///
    /// # Panics
    ///
    /// Panics with the [`CurveError`] message when `split` is not finite or
    /// lies outside `[0, 1]`.
    #[must_use]
    pub fn with_curves(split: f64, begin_curve: B, end_curve: E) -> Self {
        let begin_curve = Terminal::new(begin_curve);
        let end_curve = Terminal::new(end_curve);
        if let Err(error) = validate_split(split) {
            panic!("{error}");
        }
        Self {
            boundary: split,
            begin_curve,
            end_curve,
        }
    }
}

impl<B: Curve, E: Curve> Curve for Split<B, E> {
    #[expect(
        clippy::float_cmp,
        reason = "the boundary itself maps to itself exactly"
    )]
    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
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

/// `2^-10`: the Penner elastic envelope's value at the far end.
const ELASTIC_ENVELOPE_FLOOR: f64 = 1.0 / 1024.0;

/// The Penner elastic envelope `2^(-10 u)`, rescaled so it falls from exactly
/// 1 at `u = 0` to exactly 0 at `u = 1`.
///
/// The unscaled envelope ends at `2^-10`, so the curve jumped by up to
/// `2^-10` onto its end value on the last frame. Rescaling moves the shape by
/// at most `2^-10` and makes it continuous at both ends.
#[inline]
fn elastic_envelope(u: f64) -> f64 {
    (2.0_f64.powf(-10.0 * u) - ELASTIC_ENVELOPE_FLOOR) / (1.0 - ELASTIC_ENVELOPE_FLOOR)
}

/// The elastic oscillation at phase `u` for `period`.
#[inline]
fn elastic_wave(u: f64, period: f64) -> f64 {
    ((u - period / 4.0) * std::f64::consts::TAU / period).sin()
}

/// Admits an elastic period in `[1e-6, 1e6]`: covers every visible
/// oscillation and is narrow enough that the phase `(u − period / 4)·τ / period`
/// stays finite (an unbounded period overflows the product, a subnormal one
/// the quotient).
const fn validate_period(period: f64) -> Result<(), CurveError> {
    within(period, 1e-6, 1e6, "period", "[1e-6, 1e6]")
}

/// Declares an elastic curve type: the period field, its validated
/// constructors, `Default` (period 0.4) and its serde form.
macro_rules! elastic_curve {
    ($(#[$doc:meta])* $name:ident = $name_str:literal, $wire:ident = $wire_str:literal) => {
        $(#[$doc])*
        ///
        /// Not monotone: it overshoots `[0, 1]`. Continuous at both ends.
        ///
        /// # Serde
        ///
        /// Serializes as `{ "period": … }`; decoding rejects what `try_new`
        /// rejects.
        #[derive(Debug, Clone, Copy, PartialEq)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(try_from = $wire_str, into = $wire_str))]
        pub struct $name {
            period: f64,
        }

        impl $name {
            /// Creates the curve with the given oscillation `period`, as a fraction
            /// of the animation.
            ///
            /// # Panics
            ///
            /// Panics when the period lies outside `[1e-6, 1e6]`. In a `const`
            /// item the panic is a compile error.
            #[must_use]
            pub const fn new(period: f64) -> Self {
                match validate_period(period) {
                    Ok(()) => Self { period },
                    Err(_) => panic!(concat!(
                        stringify!($name),
                        "::new: period must lie in [1e-6, 1e6]"
                    )),
                }
            }

            /// Creates the curve, rejecting an invalid period.
            ///
            /// # Errors
            ///
            /// - [`CurveError::NonFinite`] when `period` is NaN or infinite;
            /// - [`CurveError::OutOfRange`] when `period` lies outside `[1e-6, 1e6]`.
            ///
            /// # Examples
            ///
            /// ```
            #[doc = concat!("use flui_animation::{CurveError, ", stringify!($name), "};")]
            ///
            #[doc = concat!("assert!(", stringify!($name), "::try_new(0.3).is_ok());")]
            #[doc = concat!("assert!(matches!(", stringify!($name), "::try_new(0.0), Err(CurveError::OutOfRange { .. })));")]
            /// ```
            pub fn try_new(period: f64) -> Result<Self, CurveError> {
                validate_period(period)?;
                Ok(Self { period })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new(0.4)
            }
        }

        #[cfg(feature = "serde")]
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(rename = $name_str)]
        struct $wire {
            period: f64,
        }

        #[cfg(feature = "serde")]
        impl From<$name> for $wire {
            fn from(curve: $name) -> Self {
                Self {
                    period: curve.period,
                }
            }
        }

        #[cfg(feature = "serde")]
        impl TryFrom<$wire> for $name {
            type Error = CurveError;

            fn try_from(wire: $wire) -> Result<Self, CurveError> {
                Self::try_new(wire.period)
            }
        }
    };
}

elastic_curve!(
    /// An oscillating curve that grows in magnitude while overshooting its
    /// bounds.
    ElasticInCurve = "ElasticInCurve",
    ElasticInWire = "ElasticInWire"
);

elastic_curve!(
    /// An oscillating curve that shrinks in magnitude while overshooting its
    /// bounds.
    ElasticOutCurve = "ElasticOutCurve",
    ElasticOutWire = "ElasticOutWire"
);

elastic_curve!(
    /// An oscillating curve that grows and then shrinks in magnitude while
    /// overshooting its bounds.
    ElasticInOutCurve = "ElasticInOutCurve",
    ElasticInOutWire = "ElasticInOutWire"
);

impl Curve for ElasticInCurve {
    builtin_value!(ElasticIn(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        -elastic_envelope(1.0 - t) * elastic_wave(t - 1.0, self.period)
    }
}

impl Curve for ElasticOutCurve {
    builtin_value!(ElasticOut(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        elastic_envelope(t) * elastic_wave(t, self.period) + 1.0
    }
}

impl Curve for ElasticInOutCurve {
    builtin_value!(ElasticInOut(self));

    fn transform(&self, t: f64) -> f64 {
        if let Some(settled) = settled(t) {
            return settled;
        }
        let u = 2.0 * t - 1.0;
        if u < 0.0 {
            -0.5 * elastic_envelope(-u) * elastic_wave(u, self.period)
        } else {
            0.5 * elastic_envelope(u) * elastic_wave(u, self.period) + 1.0
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
    builtin_value!(BounceOut);

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
    builtin_value!(BounceIn);

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
    builtin_value!(BounceInOut);

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
    builtin_value!(Decelerate);

    #[inline]
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        1.0 - (1.0 - t) * (1.0 - t)
    }
}

// ============================================================================
// Step Curves
// ============================================================================

/// Where a [`Steps`] curve places its jumps (CSS Easing 1 §2.3.1
/// `<step-position>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum JumpAt {
    /// The first jump happens as the curve starts (`jump-start`).
    Start,
    /// The last jump happens as the curve ends (`jump-end`, the CSS default).
    #[default]
    End,
    /// No jump at either end (`jump-none`): the first and last steps each
    /// hold for one interval.
    None,
    /// Jumps at both ends (`jump-both`).
    Both,
}

/// A staircase curve: `count` equal intervals, each holding one value
/// (CSS Easing 1 §2.3.1 `steps()`).
///
/// For `t` strictly inside `(0, 1)` the output is `step / jumps`, where
/// `step = ⌊t · count⌋` (plus one for [`JumpAt::Start`] and
/// [`JumpAt::Both`]) capped at `jumps`, and `jumps` is `count` for
/// [`JumpAt::Start`]/[`JumpAt::End`], `count − 1` for [`JumpAt::None`] and
/// `count + 1` for [`JumpAt::Both`]. Each interval is closed on the left:
/// the jump belongs to the later step.
///
/// The ends follow the [`Curve`] contract — `0 → 0`, `1 → 1` — which is the
/// CSS value with the before flag set at 0. FLUI has no before/after phases,
/// so the before flag is not modelled. Monotone (non-decreasing).
///
/// # Examples
///
/// ```
/// use flui_animation::{Curve, JumpAt, Steps};
///
/// const FOUR: Steps = Steps::new(4, JumpAt::End);
/// assert_eq!(FOUR.transform(0.24), 0.0);
/// assert_eq!(FOUR.transform(0.25), 0.25);
/// assert_eq!(Steps::new(4, JumpAt::Start).transform(0.1), 0.25);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Steps {
    count: u32,
    jump: JumpAt,
}

impl Steps {
    /// Admits `count ≥ 1`, and `count ≥ 2` for [`JumpAt::None`].
    const fn validate(count: u32, jump: JumpAt) -> Result<Self, CurveError> {
        if count == 0 {
            return Err(CurveError::OutOfRange {
                parameter: "count",
                value: 0.0,
                allowed: "[1, 4294967295]",
            });
        }
        if count < 2 && matches!(jump, JumpAt::None) {
            return Err(CurveError::TooFewSteps);
        }
        Ok(Self { count, jump })
    }

    /// Creates a `steps(count, jump)` curve.
    ///
    /// # Panics
    ///
    /// Panics when [`Steps::try_new`] would return an error. In a `const`
    /// item the panic is a compile error.
    #[must_use]
    pub const fn new(count: u32, jump: JumpAt) -> Self {
        match Self::validate(count, jump) {
            Ok(steps) => steps,
            Err(_) => panic!(
                "Steps::new: count must be at least 1, and at least 2 for JumpAt::None \
                 (Steps::try_new reports which)"
            ),
        }
    }

    /// Creates a `steps(count, jump)` curve, rejecting an invalid count.
    ///
    /// # Errors
    ///
    /// - [`CurveError::OutOfRange`] when `count` is 0;
    /// - [`CurveError::TooFewSteps`] when `jump` is [`JumpAt::None`] and
    ///   `count` is 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, JumpAt, Steps};
    ///
    /// assert!(Steps::try_new(3, JumpAt::Both).is_ok());
    /// assert_eq!(Steps::try_new(1, JumpAt::None), Err(CurveError::TooFewSteps));
    /// ```
    pub fn try_new(count: u32, jump: JumpAt) -> Result<Self, CurveError> {
        Self::validate(count, jump)
    }
}

impl Curve for Steps {
    builtin_value!(Steps(self));

    /// 0 wherever it is defined: the curve is flat between jumps, and a jump
    /// has no finite derivative, so a neighbouring keyframe segment inherits
    /// no velocity from it. NaN gives NaN, as for every curve.
    fn slope(&self, t: f64) -> f64 {
        settled_slope(t).unwrap_or(0.0)
    }

    fn transform(&self, t: f64) -> f64 {
        if let Some(end) = settled(t) {
            return end;
        }
        let count = f64::from(self.count);
        let jumps = match self.jump {
            JumpAt::Start | JumpAt::End => count,
            JumpAt::None => count - 1.0,
            JumpAt::Both => count + 1.0,
        };
        let mut step = (t * count).floor();
        if matches!(self.jump, JumpAt::Start | JumpAt::Both) {
            step += 1.0;
        }
        step.min(jumps) / jumps
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

    fn slope(&self, t: f64) -> f64 {
        // d/dt [1 − c(1 − t)] = c'(1 − t); NaN and the outside-[0, 1] zero
        // carry through.
        self.curve.slope(1.0 - t)
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        Some(BuiltinCurve::new(Builtin::Flipped(self.curve.builtin()?)))
    }
}

// ============================================================================
// Predefined Curves
// ============================================================================

/// A collection of commonly used curves.
///
/// Every constant is monotone except the `Back`, `Elastic` and `Bounce`
/// families, which overshoot `[0, 1]` (`Bounce` stays inside it but turns
/// back).
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

    /// Starts at full speed and decelerates to rest: `cubic-bezier(0, 0, 0.2, 1)`,
    /// the Material "linear out, slow in" curve (the name reads the other way).
    pub const SlowOutFastIn: Cubic = Cubic::new(0.0, 0.0, 0.2, 1.0);

    /// Starts slowly, speeds up through the middle and ends slowly (Penner
    /// `easeInOutCubic`).
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

    /// Dips below 0 first (anticipation), then accelerates into 1 (Penner
    /// `easeInBack`). Not monotone.
    pub const EaseInBack: Cubic = Cubic::new(0.6, -0.28, 0.735, 0.045);

    /// Starts fast, overshoots past 1 near the end and settles back (Penner
    /// `easeOutBack`). Not monotone.
    pub const EaseOutBack: Cubic = Cubic::new(0.175, 0.885, 0.32, 1.275);

    /// Dips below 0 at the start and overshoots past 1 at the end. Not
    /// monotone.
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
/// Wraps any `impl Curve + Send + Sync + 'static` so that a single, stable
/// concrete type can be stored in widgets and animation controllers,
/// regardless of which specific curve is used.
///
/// `ArcCurve` implements `Curve + Clone + Send + Sync + Debug`, which satisfies
/// the full bound that [`CurvedAnimation`] places on its `C` type parameter.
///
/// # Equality
///
/// A curve this crate defines compares **by value**: two `ArcCurve`s wrapping
/// equal [`Cubic`]s, [`ThreePointCubic`]s, elastic, bounce or linear curves,
/// or an [`Interval`] or [`FlippedCurve`] of those, are equal however they
/// were built. A parent that rebuilds every frame with
/// `.curve(Curves::EaseInOut)` therefore hands an implicit animation an
/// *unchanged* curve, and the animation keeps its run. Any other curve —
/// yours, a [`Split`], or a combinator over one — compares by identity
/// (`Arc::ptr_eq`), because [`Curve`] has no equality of its own: reuse the
/// same handle (clone it) to keep it equal across rebuilds.
///
/// # Examples
///
/// ```
/// use flui_animation::curve::{ArcCurve, Curve, Curves, ElasticOutCurve};
///
/// let curve = ArcCurve::new(ElasticOutCurve::default());
/// assert_eq!(curve.transform(0.0), 0.0);
/// assert_eq!(curve.transform(1.0), 1.0);
///
/// // Built-in curves compare by value.
/// assert_eq!(ArcCurve::new(Curves::EaseIn), ArcCurve::new(Curves::EaseIn));
/// assert_ne!(ArcCurve::new(Curves::EaseIn), ArcCurve::new(Curves::EaseOut));
/// ```
///
/// [`CurvedAnimation`]: crate::CurvedAnimation
#[derive(Clone)]
pub struct ArcCurve(Erased);

/// What an [`ArcCurve`] holds: a built-in curve's value, or an opaque curve.
#[derive(Clone)]
enum Erased {
    Builtin(BuiltinCurve),
    Custom(Arc<dyn Curve + Send + Sync>),
}

impl PartialEq for ArcCurve {
    /// Value equality for built-in curves, identity otherwise — see the type
    /// doc.
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Erased::Builtin(a), Erased::Builtin(b)) => a == b,
            (Erased::Custom(a), Erased::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// Built-in curve parameters are validated finite, so value equality is
// reflexive.
impl Eq for ArcCurve {}

impl ArcCurve {
    /// Wrap `curve` in a reference-counted erased handle.
    ///
    /// A built-in curve is stored by value (and compares by value); any
    /// other curve is moved behind an `Arc`. Cloning is a reference-count
    /// bump either way.
    pub fn new(curve: impl Curve + Send + Sync + 'static) -> Self {
        Self(match curve.builtin() {
            Some(builtin) => Erased::Builtin(builtin),
            None => Erased::Custom(Arc::new(curve)),
        })
    }
}

impl Curve for ArcCurve {
    fn transform(&self, t: f64) -> f64 {
        match &self.0 {
            Erased::Builtin(curve) => curve.transform(t),
            Erased::Custom(curve) => curve.transform(t),
        }
    }

    fn slope(&self, t: f64) -> f64 {
        match &self.0 {
            Erased::Builtin(curve) => curve.slope(t),
            Erased::Custom(curve) => curve.slope(t),
        }
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        match &self.0 {
            Erased::Builtin(curve) => Some(curve.clone()),
            Erased::Custom(_) => None,
        }
    }
}

impl fmt::Debug for ArcCurve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Erased::Builtin(curve) => f.debug_tuple("ArcCurve").field(curve).finish(),
            // `Curve` does not require `Debug`.
            Erased::Custom(_) => f.debug_struct("ArcCurve").finish_non_exhaustive(),
        }
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

    fn slope(&self, t: f64) -> f64 {
        (**self).slope(t)
    }

    fn builtin(&self) -> Option<BuiltinCurve> {
        (**self).builtin()
    }
}
