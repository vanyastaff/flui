//! Physics simulation types
//!
//! This module provides types for simulating physical systems like springs,
//! friction, and gravity. Used primarily for scroll physics and animations.

pub mod friction;
pub mod gravity;
pub mod spring;
pub mod tolerance;

pub use friction::{BoundedFrictionSimulation, FrictionSimulation};
pub use gravity::GravitySimulation;
pub use spring::{SpringDescription, SpringSimulation, SpringType};
pub use tolerance::Tolerance;

/// Base trait for physics simulations
///
/// Similar to Flutter's `Simulation`. Simulations calculate position and velocity
/// over time based on physical laws.
///
/// # Type Safety
/// - All methods are marked `#[must_use]` to prevent accidentally ignoring results
/// - Simulations are immutable - all operations return values without modifying state
///
/// # Performance
/// - Implementors should mark hot-path methods with `#[inline]`
/// - All calculations use stack-allocated values (no heap allocations)
///
/// # Examples
///
/// ```
/// use flui_types::physics::{Simulation, SpringSimulation, SpringDescription};
///
/// let spring = SpringDescription::new(1.0, 100.0, 10.0); // mass, stiffness, damping
/// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0); // start, end, velocity
///
/// // Get position at t=0.1 seconds
/// let pos = sim.position(0.1);
/// let vel = sim.velocity(0.1);
/// ```
pub trait Simulation {
    /// Returns the position at the given time
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{Simulation, FrictionSimulation};
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let pos = sim.position(1.0);
    /// assert!(pos > 0.0);
    /// ```
    #[must_use]
    fn position(&self, time: f32) -> f32;

    /// Returns the velocity at the given time
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{Simulation, FrictionSimulation};
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let vel = sim.velocity(1.0);
    /// assert!(vel < 100.0); // Velocity decreases due to friction
    /// ```
    #[must_use]
    fn velocity(&self, time: f32) -> f32;

    /// Returns whether the simulation is done at the given time
    ///
    /// A simulation is considered done when it has reached a stable state
    /// and won't change significantly anymore.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{Simulation, FrictionSimulation};
    ///
    /// let sim = FrictionSimulation::new(0.5, 0.0, 10.0);
    /// assert!(!sim.is_done(0.0)); // Just started
    /// assert!(sim.is_done(20.0)); // Should be done after enough time
    /// ```
    #[must_use]
    fn is_done(&self, time: f32) -> bool;

    /// Returns the tolerance for this simulation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{Simulation, FrictionSimulation, Tolerance};
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let tolerance = sim.tolerance();
    /// assert_eq!(tolerance, Tolerance::default());
    /// ```
    #[must_use]
    fn tolerance(&self) -> Tolerance {
        Tolerance::default()
    }
}

/// A simulation that clamps another simulation to a range
///
/// Similar to Flutter's `ClampedSimulation`. Wraps another simulation
/// and ensures its output stays within specified bounds.
#[derive(Debug, Clone)]
pub struct ClampedSimulation<S: Simulation> {
    /// The underlying simulation
    pub simulation: S,

    /// The minimum allowed position
    pub min: f32,

    /// The maximum allowed position
    pub max: f32,
}

impl<S: Simulation> ClampedSimulation<S> {
    /// Creates a new clamped simulation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation};
    ///
    /// let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, 0.0, 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn new(simulation: S, min: f32, max: f32) -> Self {
        Self {
            simulation,
            min,
            max,
        }
    }

    /// Returns the minimum bound
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation};
    ///
    /// let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, -10.0, 50.0);
    /// assert_eq!(clamped.min(), -10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn min(&self) -> f32 {
        self.min
    }

    /// Returns the maximum bound
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation};
    ///
    /// let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, -10.0, 50.0);
    /// assert_eq!(clamped.max(), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn max(&self) -> f32 {
        self.max
    }

    /// Returns a reference to the inner simulation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation};
    ///
    /// let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, -10.0, 50.0);
    /// assert_eq!(clamped.inner().drag(), 0.1);
    /// ```
    #[inline]
    #[must_use]
    pub fn inner(&self) -> &S {
        &self.simulation
    }

    /// Consumes the clamped simulation and returns the inner simulation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation};
    ///
    /// let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, -10.0, 50.0);
    /// let inner = clamped.into_inner();
    /// assert_eq!(inner.drag(), 0.1);
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> S {
        self.simulation
    }

    /// Checks if currently at a boundary
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{ClampedSimulation, FrictionSimulation, Simulation};
    ///
    /// let friction = FrictionSimulation::new(0.05, 10.0, 100.0);
    /// let clamped = ClampedSimulation::new(friction, 0.0, 50.0);
    /// assert!(!clamped.is_at_boundary(0.0)); // Starts at 10.0, not at boundary
    /// assert!(clamped.is_at_boundary(10.0)); // Eventually hits boundary at 50.0
    /// ```
    #[inline]
    #[must_use]
    pub fn is_at_boundary(&self, time: f32) -> bool {
        let unclamped_pos = self.simulation.position(time);
        unclamped_pos <= self.min || unclamped_pos >= self.max
    }
}

impl<S: Simulation> Simulation for ClampedSimulation<S> {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        self.simulation.position(time).clamp(self.min, self.max)
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        if self.is_at_boundary(time) {
            0.0
        } else {
            self.simulation.velocity(time)
        }
    }

    #[inline]
    fn is_done(&self, time: f32) -> bool {
        self.simulation.is_done(time)
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        self.simulation.tolerance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::FrictionSimulation;

    #[test]
    fn test_clamped_simulation() {
        let friction = FrictionSimulation::new(0.1, 0.0, 100.0);
        let clamped = ClampedSimulation::new(friction, -10.0, 50.0);

        // Position should be clamped
        let pos = clamped.position(1.0);
        assert!(pos >= -10.0 && pos <= 50.0);

        // Velocity at boundary should be zero
        let vel_at_max = clamped.velocity(5.0);
        // After long time, friction simulation stops
        assert_eq!(vel_at_max, 0.0);
    }
}
