//! Spring-based physics simulations
//!
//! This module provides simulations that model spring physics with
//! mass, stiffness, and damping.

use super::{Simulation, Tolerance};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpringType {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Critical damping - returns to rest as quickly as possible without
    /// oscillating
    Critical,

    /// Under-damped - oscillates before coming to rest
    Underdamped,

    /// Over-damped - slowly returns to rest without oscillating
    Overdamped,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringDescription {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// The mass of the spring (must be positive)
    pub mass: f32,

    /// The stiffness constant (must be positive)
    pub stiffness: f32,

    /// The damping coefficient (must be non-negative)
    pub damping: f32,
}

impl SpringDescription {
    #[must_use]
    #[inline]
    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    #[must_use]
    #[inline]
    pub fn with_critical_damping(mass: f32, stiffness: f32) -> Self {
        let damping = 2.0 * (mass * stiffness).sqrt();
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    #[must_use]
    #[inline]
    pub fn bouncy() -> Self {
        // Mass=1, Stiffness=300, Damping=10 gives ~0.4 damping ratio
        Self::new(1.0, 300.0, 10.0)
    }

    #[must_use]
    #[inline]
    pub fn stiff() -> Self {
        Self::with_critical_damping(1.0, 500.0)
    }

    #[must_use]
    #[inline]
    pub fn soft() -> Self {
        // Mass=1, Stiffness=100, Damping=30 gives ~1.5 damping ratio
        Self::new(1.0, 100.0, 30.0)
    }

    /// Returns the type of spring based on the discriminant `c²−4mk`.
    ///
    /// Mirrors Flutter's `_SpringSolution` factory
    /// (`packages/flutter/lib/src/physics/spring_simulation.dart`, line 291):
    ///
    /// ```text
    /// return switch (spring.damping * spring.damping - 4 * spring.mass * spring.stiffness) {
    ///   > 0.0 => _OverdampedSolution(...),
    ///   < 0.0 => _UnderdampedSolution(...),
    ///   _     => _CriticalSolution(...),   // exact zero only
    /// };
    /// ```
    ///
    /// The discriminant equals `4mk(ζ²−1)` where `ζ = c/(2√(mk))`, so the
    /// three regions correspond exactly to `ζ > 1` (overdamped), `ζ < 1`
    /// (underdamped), and `ζ = 1` (critically damped).
    ///
    /// A previous implementation used a ±0.001 tolerance band around ζ = 1,
    /// which misclassified springs with damping ratios in [0.999, 1.001] as
    /// [`SpringType::Critical`] while Flutter would classify them as
    /// under/overdamped. All three solution families are mathematically
    /// continuous at the boundary, so the classification matters for correctness
    /// of the formula used, not for observable discontinuities in position.
    #[must_use]
    #[inline]
    pub fn spring_type(&self) -> SpringType {
        let discriminant = self.damping * self.damping - 4.0 * self.mass * self.stiffness;
        if discriminant > 0.0 {
            SpringType::Overdamped
        } else if discriminant < 0.0 {
            SpringType::Underdamped
        } else {
            SpringType::Critical
        }
    }

    #[must_use]
    #[inline]
    pub fn damping_ratio(&self) -> f32 {
        let critical_damping = 2.0 * (self.mass * self.stiffness).sqrt();
        self.damping / critical_damping
    }

    #[must_use]
    #[inline]
    pub fn natural_frequency(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }

    #[must_use]
    #[inline]
    pub fn damped_frequency(&self) -> f32 {
        let w0 = self.natural_frequency();
        let zeta = self.damping_ratio();
        w0 * (1.0 - zeta * zeta).max(0.0).sqrt()
    }

    #[must_use]
    #[inline]
    pub fn period(&self) -> Option<f32> {
        let wd = self.damped_frequency();
        if wd > 0.0 {
            Some(2.0 * std::f32::consts::PI / wd)
        } else {
            None
        }
    }

    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.mass > 0.0
            && self.stiffness > 0.0
            && self.damping >= 0.0
            && self.mass.is_finite()
            && self.stiffness.is_finite()
            && self.damping.is_finite()
    }

    #[must_use]
    #[inline]
    pub fn critical_damping(&self) -> f32 {
        2.0 * (self.mass * self.stiffness).sqrt()
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringSimulation {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// The spring description
    spring: SpringDescription,

    /// The starting position
    start: f32,

    /// The ending position (equilibrium point)
    end: f32,

    /// The initial velocity
    initial_velocity: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl SpringSimulation {
    #[must_use]
    #[inline]
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            spring,
            start,
            end,
            initial_velocity: velocity,
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
    pub fn spring(&self) -> &SpringDescription {
        &self.spring
    }

    #[must_use]
    #[inline]
    pub fn start(&self) -> f32 {
        self.start
    }

    #[must_use]
    #[inline]
    pub fn end(&self) -> f32 {
        self.end
    }

    #[must_use]
    #[inline]
    pub fn initial_velocity(&self) -> f32 {
        self.initial_velocity
    }

    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.spring.is_valid()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.initial_velocity.is_finite()
            && self.tolerance.is_valid()
    }

    /// Calculate position for an underdamped spring
    #[inline]
    fn position_underdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let wd = w0 * (1.0 - zeta * zeta).sqrt();

        let a = self.start - self.end;
        let b = (self.initial_velocity + zeta * w0 * a) / wd;

        let envelope = (-zeta * w0 * time).exp();
        self.end + envelope * (a * (wd * time).cos() + b * (wd * time).sin())
    }

    /// Calculate velocity for an underdamped spring
    #[inline]
    fn velocity_underdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let wd = w0 * (1.0 - zeta * zeta).sqrt();

        let a = self.start - self.end;
        let b = (self.initial_velocity + zeta * w0 * a) / wd;

        let envelope = (-zeta * w0 * time).exp();
        let envelope_derivative = -zeta * w0 * envelope;

        envelope_derivative * (a * (wd * time).cos() + b * (wd * time).sin())
            + envelope * (-a * wd * (wd * time).sin() + b * wd * (wd * time).cos())
    }

    /// Calculate position for a critically damped spring
    #[inline]
    fn position_critical(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let w0 = (k / m).sqrt();

        let a = self.start - self.end;
        let b = self.initial_velocity + w0 * a;

        let envelope = (-w0 * time).exp();
        self.end + envelope * (a + b * time)
    }

    /// Calculate velocity for a critically damped spring
    #[inline]
    fn velocity_critical(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let w0 = (k / m).sqrt();

        let a = self.start - self.end;
        let b = self.initial_velocity + w0 * a;

        let envelope = (-w0 * time).exp();
        -w0 * envelope * (a + b * time) + envelope * b
    }

    /// Calculate position for an overdamped spring
    #[inline]
    fn position_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = (
            (s * a - self.initial_velocity) / (s - r),
            (self.initial_velocity - r * a) / (s - r),
        );

        self.end + b.0 * (r * time).exp() + b.1 * (s * time).exp()
    }

    /// Calculate velocity for an overdamped spring
    #[inline]
    fn velocity_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = (
            (s * a - self.initial_velocity) / (s - r),
            (self.initial_velocity - r * a) / (s - r),
        );

        b.0 * r * (r * time).exp() + b.1 * s * (s * time).exp()
    }
}

impl Simulation for SpringSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        match self.spring.spring_type() {
            SpringType::Critical => self.position_critical(time),
            SpringType::Underdamped => self.position_underdamped(time),
            SpringType::Overdamped => self.position_overdamped(time),
        }
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        match self.spring.spring_type() {
            SpringType::Critical => self.velocity_critical(time),
            SpringType::Underdamped => self.velocity_underdamped(time),
            SpringType::Overdamped => self.velocity_overdamped(time),
        }
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        let pos = self.position(time);
        let vel = self.velocity(time);

        (pos - self.end).abs() < self.tolerance.distance && vel.abs() < self.tolerance.velocity
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
    // SpringDescription::spring_type — discriminant classification
    //
    // Regression guard for the fix: the previous tolerance-band check would
    // classify ζ = 1 ± 0.0005 as Critical; the discriminant check does not.
    //
    // Reference: Flutter spring_simulation.dart line 291, and the Flutter
    // regression test "SpringSimulation results are continuous near critical
    // damping" (spring_simulation_test.dart line 36).
    // -----------------------------------------------------------------------

    #[test]
    fn spring_type_underdamped() {
        // ζ = 0.4 → discriminant = 4mk(ζ²−1) < 0 → Underdamped
        let spring = SpringDescription::new(1.0, 300.0, 10.0);
        assert!(matches!(spring.spring_type(), SpringType::Underdamped));
    }

    #[test]
    fn spring_type_overdamped() {
        // ζ = 1.5 → discriminant > 0 → Overdamped
        let spring = SpringDescription::new(1.0, 100.0, 30.0);
        assert!(matches!(spring.spring_type(), SpringType::Overdamped));
    }

    #[test]
    fn spring_type_critical_exact() {
        // with_critical_damping produces damping = 2√(mk), so discriminant = 0.
        let spring = SpringDescription::with_critical_damping(1.0, 100.0);
        assert!(matches!(spring.spring_type(), SpringType::Critical));
    }

    #[test]
    fn spring_type_slightly_underdamped_is_not_critical() {
        // ζ = 0.9995 (offset 5e-4 inside the old ±0.001 band).
        //
        // OLD tolerance-band code: |0.9995 − 1| = 0.0005 < 0.001 → Critical (BUG).
        // NEW discriminant code:   Δ = 4mk(ζ²−1) < 0            → Underdamped (correct).
        //
        // This test FAILS on the old tolerance-band code.
        let mass = 0.4_f32;
        let stiffness = 0.4_f32;
        let ratio = 1.0_f32 - 5e-4;
        let damping = ratio * 2.0 * (mass * stiffness).sqrt();
        let spring = SpringDescription::new(mass, stiffness, damping);
        assert!(
            matches!(spring.spring_type(), SpringType::Underdamped),
            "damping ratio {ratio} < 1 must be Underdamped, not Critical"
        );
    }

    #[test]
    fn spring_type_slightly_overdamped_is_not_critical() {
        // ζ = 1.0005 (offset 5e-4 inside the old ±0.001 band).
        //
        // OLD tolerance-band code: |1.0005 − 1| = 0.0005 < 0.001 → Critical (BUG).
        // NEW discriminant code:   Δ = 4mk(ζ²−1) > 0            → Overdamped (correct).
        //
        // This test FAILS on the old tolerance-band code.
        let mass = 0.4_f32;
        let stiffness = 0.4_f32;
        let ratio = 1.0_f32 + 5e-4;
        let damping = ratio * 2.0 * (mass * stiffness).sqrt();
        let spring = SpringDescription::new(mass, stiffness, damping);
        assert!(
            matches!(spring.spring_type(), SpringType::Overdamped),
            "damping ratio {ratio} > 1 must be Overdamped, not Critical"
        );
    }

    // -----------------------------------------------------------------------
    // SpringSimulation — parity tests
    //
    // Values derived from Flutter's spring test (`spring_simulation_test.dart`
    // line 36): `SpringDescription.withDampingRatio(stiffness: 0.4, mass: 0.4)`
    // with start=0, end=1, velocity=0 at t=0.4.
    //
    // Manual verification (mass=0.4, stiffness=0.4, ratio=1.0, so damping=0.8):
    //   w₀ = √(k/m) = 1.0,  a = 0 − 1 = −1,  b = 0 + 1·(−1) = −1
    //   x(t) = 1 + e^(−t)·(−1 − t)
    //   x(0.4) = 1 − 1.4·e^(−0.4) ≈ 0.0616
    //   v(t)   = e^(−t)·t
    //   v(0.4) = e^(−0.4)·0.4 ≈ 0.2681
    // -----------------------------------------------------------------------

    #[test]
    fn spring_critical_position_at_t0_4() {
        let spring = SpringDescription::with_critical_damping(0.4, 0.4);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        // Flutter reference: x(0.4) ≈ 0.0616 (epsilon: 0.01)
        assert_approx(sim.position(0.4), 0.0616, 0.01);
    }

    #[test]
    fn spring_critical_velocity_at_t0_4() {
        let spring = SpringDescription::with_critical_damping(0.4, 0.4);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        // Flutter reference: dx(0.4) ≈ 0.2681 (epsilon: 0.01)
        assert_approx(sim.velocity(0.4), 0.2681, 0.01);
    }

    #[test]
    fn spring_slightly_underdamped_continuous_with_critical() {
        // ζ = 1 − 1e−3: Underdamped after fix, but x(0.4) must still ≈ 0.0616.
        // Verifies the formulas are continuous at the critical-damping boundary.
        // Reference: Flutter's regression test (spring_simulation_test.dart:59).
        let mass = 0.4_f32;
        let stiffness = 0.4_f32;
        let damping = (1.0 - 1e-3) * 2.0 * (mass * stiffness).sqrt();
        let spring = SpringDescription::new(mass, stiffness, damping);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        assert_approx(sim.position(0.4), 0.0616, 0.01);
        assert_approx(sim.velocity(0.4), 0.2681, 0.01);
    }

    #[test]
    fn spring_slightly_overdamped_continuous_with_critical() {
        // ζ = 1 + 1e−3: Overdamped after fix, x(0.4) must still ≈ 0.0616.
        // Reference: Flutter's regression test (spring_simulation_test.dart:50).
        let mass = 0.4_f32;
        let stiffness = 0.4_f32;
        let damping = (1.0 + 1e-3) * 2.0 * (mass * stiffness).sqrt();
        let spring = SpringDescription::new(mass, stiffness, damping);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        assert_approx(sim.position(0.4), 0.0616, 0.01);
        assert_approx(sim.velocity(0.4), 0.2681, 0.01);
    }

    #[test]
    fn spring_underdamped_oscillates() {
        // An underdamped spring (ζ ≈ 0.4) must overshoot its target.
        let spring = SpringDescription::new(1.0, 300.0, 10.0); // bouncy preset
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        // Position must exceed 1.0 at some point (overshoot) to confirm oscillation.
        let overshoot = (0..200).any(|i| sim.position(i as f32 * 0.01) > 1.001);
        assert!(overshoot, "underdamped spring must overshoot its target");
    }

    #[test]
    fn spring_overdamped_does_not_oscillate() {
        // An overdamped spring (ζ ≈ 1.5) must not overshoot.
        let spring = SpringDescription::new(1.0, 100.0, 30.0); // soft preset
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        let overshoot = (0..500).any(|i| sim.position(i as f32 * 0.01) > 1.0 + 1e-3);
        assert!(!overshoot, "overdamped spring must not overshoot");
    }

    #[test]
    fn spring_is_done_when_position_and_velocity_within_tolerance() {
        let spring = SpringDescription::with_critical_damping(1.0, 100.0);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
        // A critically-damped spring converges; is_done must eventually hold.
        assert!(!sim.is_done(0.0), "not done at start");
        assert!(sim.is_done(100.0), "done after sufficient time");
    }
}
