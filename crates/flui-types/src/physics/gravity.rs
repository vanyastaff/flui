//! Gravity-based physics simulations
//!
//! This module provides simulations that model motion under gravity.

use super::{Simulation, Tolerance};

/// A constant-acceleration simulation: `x(t) = x₀ + v₀·t + ½·a·t²`.
///
/// Positions are in logical pixels, time in seconds, acceleration in logical
/// pixels per second squared. The simulation is done once the position
/// reaches or passes the signed target `end` (within the distance tolerance)
/// in the direction implied by the sign of `acceleration`, or of `velocity`
/// when acceleration is zero — see `GravitySimulation::new` for how this
/// diverges from Flutter's magnitude-threshold `endDistance`.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GravitySimulation {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// The acceleration due to gravity (in pixels per second squared)
    acceleration: f32,

    /// The starting position
    start: f32,

    /// The ending position (where the simulation should stop)
    end: f32,

    /// The initial velocity
    initial_velocity: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl GravitySimulation {
    /// Creates a gravity simulation.
    ///
    /// # Parameter convention — intentional divergence from Flutter
    ///
    /// Flutter's `GravitySimulation` constructor signature is
    /// `(acceleration, distance, endDistance, velocity)` where `endDistance`
    /// is a **non-negative magnitude threshold**: the simulation finishes when
    /// `|x(t)| >= endDistance`, allowing the particle to cross that magnitude
    /// in either direction (source:
    /// `packages/flutter/lib/src/physics/gravity_simulation.dart`, line 71).
    ///
    /// This type uses a **signed target position** for `end`: the simulation
    /// finishes when the particle reaches or passes `end` in the direction
    /// implied by the sign of `acceleration` (or `velocity` when `acceleration`
    /// is zero). Using a signed target is more explicit and avoids the
    /// surprising Flutter behaviour where `endDistance = 6.0` catches the
    /// particle at `x = −6` just as well as `x = 6`.
    ///
    /// `flui_animation::GravitySimulation` uses the same signed-target
    /// convention, making both FLUI physics layers consistent with each other.
    ///
    /// Migration note: to convert a Flutter-style call
    /// `GravitySimulation(a, x₀, endDist, v₀)` to this type, pass
    /// `end = sign(a) * endDist` (choose the sign matching the direction of
    /// travel).
    #[must_use]
    #[inline]
    pub fn new(acceleration: f32, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            acceleration,
            start,
            end,
            initial_velocity: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Returns the simulation with its tolerance replaced (builder style).
    ///
    /// The distance tolerance widens the end-position check used by `is_done`.
    #[must_use]
    #[inline]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the constant acceleration, in logical pixels per second
    /// squared.
    #[must_use]
    #[inline]
    pub fn acceleration(&self) -> f32 {
        self.acceleration
    }

    /// Returns the starting position, in logical pixels.
    #[must_use]
    #[inline]
    pub fn start(&self) -> f32 {
        self.start
    }

    /// Returns the signed target position at which the simulation finishes,
    /// in logical pixels.
    #[must_use]
    #[inline]
    pub fn end(&self) -> f32 {
        self.end
    }

    /// Returns the initial velocity, in logical pixels per second.
    #[must_use]
    #[inline]
    pub fn initial_velocity(&self) -> f32 {
        self.initial_velocity
    }

    /// Returns whether the simulation is well-formed: finite acceleration,
    /// start, end, and velocity, and a valid tolerance.
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.acceleration.is_finite()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.initial_velocity.is_finite()
            && self.tolerance.is_valid()
    }

    /// Returns the earliest non-negative time, in seconds, at which the
    /// position exactly equals `end`, or `None` if the trajectory never
    /// reaches it (e.g. gravity pulls away from the target, or the particle
    /// is not moving).
    ///
    /// Solved from the quadratic `½·a·t² + v₀·t − (end − start) = 0`; with
    /// negligible acceleration the linear case `t = (end − start)/v₀` is used.
    #[must_use]
    #[inline]
    pub fn time_at_end(&self) -> Option<f32> {
        let distance = self.end - self.start;

        // Using quadratic formula: position = start + velocity*t + 0.5*acceleration*t^2
        // distance = velocity*t + 0.5*acceleration*t^2
        // 0.5*a*t^2 + v*t - distance = 0

        let a = 0.5 * self.acceleration;
        let b = self.initial_velocity;
        let c = -distance;

        if a.abs() < 1e-6 {
            // Linear motion (no acceleration)
            if b.abs() < 1e-6 {
                return None; // Not moving
            }
            let t = -c / b;
            return if t >= 0.0 { Some(t) } else { None };
        }

        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None; // No real solution
        }

        let sqrt_discriminant = discriminant.sqrt();
        let t1 = (-b + sqrt_discriminant) / (2.0 * a);
        let t2 = (-b - sqrt_discriminant) / (2.0 * a);

        // Return the smallest positive time
        match (t1 >= 0.0, t2 >= 0.0) {
            (true, true) => Some(t1.min(t2)),
            (true, false) => Some(t1),
            (false, true) => Some(t2),
            (false, false) => None,
        }
    }
}

impl Simulation for GravitySimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        // position = start + velocity*t + 0.5*acceleration*t^2
        self.start + self.initial_velocity * time + 0.5 * self.acceleration * time * time
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        // velocity = initial_velocity + acceleration*t
        self.initial_velocity + self.acceleration * time
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        let pos = self.position(time);

        // Check if we've reached or passed the end position
        if self.acceleration > 0.0 {
            // Moving in positive direction
            pos >= self.end - self.tolerance.distance
        } else if self.acceleration < 0.0 {
            // Moving in negative direction
            pos <= self.end + self.tolerance.distance
        } else {
            // No acceleration - check if we've reached end with initial velocity
            if self.initial_velocity > 0.0 {
                pos >= self.end - self.tolerance.distance
            } else if self.initial_velocity < 0.0 {
                pos <= self.end + self.tolerance.distance
            } else {
                // Not moving at all
                (pos - self.end).abs() < self.tolerance.distance
            }
        }
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert two f32 values are within `epsilon` of each other.
    #[track_caller]
    fn assert_approx(actual: f32, expected: f32, epsilon: f32) {
        assert!(
            (actual - expected).abs() <= epsilon,
            "expected {expected} ± {epsilon}, got {actual}"
        );
    }

    // -----------------------------------------------------------------------
    // Position and velocity — formulae verified against Flutter's gravity test
    // (`packages/flutter/test/physics/gravity_simulation_test.dart`, line 14).
    //
    // Flutter test: GravitySimulation(-10, 0.0, 6.0, 10.0)
    // FLUI mapping: acceleration=-10, start=0.0, end=-6.0, velocity=10.0
    //   (end is the signed target in the direction of travel: the particle
    //    starts going up, reverses, then falls; we stop it when x ≤ −6.0)
    //
    // x(t) = 0 + 10t + 0.5·(−10)·t² = 10t − 5t²
    // v(t) = 10 + (−10)·t = 10 − 10t
    // -----------------------------------------------------------------------

    #[test]
    fn gravity_position_at_t0() {
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.position(0.0), 0.0, 1e-4);
    }

    #[test]
    fn gravity_velocity_at_t0() {
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.velocity(0.0), 10.0, 1e-4);
    }

    #[test]
    fn gravity_position_at_t1() {
        // x(1) = 10·1 − 5·1 = 5.0
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.position(1.0), 5.0, 1e-4);
    }

    #[test]
    fn gravity_velocity_at_t1() {
        // v(1) = 10 − 10 = 0.0  (apex)
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.velocity(1.0), 0.0, 1e-4);
    }

    #[test]
    fn gravity_position_at_t2() {
        // x(2) = 20 − 20 = 0.0  (back at origin)
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.position(2.0), 0.0, 1e-4);
    }

    #[test]
    fn gravity_velocity_at_t2() {
        // v(2) = 10 − 20 = −10.0
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.velocity(2.0), -10.0, 1e-4);
    }

    #[test]
    fn gravity_position_at_t3() {
        // x(3) = 30 − 45 = −15.0
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.position(3.0), -15.0, 1e-4);
    }

    #[test]
    fn gravity_velocity_at_t3() {
        // v(3) = 10 − 30 = −20.0
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert_approx(sim.velocity(3.0), -20.0, 1e-4);
    }

    // -----------------------------------------------------------------------
    // is_done — mirrors Flutter's isDone from the same test
    // Flutter: isDone(t) = |x(t)| >= endDistance (6.0)
    // FLUI:   is_done uses signed end = −6.0; acceleration < 0 → pos <= end+ε
    // -----------------------------------------------------------------------

    #[test]
    fn gravity_not_done_at_t0() {
        // x(0) = 0, not yet past −6
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert!(!sim.is_done(0.0));
    }

    #[test]
    fn gravity_not_done_at_t2() {
        // Flutter isDone(2.0) is false: |x(2)| = 0 < 6.
        // FLUI: x(2) = 0 > −6 + ε → not done.
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert!(!sim.is_done(2.0));
    }

    #[test]
    fn gravity_done_at_t3() {
        // Flutter isDone(3.0) is true: |x(3)| = 15 >= 6.
        // FLUI: x(3) = −15 <= −6 + ε → done.
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        assert!(sim.is_done(3.0));
    }

    #[test]
    fn gravity_positive_acceleration_example() {
        // Flutter test: GravitySimulation(9.81, 10.0, 0.0, 0.0)
        //   expects x(10) ≈ 50·9.81 + 10 = 500.5
        // FLUI mapping: acceleration=9.81, start=10.0, end=500.0, velocity=0.0
        let sim = GravitySimulation::new(9.81, 10.0, 500.0, 0.0);
        // x(10) = 10 + 0 + 0.5·9.81·100 = 10 + 490.5 = 500.5
        assert_approx(sim.position(10.0), 10.0 + 0.5 * 9.81 * 100.0, 1.0);
    }
}
