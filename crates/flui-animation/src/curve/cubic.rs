//! Cubic Bézier easing: `Cubic` and the two-segment `ThreePointCubic`.

use std::fmt;

use super::{
    Curve, CurveError, CurveParameter, difference_slope, settled, settled_slope,
    strictly_inside_unit, within,
};
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
/// The solver follows the Newton/bisection structure of
/// [WebKit `UnitBezier`](https://github.com/WebKit/WebKit/blob/029da7d3d074a75a24c2413e98b183a8bbfcfb62/Source/WebCore/platform/graphics/UnitBezier.h)
/// with an 11-sample table, up to four Newton steps, then bisection. FLUI
/// stops on the **output**: it brackets the bezier parameter until the
/// bracket's width times the largest `|dy/ds|` is below `1e-7`, so the
/// result is within `1e-7` of the exact y(x) — including next to a vertical
/// tangent such as [`curves::EASE_IN_OUT_EXPO`](crate::curves::EASE_IN_OUT_EXPO)'s, where a solver that stops on
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
        check!(within(x1, 0.0, 1.0, CurveParameter::X1));
        check!(within(y1, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::Y1));
        check!(within(x2, 0.0, 1.0, CurveParameter::X2));
        check!(within(y2, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::Y2));
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
    /// use flui_animation::{Cubic, CurveError, CurveParameter};
    ///
    /// assert!(Cubic::try_new(0.68, -0.55, 0.265, 1.55).is_ok());
    /// assert!(matches!(
    ///     Cubic::try_new(1.5, 0.0, 0.5, 1.0),
    ///     Err(CurveError::OutOfRange { parameter: CurveParameter::X1, .. })
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
    /// The parameter of `x` found by bisection to the last representable bit;
    /// slower than [`Self::solve`] (which bounds the output, not the
    /// parameter) and used only where a derivative needs the exact point.
    fn parameter_exact(&self, x: f64) -> f64 {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        loop {
            let mid = f64::midpoint(lo, hi);
            if mid <= lo || mid >= hi {
                return mid;
            }
            if self.x.at(mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }

    fn tangent_slope(&self, x: f64) -> Option<f64> {
        let cubed = self.x.a;
        if cubed <= 0.0 {
            let s = self.parameter_exact(x);
            let quotient = self.y.slope(s) / self.x.slope(s);
            return quotient.is_finite().then_some(quotient);
        }
        // The expansion `a·u³ + x'(c)·u` holds only about a genuine stationary
        // point of `x'`: outside [0, 1] the `u²` term does not vanish.
        let centre = -self.x.b / (3.0 * cubed);
        if !(0.0..=1.0).contains(&centre) {
            // No stationary point inside: `x` is strictly monotone with a
            // nonzero quadratic part, so re-solve the parameter to full
            // precision and take the exact quotient there.
            let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
            loop {
                let mid = f64::midpoint(lo, hi);
                if mid <= lo || mid >= hi {
                    break;
                }
                if self.x.at(mid) < x {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let s = f64::midpoint(lo, hi);
            let quotient = self.y.slope(s) / self.x.slope(s);
            return quotient.is_finite().then_some(quotient);
        }
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
    /// point of `x'` instead (`Cubic::tangent_slope`). Where `x'(s) = 0`
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
        let reach = OUTPUT_TOLERANCE / self.y_slope_bound;
        let x_error = self.x.curvature(s).abs() * reach + 3.0 * self.x.a.abs() * reach * reach;
        let near_tangent = interior && self.x.slope(s).abs() <= x_error;
        // Away from a stationary point of `x'` the quotient needs the exact
        // parameter: the solver bounds the output, not `s`, and near an end a
        // small genuine `x'` is sensitive to it.
        let s = if interior && !near_tangent {
            self.parameter_exact(t)
        } else {
            s
        };
        let dx = self.x.slope(s);
        let dy = self.y.slope(s);
        let ratio = if near_tangent {
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
        check!(strictly_inside_unit(mx, CurveParameter::MidpointX));
        check!(strictly_inside_unit(my, CurveParameter::MidpointY));
        check!(within(a1.0, 0.0, mx, CurveParameter::A1X));
        check!(within(a1.1, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::A1Y));
        check!(within(b1.0, 0.0, mx, CurveParameter::B1X));
        check!(within(b1.1, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::B1Y));
        check!(within(a2.0, mx, 1.0, CurveParameter::A2X));
        check!(within(a2.1, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::A2Y));
        check!(within(b2.0, mx, 1.0, CurveParameter::B2X));
        check!(within(b2.1, -MAX_CUBIC_Y, MAX_CUBIC_Y, CurveParameter::B2Y));
        // Rescaling keeps x inside [0, 1] (IEEE division and subtraction are
        // monotone). With every y within MAX_CUBIC_Y, a rescaled y reaches
        // MAX_SEGMENT_Y only for a midpoint.y within about 1e-294 of 0 or 1.
        let (wide, high) = (1.0 - mx, 1.0 - my);
        let first = (a1.1 / my, b1.1 / my);
        let second = ((a2.1 - my) / high, (b2.1 - my) / high);
        if !(segment_y(first.0) && segment_y(first.1) && segment_y(second.0) && segment_y(second.1))
        {
            return Err(CurveError::RescalingOutOfRange { midpoint_y: my });
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
    ///   `[midpoint.x, 1]`, or when a control y lies outside `[-1e6, 1e6]`;
    /// - [`CurveError::RescalingOutOfRange`] when admitted finite controls cannot
    ///   be normalized inside the segment solver's finite arithmetic range, with
    ///   normalized control outputs larger than `1e300`, for example with a
    ///   subnormal `midpoint.y` and a nonzero first-segment control output.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{CurveError, CurveParameter, ThreePointCubic};
    ///
    /// let on_edge =
    ///     ThreePointCubic::try_new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0));
    /// assert!(matches!(
    ///     on_edge,
    ///     Err(CurveError::OutOfRange { parameter: CurveParameter::MidpointX, .. })
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
