//! Friction-based physics simulations
//!
//! This module provides simulations that model friction and deceleration.

use super::{Simulation, Tolerance};

/// A simulation that applies friction to slow down motion
///
/// Similar to Flutter's `FrictionSimulation`. Models an object slowing
/// down due to friction, using exponential decay.
///
/// # Physics Model
/// - Position: `p(t) = p₀ + v₀ * (1 - e^(-k*t)) / k`
/// - Velocity: `v(t) = v₀ * e^(-k*t)`
/// - Where k is the drag coefficient
///
/// # Memory Safety
/// - Stack-allocated `Copy` type with no heap allocations
/// - All calculations use safe floating-point math
///
/// # Type Safety
/// - `#[must_use]` on all pure methods
/// - Validation methods prevent invalid states
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
    #[inline]
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        Self {
            drag,
            position_at_zero: position,
            velocity_at_zero: velocity,
            tolerance: Tolerance::default(),
        }
    }

    /// Creates a new friction simulation with a custom tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{FrictionSimulation, Tolerance};
    ///
    /// let tolerance = Tolerance::new(0.01, 0.1, 0.01);
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0)
    ///     .with_tolerance(tolerance);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// Returns the drag coefficient
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// assert_eq!(sim.drag(), 0.1);
    /// ```
    #[inline]
    #[must_use]
    pub fn drag(&self) -> f32 {
        self.drag
    }

    /// Returns the starting position
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 10.0, 100.0);
    /// assert_eq!(sim.start_position(), 10.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn start_position(&self) -> f32 {
        self.position_at_zero
    }

    /// Returns the initial velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// assert_eq!(sim.initial_velocity(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn initial_velocity(&self) -> f32 {
        self.velocity_at_zero
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
    #[inline]
    #[must_use]
    pub fn final_position(&self) -> f32 {
        self.position_at_zero + self.velocity_at_zero / self.drag
    }

    /// Checks if the simulation parameters are valid
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let valid = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// assert!(valid.is_valid());
    ///
    /// let invalid = FrictionSimulation::new(-0.1, 0.0, 100.0);
    /// assert!(!invalid.is_valid());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.drag > 0.0
            && self.drag.is_finite()
            && self.position_at_zero.is_finite()
            && self.velocity_at_zero.is_finite()
            && self.tolerance.is_valid()
    }

    /// Returns the time required to decelerate to a specific velocity
    ///
    /// Returns None if the target velocity cannot be reached or if parameters are invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let time = sim.time_to_velocity(50.0);
    /// assert!(time.is_some());
    /// ```
    #[must_use]
    pub fn time_to_velocity(&self, target_velocity: f32) -> Option<f32> {
        if self.drag <= 0.0 || target_velocity.abs() > self.velocity_at_zero.abs() {
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

        Some(-ratio.ln() / self.drag)
    }

    /// Returns the distance traveled during deceleration from current velocity to target velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let distance = sim.distance_to_velocity(50.0);
    /// assert!(distance > 0.0);
    /// ```
    #[must_use]
    pub fn distance_to_velocity(&self, target_velocity: f32) -> f32 {
        if let Some(time) = self.time_to_velocity(target_velocity) {
            self.position(time) - self.position_at_zero
        } else {
            self.final_position() - self.position_at_zero
        }
    }

    /// Calculates the deceleration rate at a given time
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::FrictionSimulation;
    ///
    /// let sim = FrictionSimulation::new(0.1, 0.0, 100.0);
    /// let decel = sim.deceleration(1.0);
    /// assert!(decel < 0.0); // Negative acceleration (deceleration)
    /// ```
    #[inline]
    #[must_use]
    pub fn deceleration(&self, time: f32) -> f32 {
        -self.drag * self.velocity(time)
    }
}

impl Simulation for FrictionSimulation {
    #[inline]
    fn position(&self, time: f32) -> f32 {
        self.position_at_zero
            + self.velocity_at_zero * (1.0 - (-self.drag * time).exp()) / self.drag
    }

    #[inline]
    fn velocity(&self, time: f32) -> f32 {
        self.velocity_at_zero * (-self.drag * time).exp()
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
    #[inline]
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32, boundary: f32) -> Self {
        Self {
            friction: FrictionSimulation::new(drag, position, velocity),
            boundary,
            positive_direction: velocity > 0.0,
        }
    }

    /// Creates a new bounded friction simulation with a custom tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{BoundedFrictionSimulation, Tolerance};
    ///
    /// let tolerance = Tolerance::new(0.01, 0.1, 0.01);
    /// let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0)
    ///     .with_tolerance(tolerance);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: Tolerance) -> Self {
        self.friction = self.friction.with_tolerance(tolerance);
        self
    }

    /// Returns the boundary position
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::BoundedFrictionSimulation;
    ///
    /// let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
    /// assert_eq!(sim.boundary(), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn boundary(&self) -> f32 {
        self.boundary
    }

    /// Returns the underlying friction simulation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::BoundedFrictionSimulation;
    ///
    /// let sim = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
    /// let friction = sim.inner();
    /// assert_eq!(friction.drag(), 0.1);
    /// ```
    #[inline]
    #[must_use]
    pub fn inner(&self) -> &FrictionSimulation {
        &self.friction
    }

    /// Checks if the boundary will be hit
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::BoundedFrictionSimulation;
    ///
    /// let sim = BoundedFrictionSimulation::new(0.05, 0.0, 100.0, 50.0);
    /// assert!(sim.will_hit_boundary());
    ///
    /// let sim2 = BoundedFrictionSimulation::new(0.5, 0.0, 10.0, 100.0);
    /// assert!(!sim2.will_hit_boundary());
    /// ```
    #[inline]
    #[must_use]
    pub fn will_hit_boundary(&self) -> bool {
        let final_pos = self.friction.final_position();
        if self.positive_direction {
            final_pos >= self.boundary
        } else {
            final_pos <= self.boundary
        }
    }

    /// Checks if currently at the boundary
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::{BoundedFrictionSimulation, Simulation};
    ///
    /// let sim = BoundedFrictionSimulation::new(0.05, 0.0, 100.0, 50.0);
    /// assert!(!sim.is_at_boundary(0.0));
    /// assert!(sim.is_at_boundary(10.0)); // After enough time
    /// ```
    #[inline]
    #[must_use]
    pub fn is_at_boundary(&self, time: f32) -> bool {
        let pos = self.friction.position(time);
        if self.positive_direction {
            pos >= self.boundary
        } else {
            pos <= self.boundary
        }
    }

    /// Checks if the simulation parameters are valid
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::BoundedFrictionSimulation;
    ///
    /// let valid = BoundedFrictionSimulation::new(0.1, 0.0, 100.0, 50.0);
    /// assert!(valid.is_valid());
    /// ```
    #[inline]
    #[must_use]
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
