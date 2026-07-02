//! Friction-based physics simulations
//!
//! This module provides simulations that model friction and deceleration.

use super::{Simulation, Tolerance};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrictionSimulation {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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

    #[must_use]
    #[inline]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    #[must_use]
    #[inline]
    pub fn decay_rate(&self) -> f32 {
        self.decay_rate
    }

    #[must_use]
    #[inline]
    pub fn start_position(&self) -> f32 {
        self.position_at_zero
    }

    #[must_use]
    #[inline]
    pub fn initial_velocity(&self) -> f32 {
        self.velocity_at_zero
    }

    #[must_use]
    #[inline]
    pub fn final_position(&self) -> f32 {
        self.position_at_zero + self.velocity_at_zero / self.decay_rate
    }

    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.decay_rate > 0.0
            && self.decay_rate.is_finite()
            && self.position_at_zero.is_finite()
            && self.velocity_at_zero.is_finite()
            && self.tolerance.is_valid()
    }

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

    #[must_use]
    #[inline]
    pub fn distance_to_velocity(&self, target_velocity: f32) -> f32 {
        if let Some(time) = self.time_to_velocity(target_velocity) {
            self.position(time) - self.position_at_zero
        } else {
            self.final_position() - self.position_at_zero
        }
    }

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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BoundedFrictionSimulation {
    // PORT-CHECK-OK-SP3: parallel to flui-animation::simulation::BoundedFrictionSimulation; the two physics layers use distinct Simulation traits (position/velocity here vs x/dx + Send+Sync there). Consolidation tracked.
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

    #[must_use]
    #[inline]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.friction = self.friction.with_tolerance(tolerance);
        self
    }

    #[must_use]
    #[inline]
    pub fn boundary(&self) -> f32 {
        self.boundary
    }

    #[must_use]
    #[inline]
    pub fn inner(&self) -> &FrictionSimulation {
        &self.friction
    }

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

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Assert two f32 values are within `epsilon` of each other.
    #[track_caller]
    fn assert_approx(actual: f32, expected: f32, epsilon: f32) {
        assert!(
            (actual - expected).abs() <= epsilon,
            "expected {expected} ± {epsilon}, got {actual}"
        );
    }

    // -----------------------------------------------------------------------
    // FrictionSimulation — formula parity
    //
    // Convention: this type uses decay rate k (not Flutter's drag coefficient).
    // k = −ln(cₓ) where cₓ is Flutter's drag.  Flutter's cₓ = 0.135 gives
    // k ≈ 2.0.  All numeric expectations below are derived from
    // v(t) = v₀·e^(−k·t)  and  x(t) = x₀ + v₀·(1−e^(−k·t))/k.
    // -----------------------------------------------------------------------

    #[test]
    fn friction_position_at_t0_is_start() {
        let sim = FrictionSimulation::new(2.0, 100.0, 100.0);
        assert_approx(sim.position(0.0), 100.0, 1e-4);
    }

    #[test]
    fn friction_velocity_at_t0_is_initial() {
        let sim = FrictionSimulation::new(2.0, 100.0, 100.0);
        assert_approx(sim.velocity(0.0), 100.0, 1e-4);
    }

    #[test]
    fn friction_position_decays_over_time() {
        // x(t) = 100 + 100·(1−e^(−2t))/2 = 100 + 50·(1−e^(−2t))
        let sim = FrictionSimulation::new(2.0, 100.0, 100.0);
        // t=0.1: 100 + 50·(1−e^−0.2) ≈ 100 + 50·0.1813 ≈ 109.07
        assert_approx(sim.position(0.1), 109.07, 0.5);
        // t=0.5: 100 + 50·(1−e^−1.0) ≈ 100 + 50·0.6321 ≈ 131.61
        assert_approx(sim.position(0.5), 131.6, 1.0);
        // t=2.0: 100 + 50·(1−e^−4.0) ≈ 100 + 50·0.9817 ≈ 149.08
        assert_approx(sim.position(2.0), 149.1, 1.0);
    }

    #[test]
    fn friction_velocity_decays_exponentially() {
        // v(t) = 100·e^(−2t)
        let sim = FrictionSimulation::new(2.0, 100.0, 100.0);
        // t=0.5: 100·e^−1.0 ≈ 36.79
        assert_approx(sim.velocity(0.5), 36.79, 0.5);
        // t=2.0: 100·e^−4.0 ≈ 1.83
        assert_approx(sim.velocity(2.0), 1.83, 0.1);
    }

    #[test]
    fn friction_final_position_formula() {
        // final_position = x₀ + v₀/k = 100 + 100/2 = 150
        let sim = FrictionSimulation::new(2.0, 100.0, 100.0);
        assert_approx(sim.final_position(), 150.0, 0.1);
    }

    #[test]
    fn friction_is_done_when_velocity_below_tolerance() {
        // With default velocity tolerance = 0.001, done when |v| < 0.001.
        // v(t) = 100·e^(−2t) → need t ≈ ln(100/0.001)/2 ≈ 5.75 s.
        let sim = FrictionSimulation::new(2.0, 0.0, 100.0);
        assert!(!sim.is_done(2.0), "velocity still decaying at t=2");
        assert!(sim.is_done(10.0), "velocity negligible at t=10");
    }

    #[test]
    fn friction_negative_velocity_moves_in_negative_direction() {
        // v(t) = −100·e^(−2t), x(t) = 100 + (−100)·(1−e^(−2t))/2 = 100 − 50·(1−e^(−2t))
        let sim = FrictionSimulation::new(2.0, 100.0, -100.0);
        assert_approx(sim.velocity(0.0), -100.0, 1e-4);
        // At t=0.5: 100 − 50·(1−e^−1) ≈ 100 − 31.6 ≈ 68.4
        assert_approx(sim.position(0.5), 68.4, 1.0);
        // final: 100 − 50 = 50
        assert_approx(sim.final_position(), 50.0, 0.5);
    }

    #[test]
    fn friction_time_to_velocity_round_trip() {
        let sim = FrictionSimulation::new(2.0, 0.0, 100.0);
        // time_to_velocity(50) should satisfy v(t) = 50, i.e. t = ln(2)/2 ≈ 0.347
        let t = sim.time_to_velocity(50.0).expect("valid target velocity");
        assert_approx(t, 0.5_f32.ln() / (-2.0_f32), 1e-3);
        assert_approx(sim.velocity(t), 50.0, 0.1);
    }

    // -----------------------------------------------------------------------
    // BoundedFrictionSimulation — parity tests
    // -----------------------------------------------------------------------

    #[test]
    fn bounded_friction_clamps_position_at_boundary() {
        // final_position = x₀ + v₀/k = 0 + 100/2 = 50, which exceeds boundary = 40.
        // At t=1.0, unclamped position ≈ 50·(1 − e^−2) ≈ 43.23 > 40 (boundary).
        // The clamp must lower the observed position to the boundary.
        let boundary = 40.0_f32;
        let sim = BoundedFrictionSimulation::new(2.0, 0.0, 100.0, boundary);
        let t = 1.0_f32;
        let unclamped = sim.inner().position(t);
        let clamped = sim.position(t);
        assert!(
            unclamped > boundary,
            "unclamped position {unclamped} must exceed boundary {boundary}"
        );
        assert!(
            clamped <= boundary,
            "clamped position {clamped} must not exceed boundary {boundary}"
        );
        assert!(
            clamped < unclamped,
            "clamp must lower the position: clamped={clamped}, unclamped={unclamped}"
        );
    }

    #[test]
    fn bounded_friction_zeroes_velocity_at_boundary() {
        // Intentional divergence from Flutter (documented in `new`): velocity is
        // zeroed once the boundary is reached.  A Flutter `BoundedFrictionSimulation`
        // would report the still-decaying friction velocity here.
        let sim = BoundedFrictionSimulation::new(2.0, 0.0, 100.0, 50.0);
        // At t=10 the unclamped position is far past 50; boundary is hit.
        assert!(sim.is_at_boundary(10.0), "should be at boundary at t=10");
        assert_approx(sim.velocity(10.0), 0.0, 1e-6);
    }

    #[test]
    fn bounded_friction_is_done_at_boundary() {
        let sim = BoundedFrictionSimulation::new(2.0, 0.0, 100.0, 50.0);
        assert!(
            sim.is_done(10.0),
            "simulation ends when boundary is reached"
        );
    }

    #[test]
    fn bounded_friction_negative_direction() {
        let sim = BoundedFrictionSimulation::new(2.0, 0.0, -100.0, -50.0);
        assert!(
            sim.position(10.0) >= -50.0 - f32::EPSILON,
            "position clamped at negative boundary"
        );
        assert!(sim.is_done(10.0));
        assert_approx(sim.velocity(10.0), 0.0, 1e-6);
    }
}
