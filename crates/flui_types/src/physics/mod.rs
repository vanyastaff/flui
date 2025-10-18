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
    fn position(&self, time: f32) -> f32;

    /// Returns the velocity at the given time
    fn velocity(&self, time: f32) -> f32;

    /// Returns whether the simulation is done at the given time
    ///
    /// A simulation is considered done when it has reached a stable state
    /// and won't change significantly anymore.
    fn is_done(&self, time: f32) -> bool;

    /// Returns the tolerance for this simulation
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
    pub fn new(simulation: S, min: f32, max: f32) -> Self {
        Self {
            simulation,
            min,
            max,
        }
    }
}

impl<S: Simulation> Simulation for ClampedSimulation<S> {
    fn position(&self, time: f32) -> f32 {
        self.simulation.position(time).clamp(self.min, self.max)
    }

    fn velocity(&self, time: f32) -> f32 {
        let position = self.simulation.position(time);
        if position < self.min || position > self.max {
            // If we're at the boundary, velocity is zero
            0.0
        } else {
            self.simulation.velocity(time)
        }
    }

    fn is_done(&self, time: f32) -> bool {
        self.simulation.is_done(time)
    }

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
