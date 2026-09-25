//! Friction-based physics simulations
//!
//! This module provides simulations that model friction and deceleration.

use super::{Simulation, Tolerance};

/// A friction (exponential deceleration) simulation: velocity decays as
/// `v(t) = v₀·e^(−k·t)` toward a finite stopping position.
///
/// Positions are in logical pixels, time in seconds, `k` is the decay rate
/// (see `FrictionSimulation::new` for how this differs from Flutter's drag
/// coefficient). The simulation is done once the speed drops below the
/// velocity tolerance; position converges to `final_position` but never
/// reverses direction.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrictionSimulation {
    /// Exponential decay rate k (NOT Flutter's drag coefficient cₓ; k = −ln(cₓ)).
    /// Higher = faster decay.
    decay_rate: f32,

    /// The starting position
    position_at_zero: f32,

    /// The initial velocity in pixels per second
    velocity_at_zero: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl FrictionSimulation {
    /// Creates a friction simulation using exponential-decay parameterisation.
    ///
    /// # Parameter convention — intentional divergence from Flutter
    ///
    /// Flutter's `FrictionSimulation` takes a *drag coefficient* `cₓ ∈ (0, 1)` where
    /// `v(t) = v₀ · cₓ^t` (`drag^t = e^(t·ln cₓ)`, with `ln cₓ < 0` for `cₓ < 1`).
    ///
    /// This type uses a *decay rate* `k > 0` where `v(t) = v₀ · e^(−k·t)` and
    /// `x(t) = x₀ + v₀·(1 − e^(−k·t)) / k`. The two forms are the same curve
    /// family; the conversion is `k = −ln(cₓ)`.  Concretely, Flutter's `cₓ = 0.135`
    /// corresponds to `k ≈ 2.0` here (higher `k` → stronger friction).
    ///
    /// The decay-rate form was chosen because:
    /// - It is the standard physics convention (`F = −kv`).
    /// - `k > 0` (unbounded) is less surprising than `cₓ ∈ (0, 1)`.
    /// - `final_position = x₀ + v₀/k` follows directly from the formula.
    ///
    /// Scroll physics and animations use `flui_animation::FrictionSimulation`,
    /// which follows Flutter's `cₓ` convention. This type is the value-physics
    /// layer and is not used by the scroll-physics pipeline.
    ///
    /// Reference: Flutter source
    /// `packages/flutter/lib/src/physics/friction_simulation.dart`, line 40.
    #[must_use]
    #[inline]
    pub fn new(decay_rate: f32, position: f32, velocity: f32) -> Self {
        Self {
            decay_rate,
            position_at_zero: position,
            velocity_at_zero: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Returns the simulation with its tolerance replaced (builder style).
    ///
    /// The velocity tolerance controls when `is_done` reports the motion as
    /// stopped.
    #[must_use]
    #[inline]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the exponential decay rate `k` (per second); higher values
    /// mean stronger friction.
    #[must_use]
    #[inline]
    pub fn decay_rate(&self) -> f32 {
        self.decay_rate
    }

    /// Returns the position at `t = 0`, in logical pixels.
    #[must_use]
    #[inline]
    pub fn start_position(&self) -> f32 {
        self.position_at_zero
    }

    /// Returns the velocity at `t = 0`, in logical pixels per second.
    #[must_use]
    #[inline]
    pub fn initial_velocity(&self) -> f32 {
        self.velocity_at_zero
    }

    /// Returns the position the simulation converges to as `t → ∞`:
    /// `x₀ + v₀/k`, in logical pixels.
    #[must_use]
    #[inline]
    pub fn final_position(&self) -> f32 {
        self.position_at_zero + self.velocity_at_zero / self.decay_rate
    }

    /// Returns whether the simulation is well-formed: a positive finite decay
    /// rate, finite position and velocity, and a valid tolerance.
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.decay_rate > 0.0
            && self.decay_rate.is_finite()
            && self.position_at_zero.is_finite()
            && self.velocity_at_zero.is_finite()
            && self.tolerance.is_valid()
    }

    /// Returns the time, in seconds, at which the velocity decays to
    /// `target_velocity`.
    ///
    /// Returns `Some(f32::INFINITY)` for a target of exactly `0.0` (the decay
    /// only reaches zero asymptotically), and `None` when the target is
    /// unreachable: its magnitude exceeds the initial speed, or its sign
    /// differs from the initial velocity's.
    #[must_use]
    #[inline]
    pub fn time_to_velocity(&self, target_velocity: f32) -> Option<f32> {
        if self.decay_rate <= 0.0 || target_velocity.abs() > self.velocity_at_zero.abs() {
            return None;
        }

        if target_velocity == 0.0 {
            return Some(f32::INFINITY);
        }

        // v(t) = v₀ * e^(-k*t)
        // t = -ln(v/v₀) / k
        let ratio = target_velocity / self.velocity_at_zero;
        if ratio <= 0.0 || ratio > 1.0 {
            return None;
        }

        Some(-ratio.ln() / self.decay_rate)
    }

    /// Returns the signed distance travelled, in logical pixels, until the
    /// velocity decays to `target_velocity`.
    ///
    /// When the target velocity is unreachable, returns the total remaining
    /// travel (`final_position` minus the start position) instead.
    #[must_use]
    #[inline]
    pub fn distance_to_velocity(&self, target_velocity: f32) -> f32 {
        if let Some(time) = self.time_to_velocity(target_velocity) {
            self.position(time) - self.position_at_zero
        } else {
            self.final_position() - self.position_at_zero
        }
    }

    /// Returns the acceleration `−k·v(t)` at `time` seconds, in logical
    /// pixels per second squared.
    ///
    /// Its sign always opposes the current velocity (friction decelerates).
    #[must_use]
    #[inline]
    pub fn deceleration(&self, time: f32) -> f32 {
        -self.decay_rate * self.velocity(time)
    }
}

impl Simulation for FrictionSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        self.position_at_zero
            + self.velocity_at_zero * (1.0 - (-self.decay_rate * time).exp()) / self.decay_rate
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        self.velocity_at_zero * (-self.decay_rate * time).exp()
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        self.velocity(time).abs() < self.tolerance.velocity
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

/// A friction simulation whose position stops at a single directional
/// boundary.
///
/// The travel direction is inferred from the sign of the initial velocity;
/// position is clamped at `boundary` in that direction and velocity reports
/// `0.0` once the boundary is reached. See
/// `BoundedFrictionSimulation::new` for the documented divergences from
/// Flutter's two-bound `BoundedFrictionSimulation`.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoundedFrictionSimulation {
    /// The underlying friction simulation
    friction: FrictionSimulation,

    /// The boundary position
    boundary: f32,

    /// Whether we're going in the positive direction
    positive_direction: bool,
}

impl BoundedFrictionSimulation {
    /// Creates a bounded friction simulation with a single directional boundary.
    ///
    /// # Divergences from Flutter
    ///
    /// **API shape** — Flutter's `BoundedFrictionSimulation` takes both a
    /// `minX` and `maxX` bound (line 181 of
    /// `packages/flutter/lib/src/physics/friction_simulation.dart`), clamping
    /// the position to `[minX, maxX]` regardless of travel direction.
    /// This type accepts a single `boundary` with direction inferred from the
    /// sign of `velocity`. The `flui_animation::BoundedFrictionSimulation`
    /// already uses the Flutter-correct two-bound API and is what the
    /// scroll-physics pipeline uses; this single-boundary form exists for
    /// simpler value-physics call sites. Fixing the API here would be a
    /// breaking change with no active callers — tracked but not yet worth the
    /// churn.
    ///
    /// **Velocity at boundary** — Flutter's `BoundedFrictionSimulation` does
    /// NOT override `dx()`: velocity continues to report the unbounded simulated
    /// value even after the position is clamped ("Only the position is
    /// clamped"). This type zeros velocity once the boundary is reached.
    /// The `flui_animation` layer makes the same choice with the explicit
    /// rationale that a controller sampling a pinned simulation should see
    /// zero velocity, not the still-decaying friction velocity. This is a
    /// consistent intentional divergence across both FLUI physics layers.
    #[must_use]
    #[inline]
    pub fn new(decay_rate: f32, position: f32, velocity: f32, boundary: f32) -> Self {
        Self {
            friction: FrictionSimulation::new(decay_rate, position, velocity),
            boundary,
            positive_direction: velocity > 0.0,
        }
    }

    /// Returns the simulation with the underlying friction simulation's
    /// tolerance replaced (builder style).
    #[must_use]
    #[inline]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.friction = self.friction.with_tolerance(tolerance);
        self
    }

    /// Returns the boundary position, in logical pixels.
    #[must_use]
    #[inline]
    pub fn boundary(&self) -> f32 {
        self.boundary
    }

    /// Returns a reference to the underlying unbounded friction simulation.
    #[must_use]
    #[inline]
    pub fn inner(&self) -> &FrictionSimulation {
        &self.friction
    }

    /// Returns whether the unbounded simulation's final position reaches or
    /// passes the boundary in the direction of travel.
    #[must_use]
    #[inline]
    pub fn will_hit_boundary(&self) -> bool {
        let final_pos = self.friction.final_position();
        if self.positive_direction {
            final_pos >= self.boundary
        } else {
            final_pos <= self.boundary
        }
    }

    /// Returns whether the unbounded position at `time` has reached or passed
    /// the boundary in the direction of travel.
    ///
    /// When this is `true`, `velocity` reports `0.0`.
    #[must_use]
    #[inline]
    pub fn is_at_boundary(&self, time: f32) -> bool {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos >= self.boundary
        } else {
            pos <= self.boundary
        }
    }

    /// Returns whether the simulation is well-formed: a valid underlying
    /// friction simulation and a finite boundary.
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.friction.is_valid() && self.boundary.is_finite()
    }
}

impl Simulation for BoundedFrictionSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos.min(self.boundary)
        } else {
            pos.max(self.boundary)
        }
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        if self.is_at_boundary(time) {
            0.0
        } else {
            self.friction.velocity(time)
        }
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        self.is_at_boundary(time) || self.friction.is_done(time)
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1e-3,
            "expected {expected}, got {actual}"
        );
    }

    /// Decay rate k (Flutter's drag c maps to k = -ln c; c = 0.135 is k ≈ 2):
    /// `v(t) = v₀·e^(−kt)`, `x(t) = x₀ + v₀·(1 − e^(−kt))/k`, which settles at
    /// `x₀ + v₀/k`.
    fn sim() -> FrictionSimulation {
        FrictionSimulation::new(2.0, 100.0, 100.0)
    }

    #[test]
    fn trajectory() {
        for (t, x, v) in [
            (0.0, 100.0, 100.0),
            (0.5, 131.606, 36.788),
            (2.0, 149.084, 1.832),
        ] {
            assert_approx(sim().position(t), x);
            assert_approx(sim().velocity(t), v);
            assert_approx(sim().deceleration(t), -2.0 * v);
        }
        assert_approx(sim().final_position(), 150.0);
    }

    /// Throwing the other way mirrors the trajectory about the start.
    #[test]
    fn negative_velocity_mirrors() {
        let back = FrictionSimulation::new(2.0, 100.0, -100.0);
        for t in [0.0, 0.3, 1.0, 4.0] {
            assert_eq!(back.velocity(t), -sim().velocity(t));
            // Only up to rounding: adding the start back rounds 100 - d and 100 + d differently.
            assert_approx(back.position(t) - 100.0, -(sim().position(t) - 100.0));
        }
        assert_eq!(back.final_position(), 50.0);
    }

    /// Done once the speed falls under the velocity tolerance.
    #[test]
    fn is_done_uses_the_velocity_tolerance() {
        assert!(!sim().is_done(2.0) && sim().is_done(10.0));
        let loose = sim().with_tolerance(Tolerance::new(0.001, 40.0, 0.001));
        assert!(!loose.is_done(0.4)); // v ≈ 44.9
        assert!(loose.is_done(0.5)); // v ≈ 36.8
        assert_eq!(loose.tolerance().velocity, 40.0);
        assert_eq!(sim().tolerance(), Tolerance::DEFAULT);
    }

    #[test]
    fn time_and_distance_to_a_velocity() {
        let s = FrictionSimulation::new(2.0, 0.0, 100.0);
        assert_approx(
            s.time_to_velocity(50.0).unwrap(),
            std::f32::consts::LN_2 / 2.0,
        );
        assert_eq!(s.time_to_velocity(100.0), Some(0.0));
        assert_eq!(s.time_to_velocity(0.0), Some(f32::INFINITY));
        assert_eq!(s.time_to_velocity(150.0), None); // faster than it ever goes
        assert_eq!(s.time_to_velocity(-50.0), None); // the wrong direction
        assert_eq!(
            FrictionSimulation::new(0.0, 0.0, 100.0).time_to_velocity(50.0),
            None
        );
        // Half the speed is shed over half the total distance.
        assert_approx(s.distance_to_velocity(50.0), 25.0);
        // An unreachable speed answers the whole remaining distance.
        assert_approx(s.distance_to_velocity(150.0), 50.0);
    }

    #[test]
    fn accessors_and_validity() {
        let s = FrictionSimulation::new(2.0, 3.0, 4.0);
        assert_eq!(
            (s.decay_rate(), s.start_position(), s.initial_velocity()),
            (2.0, 3.0, 4.0)
        );
        assert!(s.is_valid());
        let nan = f32::NAN;
        for broken in [
            FrictionSimulation::new(0.0, 3.0, 4.0),
            FrictionSimulation::new(-1.0, 3.0, 4.0),
            FrictionSimulation::new(f32::INFINITY, 3.0, 4.0),
            FrictionSimulation::new(2.0, nan, 4.0),
            FrictionSimulation::new(2.0, 3.0, nan),
            FrictionSimulation::new(2.0, 3.0, 4.0).with_tolerance(Tolerance::new(-1.0, 0.0, 0.0)),
        ] {
            assert!(!broken.is_valid(), "{broken:?}");
        }
    }

    /// Clamped to the boundary in the direction of travel, velocity zero
    /// once there. (Flutter keeps reporting the decaying friction velocity
    /// at the boundary; zeroing it is a deliberate divergence.)
    #[test]
    fn bounded_friction() {
        let forward = BoundedFrictionSimulation::new(2.0, 0.0, 100.0, 40.0);
        let backward = BoundedFrictionSimulation::new(2.0, 0.0, -100.0, -40.0);
        for (sim, sign) in [(&forward, 1.0), (&backward, -1.0)] {
            // Before the boundary it is plain friction.
            assert_eq!(sim.position(0.1), sim.inner().position(0.1));
            assert_eq!(sim.velocity(0.1), sim.inner().velocity(0.1));
            assert!(!sim.is_at_boundary(0.1) && !sim.is_done(0.1));
            // Unclamped it would be at ±43.2 by t = 1.
            assert_eq!(sim.position(1.0), sign * 40.0);
            assert_eq!(sim.velocity(1.0), 0.0);
            assert!(sim.is_at_boundary(1.0) && sim.is_done(1.0));
            assert!(sim.will_hit_boundary());
            assert_eq!(sim.boundary(), sign * 40.0);
            assert!(sim.is_valid());
        }

        // Settling at ±50 short of a ±60 boundary: done by friction alone.
        for far in [
            BoundedFrictionSimulation::new(2.0, 0.0, 100.0, 60.0),
            BoundedFrictionSimulation::new(2.0, 0.0, -100.0, -60.0),
        ] {
            assert!(!far.will_hit_boundary());
            assert!(!far.is_done(2.0) && far.is_done(10.0) && !far.is_at_boundary(10.0));
        }

        let tol = Tolerance::new(0.25, 0.5, 0.75);
        assert_eq!(forward.with_tolerance(tol).tolerance(), tol);
        assert!(!BoundedFrictionSimulation::new(2.0, 0.0, 100.0, f32::NAN).is_valid());
        assert!(!BoundedFrictionSimulation::new(0.0, 0.0, 100.0, 40.0).is_valid());
    }
}
