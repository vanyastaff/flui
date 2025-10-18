//! Friction-based physics simulations
//!
//! This module provides simulations that model friction and deceleration.

use super::{Simulation, Tolerance};

/// A simulation that applies friction to slow down motion
///
/// Similar to Flutter's `FrictionSimulation`. Models an object slowing
/// down due to friction, using exponential decay.
///
/// # Examples
///
/// ```
/// use flui_types::physics::{FrictionSimulation, Simulation};
///
/// // Create a friction simulation
/// // drag: 0.1, start position: 0.0, initial velocity: 100.0
/// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
///
/// // Get position and velocity at t=1.0 seconds
/// let pos = sim.position(1.0);
/// let vel = sim.velocity(1.0);
///
/// assert!(pos > 0.0); // Object has moved forward
/// assert!(vel < 100.0); // Velocity has decreased
/// assert!(vel > 0.0); // Still moving
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrictionSimulation {
    /// The drag coefficient (higher = more friction)
    drag: f32,

    /// The starting position
    position_at_zero: f32,

    /// The initial velocity in pixels per second
    velocity_at_zero: f32,

    /// The tolerance for this simulation
    tolerance: Tolerance,
}

impl FrictionSimulation {
    /// Creates a new friction simulation
    ///
    /// # Arguments
    ///
    /// * `drag` - The drag coefficient (must be positive)
    /// * `position` - The starting position
    /// * `velocity` - The initial velocity in pixels per second
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// ```
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        Self {
            drag,
            position_at_zero: position,
            velocity_at_zero: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Creates a new friction simulation with a custom tolerance
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the final position where the object will stop
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let final_pos = sim.final_position();
    /// assert!(final_pos > 0.0);
    /// ```
    pub fn final_position(&self) -> f32 {
        self.position_at_zero + self.velocity_at_zero / self.drag
    }
}

impl Simulation for FrictionSimulation {
    fn position(&self, time: f32) -> f32 {
        self.position_at_zero
            + self.velocity_at_zero * (1.0 - (-self.drag * time).exp()) / self.drag
    }

    fn velocity(&self, time: f32) -> f32 {
        self.velocity_at_zero * (-self.drag * time).exp()
    }

    fn is_done(&self, time: f32) -> bool {
        self.velocity(time).abs() < self.tolerance.velocity
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

/// A friction simulation that stops at a boundary
///
/// Similar to Flutter's `BoundedFrictionSimulation`. Wraps a friction
/// simulation and ensures it doesn't go past a specified boundary.
///
/// # Examples
///
/// ```
/// use flui_types::physics::{BoundedFrictionSimulation, Simulation};
///
/// // Create a bounded friction simulation
/// // Position will stop at boundary of 50.0
/// let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
///
/// // Even after a long time, position won't exceed boundary
/// let pos = sim.position(10.0);
/// assert!(pos <= 50.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Creates a new bounded friction simulation
    ///
    /// # Arguments
    ///
    /// * `drag` - The drag coefficient
    /// * `position` - The starting position
    /// * `velocity` - The initial velocity
    /// * `boundary` - The boundary position that cannot be exceeded
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::BoundedFrictionSimulation;
    ///
    /// let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
    /// ```
    pub fn new(drag: f32, position: f32, velocity: f32, boundary: f32) -> Self {
        Self {
            friction: FrictionSimulation::new(drag, position, velocity),
            boundary,
            positive_direction: velocity > 0.0,
        }
    }

    /// Creates a new bounded friction simulation with a custom tolerance
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.friction = self.friction.with_tolerance(tolerance);
        self
    }
}

impl Simulation for BoundedFrictionSimulation {
    fn position(&self, time: f32) -> f32 {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos.min(self.boundary)
        } else {
            pos.max(self.boundary)
        }
    }

    fn velocity(&self, time: f32) -> f32 {
        let pos = self.friction.position(time);
        let at_boundary = if self.positive_direction {
            pos >= self.boundary
        } else {
            pos <= self.boundary
        };

        if at_boundary {
            0.0
        } else {
            self.friction.velocity(time)
        }
    }

    fn is_done(&self, time: f32) -> bool {
        let pos = self.friction.position(time);
        let at_boundary = if self.positive_direction {
            pos >= self.boundary
        } else {
            pos <= self.boundary
        };

        at_boundary || self.friction.is_done(time)
    }

    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_friction_simulation_new() {
        let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
        assert_eq!(sim.position(0.0), 0.0);
        assert_eq!(sim.velocity(0.0), 100.0);
    }

    #[test]
    fn test_friction_simulation_slows_down() {
        let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
        let vel_at_1 = sim.velocity(1.0);
        let vel_at_2 = sim.velocity(2.0);

        assert!(vel_at_1 < 100.0);
        assert!(vel_at_2 < vel_at_1);
    }

    #[test]
    fn test_friction_simulation_final_position() {
        let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
        let final_pos = sim.final_position();

        // After a very long time, position should approach final_position
        let pos_at_100 = sim.position(100.0);
        assert!((pos_at_100 - final_pos).abs() < 0.1);
    }

    #[test]
    fn test_friction_simulation_is_done() {
        let sim = FrictionSimulation::new(0.5, 0.0, 10.0);

        assert!(!sim.is_done(0.0));
        assert!(sim.is_done(20.0)); // Should be done after long enough time
    }

    #[test]
    fn test_friction_simulation_tolerance() {
        let tolerance = Tolerance::new(0.01, 0.1, 0.01);
        let sim = FrictionSimulation::new(0.1, 0.0, 100.0).with_tolerance(tolerance);

        assert_eq!(sim.tolerance(), tolerance);
    }

    #[test]
    fn test_bounded_friction_simulation_new() {
        let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
        assert_eq!(sim.position(0.0), 0.0);
        assert_eq!(sim.velocity(0.0), 100.0);
    }

    #[test]
    fn test_bounded_friction_simulation_stops_at_boundary() {
        let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);

        // Position should never exceed boundary
        for i in 0..100 {
            let time = i as f32 * 0.1;
            let pos = sim.position(time);
            assert!(pos <= 50.0, "Position {} exceeded boundary at time {}", pos, time);
        }
    }

    #[test]
    fn test_bounded_friction_simulation_velocity_zero_at_boundary() {
        let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);

        // After long enough time, should be at boundary with zero velocity
        let pos = sim.position(10.0);
        let vel = sim.velocity(10.0);

        if pos >= 50.0 {
            assert_eq!(vel, 0.0);
        }
    }

    #[test]
    fn test_bounded_friction_simulation_negative_direction() {
        let sim = BoundedFrictionSimulation::new(0.1, 0.0, -100.0, -50.0);

        // Position should never go below boundary
        for i in 0..100 {
            let time = i as f32 * 0.1;
            let pos = sim.position(time);
            assert!(pos >= -50.0, "Position {} went below boundary at time {}", pos, time);
        }
    }

    #[test]
    fn test_bounded_friction_simulation_is_done_at_boundary() {
        let sim = BoundedFrictionSimulation::new(0.05, 0.0, 100.0, 20.0);

        // Should be done when reaching boundary
        assert!(sim.is_done(10.0));
    }
}
