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
//!   target; its value and velocity are analytic for every `t`. It never
//!   snaps: *settled* means within the spring's distance tolerance of the
//!   target, and the value keeps converging continuously from there, with no
//!   final jump. As `t → ∞` the value reaches the target and the velocity
//!   zero, and both stay finite for every `t`.
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
    /// The simulation does not snap to its target: being within tolerance
    /// only makes [`is_done`](Self::is_done) true, and the published value
    /// and velocity stay the analytic spring's. The seam reads `x0` and `v0`
    /// exactly, and is never done.
    Spring {
        simulation: SpringSimulation,
        x0: f64,
        v0: f64,
    },
    /// A curve with a Hermite velocity correction.
    Curve(CurveSegment),
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
    /// The inherited velocity the curve does not account for, per second.
    excess: f64,
    /// `span·c'(1)`: the arrival velocity the curve itself would have, times
    /// the duration, which the second Hermite term cancels.
    arrival: f64,
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
                simulation: SpringSimulation::new(*spring, x0, target, v0),
                x0,
                v0,
            },
            MotionSpec::Curve { duration, curve } => {
                let scale = if shortening.is_finite() {
                    shortening.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let full = duration.as_secs_f64();
                if full < MIN_CURVE_SECONDS {
                    return Self::Rest(target);
                }
                // A reversal shortens, but never below the floor: the seam's
                // value and velocity are kept however early it reverses.
                let duration = (full * scale).max(MIN_CURVE_SECONDS);
                let excess = v0 - span * curve.slope(0.0) / duration;
                let arrival = span * curve.slope(1.0);
                if !(excess.is_finite() && arrival.is_finite()) {
                    return Self::Rest(target);
                }
                Self::Curve(CurveSegment {
                    from: x0,
                    to: target,
                    span,
                    duration,
                    excess,
                    arrival,
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
            // A non-finite sample means the phase or the polynomial term
            // overflowed long after the exponential decayed to zero: the
            // spring's limit, its target.
            Self::Spring { simulation, .. } => {
                finite_or(simulation.x(t), simulation.end_position())
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
            Self::Spring { simulation, .. } => finite_or(simulation.dx(t), 0.0),
            Self::Curve(curve) => curve.dx(t),
        }
    }

    /// Whether the segment has settled by `t`: a curve has arrived, a spring
    /// is within its distance tolerance of the target. Settling never changes
    /// [`x`](Self::x) or [`dx`](Self::dx); a spring keeps converging.
    pub(crate) fn is_done(&self, t: f64) -> bool {
        let t = seam_time(t);
        match self {
            Self::Rest(_) => true,
            Self::Spring { simulation, .. } => {
                t > 0.0
                    && (simulation.is_done(t)
                        || !(simulation.x(t).is_finite() && simulation.dx(t).is_finite()))
            }
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
        let u = 1.0 - tau;
        // `excess · τ(1 − τ)²` first: it is at most `excess`, so the product
        // with the duration overflows only when the correction itself does.
        let departure = self.excess * (tau * u * u) * self.duration;
        let arrival = self.arrival * (tau * tau * u);
        finite_or(
            self.from + self.span * self.curve.transform(tau) + departure + arrival,
            self.to,
        )
    }

    fn dx(&self, t: f64) -> f64 {
        if t >= self.duration {
            return 0.0;
        }
        let tau = t / self.duration;
        let curve = self.span * self.curve.slope(tau) / self.duration;
        let departure = self.excess * (1.0 - tau) * (1.0 - 3.0 * tau);
        let arrival = self.arrival / self.duration * (tau * (2.0 - 3.0 * tau));
        finite_or(curve + departure + arrival, 0.0)
    }
}

/// `value` if finite, else `fallback`: a sample of finite admitted state
/// whose exact value is not representable never publishes inf or NaN.
fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}
