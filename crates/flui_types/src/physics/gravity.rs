//! Gravity-based physics simulations
//!
//! This module provides simulations that model motion under gravity.

use super::{Simulation, Tolerance};

/// A simulation of an object moving under gravity
///
/// Similar to Flutter's `GravitySimulation`. Models projectile motion
/// with constant acceleration (like throwing or dropping an object).
///
/// # Examples
///
/// ```
/// use flui_types::physics::{GravitySimulation, Simulation};
///
/// // Create a gravity simulation
/// // acceleration: 9.8 m/s^2, initial position: 0.0, end position: 100.0, initial velocity: 0.0
/// let sim = GravitySimulation::new(9.8, 0.0, 100.0, 0.0);
///
/// // Get position and velocity at t=1.0 seconds
/// let pos = sim.position(1.0);
/// let vel = sim.velocity(1.0);
///
/// assert!(pos > 0.0); // Object has moved
/// assert!(vel > 0.0); // Velocity has increased due to gravity
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Creates a new gravity simulation
    ///
    /// # Arguments
    ///
    /// * `acceleration` - The acceleration due to gravity (positive = downward)
    /// * `start` - The starting position
    /// * `end` - The ending position (where simulation should stop)
    /// * `velocity` - The initial velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::GravitySimulation;
    ///
    /// // Simulate dropping an object
    /// let sim = GravitySimulation::new(9.8, 0.0, 100.0, 0.0);
    /// ```
    pub fn new(acceleration: f32, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            acceleration,
            start,
            end,
            initial_velocity: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Creates a new gravity simulation with a custom tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{GravitySimulation, Tolerance};
    ///
    /// let tolerance = Tolerance::new(0.01, 0.1, 0.01);
    /// let sim = GravitySimulation::new(9.8, 0.0, 100.0, 0.0)
    ///     .with_tolerance(tolerance);
    /// ```
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the time at which the simulation reaches the end position
    ///
    /// Returns None if the object never reaches the end position.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::GravitySimulation;
    ///
    /// let sim = GravitySimulation::new(10.0, 0.0, 100.0, 0.0);
    /// let time = sim.time_at_end();
    ///
    /// assert!(time.is_some());
    /// assert!(time.unwrap() > 0.0);
    /// ```
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
    fn position(&self, time: f32) -> f32 {
        // position = start + velocity*t + 0.5*acceleration*t^2
        self.start + self.initial_velocity * time + 0.5 * self.acceleration * time * time
    }

    fn velocity(&self, time: f32) -> f32 {
        // velocity = initial_velocity + acceleration*t
        self.initial_velocity + self.acceleration * time
    }

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

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gravity_simulation_new() {
        let sim = GravitySimulation::new(9.8, 0.0, 100.0, 0.0);
        assert_eq!(sim.position(0.0), 0.0);
        assert_eq!(sim.velocity(0.0), 0.0);
    }

    #[test]
    fn test_gravity_simulation_accelerates() {
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 0.0);

        let vel_at_1 = sim.velocity(1.0);
        let vel_at_2 = sim.velocity(2.0);

        assert_eq!(vel_at_1, 10.0);
        assert_eq!(vel_at_2, 20.0);
    }

    #[test]
    fn test_gravity_simulation_position() {
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 0.0);

        // At t=1: position = 0 + 0*1 + 0.5*10*1^2 = 5
        let pos_at_1 = sim.position(1.0);
        assert_eq!(pos_at_1, 5.0);

        // At t=2: position = 0 + 0*2 + 0.5*10*2^2 = 20
        let pos_at_2 = sim.position(2.0);
        assert_eq!(pos_at_2, 20.0);
    }

    #[test]
    fn test_gravity_simulation_with_initial_velocity() {
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 20.0);

        // At t=1: position = 0 + 20*1 + 0.5*10*1^2 = 25
        let pos_at_1 = sim.position(1.0);
        assert_eq!(pos_at_1, 25.0);

        // velocity = 20 + 10*1 = 30
        let vel_at_1 = sim.velocity(1.0);
        assert_eq!(vel_at_1, 30.0);
    }

    #[test]
    fn test_gravity_simulation_time_at_end() {
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 0.0);
        let time = sim.time_at_end();

        assert!(time.is_some());
        let t = time.unwrap();

        // Check that position at this time is approximately the end position
        let pos = sim.position(t);
        assert!((pos - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_gravity_simulation_time_at_end_with_velocity() {
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 20.0);
        let time = sim.time_at_end();

        assert!(time.is_some());
        let t = time.unwrap();

        let pos = sim.position(t);
        assert!((pos - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_gravity_simulation_is_done() {
        let sim = GravitySimulation::new(10.0, 0.0, 50.0, 0.0);

        assert!(!sim.is_done(0.0));
        assert!(!sim.is_done(1.0)); // position = 5.0
        assert!(!sim.is_done(2.0)); // position = 20.0
        assert!(sim.is_done(10.0)); // position = 500.0 (way past end)
    }

    #[test]
    fn test_gravity_simulation_tolerance() {
        let tolerance = Tolerance::new(0.01, 0.1, 0.01);
        let sim = GravitySimulation::new(10.0, 0.0, 100.0, 0.0).with_tolerance(tolerance);

        assert_eq!(sim.tolerance(), tolerance);
    }

    #[test]
    fn test_gravity_simulation_negative_acceleration() {
        let sim = GravitySimulation::new(-10.0, 100.0, 0.0, 0.0);

        // Should move backwards
        let pos_at_1 = sim.position(1.0);
        assert!(pos_at_1 < 100.0);

        let vel_at_1 = sim.velocity(1.0);
        assert_eq!(vel_at_1, -10.0);
    }

    #[test]
    fn test_gravity_simulation_no_acceleration() {
        let sim = GravitySimulation::new(0.0, 0.0, 100.0, 10.0);

        // Should move at constant velocity
        let pos_at_1 = sim.position(1.0);
        assert_eq!(pos_at_1, 10.0);

        let pos_at_2 = sim.position(2.0);
        assert_eq!(pos_at_2, 20.0);

        let vel_at_1 = sim.velocity(1.0);
        assert_eq!(vel_at_1, 10.0);
    }
}
