//! Retargeting: a new motion that starts from the value and velocity of the
//! old one.
//!
//! [`MotionSpec`] says how a value moves toward a target. A *segment* is one
//! stretch of that motion, from the instant it was (re)targeted to the next
//! retarget or rest. Every segment starts from the position `x0` and velocity
//! `v0` the previous segment had at the seam, so the motion is continuous in
//! value and velocity (C⁰ and C¹) however often it is interrupted.
//!
//! - A **spring** segment is the damped spring from `(x0, v0)` toward the
//!   target until its physical rest threshold. A cubic Hermite transition
//!   then carries that position and velocity to the exact target at rest.
//!   Its duration is at most one inverse natural frequency, shortened when
//!   needed to keep the inherited displacement within the position tolerance.
//!   No sample snaps: both joins are C¹, and the finite completion time is
//!   independent of the frame that observes it.
//! - A **curve** segment follows `x0 + Δ·c(τ)` over the duration, `τ = t / D`,
//!   bent by two Hermite terms, `r·D·τ(1 − τ)²` with `r = v0 − Δ·c'(0) / D`
//!   and `a·τ²(1 − τ)` with `a = Δ·c'(1)`. Both are zero at both ends; the
//!   first has derivative `r` at the start and zero at the end, the second
//!   zero at the start and `−a/D` at the end. So the segment starts with the
//!   inherited velocity, arrives exactly at `D` with zero velocity (the rest
//!   that follows is reached C¹ even for a curve such as linear whose own
//!   terminal slope is not zero), and strays from the plain curve by at most
//!   `(|r|·D + |a|)·4/27`. With no inherited excess velocity and a curve that
//!   ends flat it is the plain curve. A sample whose exact value is not
//!   representable reads as the target (value) or zero (velocity).

use std::time::Duration;

use crate::curve::{Curve, SharedCurve};
use crate::simulation::{
    Simulation, SimulationError, SimulationParameter, SpringDescription, SpringSimulation,
    Tolerance,
};

/// How a value moves toward a new target.
///
/// Both modes keep the value and the velocity continuous when the target
/// changes mid-flight. Equality is by value: two specs are equal when their
/// durations and curves (by [`SharedCurve`]'s equality) or springs are.
///
/// # Examples
///
/// ```
/// use flui_animation::{SharedCurve, curves, MotionSpec, SpringDescription};
/// use std::time::Duration;
///
/// let eased = MotionSpec::Curve {
///     duration: Duration::from_millis(200),
///     curve: SharedCurve::new(curves::EASE_IN_OUT),
/// };
/// let sprung = MotionSpec::Spring(
///     SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).unwrap(),
/// );
/// assert_ne!(eased, sprung);
/// ```
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum MotionSpec {
    /// Over `duration` along `curve`. A segment that starts with a velocity
    /// the curve does not have bends by a term that vanishes at both ends, so
    /// it still arrives at the target exactly after `duration`. Reversing
    /// toward where the previous segment started shortens the duration by
    /// the eased fraction already travelled (CSS Transitions, "Faster
    /// reversing of interrupted transitions"), but not below one
    /// millisecond. Every segment arrives at rest, with zero velocity.
    Curve {
        /// How long a full segment takes. Below one millisecond a segment
        /// jumps to its target.
        duration: Duration,
        /// The easing of the segment's progress.
        curve: SharedCurve,
    },
    /// A damped spring. A retarget keeps the inherited velocity.
    Spring(SpringDescription),
}

impl MotionSpec {
    /// The duration of [`MotionSpec::default`].
    pub const DEFAULT_DURATION: Duration = Duration::from_millis(200);

    /// Curve motion over `duration`, keeping this spec's easing. A spring has
    /// none to keep, so it takes the default easing.
    ///
    /// ```
    /// use flui_animation::{SharedCurve, MotionSpec, curves};
    /// use std::time::Duration;
    ///
    /// let spec = MotionSpec::default()
    ///     .with_curve(curves::LINEAR)
    ///     .with_duration(Duration::from_millis(500));
    /// assert_eq!(
    ///     spec,
    ///     MotionSpec::Curve {
    ///         duration: Duration::from_millis(500),
    ///         curve: SharedCurve::new(curves::LINEAR),
    ///     }
    /// );
    /// ```
    #[must_use]
    pub fn with_duration(self, duration: Duration) -> Self {
        let curve = match self {
            Self::Curve { curve, .. } => curve,
            Self::Spring(_) => SharedCurve::new(crate::curves::EASE_IN_OUT),
        };
        Self::Curve { duration, curve }
    }

    /// Curve motion eased by `curve`, keeping this spec's duration. A spring
    /// has none to keep, so it takes [`DEFAULT_DURATION`](Self::DEFAULT_DURATION).
    #[must_use]
    pub fn with_curve(self, curve: impl Curve + 'static) -> Self {
        let duration = match self {
            Self::Curve { duration, .. } => duration,
            Self::Spring(_) => Self::DEFAULT_DURATION,
        };
        Self::Curve {
            duration,
            curve: SharedCurve::new(curve),
        }
    }
}

/// 200 ms ease-in-out: the motion an implicit animation uses until told
/// otherwise.
impl Default for MotionSpec {
    fn default() -> Self {
        Self::Curve {
            duration: Self::DEFAULT_DURATION,
            curve: SharedCurve::new(crate::curves::EASE_IN_OUT),
        }
    }
}

/// Below this configured duration a curve segment jumps straight to its
/// target, and a reversal never shortens a segment below it: the Hermite
/// term's `r = v0 − Δ·c'(0)/D` would grow without bound as `D → 0`.
const MIN_CURVE_SECONDS: f64 = 1e-3;

/// What a curve segment remembers so the next one can reverse faster (CSS
/// Transitions, "Faster reversing of interrupted transitions"): the
/// reversing-adjusted start a reversal aims at, and the factor that scaled
/// this segment's configured duration. Scalar and vector owners share it, so
/// both apply `S' = |c(τ)·S + (1 − S)|`, `D' = D·S'` identically.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reversal<V> {
    adjusted_start: V,
    shortening: f64,
}

impl<V: Copy + PartialEq> Reversal<V> {
    /// A segment from `start` that does not reverse an earlier one.
    pub(crate) const fn from_start(start: V) -> Self {
        Self {
            adjusted_start: start,
            shortening: 1.0,
        }
    }

    /// The factor that scales the configured curve duration, in `[0, 1]`.
    pub(crate) const fn shortening(&self) -> f64 {
        self.shortening
    }

    /// The reversal of a segment toward `goal`, moving as `motion` says, that
    /// interrupts this one at the seam value `seam`. Only a curve segment whose
    /// goal equals the reversing-adjusted start shortens; `progress` then
    /// yields the interrupted segment's target and the eased fraction of its
    /// curve it had travelled, or `None` if it had no curve to reverse.
    /// Otherwise the segment starts afresh from the seam, and `progress`,
    /// which may call a user curve, is not called.
    ///
    /// The recorded factor is the one the segment actually runs with, after the
    /// one-millisecond floor, so a later reversal shortens the motion that ran.
    pub(crate) fn interrupt(
        self,
        seam: V,
        goal: V,
        motion: &MotionSpec,
        progress: impl FnOnce() -> Option<(V, f64)>,
    ) -> Self {
        let MotionSpec::Curve { duration, .. } = motion else {
            return Self::from_start(seam);
        };
        if goal != self.adjusted_start {
            return Self::from_start(seam);
        }
        let Some((target, eased)) = progress() else {
            return Self::from_start(seam);
        };
        let old = self.shortening;
        let requested = (eased * old + (1.0 - old)).abs().clamp(0.0, 1.0);
        Self {
            adjusted_start: target,
            shortening: effective_shortening(requested, duration.as_secs_f64()),
        }
    }
}

/// The factor a curve segment of `full` configured seconds actually runs with:
/// [`Segment::start`] ignores a non-finite factor and never runs a shortened
/// segment below [`MIN_CURVE_SECONDS`].
fn effective_shortening(requested: f64, full: f64) -> f64 {
    if !requested.is_finite() {
        1.0
    } else if full >= MIN_CURVE_SECONDS {
        requested.max(MIN_CURVE_SECONDS / full)
    } else {
        requested
    }
}

/// The eased fraction of a `duration`-second curve segment travelled after
/// `elapsed` seconds, or `None` for a segment with no duration. Calls the
/// user's curve, so owners call it outside their state borrows.
pub(crate) fn eased_progress(curve: &SharedCurve, elapsed: f64, duration: f64) -> Option<f64> {
    (duration > 0.0).then(|| curve.transform((elapsed / duration).clamp(0.0, 1.0)))
}

/// One component's motion from a seam to its target.
///
/// Time `t` is in seconds since the seam; negative or NaN `t` reads as the
/// seam itself.
#[derive(Clone, Debug)]
pub(crate) enum Segment {
    /// At rest at a value.
    Rest(f64),
    /// A spring from the seam's value and velocity.
    ///
    /// Native spring motion followed by a continuous transition to exact rest.
    /// The seam reads `x0` and `v0` exactly, and is never done.
    Spring {
        simulation: SpringSimulation,
        x0: f64,
        v0: f64,
        rest: SpringRest,
    },
    /// A curve with a Hermite velocity correction.
    Curve(CurveSegment),
}

impl Simulation for Segment {
    fn x(&self, time: f64) -> f64 {
        Self::x(self, time)
    }

    fn dx(&self, time: f64) -> f64 {
        Self::dx(self, time)
    }

    fn is_done(&self, time: f64) -> bool {
        Self::is_done(self, time)
    }
}

/// A curve segment:
/// `from + span·c(τ) + excess·duration·τ(1 − τ)² + arrival·τ²(1 − τ)`,
/// exactly `to` from `duration` on.
#[derive(Clone, Debug)]
pub(crate) struct CurveSegment {
    from: f64,
    to: f64,
    span: f64,
    /// Seconds; at least [`MIN_CURVE_SECONDS`].
    duration: f64,
    /// Kept independently so a much larger natural rate cannot round it away.
    v0: f64,
    start_slope: f64,
    end_slope: f64,
    curve: SharedCurve,
}

/// The small remaining spring displacement, retired without a final snap.
#[derive(Clone, Debug)]
pub(crate) struct SpringRest {
    start: f64,
    duration: f64,
    end: f64,
    from: f64,
    target: f64,
    velocity: f64,
}

impl SpringRest {
    fn try_new(
        simulation: &SpringSimulation,
        x0: f64,
        v0: f64,
        target: f64,
    ) -> Result<Self, SimulationError> {
        let (start, duration) = simulation.rest_transition();
        Self::at(simulation, x0, v0, target, start, duration)
    }

    fn at(
        simulation: &SpringSimulation,
        x0: f64,
        v0: f64,
        target: f64,
        start: f64,
        duration: f64,
    ) -> Result<Self, SimulationError> {
        let (from, velocity) = if start == 0.0 {
            (x0, v0)
        } else {
            simulation.analytic_sample(start)
        };
        let end = start + duration;
        if !end.is_finite()
            || end <= start
            || !(velocity * duration).is_finite()
            || !(from - target).is_finite()
        {
            return Err(SimulationError::Overflow);
        }
        Ok(Self {
            start,
            duration,
            end,
            from,
            target,
            velocity,
        })
    }

    fn x(&self, t: f64) -> f64 {
        if t <= self.start {
            return self.from;
        }
        if t >= self.end {
            return self.target;
        }
        let tau = (t - self.start) / self.duration;
        let u = 1.0 - tau;
        // Factoring the remaining displacement keeps both endpoint weights
        // exact and avoids subtraction of nearly equal cubic polynomials.
        finite_or(
            self.target
                + (self.from - self.target) * u * u * (1.0 + 2.0 * tau)
                + self.velocity * self.duration * tau * u * u,
            self.target,
        )
    }

    fn dx(&self, t: f64) -> f64 {
        if t <= self.start {
            return self.velocity;
        }
        if t >= self.end {
            return 0.0;
        }
        let tau = (t - self.start) / self.duration;
        let u = 1.0 - tau;
        finite_or(
            rate(self.target - self.from, 6.0 * tau * u, self.duration)
                + self.velocity * u * (1.0 - 3.0 * tau),
            0.0,
        )
    }
}

impl Segment {
    pub(crate) fn matches_spring(&self, target: f64, description: SpringDescription) -> bool {
        matches!(self, Self::Spring { simulation, rest, .. }
            if rest.target == target && simulation.matches_description(description))
    }

    pub(crate) fn rest_start(&self) -> Option<f64> {
        match self {
            Self::Spring { rest, .. } => Some(rest.start),
            _ => None,
        }
    }

    pub(crate) fn rest_duration_at(&self, start: f64) -> Option<f64> {
        match self {
            Self::Spring { simulation, .. } => Some(simulation.rest_duration_at(start)),
            _ => None,
        }
    }

    pub(crate) fn synchronize_rest(
        &mut self,
        start: f64,
        duration: f64,
    ) -> Result<(), SimulationError> {
        if let Self::Spring {
            simulation,
            x0,
            v0,
            rest,
        } = self
        {
            *rest = SpringRest::at(simulation, *x0, *v0, rest.target, start, duration)?;
        }
        Ok(())
    }

    /// A curve segment's target and the eased fraction it had travelled after
    /// `t` seconds; `None` for rest and springs, which a reversal cannot shorten.
    pub(crate) fn eased_progress(&self, t: f64) -> Option<(f64, f64)> {
        match self {
            Self::Curve(curve) => eased_progress(&curve.curve, seam_time(t), curve.duration)
                .map(|eased| (curve.to, eased)),
            Self::Rest(_) | Self::Spring { .. } => None,
        }
    }

    pub(crate) fn curve_duration(&self) -> Option<Duration> {
        match self {
            Self::Rest(_) => Some(Duration::ZERO),
            Self::Curve(curve) => {
                Some(Duration::try_from_secs_f64(curve.duration).unwrap_or(Duration::MAX))
            }
            Self::Spring { .. } => None,
        }
    }

    /// The segment that starts at `x0` with velocity `v0` and moves to
    /// `target` as `motion` says.
    ///
    /// `shortening` (in `(0, 1]`, clamped) scales a curve's duration; it is 1
    /// except for a reversal. A non-finite `x0` or `target`, or a span that
    /// overflows, is refused; a non-finite `v0` counts as zero.
    pub(crate) fn start(
        x0: f64,
        v0: f64,
        target: f64,
        motion: &MotionSpec,
        shortening: f64,
        tolerance: Tolerance,
    ) -> Result<Self, SimulationError> {
        for position in [x0, target] {
            if !position.is_finite() {
                return Err(SimulationError::NonFinite {
                    parameter: SimulationParameter::Position,
                });
            }
        }
        let span = target - x0;
        if !span.is_finite() {
            return Err(SimulationError::Overflow);
        }
        let v0 = if v0.is_finite() { v0 } else { 0.0 };
        if span == 0.0 && v0 == 0.0 {
            return Ok(Self::Rest(target));
        }
        Ok(match motion {
            MotionSpec::Spring(spring) => {
                let simulation = SpringSimulation::try_new(*spring, x0, target, v0, tolerance)?;
                let rest = SpringRest::try_new(&simulation, x0, v0, target)?;
                Self::Spring {
                    simulation,
                    x0,
                    v0,
                    rest,
                }
            }
            MotionSpec::Curve { duration, curve } => {
                let scale = if shortening.is_finite() {
                    shortening.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let full = duration.as_secs_f64();
                if full < MIN_CURVE_SECONDS {
                    return Ok(Self::Rest(target));
                }
                // A reversal shortens, but never below the floor: the seam's
                // value and velocity are kept however early it reverses.
                let duration = (full * scale).max(MIN_CURVE_SECONDS);
                let start_slope = curve.slope(0.0);
                let end_slope = curve.slope(1.0);
                if !(start_slope.is_finite() && end_slope.is_finite()) {
                    return Err(SimulationError::Overflow);
                }
                Self::Curve(CurveSegment {
                    from: x0,
                    to: target,
                    span,
                    duration,
                    v0,
                    start_slope,
                    end_slope,
                    curve: curve.clone(),
                })
            }
        })
    }

    /// The value `t` seconds after the seam.
    pub(crate) fn x(&self, t: f64) -> f64 {
        let t = seam_time(t);
        match self {
            Self::Rest(value) => *value,
            Self::Spring { x0, .. } if t == 0.0 => *x0,
            // A non-finite sample means the phase or the polynomial term
            // overflowed long after the exponential decayed to zero: the
            // spring's limit, its target.
            Self::Spring { rest, .. } if t >= rest.start => rest.x(t),
            Self::Spring { simulation, .. } => {
                finite_or(simulation.analytic_sample(t).0, simulation.x(f64::INFINITY))
            }
            Self::Curve(curve) => curve.x(t),
        }
    }

    /// The velocity `t` seconds after the seam, per second.
    pub(crate) fn dx(&self, t: f64) -> f64 {
        let t = seam_time(t);
        match self {
            Self::Rest(_) => 0.0,
            Self::Spring { v0, .. } if t == 0.0 => *v0,
            Self::Spring { rest, .. } if t >= rest.start => rest.dx(t),
            Self::Spring { simulation, .. } => finite_or(simulation.analytic_sample(t).1, 0.0),
            Self::Curve(curve) => curve.dx(t),
        }
    }

    /// Whether the segment has arrived at its exact target with zero velocity.
    pub(crate) fn is_done(&self, t: f64) -> bool {
        let t = seam_time(t);
        match self {
            Self::Rest(_) => true,
            Self::Spring { rest, .. } => t >= rest.end,
            Self::Curve(curve) => t >= curve.duration,
        }
    }
}

/// Negative or NaN time reads as the seam.
fn seam_time(t: f64) -> f64 {
    if t > 0.0 { t } else { 0.0 }
}

impl CurveSegment {
    fn x(&self, t: f64) -> f64 {
        if t == 0.0 {
            return self.from;
        }
        if t >= self.duration {
            return self.to;
        }
        let tau = t / self.duration;
        let u = 1.0 - tau;
        // Weight and combine the curve's corrections before scaling by its span.
        // An unweighted endpoint rate can overflow even though this sample fits.
        let departure_weight = tau * u * u;
        let arrival_weight = tau * tau * u;
        let progress = self.end_slope.mul_add(
            arrival_weight,
            self.curve.transform(tau) - self.start_slope * departure_weight,
        );
        let inherited = self.v0 * departure_weight * self.duration;
        finite_or(self.from + self.span * progress + inherited, self.to)
    }

    fn dx(&self, t: f64) -> f64 {
        if t == 0.0 {
            return self.v0;
        }
        if t >= self.duration {
            return 0.0;
        }
        let tau = t / self.duration;
        let departure_weight = (1.0 - tau) * (1.0 - 3.0 * tau);
        let arrival_weight = tau * (2.0 - 3.0 * tau);
        let slope = self.end_slope.mul_add(
            arrival_weight,
            self.curve.slope(tau) - self.start_slope * departure_weight,
        );
        let mut velocity = rate(self.span, slope, self.duration) + self.v0 * departure_weight;
        if !velocity.is_finite() {
            let scale = self.span.abs().max(self.v0.abs());
            velocity = (rate(self.span / scale, slope, self.duration)
                + (self.v0 / scale) * departure_weight)
                * scale;
        }
        finite_or(velocity, 0.0)
    }
}

/// `a · b / d` without intermediate overflow or avoidable underflow: tries the
/// grouping that keeps the intermediate in range for a small `b`, then the one
/// for a large `b`, then dividing `b` first. Non-finite only when the rate
/// itself is not representable.
pub(crate) fn rate(a: f64, b: f64, d: f64) -> f64 {
    let product_first = a * b / d;
    for candidate in [product_first, a / d * b, a * (b / d)] {
        if candidate.is_finite() && candidate != 0.0 {
            return candidate;
        }
    }
    // A zero may be intermediate underflow; try each association before
    // accepting it. Preserve an unrepresentable result for admission to refuse.
    product_first
}

/// [`rate`] `a · b / d` times a clock `scale`, without first overflowing or
/// underflowing the slope product or the unscaled rate. With `scale == 1`
/// this is [`rate`]; like it, the result is non-finite only when the scaled
/// rate is not representable or an input is not finite.
pub(crate) fn scaled_rate(a: f64, b: f64, d: f64, scale: f64) -> f64 {
    let direct = rate(a, b * scale, d);
    for candidate in [direct, rate(a, b, d) * scale, rate(a, scale, d) * b] {
        if candidate.is_finite() && candidate != 0.0 {
            return candidate;
        }
    }
    direct
}

/// `value` if finite, else `fallback`: a sample of finite admitted state
/// whose exact value is not representable never publishes inf or NaN.
fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}
