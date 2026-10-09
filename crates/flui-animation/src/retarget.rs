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

use crate::curve::{ArcCurve, Curve};
use crate::simulation::{
    Simulation, SimulationError, SimulationParameter, SpringDescription, SpringSimulation,
    Tolerance,
};

/// How a value moves toward a new target.
///
/// Both modes keep the value and the velocity continuous when the target
/// changes mid-flight. Equality is by value: two specs are equal when their
/// durations and curves (by [`ArcCurve`]'s equality) or springs are.
///
/// # Examples
///
/// ```
/// use flui_animation::{ArcCurve, Curves, MotionSpec, SpringDescription};
/// use std::time::Duration;
///
/// let eased = MotionSpec::Curve {
///     duration: Duration::from_millis(200),
///     curve: ArcCurve::new(Curves::EaseInOut),
/// };
/// let sprung = MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0));
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
        curve: ArcCurve,
    },
    /// A damped spring. A retarget keeps the inherited velocity.
    Spring(SpringDescription),
}

/// Below this configured duration a curve segment jumps straight to its
/// target, and a reversal never shortens a segment below it: the Hermite
/// term's `r = v0 − Δ·c'(0)/D` would grow without bound as `D → 0`.
const MIN_CURVE_SECONDS: f64 = 1e-3;

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
    curve: ArcCurve,
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
    fn new(
        simulation: &SpringSimulation,
        x0: f64,
        v0: f64,
        target: f64,
    ) -> Result<Self, SimulationError> {
        let (start, duration) = simulation.rest_transition();
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
    ) -> Result<Self, SimulationError> {
        for position in [x0, target] {
            if !position.is_finite() {
                return Err(SimulationError::OutOfRange {
                    parameter: SimulationParameter::Position,
                    value: position,
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
                let simulation =
                    SpringSimulation::try_new(*spring, x0, target, v0, Tolerance::DEFAULT)?;
                let rest = SpringRest::new(&simulation, x0, v0, target)?;
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

/// `value` if finite, else `fallback`: a sample of finite admitted state
/// whose exact value is not representable never publishes inf or NaN.
/// `a · b / d` without an overflow the result does not need: tries the
/// grouping that keeps the intermediate in range for a small `b`, then the one
/// for a large `b`, then dividing `b` first. Non-finite only when the rate
/// itself is not representable.
pub(crate) fn rate(a: f64, b: f64, d: f64) -> f64 {
    let small_b = a * b / d;
    if small_b.is_finite() {
        return small_b;
    }
    let large_b = a / d * b;
    if large_b.is_finite() {
        return large_b;
    }
    a * (b / d)
}

/// Apply a clock scale without first overflowing or underflowing the slope
/// product or the unscaled rate. Invalid or unrepresentable derivatives are zero.
pub(crate) fn scaled_rate(a: f64, b: f64, d: f64, scale: f64) -> f64 {
    if !(a.is_finite() && b.is_finite() && d.is_finite() && scale.is_finite()) || d <= 0.0 {
        return 0.0;
    }
    for candidate in [
        rate(a, b * scale, d),
        rate(a, b, d) * scale,
        rate(a, scale, d) * b,
    ] {
        if candidate.is_finite() && candidate != 0.0 {
            return candidate;
        }
    }
    0.0
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}
