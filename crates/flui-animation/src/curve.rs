//! Animation curves for interpolation.

use std::fmt;

mod piecewise;
mod spring;
pub use piecewise::{LinearStop, PiecewiseLinear, PiecewiseLinearError};
pub use spring::SpringCurve;

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
///   curve such as [`curves::EASE_OUT_BACK`](crate::curves::EASE_OUT_BACK) or [`Elastic`]).
/// - A finite `t` outside `[0, 1]`, and `±∞`, is clamped: the result is the
///   value at the nearest end. Curves do not extrapolate.
/// - `transform(f64::NAN)` returns NaN. A curve never disguises a broken
///   input as valid progress; the caller decides what a non-finite sample
///   means.
/// - Monotonicity belongs to the individual curve and is stated in its
///   documentation.
///
/// Combinators keep the policy: [`Flipped`] preserves both the
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
///         let t = t.clamp(0.0, 1.0);
///         t * t // quadratic ease-in
///     }
/// }
///
/// let curve = MyCurve;
/// assert_eq!(curve.transform(0.0), 0.0);
/// assert_eq!(curve.transform(0.5), 0.25);
/// assert_eq!(curve.transform(1.0), 1.0);
/// assert_eq!(curve.transform(-0.5), 0.0);
/// assert_eq!(curve.transform(f64::INFINITY), 1.0);
/// assert!(curve.transform(f64::NAN).is_nan());
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
    fn flipped(self) -> Flipped<Self>
    where
        Self: Sized,
    {
        Flipped { curve: self }
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
    /// [`Linear`], [`Cubic`], [`Interval`], [`Flipped`] and
    /// [`SharedCurve`] override it exactly.
    ///
    /// Keyframe tracks read it to match a cubic segment's velocity to a
    /// neighbouring curved segment.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{Curve, curves};
    ///
    /// assert_eq!(curves::LINEAR.slope(0.3), 1.0);
    /// assert_eq!(curves::EASE_IN.slope(2.0), 0.0);
    /// ```
    fn slope(&self, t: f64) -> f64 {
        difference_slope(|t| self.transform(t), t)
    }

    /// The value of a curve this crate defines, which lets [`SharedCurve`]
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

/// The closed set of curves [`SharedCurve`] compares by value.
///
/// A new built-in curve family adds a variant here, with its arms in
/// `transform` and `slope`, and implements `Curve::builtin` with
/// `builtin_value!`; otherwise two equal values wrapped separately compare
/// unequal and an implicit animation rebuilt each frame restarts its run.
mod builtin {
    use super::interval::{interval_slope, interval_transform};
    use super::split::split_transform;
    use super::{Bounce, Cubic, Curve, Decelerate, Elastic, Steps, ThreePointCubic};
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
                Builtin::Decelerate => Decelerate.transform(t),
                Builtin::Bounce(curve) => curve.transform(t),
                Builtin::Cubic(curve) => curve.transform(t),
                Builtin::ThreePointCubic(curve) => curve.transform(t),
                Builtin::Elastic(curve) => curve.transform(t),
                Builtin::Steps(curve) => curve.transform(t),
                Builtin::PiecewiseLinear(curve) => curve.transform(t),
                Builtin::Spring(curve) => curve.transform(t),
                Builtin::Interval { begin, end, curve } => {
                    interval_transform(*begin, *end, t, |local| curve.transform(local))
                }
                Builtin::Flipped(curve) => 1.0 - curve.transform(1.0 - t),
                Builtin::Split { split, begin, end } => split_transform(
                    *split,
                    t,
                    |local| begin.transform(local),
                    |local| end.transform(local),
                ),
            }
        }

        pub(super) fn slope(&self, t: f64) -> f64 {
            match &*self.0 {
                Builtin::Linear => super::Linear.slope(t),
                Builtin::Decelerate => Decelerate.slope(t),
                Builtin::Bounce(curve) => curve.slope(t),
                Builtin::Cubic(curve) => curve.slope(t),
                Builtin::ThreePointCubic(curve) => curve.slope(t),
                Builtin::Elastic(curve) => curve.slope(t),
                Builtin::Steps(curve) => curve.slope(t),
                Builtin::PiecewiseLinear(curve) => curve.slope(t),
                Builtin::Spring(curve) => curve.slope(t),
                Builtin::Interval { begin, end, curve } => interval_slope(
                    *begin,
                    *end,
                    t,
                    |local| curve.slope(local),
                    |local| curve.transform(local),
                ),
                Builtin::Flipped(curve) => curve.slope(1.0 - t),
                // The same derivative `Split` reports: the trait default.
                Builtin::Split { .. } => super::difference_slope(|t| self.transform(t), t),
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
        Bounce(Bounce),
        Cubic(Cubic),
        ThreePointCubic(ThreePointCubic),
        Elastic(Elastic),
        Steps(Steps),
        PiecewiseLinear(super::PiecewiseLinear),
        Spring(super::SpringCurve),
        Interval {
            begin: f64,
            end: f64,
            curve: BuiltinCurve,
        },
        Flipped(BuiltinCurve),
        Split {
            split: f64,
            begin: BuiltinCurve,
            end: BuiltinCurve,
        },
    }
}

use builtin::{Builtin, BuiltinCurve};

/// Implements [`Curve::builtin`] for a curve with no parameters or a
/// `Copy` value.
macro_rules! builtin_value {
    ($variant:ident) => {
        fn builtin(&self) -> Option<$crate::curve::BuiltinCurve> {
            Some($crate::curve::BuiltinCurve::new(
                $crate::curve::Builtin::$variant,
            ))
        }
    };
    ($variant:ident(self)) => {
        fn builtin(&self) -> Option<$crate::curve::BuiltinCurve> {
            Some($crate::curve::BuiltinCurve::new(
                $crate::curve::Builtin::$variant(*self),
            ))
        }
    };
}

/// A checked curve's constructor argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CurveParameter {
    /// First cubic control point's input.
    X1,
    /// First cubic control point's output.
    Y1,
    /// Second cubic control point's input.
    X2,
    /// Second cubic control point's output.
    Y2,
    /// Three-point curve midpoint's input.
    MidpointX,
    /// Three-point curve midpoint's output.
    MidpointY,
    /// First segment's first control input.
    A1X,
    /// First segment's first control output.
    A1Y,
    /// First segment's second control input.
    B1X,
    /// First segment's second control output.
    B1Y,
    /// Second segment's first control input.
    A2X,
    /// Second segment's first control output.
    A2Y,
    /// Second segment's second control input.
    B2X,
    /// Second segment's second control output.
    B2Y,
    /// Interval start.
    Begin,
    /// Interval end.
    End,
    /// Elastic period.
    Period,
    /// Number of step intervals.
    StepCount,
    /// Progress separating two curves.
    Split,
}

impl CurveParameter {
    const fn allowed(self) -> &'static str {
        match self {
            Self::X1 | Self::X2 | Self::Begin | Self::End | Self::Split => "[0, 1]",
            Self::MidpointX | Self::MidpointY => "(0, 1)",
            Self::Y1 | Self::Y2 | Self::A1Y | Self::B1Y | Self::A2Y | Self::B2Y => "[-1e6, 1e6]",
            Self::A1X | Self::B1X => "[0, midpoint.x]",
            Self::A2X | Self::B2X => "[midpoint.x, 1]",
            Self::Period => "[1e-6, 1e6]",
            Self::StepCount => "[1, 4294967295]",
        }
    }
}

impl fmt::Display for CurveParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::X1 => "x1",
            Self::Y1 => "y1",
            Self::X2 => "x2",
            Self::Y2 => "y2",
            Self::MidpointX => "midpoint.x",
            Self::MidpointY => "midpoint.y",
            Self::A1X => "a1.x",
            Self::A1Y => "a1.y",
            Self::B1X => "b1.x",
            Self::B1Y => "b1.y",
            Self::A2X => "a2.x",
            Self::A2Y => "a2.y",
            Self::B2X => "b2.x",
            Self::B2Y => "b2.y",
            Self::Begin => "begin",
            Self::End => "end",
            Self::Period => "period",
            Self::StepCount => "count",
            Self::Split => "split",
        })
    }
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
        parameter: CurveParameter,
    },
    /// The parameter is finite but outside the range the curve admits.
    #[error("curve parameter `{parameter}` = {value} is outside {}", parameter.allowed())]
    OutOfRange {
        /// The constructor argument that was rejected.
        parameter: CurveParameter,
        /// The rejected value.
        value: f64,
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
    /// Finite controls rescale beyond the segment solver's arithmetic range.
    #[error(
        "midpoint.y {midpoint_y} rescales control points beyond the segment solver's finite arithmetic range"
    )]
    RescalingOutOfRange {
        /// The admitted midpoint output whose rescaling exceeds that range.
        midpoint_y: f64,
    },
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
const fn finite(value: f64, parameter: CurveParameter) -> Result<(), CurveError> {
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
    parameter: CurveParameter,
) -> Result<(), CurveError> {
    check!(finite(value, parameter));
    if value < low || value > high {
        return Err(CurveError::OutOfRange { parameter, value });
    }
    Ok(())
}

/// `value` is finite and inside the open interval `(0, 1)`.
const fn strictly_inside_unit(value: f64, parameter: CurveParameter) -> Result<(), CurveError> {
    check!(finite(value, parameter));
    if value <= 0.0 || value >= 1.0 {
        return Err(CurveError::OutOfRange { parameter, value });
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
// Standard curves
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

// Each family lives in its own module; declared after the shared helper
// macros so their textual scope reaches every module.
mod cubic;
mod erased;
mod interval;
mod penner;
mod split;
mod steps;

pub use cubic::{Cubic, ThreePointCubic};
pub use erased::{Flipped, SharedCurve};
pub use interval::Interval;
pub use penner::{Bounce, Decelerate, Ease, Elastic};
pub use split::Split;
pub use steps::{JumpAt, Steps};
