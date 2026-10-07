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
//!   target; its velocity is analytic.
//! - A **curve** segment follows `x0 + Δ·c(τ)` over the duration, `τ = t / D`,
//!   bent by a Hermite term `r·D·τ(1 − τ)²` with
//!   `r = v0 − Δ·c'(0) / D`. The term is zero at both ends and its derivative
//!   is `r` at the start and zero at the end, so the segment starts with the
//!   inherited velocity, arrives exactly at `D` with the velocity the curve
//!   itself gives there, and strays from the plain curve by at most
//!   `|r|·D·4/27`. With no inherited excess velocity it is the plain curve.

use std::time::Duration;

use crate::curve::{ArcCurve, Curve};
use crate::simulation::{Simulation, SpringDescription, SpringSimulation};

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
    /// reversing of interrupted transitions").
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

/// Below this duration a curve segment jumps straight to its target: the
/// Hermite term's `r = v0 − Δ·c'(0)/D` would grow without bound as `D → 0`.
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
    /// The simulation snaps to its target once within tolerance, which a
    /// small retarget from rest already is at the seam; the seam itself
    /// reads `x0` and `v0` and is never done, so the snap lands on the next
    /// sample like any spring's arrival instead of breaking C⁰ at the seam.
    Spring {
        simulation: SpringSimulation,
        x0: f64,
        v0: f64,
    },
    /// A curve with a Hermite velocity correction.
    Curve(CurveSegment),
}

/// A curve segment: `from + span·c(τ) + excess·duration·τ(1 − τ)²`, exactly
/// `to` from `duration` on.
#[derive(Clone, Debug)]
pub(crate) struct CurveSegment {
    from: f64,
    to: f64,
    span: f64,
    /// Seconds; at least [`MIN_CURVE_SECONDS`].
    duration: f64,
    /// The inherited velocity the curve does not account for, per second.
    excess: f64,
    curve: ArcCurve,
}

impl Segment {
    /// The segment that starts at `x0` with velocity `v0` and moves to
    /// `target` as `motion` says.
    ///
    /// `shortening` (in `(0, 1]`, clamped) scales a curve's duration; it is 1
    /// except for a reversal. A non-finite `x0` or `target`, or a span that
    /// overflows, rests at the target (or at `x0` if only the target is
    /// unusable); a non-finite `v0` counts as zero.
    pub(crate) fn start(
        x0: f64,
        v0: f64,
        target: f64,
        motion: &MotionSpec,
        shortening: f64,
    ) -> Self {
        if !target.is_finite() {
            return Self::Rest(if x0.is_finite() { x0 } else { 0.0 });
        }
        let span = target - x0;
        if !span.is_finite() {
            return Self::Rest(target);
        }
        let v0 = if v0.is_finite() { v0 } else { 0.0 };
        if span == 0.0 && v0 == 0.0 {
            return Self::Rest(target);
        }
        match motion {
            MotionSpec::Spring(spring) => Self::Spring {
                simulation: SpringSimulation::new(*spring, x0, target, v0).with_snap_to_end(true),
                x0,
                v0,
            },
            MotionSpec::Curve { duration, curve } => {
                let scale = if shortening.is_finite() {
                    shortening.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let duration = duration.as_secs_f64() * scale;
                if duration < MIN_CURVE_SECONDS {
                    return Self::Rest(target);
                }
                let excess = v0 - span * curve.slope(0.0) / duration;
                if !excess.is_finite() {
                    return Self::Rest(target);
                }
                Self::Curve(CurveSegment {
                    from: x0,
                    to: target,
                    span,
                    duration,
                    excess,
                    curve: curve.clone(),
                })
            }
        }
    }

    /// The value `t` seconds after the seam.
    pub(crate) fn x(&self, t: f64) -> f64 {
        let t = seam_time(t);
        match self {
            Self::Rest(value) => *value,
            Self::Spring { x0, .. } if t == 0.0 => *x0,
            Self::Spring { simulation, .. } => simulation.x(t),
            Self::Curve(curve) => curve.x(t),
        }
    }

    /// The velocity `t` seconds after the seam, per second.
    pub(crate) fn dx(&self, t: f64) -> f64 {
        let t = seam_time(t);
        match self {
            Self::Rest(_) => 0.0,
            Self::Spring { v0, .. } if t == 0.0 => *v0,
            Self::Spring { simulation, .. } => simulation.dx(t),
            Self::Curve(curve) => curve.dx(t),
        }
    }

    /// Whether the segment has come to rest at its target by `t`.
    pub(crate) fn is_done(&self, t: f64) -> bool {
        let t = seam_time(t);
        match self {
            Self::Rest(_) => true,
            Self::Spring { simulation, .. } => t > 0.0 && simulation.is_done(t),
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
        if t >= self.duration {
            return self.to;
        }
        let tau = t / self.duration;
        let hermite = tau * (1.0 - tau) * (1.0 - tau);
        self.from + self.span * self.curve.transform(tau) + self.excess * self.duration * hermite
    }

    fn dx(&self, t: f64) -> f64 {
        if t >= self.duration {
            return 0.0;
        }
        let tau = t / self.duration;
        let hermite_slope = (1.0 - tau) * (1.0 - 3.0 * tau);
        self.span * self.curve.slope(tau) / self.duration + self.excess * hermite_slope
    }
}
