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
    #[must_use]
    fn position(&self, time: f32) -> f32;

    #[must_use]
    fn velocity(&self, time: f32) -> f32;

    #[must_use]
    fn is_done(&self, time: f32) -> bool;

    #[must_use]
    fn tolerance(&self) -> Tolerance {
        Tolerance::default()
    }
}

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
    #[must_use]
    pub fn new(simulation: S, min: f32, max: f32) -> Self {
        Self {
            simulation,
            min,
            max,
        }
    }

    #[must_use]
    pub fn min(&self) -> f32 {
        self.min
    }

    #[must_use]
    pub fn max(&self) -> f32 {
        self.max
    }

    #[must_use]
    pub fn inner(&self) -> &S {
        &self.simulation
    }

    #[must_use]
    pub fn into_inner(self) -> S {
        self.simulation
    }

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

