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
/// Similar to Flutter's `Simulation`. Simulations calculate position and
/// velocity over time based on physical laws.
///
/// # Type Safety
/// - All methods are marked `#[must_use]` to prevent accidentally ignoring
///   results
/// - Simulations are immutable - all operations return values without modifying
///   state
///
/// # Performance
/// - Implementors should mark hot-path methods with `#[inline]`
/// - All calculations use stack-allocated values (no heap allocations)
///
/// # Examples
///
/// ```
/// use flui_types::physics::{Simulation, SpringDescription, SpringSimulation};
///
/// let spring = SpringDescription::new(1.0, 100.0, 10.0); // mass, stiffness, damping
/// let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0); // start, end, velocity
///
/// // Get position at t=0.1 seconds
/// let pos = sim.position(0.1);
/// let vel = sim.velocity(0.1);
/// ```
pub trait Simulation {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Returns the position at `time` seconds, in logical pixels.
    #[must_use]
    fn position(&self, time: f32) -> f32;

    /// Returns the velocity at `time` seconds, in logical pixels per second.
    #[must_use]
    fn velocity(&self, time: f32) -> f32;

    /// Returns whether the simulation has settled at `time` seconds.
    ///
    /// Once this returns `true` the caller may stop sampling; what "settled"
    /// means (position and/or velocity within [`Tolerance`]) is defined by
    /// each implementation.
    #[must_use]
    fn is_done(&self, time: f32) -> bool;

    /// Returns the tolerance used to decide when this simulation is done.
    ///
    /// Defaults to `Tolerance::default()`.
    #[must_use]
    fn tolerance(&self) -> Tolerance {
        Tolerance::default()
    }
}

/// A simulation that clamps another simulation's position to `[min, max]`.
///
/// Position is clamped on every sample; velocity reports `0.0` whenever the
/// underlying (unclamped) position is at or beyond a boundary, so callers see
/// the object as pinned rather than still moving. `is_done` and `tolerance`
/// delegate to the inner simulation unchanged.
#[derive(Debug, Clone)]
pub struct ClampedSimulation<S: Simulation> {
    // PORT-CHECK-OK-SP3: parallel to flui-animation::simulation::ClampedSimulation; the two physics layers use distinct Simulation traits (position/velocity here vs x/dx + Send+Sync there). Consolidation tracked.
    /// The underlying simulation
    pub simulation: S,

    /// The minimum allowed position
    pub min: f32,

    /// The maximum allowed position
    pub max: f32,
}

impl<S: Simulation> ClampedSimulation<S> {
    /// Creates a clamped simulation wrapping `simulation`, limiting its
    /// position to `[min, max]`.
    #[must_use]
    pub fn new(simulation: S, min: f32, max: f32) -> Self {
        Self {
            simulation,
            min,
            max,
        }
    }

    /// Returns the minimum allowed position, in logical pixels.
    #[must_use]
    pub fn min(&self) -> f32 {
        self.min
    }

    /// Returns the maximum allowed position, in logical pixels.
    #[must_use]
    pub fn max(&self) -> f32 {
        self.max
    }

    /// Returns a reference to the wrapped simulation.
    #[must_use]
    pub fn inner(&self) -> &S {
        &self.simulation
    }

    /// Consumes the wrapper and returns the wrapped simulation.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.simulation
    }

    /// Returns whether the unclamped position at `time` is at or beyond
    /// either boundary.
    ///
    /// When this is `true`, `velocity` reports `0.0`.
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
