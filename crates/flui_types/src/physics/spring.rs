//! Spring-based physics simulations
//!
//! This module provides simulations that model spring physics with
//! mass, stiffness, and damping.

use super::{Simulation, Tolerance};

/// The type of spring based on its damping characteristics
///
/// Similar to Flutter's `SpringType`. Determines how a spring behaves
/// when returning to equilibrium.
///
/// # Examples
///
/// ```
/// use flui_types::physics::SpringType;
///
/// let spring_type = SpringType::Underdamped;
/// assert_eq!(spring_type, SpringType::Underdamped);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpringType {
    /// Critical damping - returns to rest as quickly as possible without oscillating
    Critical,

    /// Under-damped - oscillates before coming to rest
    Underdamped,

    /// Over-damped - slowly returns to rest without oscillating
    Overdamped,
}

/// Description of a spring's physical characteristics
///
/// Similar to Flutter's `SpringDescription`. Defines the mass, stiffness,
/// and damping of a spring system.
///
/// # Physics
/// - **Damping ratio** (ζ): `damping / (2 * sqrt(mass * stiffness))`
/// - **Natural frequency** (ω₀): `sqrt(stiffness / mass)`
/// - **Critical damping**: `2 * sqrt(mass * stiffness)`
///
/// # Memory Safety
/// - Stack-allocated `Copy` type with no heap allocations
/// - All fields are plain `f32` values
///
/// # Type Safety
/// - Const constructors for compile-time evaluation
/// - Validation methods prevent invalid states
/// - `#[must_use]` on all pure methods
///
/// # Examples
///
/// ```
/// use flui_types::physics::SpringDescription;
///
/// // Create a spring with mass=1.0, stiffness=100.0, damping=10.0
/// let spring = SpringDescription::new(1.0, 100.0, 10.0);
///
/// assert_eq!(spring.mass, 1.0);
/// assert_eq!(spring.stiffness, 100.0);
/// assert_eq!(spring.damping, 10.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringDescription {
    /// The mass of the spring (must be positive)
    pub mass: f32,

    /// The stiffness constant (must be positive)
    pub stiffness: f32,

    /// The damping coefficient (must be non-negative)
    pub damping: f32,
}

impl SpringDescription {
    /// Creates a new spring description
    ///
    /// # Arguments
    ///
    /// * `mass` - The mass of the spring (must be positive)
    /// * `stiffness` - The stiffness constant (must be positive)
    /// * `damping` - The damping coefficient (must be non-negative)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// Creates a spring with critical damping
    ///
    /// Critical damping returns to rest as quickly as possible without oscillating.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringDescription, SpringType};
    ///
    /// let spring = SpringDescription::with_critical_damping(1.0, 100.0);
    /// assert_eq!(spring.spring_type(), SpringType::Critical);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_critical_damping(mass: f32, stiffness: f32) -> Self {
        let damping = 2.0 * (mass * stiffness).sqrt();
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// Returns the type of this spring
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringDescription, SpringType};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 5.0);
    /// assert_eq!(spring.spring_type(), SpringType::Underdamped);
    /// ```
    #[inline]
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        let critical_damping = 2.0 * (self.mass * self.stiffness).sqrt();
        let damping_ratio = self.damping / critical_damping;

        if (damping_ratio - 1.0).abs() < 0.001 {
            SpringType::Critical
        } else if damping_ratio < 1.0 {
            SpringType::Underdamped
        } else {
            SpringType::Overdamped
        }
    }

    /// Returns the damping ratio (ζ)
    ///
    /// - ζ < 1: Underdamped (oscillates)
    /// - ζ = 1: Critically damped
    /// - ζ > 1: Overdamped (slow return)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let ratio = spring.damping_ratio();
    /// assert!(ratio < 1.0); // Underdamped
    /// ```
    #[inline]
    #[must_use]
    pub fn damping_ratio(&self) -> f32 {
        let critical_damping = 2.0 * (self.mass * self.stiffness).sqrt();
        self.damping / critical_damping
    }

    /// Returns the natural frequency (ω₀) in radians per second
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let freq = spring.natural_frequency();
    /// assert_eq!(freq, 10.0); // sqrt(100/1) = 10
    /// ```
    #[inline]
    #[must_use]
    pub fn natural_frequency(&self) -> f32 {
        (self.stiffness / self.mass).sqrt()
    }

    /// Returns the damped frequency (ωd) in radians per second
    ///
    /// This is the actual oscillation frequency for underdamped springs.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 5.0);
    /// let freq = spring.damped_frequency();
    /// assert!(freq > 0.0 && freq < spring.natural_frequency());
    /// ```
    #[inline]
    #[must_use]
    pub fn damped_frequency(&self) -> f32 {
        let w0 = self.natural_frequency();
        let zeta = self.damping_ratio();
        w0 * (1.0 - zeta * zeta).max(0.0).sqrt()
    }

    /// Returns the period of oscillation for underdamped springs
    ///
    /// Returns None for critically damped or overdamped springs.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 5.0);
    /// let period = spring.period();
    /// assert!(period.is_some());
    /// ```
    #[must_use]
    pub fn period(&self) -> Option<f32> {
        let wd = self.damped_frequency();
        if wd > 0.0 {
            Some(2.0 * std::f32::consts::PI / wd)
        } else {
            None
        }
    }

    /// Checks if all spring parameters are valid
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let valid = SpringDescription::new(1.0, 100.0, 10.0);
    /// assert!(valid.is_valid());
    ///
    /// let invalid = SpringDescription::new(-1.0, 100.0, 10.0);
    /// assert!(!invalid.is_valid());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.mass > 0.0
            && self.stiffness > 0.0
            && self.damping >= 0.0
            && self.mass.is_finite()
            && self.stiffness.is_finite()
            && self.damping.is_finite()
    }

    /// Returns the critical damping coefficient for this mass and stiffness
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::SpringDescription;
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let critical = spring.critical_damping();
    /// assert_eq!(critical, 20.0); // 2 * sqrt(1 * 100)
    /// ```
    #[inline]
    #[must_use]
    pub fn critical_damping(&self) -> f32 {
        2.0 * (self.mass * self.stiffness).sqrt()
    }
}

/// A spring simulation
///
/// Similar to Flutter's `SpringSimulation`. Simulates a spring moving
/// from a start position to an end position with a given initial velocity.
///
/// # Examples
///
/// ```
/// use flui_types::physics::{SpringSimulation, SpringDescription, Simulation};
///
/// let spring = SpringDescription::new(1.0, 100.0, 10.0);
/// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
///
/// // Get position at t=0.1 seconds
/// let pos = sim.position(0.1);
/// let vel = sim.velocity(0.1);
///
/// assert!(pos > 0.0); // Moving towards end
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpringSimulation {
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
    /// Creates a new spring simulation
    ///
    /// # Arguments
    ///
    /// * `spring` - The spring description
    /// * `start` - The starting position
    /// * `end` - The ending position (equilibrium point)
    /// * `velocity` - The initial velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            spring,
            start,
            end,
            initial_velocity: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Creates a new spring simulation with a custom tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription, Tolerance};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let tolerance = Tolerance::new(0.01, 0.1, 0.01);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0)
    ///     .with_tolerance(tolerance);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the spring description
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
    /// assert_eq!(sim.spring().mass, 1.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn spring(&self) -> &SpringDescription {
        &self.spring
    }

    /// Returns the starting position
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 10.0, 100.0, 0.0);
    /// assert_eq!(sim.start(), 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn start(&self) -> f32 {
        self.start
    }

    /// Returns the ending position (equilibrium point)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
    /// assert_eq!(sim.end(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn end(&self) -> f32 {
        self.end
    }

    /// Returns the initial velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 50.0);
    /// assert_eq!(sim.initial_velocity(), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn initial_velocity(&self) -> f32 {
        self.initial_velocity
    }

    /// Checks if the simulation parameters are valid
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{SpringSimulation, SpringDescription};
    ///
    /// let spring = SpringDescription::new(1.0, 100.0, 10.0);
    /// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
    /// assert!(sim.is_valid());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.spring.is_valid()
            && self.start.is_finite()
            && self.end.is_finite()
            && self.initial_velocity.is_finite()
            && self.tolerance.is_valid()
    }

    /// Calculate position for an underdamped spring
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
    fn position_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = ((s * a - self.initial_velocity) / (s - r), (self.initial_velocity - r * a) / (s - r));

        self.end + b.0 * (r * time).exp() + b.1 * (s * time).exp()
    }

    /// Calculate velocity for an overdamped spring
    fn velocity_overdamped(&self, time: f32) -> f32 {
        let m = self.spring.mass;
        let k = self.spring.stiffness;
        let c = self.spring.damping;

        let w0 = (k / m).sqrt();
        let zeta = c / (2.0 * (m * k).sqrt());
        let r = -w0 * (zeta - (zeta * zeta - 1.0).sqrt());
        let s = -w0 * (zeta + (zeta * zeta - 1.0).sqrt());

        let a = self.start - self.end;
        let b = ((s * a - self.initial_velocity) / (s - r), (self.initial_velocity - r * a) / (s - r));

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

        (pos - self.end).abs() < self.tolerance.distance
            && vel.abs() < self.tolerance.velocity
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spring_description_new() {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        assert_eq!(spring.mass, 1.0);
        assert_eq!(spring.stiffness, 100.0);
        assert_eq!(spring.damping, 10.0);
    }

    #[test]
    fn test_spring_description_critical_damping() {
        let spring = SpringDescription::with_critical_damping(1.0, 100.0);
        assert_eq!(spring.spring_type(), SpringType::Critical);
    }

    #[test]
    fn test_spring_type_underdamped() {
        let spring = SpringDescription::new(1.0, 100.0, 5.0);
        assert_eq!(spring.spring_type(), SpringType::Underdamped);
    }

    #[test]
    fn test_spring_type_overdamped() {
        let spring = SpringDescription::new(1.0, 100.0, 30.0);
        assert_eq!(spring.spring_type(), SpringType::Overdamped);
    }

    #[test]
    fn test_spring_simulation_new() {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

        assert_eq!(sim.position(0.0), 0.0);
        assert_eq!(sim.velocity(0.0), 0.0);
    }

    #[test]
    fn test_spring_simulation_moves_toward_end() {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

        let pos_at_0_1 = sim.position(0.1);
        assert!(pos_at_0_1 > 0.0 && pos_at_0_1 < 100.0);
    }

    #[test]
    fn test_spring_simulation_eventually_settles() {
        let spring = SpringDescription::new(1.0, 100.0, 20.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

        // After long enough time, should be done
        assert!(sim.is_done(10.0));
    }

    #[test]
    fn test_spring_simulation_critical() {
        let spring = SpringDescription::with_critical_damping(1.0, 100.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

        let pos = sim.position(0.1);
        assert!(pos > 0.0 && pos < 100.0);
    }

    #[test]
    fn test_spring_simulation_overdamped() {
        let spring = SpringDescription::new(1.0, 100.0, 30.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

        let pos = sim.position(0.1);
        assert!(pos > 0.0 && pos < 100.0);
    }

    #[test]
    fn test_spring_simulation_with_initial_velocity() {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 50.0);

        let vel = sim.velocity(0.0);
        assert_eq!(vel, 50.0);
    }

    #[test]
    fn test_spring_simulation_tolerance() {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let tolerance = Tolerance::new(0.01, 0.1, 0.01);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0).with_tolerance(tolerance);

        assert_eq!(sim.tolerance(), tolerance);
    }
}
