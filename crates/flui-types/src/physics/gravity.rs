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

    #[track_caller]
    fn assert_approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1e-4,
            "expected {expected}, got {actual}"
        );
    }

    /// Flutter's gravity test (`test/physics/gravity_simulation_test.dart`):
    /// `GravitySimulation(-10, 0.0, 6.0, 10.0)`. FLUI takes a signed end in
    /// the direction of travel, so the same fall stops at -6:
    /// `x(t) = 10t - 5t²`, `v(t) = 10 - 10t`, done once `x <= -6`.
    #[test]
    fn thrown_up_then_falling() {
        let sim = GravitySimulation::new(-10.0, 0.0, -6.0, 10.0);
        for (t, x, v, done) in [
            (0.0, 0.0, 10.0, false),
            (1.0, 5.0, 0.0, false),
            (2.0, 0.0, -10.0, false),
            (3.0, -15.0, -20.0, true),
        ] {
            assert_approx(sim.position(t), x);
            assert_approx(sim.velocity(t), v);
            assert_eq!(sim.is_done(t), done, "t = {t}");
        }
        // Flutter: GravitySimulation(9.81, 10.0, 0.0, 0.0) reaches 500.5 at t = 10.
        assert_approx(
            GravitySimulation::new(9.81, 10.0, 500.0, 0.0).position(10.0),
            500.5,
        );
    }

    /// Done means at or past `end` (within the distance tolerance) in the
    /// direction of travel: the acceleration's sign, or the velocity's when
    /// there is none, or exactly at `end` when not moving at all.
    #[test]
    fn is_done_follows_the_direction_of_travel() {
        let tol = Tolerance::new(0.5, 0.001, 0.001);
        for (sim, t, done) in [
            (GravitySimulation::new(2.0, 0.0, 10.0, 0.0), 3.0, false), // x = 9
            (GravitySimulation::new(2.0, 0.0, 10.0, 0.0), 3.2, true),  // x = 10.24
            (GravitySimulation::new(0.0, 0.0, 10.0, 2.0), 4.0, false), // x = 8
            (GravitySimulation::new(0.0, 0.0, 10.0, 2.0), 5.0, true),  // x = 10
            (GravitySimulation::new(0.0, 0.0, -10.0, -2.0), 4.0, false),
            (GravitySimulation::new(0.0, 0.0, -10.0, -2.0), 5.0, true),
            // Past the end still counts, which a check for "at end" misses.
            (GravitySimulation::new(0.0, 0.0, 10.0, 2.0), 6.0, true),
            (GravitySimulation::new(0.0, 0.0, -10.0, -2.0), 6.0, true),
            (GravitySimulation::new(0.0, 3.0, 3.0, 0.0), 9.0, true),
            (GravitySimulation::new(0.0, 3.0, 4.0, 0.0), 9.0, false),
            // Within the tolerance of the end counts as done, on each branch.
            (
                GravitySimulation::new(2.0, 0.0, 9.4, 0.0).with_tolerance(tol),
                3.0,
                true,
            ),
            (
                GravitySimulation::new(-2.0, 0.0, -9.4, 0.0).with_tolerance(tol),
                3.0,
                true,
            ),
            (
                GravitySimulation::new(0.0, 0.0, 8.4, 2.0).with_tolerance(tol),
                4.0,
                true,
            ),
            (
                GravitySimulation::new(0.0, 0.0, -8.4, -2.0).with_tolerance(tol),
                4.0,
                true,
            ),
            (
                GravitySimulation::new(0.0, 3.0, 3.4, 0.0).with_tolerance(tol),
                0.0,
                true,
            ),
            (
                GravitySimulation::new(0.0, 3.0, 3.6, 0.0).with_tolerance(tol),
                0.0,
                false,
            ),
        ] {
            assert_eq!(sim.is_done(t), done, "{sim:?} at t = {t}");
        }
    }

    /// The earliest non-negative time the particle reaches `end`.
    #[test]
    fn time_at_end() {
        let at = |a, end, v| GravitySimulation::new(a, 0.0, end, v).time_at_end();
        // Linear motion.
        assert_eq!(at(0.0, 10.0, 2.0), Some(5.0));
        assert_eq!(at(0.0, 10.0, -2.0), None);
        assert_eq!(at(0.0, 10.0, 0.0), None);
        // x = t², reaching 9 at t = 3.
        assert_eq!(at(2.0, 9.0, 0.0), Some(3.0));
        // x = 10t - 5t² passes 3.2 at 0.4 and again at 1.6: the earlier one.
        assert_approx(at(-10.0, 3.2, 10.0).unwrap(), 0.4);
        // Only the later root is in the future: 10t - 5t² = -15 at t = 3 (and -1).
        assert_approx(at(-10.0, -15.0, 10.0).unwrap(), 3.0);
        // x = -5t² never reaches 5; x = 10t + 5t² reaches -1 only in the past.
        assert_eq!(at(-10.0, 5.0, 0.0), None);
        assert_eq!(at(10.0, -1.0, 10.0), None);
        // Starting away from zero measures the distance from `start`.
        assert_eq!(
            GravitySimulation::new(0.0, 4.0, 10.0, 2.0).time_at_end(),
            Some(3.0)
        );
        // Already at the end counts, on either root: x = t² - 2t and
        // x = 2t - t² are 0 at t = 0 and again at t = 2.
        assert_eq!(at(0.0, 0.0, 2.0), Some(0.0));
        assert_eq!(at(2.0, 0.0, -2.0), Some(0.0));
        assert_eq!(at(-2.0, 0.0, 2.0), Some(0.0));
        // A trajectory that only touches the end: 2t - t² peaks at 1.
        assert_eq!(at(-2.0, 1.0, 2.0), Some(1.0));
    }

    /// The tolerance bounds are inclusive on every branch, and a particle
    /// with neither acceleration nor velocity is done only within the
    /// tolerance of the end, never merely past it.
    #[test]
    fn is_done_boundaries() {
        let tol = Tolerance::new(0.5, 0.001, 0.001);
        let sim = |a, start, end, v| GravitySimulation::new(a, start, end, v).with_tolerance(tol);
        // x = t² reaches 9 = 9.5 - 0.5 at t = 3; x = -t² reaches -9.
        assert!(sim(2.0, 0.0, 9.5, 0.0).is_done(3.0));
        assert!(sim(-2.0, 0.0, -9.5, 0.0).is_done(3.0));
        // x = 2t reaches 10 = 10.5 - 0.5 at t = 5; x = -2t reaches -10.
        assert!(sim(0.0, 0.0, 10.5, 2.0).is_done(5.0));
        assert!(sim(0.0, 0.0, -10.5, -2.0).is_done(5.0));
        // x = -t² is at -6 at t = √6: short of -9.5 + 0.5, however the
        // bound is formed.
        assert!(!sim(-2.0, 0.0, -9.5, 0.0).is_done(6.0_f32.sqrt()));
        // At rest: exactly the tolerance away is not done, and being past
        // the end does not count as arriving.
        assert!(!sim(0.0, 3.0, 3.5, 0.0).is_done(1.0));
        assert!(sim(0.0, 3.0, 3.25, 0.0).is_done(1.0));
        assert!(!sim(0.0, 0.0, -5.0, 0.0).is_done(1.0));
    }

    #[test]
    fn accessors_and_validity() {
        let tol = Tolerance::new(0.25, 0.5, 0.75);
        let sim = GravitySimulation::new(1.5, 2.0, 3.0, 4.0).with_tolerance(tol);
        assert_eq!(
            (
                sim.acceleration(),
                sim.start(),
                sim.end(),
                sim.initial_velocity()
            ),
            (1.5, 2.0, 3.0, 4.0)
        );
        assert_eq!(sim.tolerance(), tol);
        assert_eq!(
            GravitySimulation::new(1.0, 2.0, 3.0, 4.0).tolerance(),
            Tolerance::DEFAULT
        );
        assert!(sim.is_valid());
        let nan = f32::NAN;
        for broken in [
            GravitySimulation::new(nan, 2.0, 3.0, 4.0),
            GravitySimulation::new(1.0, nan, 3.0, 4.0),
            GravitySimulation::new(1.0, 2.0, nan, 4.0),
            GravitySimulation::new(1.0, 2.0, 3.0, nan),
            GravitySimulation::new(1.0, 2.0, 3.0, 4.0)
                .with_tolerance(Tolerance::new(-1.0, 0.0, 0.0)),
        ] {
            assert!(!broken.is_valid(), "{broken:?}");
        }
    }
}
