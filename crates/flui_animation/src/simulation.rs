//! Physics simulations for animation.
//!
//! This module provides simulation types for physics-based animations,
//! including spring physics. Simulations model objects in one-dimensional
//! space with forces applied.
//!
//! # Example
//!
//! ```
//! use flui_animation::simulation::{Simulation, SpringSimulation, SpringDescription, Tolerance};
//!
//! // Create a spring with damping ratio
//! let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
//!
//! // Create simulation from position 0 to 1 with initial velocity 0
//! let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
//!
//! // Query position and velocity at time t
//! let position = sim.x(0.1);
//! let velocity = sim.dx(0.1);
//! let done = sim.is_done(0.1);
//! ```

use std::f32::consts::PI;

/// Tolerance for determining when simulations are "done".
///
/// Specifies maximum allowable magnitudes for distances and velocities
/// to be considered at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Maximum distance from target to be considered "at rest".
    pub distance: f32,
    /// Maximum velocity to be considered "at rest".
    pub velocity: f32,
    /// Maximum time difference to be considered equal.
    pub time: f32,
}

impl Tolerance {
    /// Default tolerance with all values at 0.001.
    pub const DEFAULT: Tolerance = Tolerance {
        distance: 1e-3,
        velocity: 1e-3,
        time: 1e-3,
    };

    /// Create a new tolerance with custom values.
    #[must_use]
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A physics simulation in one-dimensional space.
///
/// Simulations model an object with position and velocity, subject to forces.
/// They expose:
/// - Position via [`x()`](Simulation::x)
/// - Velocity via [`dx()`](Simulation::dx)
/// - Completion state via [`is_done()`](Simulation::is_done)
pub trait Simulation: Send + Sync {
    /// The position of the object at the given time.
    fn x(&self, time: f32) -> f32;

    /// The velocity of the object at the given time.
    fn dx(&self, time: f32) -> f32;

    /// Whether the simulation is "done" at the given time.
    ///
    /// Typically returns true when the object has come to rest
    /// within the specified tolerance.
    fn is_done(&self, time: f32) -> bool;

    /// The tolerance used to determine when the simulation is done.
    fn tolerance(&self) -> Tolerance;
}

/// Description of a spring's physical properties.
///
/// Used to configure [`SpringSimulation`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringDescription {
    /// The mass of the spring (m).
    pub mass: f32,
    /// The spring constant / stiffness (k).
    pub stiffness: f32,
    /// The damping coefficient (c).
    pub damping: f32,
}

impl SpringDescription {
    /// Creates a spring with explicit mass, stiffness, and damping.
    ///
    /// # Panics
    /// Panics if mass or stiffness is not positive, or if damping is negative.
    #[must_use]
    pub fn new(mass: f32, stiffness: f32, damping: f32) -> Self {
        assert!(mass > 0.0, "Mass must be positive");
        assert!(stiffness > 0.0, "Stiffness must be positive");
        assert!(damping >= 0.0, "Damping must be non-negative");
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// Creates a spring given mass, stiffness, and damping ratio.
    ///
    /// The damping ratio describes oscillation decay:
    /// - `ratio = 1.0`: critically damped (no oscillation, fastest settling)
    /// - `ratio > 1.0`: overdamped (slow, no oscillation)
    /// - `ratio < 1.0`: underdamped (oscillates before settling)
    ///
    /// # Panics
    /// Panics if mass or stiffness is not positive, or if ratio is negative.
    #[must_use]
    pub fn with_damping_ratio(mass: f32, stiffness: f32, ratio: f32) -> Self {
        assert!(mass > 0.0, "Mass must be positive");
        assert!(stiffness > 0.0, "Stiffness must be positive");
        assert!(ratio >= 0.0, "Damping ratio must be non-negative");
        let damping = ratio * 2.0 * (mass * stiffness).sqrt();
        Self {
            mass,
            stiffness,
            damping,
        }
    }

    /// Creates a spring based on desired animation duration and bounce.
    ///
    /// # Arguments
    /// * `duration_secs` - Perceptual duration of the spring animation
    /// * `bounce` - Bounciness: 0 = critically damped, 0..1 = bouncy, <0 = overdamped
    #[must_use]
    pub fn with_duration_and_bounce(duration_secs: f32, bounce: f32) -> Self {
        const MASS: f32 = 1.0;

        debug_assert!(duration_secs > 0.0, "Duration must be positive");

        let stiffness = (4.0 * PI * PI * MASS) / (duration_secs * duration_secs);
        let damping_ratio = if bounce > 0.0 {
            1.0 - bounce
        } else {
            1.0 / (bounce + 1.0)
        };
        let damping = damping_ratio * 2.0 * (MASS * stiffness).sqrt();

        Self {
            mass: MASS,
            stiffness,
            damping,
        }
    }

    /// Returns the damping ratio of this spring.
    ///
    /// - `1.0`: critically damped
    /// - `> 1.0`: overdamped
    /// - `< 1.0`: underdamped
    #[must_use]
    pub fn damping_ratio(&self) -> f32 {
        self.damping / (2.0 * (self.mass * self.stiffness).sqrt())
    }

    /// Returns the bounce value (inverse of damping ratio mapping).
    #[must_use]
    pub fn bounce(&self) -> f32 {
        let ratio = self.damping_ratio();
        if ratio < 1.0 {
            1.0 - ratio
        } else {
            (1.0 / ratio) - 1.0
        }
    }

    /// Returns the type of spring based on damping.
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        let discriminant = self.damping * self.damping - 4.0 * self.mass * self.stiffness;
        if discriminant > 0.0 {
            SpringType::Overdamped
        } else if discriminant < 0.0 {
            SpringType::Underdamped
        } else {
            SpringType::CriticallyDamped
        }
    }
}

/// The type of spring behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpringType {
    /// A spring that does not bounce and returns to rest in the shortest time.
    CriticallyDamped,
    /// A spring that bounces (oscillates) before settling.
    Underdamped,
    /// A spring that does not bounce but takes longer to settle than critically damped.
    Overdamped,
}

/// A spring physics simulation.
///
/// Models a particle attached to a spring following Hooke's law.
#[derive(Debug, Clone)]
pub struct SpringSimulation {
    end_position: f32,
    solution: SpringSolution,
    tolerance: Tolerance,
    snap_to_end: bool,
}

impl SpringSimulation {
    /// Creates a new spring simulation.
    ///
    /// # Arguments
    /// * `spring` - The spring's physical properties
    /// * `start` - Starting position
    /// * `end` - Target/end position
    /// * `velocity` - Initial velocity
    #[must_use]
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> Self {
        Self {
            end_position: end,
            solution: SpringSolution::new(spring, start - end, velocity),
            tolerance: Tolerance::DEFAULT,
            snap_to_end: false,
        }
    }

    /// Creates a spring simulation with custom tolerance.
    #[must_use]
    pub fn with_tolerance(
        spring: SpringDescription,
        start: f32,
        end: f32,
        velocity: f32,
        tolerance: Tolerance,
    ) -> Self {
        Self {
            end_position: end,
            solution: SpringSolution::new(spring, start - end, velocity),
            tolerance,
            snap_to_end: false,
        }
    }

    /// Enables snapping to the exact end position when done.
    ///
    /// When enabled, [`x()`](Self::x) returns exactly `end` and [`dx()`](Self::dx)
    /// returns 0 once [`is_done()`](Self::is_done) is true.
    #[must_use]
    pub fn with_snap_to_end(mut self, snap: bool) -> Self {
        self.snap_to_end = snap;
        self
    }

    /// Returns the type of spring behavior.
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        self.solution.spring_type()
    }

    /// Returns the target end position.
    #[must_use]
    pub fn end_position(&self) -> f32 {
        self.end_position
    }
}

impl Simulation for SpringSimulation {
    fn x(&self, time: f32) -> f32 {
        if self.snap_to_end && self.is_done(time) {
            self.end_position
        } else {
            self.end_position + self.solution.x(time)
        }
    }

    fn dx(&self, time: f32) -> f32 {
        if self.snap_to_end && self.is_done(time) {
            0.0
        } else {
            self.solution.dx(time)
        }
    }

    fn is_done(&self, time: f32) -> bool {
        near_zero(self.solution.x(time), self.tolerance.distance)
            && near_zero(self.solution.dx(time), self.tolerance.velocity)
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

// Internal spring solution variants
#[derive(Debug, Clone)]
enum SpringSolution {
    Critical(CriticalSolution),
    Overdamped(OverdampedSolution),
    Underdamped(UnderdampedSolution),
}

impl SpringSolution {
    fn new(spring: SpringDescription, initial_position: f32, initial_velocity: f32) -> Self {
        let discriminant = spring.damping * spring.damping - 4.0 * spring.mass * spring.stiffness;

        if discriminant > 0.0 {
            SpringSolution::Overdamped(OverdampedSolution::new(
                spring,
                initial_position,
                initial_velocity,
            ))
        } else if discriminant < 0.0 {
            SpringSolution::Underdamped(UnderdampedSolution::new(
                spring,
                initial_position,
                initial_velocity,
            ))
        } else {
            SpringSolution::Critical(CriticalSolution::new(
                spring,
                initial_position,
                initial_velocity,
            ))
        }
    }

    fn x(&self, time: f32) -> f32 {
        match self {
            SpringSolution::Critical(s) => s.x(time),
            SpringSolution::Overdamped(s) => s.x(time),
            SpringSolution::Underdamped(s) => s.x(time),
        }
    }

    fn dx(&self, time: f32) -> f32 {
        match self {
            SpringSolution::Critical(s) => s.dx(time),
            SpringSolution::Overdamped(s) => s.dx(time),
            SpringSolution::Underdamped(s) => s.dx(time),
        }
    }

    fn spring_type(&self) -> SpringType {
        match self {
            SpringSolution::Critical(_) => SpringType::CriticallyDamped,
            SpringSolution::Overdamped(_) => SpringType::Overdamped,
            SpringSolution::Underdamped(_) => SpringType::Underdamped,
        }
    }
}

/// Critically damped spring solution.
#[derive(Debug, Clone)]
struct CriticalSolution {
    r: f32,
    c1: f32,
    c2: f32,
}

impl CriticalSolution {
    fn new(spring: SpringDescription, distance: f32, velocity: f32) -> Self {
        let r = -spring.damping / (2.0 * spring.mass);
        let c1 = distance;
        let c2 = velocity - (r * distance);
        Self { r, c1, c2 }
    }

    fn x(&self, time: f32) -> f32 {
        (self.c1 + self.c2 * time) * (self.r * time).exp()
    }

    fn dx(&self, time: f32) -> f32 {
        let power = (self.r * time).exp();
        self.r * (self.c1 + self.c2 * time) * power + self.c2 * power
    }
}

/// Overdamped spring solution.
#[derive(Debug, Clone)]
struct OverdampedSolution {
    r1: f32,
    r2: f32,
    c1: f32,
    c2: f32,
}

impl OverdampedSolution {
    fn new(spring: SpringDescription, distance: f32, velocity: f32) -> Self {
        let cmk = spring.damping * spring.damping - 4.0 * spring.mass * spring.stiffness;
        let r1 = (-spring.damping - cmk.sqrt()) / (2.0 * spring.mass);
        let r2 = (-spring.damping + cmk.sqrt()) / (2.0 * spring.mass);
        let c2 = (velocity - r1 * distance) / (r2 - r1);
        let c1 = distance - c2;
        Self { r1, r2, c1, c2 }
    }

    fn x(&self, time: f32) -> f32 {
        self.c1 * (self.r1 * time).exp() + self.c2 * (self.r2 * time).exp()
    }

    fn dx(&self, time: f32) -> f32 {
        self.c1 * self.r1 * (self.r1 * time).exp() + self.c2 * self.r2 * (self.r2 * time).exp()
    }
}

/// Underdamped spring solution.
#[derive(Debug, Clone)]
struct UnderdampedSolution {
    w: f32,
    r: f32,
    c1: f32,
    c2: f32,
}

impl UnderdampedSolution {
    fn new(spring: SpringDescription, distance: f32, velocity: f32) -> Self {
        let w = (4.0 * spring.mass * spring.stiffness - spring.damping * spring.damping).sqrt()
            / (2.0 * spring.mass);
        let r = -(spring.damping / (2.0 * spring.mass));
        let c1 = distance;
        let c2 = (velocity - r * distance) / w;
        Self { w, r, c1, c2 }
    }

    fn x(&self, time: f32) -> f32 {
        (self.r * time).exp() * (self.c1 * (self.w * time).cos() + self.c2 * (self.w * time).sin())
    }

    fn dx(&self, time: f32) -> f32 {
        let power = (self.r * time).exp();
        let cosine = (self.w * time).cos();
        let sine = (self.w * time).sin();
        power * (self.c2 * self.w * cosine - self.c1 * self.w * sine)
            + self.r * power * (self.c2 * sine + self.c1 * cosine)
    }
}

/// Checks if a value is near zero within the given threshold.
#[inline]
fn near_zero(value: f32, threshold: f32) -> bool {
    value.abs() < threshold
}

/// A friction simulation that slows an object by a constant deceleration.
#[derive(Debug, Clone)]
pub struct FrictionSimulation {
    drag: f32,
    drag_log: f32,
    initial_position: f32,
    initial_velocity: f32,
    tolerance: Tolerance,
}

impl FrictionSimulation {
    /// Creates a new friction simulation.
    ///
    /// # Arguments
    /// * `drag` - Drag coefficient (must be > 0 and != 1.0, typically around 0.01-0.1)
    /// * `position` - Initial position
    /// * `velocity` - Initial velocity
    ///
    /// # Panics
    /// Panics if drag is not positive or if drag equals 1.0 (which would cause division by zero).
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        assert!(drag > 0.0, "Drag must be positive");
        assert!(
            (drag - 1.0).abs() > 1e-6,
            "Drag cannot be 1.0 (causes division by zero)"
        );
        Self {
            drag,
            drag_log: drag.ln(),
            initial_position: position,
            initial_velocity: velocity,
            tolerance: Tolerance::DEFAULT,
        }
    }

    /// Creates a friction simulation with custom tolerance.
    ///
    /// # Panics
    /// Panics if drag is not positive or if drag equals 1.0.
    #[must_use]
    pub fn with_tolerance(drag: f32, position: f32, velocity: f32, tolerance: Tolerance) -> Self {
        assert!(drag > 0.0, "Drag must be positive");
        assert!(
            (drag - 1.0).abs() > 1e-6,
            "Drag cannot be 1.0 (causes division by zero)"
        );
        Self {
            drag,
            drag_log: drag.ln(),
            initial_position: position,
            initial_velocity: velocity,
            tolerance,
        }
    }

    /// Returns the final resting position.
    #[must_use]
    pub fn final_x(&self) -> f32 {
        self.initial_position - self.initial_velocity / self.drag_log
    }

    /// Returns the time at which the simulation reaches the given position.
    #[must_use]
    pub fn time_at_x(&self, x: f32) -> f32 {
        if (x - self.initial_position).abs() < 1e-6 {
            0.0
        } else {
            ((self.drag_log * (self.initial_position - x) / self.initial_velocity) + 1.0).ln()
                / self.drag_log
        }
    }
}

impl Simulation for FrictionSimulation {
    fn x(&self, time: f32) -> f32 {
        self.initial_position + self.initial_velocity * self.drag.powf(time) / self.drag_log
            - self.initial_velocity / self.drag_log
    }

    fn dx(&self, time: f32) -> f32 {
        self.initial_velocity * self.drag.powf(time)
    }

    fn is_done(&self, time: f32) -> bool {
        self.dx(time).abs() < self.tolerance.velocity
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

/// A gravity simulation with constant acceleration.
#[derive(Debug, Clone)]
pub struct GravitySimulation {
    acceleration: f32,
    initial_position: f32,
    initial_velocity: f32,
    end_position: f32,
    tolerance: Tolerance,
}

impl GravitySimulation {
    /// Creates a new gravity simulation.
    ///
    /// # Arguments
    /// * `acceleration` - Gravitational acceleration (positive = downward if end > start)
    /// * `position` - Initial position
    /// * `velocity` - Initial velocity
    /// * `end` - Target position where simulation ends
    #[must_use]
    pub fn new(acceleration: f32, position: f32, velocity: f32, end: f32) -> Self {
        Self {
            acceleration,
            initial_position: position,
            initial_velocity: velocity,
            end_position: end,
            tolerance: Tolerance::DEFAULT,
        }
    }

    /// Creates a gravity simulation with custom tolerance.
    #[must_use]
    pub fn with_tolerance(
        acceleration: f32,
        position: f32,
        velocity: f32,
        end: f32,
        tolerance: Tolerance,
    ) -> Self {
        Self {
            acceleration,
            initial_position: position,
            initial_velocity: velocity,
            end_position: end,
            tolerance,
        }
    }
}

impl Simulation for GravitySimulation {
    fn x(&self, time: f32) -> f32 {
        self.initial_position + self.initial_velocity * time + 0.5 * self.acceleration * time * time
    }

    fn dx(&self, time: f32) -> f32 {
        self.initial_velocity + self.acceleration * time
    }

    fn is_done(&self, time: f32) -> bool {
        let current = self.x(time);
        // Check if we've passed the end position
        if self.end_position >= self.initial_position {
            current >= self.end_position
        } else {
            current <= self.end_position
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
    fn test_tolerance_default() {
        let tol = Tolerance::DEFAULT;
        assert_eq!(tol.distance, 1e-3);
        assert_eq!(tol.velocity, 1e-3);
        assert_eq!(tol.time, 1e-3);
    }

    #[test]
    fn test_spring_description_damping_ratio() {
        let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
        assert!((spring.damping_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_spring_types() {
        // Critically damped
        let critical = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
        assert_eq!(critical.spring_type(), SpringType::CriticallyDamped);

        // Underdamped (bouncy)
        let underdamped = SpringDescription::with_damping_ratio(1.0, 500.0, 0.5);
        assert_eq!(underdamped.spring_type(), SpringType::Underdamped);

        // Overdamped (slow)
        let overdamped = SpringDescription::with_damping_ratio(1.0, 500.0, 2.0);
        assert_eq!(overdamped.spring_type(), SpringType::Overdamped);
    }

    #[test]
    fn test_spring_simulation_converges() {
        let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);

        // At time 0, should be at start
        assert!((sim.x(0.0) - 0.0).abs() < 0.01);

        // Should converge toward end position
        let late_position = sim.x(1.0);
        assert!((late_position - 1.0).abs() < 0.1);

        // Eventually should be done
        assert!(sim.is_done(2.0));
    }

    #[test]
    fn test_spring_simulation_snap_to_end() {
        let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0).with_snap_to_end(true);

        // When done, should snap to exact end
        if sim.is_done(2.0) {
            assert_eq!(sim.x(2.0), 1.0);
            assert_eq!(sim.dx(2.0), 0.0);
        }
    }

    #[test]
    fn test_underdamped_oscillates() {
        let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 0.3);
        let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);

        // Underdamped spring should overshoot
        let mut found_overshoot = false;
        for i in 1..100 {
            let t = i as f32 * 0.01;
            if sim.x(t) > 1.0 {
                found_overshoot = true;
                break;
            }
        }
        assert!(
            found_overshoot,
            "Underdamped spring should overshoot target"
        );
    }

    #[test]
    fn test_friction_simulation() {
        let sim = FrictionSimulation::new(0.1, 0.0, 100.0);

        // Should start at initial position
        assert!((sim.x(0.0) - 0.0).abs() < 0.01);

        // Velocity should decrease over time
        let v1 = sim.dx(0.0);
        let v2 = sim.dx(1.0);
        assert!(v1.abs() > v2.abs());

        // Should eventually stop
        assert!(sim.is_done(100.0));
    }

    #[test]
    fn test_gravity_simulation() {
        let sim = GravitySimulation::new(9.8, 0.0, 0.0, 100.0);

        // Should start at initial position
        assert_eq!(sim.x(0.0), 0.0);

        // Position should increase with gravity
        assert!(sim.x(1.0) > 0.0);

        // Velocity should increase
        assert!(sim.dx(1.0) > sim.dx(0.0));
    }

    #[test]
    fn test_spring_with_duration_and_bounce() {
        let spring = SpringDescription::with_duration_and_bounce(0.5, 0.0);
        // Should be approximately critically damped
        assert!((spring.damping_ratio() - 1.0).abs() < 0.1);

        let bouncy = SpringDescription::with_duration_and_bounce(0.5, 0.5);
        // Should be underdamped
        assert!(bouncy.damping_ratio() < 1.0);
    }
}
