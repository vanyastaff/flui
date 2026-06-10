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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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

    /// Build a spring from Apple's perceptual parameters: `response` is the
    /// natural period in seconds (the time for one undamped oscillation) and
    /// `damping_fraction` is the damping ratio (`1.0` = critically damped / no
    /// overshoot, `< 1.0` = bouncy, `> 1.0` = sluggish).
    ///
    /// This is the parameterization designers reason about and the recommended
    /// way to configure a UI spring — far more intuitive than raw
    /// mass/stiffness/damping. Mirrors SwiftUI's
    /// `spring(response:dampingFraction:)`.
    #[must_use]
    pub fn with_response_and_damping(response: f32, damping_fraction: f32) -> Self {
        const MASS: f32 = 1.0;
        debug_assert!(response > 0.0, "response (natural period) must be positive");
        // ω = 2π/response ; k = ω²·m ; c = 2·ζ·√(k·m)
        let omega = 2.0 * PI / response;
        let stiffness = omega * omega * MASS;
        let damping = 2.0 * damping_fraction * (stiffness * MASS).sqrt();
        Self {
            mass: MASS,
            stiffness,
            damping,
        }
    }

    /// Apple `smooth` preset: a gentle spring with no bounce (response 0.5s).
    #[must_use]
    pub fn smooth() -> Self {
        Self::with_duration_and_bounce(0.5, 0.0)
    }

    /// Apple `snappy` preset: quick with a slight bounce (response 0.5s).
    #[must_use]
    pub fn snappy() -> Self {
        Self::with_duration_and_bounce(0.5, 0.15)
    }

    /// Apple `bouncy` preset: a lively spring with noticeable bounce (response 0.5s).
    #[must_use]
    pub fn bouncy() -> Self {
        Self::with_duration_and_bounce(0.5, 0.3)
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    /// * `drag` - Drag coefficient, in the open range `(0, 1)` (typically ~0.01-0.2)
    /// * `position` - Initial position
    /// * `velocity` - Initial velocity
    ///
    /// # Panics
    /// Panics if `drag` is not in `(0, 1)`. A drag of exactly 1.0 divides by
    /// `ln(1) = 0`; a drag `>= 1` makes the object *accelerate* (`v·drag^t`
    /// grows), so the simulation would never come to rest — a hang, not a slow
    /// stop.
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32) -> Self {
        assert!(
            drag > 0.0 && drag < 1.0,
            "friction drag must be in (0, 1), got {drag}: drag >= 1 never decelerates"
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
    /// Panics if `drag` is not in `(0, 1)` (see [`new`](Self::new)).
    #[must_use]
    pub fn with_tolerance(drag: f32, position: f32, velocity: f32, tolerance: Tolerance) -> Self {
        assert!(
            drag > 0.0 && drag < 1.0,
            "friction drag must be in (0, 1), got {drag}: drag >= 1 never decelerates"
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
            // Solve x(t) = x for t. From `x(t) = x0 + v·(drag^t - 1)/ln(drag)`,
            //   drag^t = ln(drag)·(x - x0)/v + 1, so t = ln(that) / ln(drag).
            // (Previously used `(x0 - x)`, the wrong sign, which returned
            // negative times for forward motion; `time_at_x(final_x)` is `+inf`,
            // the asymptote.)
            ((self.drag_log * (x - self.initial_position) / self.initial_velocity) + 1.0).ln()
                / self.drag_log
        }
    }

    /// Creates a friction simulation that travels from `start_position` to
    /// `end_position`, decelerating from `start_velocity` down to
    /// `end_velocity`.
    ///
    /// This is how scrollables fling to a *specific* resting point (e.g. snapping
    /// to a page boundary): the drag is solved so the object arrives at
    /// `end_position` with `end_velocity`, and the tolerance is set so the
    /// simulation reports done at that velocity. Mirrors Flutter's
    /// `FrictionSimulation.through`.
    ///
    /// # Panics
    /// Panics if the solved drag is not in `(0, 1)` — which happens only for
    /// physically impossible requests (e.g. asking to *speed up* over the span,
    /// or to move opposite the velocity).
    #[must_use]
    pub fn through(
        start_position: f32,
        end_position: f32,
        start_velocity: f32,
        end_velocity: f32,
    ) -> Self {
        // drag = e^((vStart - vEnd) / (xStart - xEnd))  (Flutter's `_dragFor`).
        let drag = std::f32::consts::E
            .powf((start_velocity - end_velocity) / (start_position - end_position));
        Self::with_tolerance(
            drag,
            start_position,
            start_velocity,
            Tolerance {
                velocity: end_velocity.abs(),
                ..Tolerance::DEFAULT
            },
        )
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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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

/// A spring tuned for scroll overscroll: identical to [`SpringSimulation`] but
/// it never snaps to the end, so the spring's natural overshoot is visible —
/// that is exactly the bounce a scrollable shows when dragged past its edge.
/// Mirrors Flutter's `ScrollSpringSimulation`.
#[derive(Debug, Clone)]
pub struct ScrollSpringSimulation {
    spring: SpringSimulation,
}

impl ScrollSpringSimulation {
    /// Creates a scroll spring from `start` toward `end` with an initial `velocity`.
    #[must_use]
    pub fn new(spring: SpringDescription, start: f32, end: f32, velocity: f32) -> Self {
        // `with_snap_to_end` is left at its default (false): overscroll bounce
        // requires the un-snapped value.
        Self {
            spring: SpringSimulation::new(spring, start, end, velocity),
        }
    }
}

impl Simulation for ScrollSpringSimulation {
    fn x(&self, time: f32) -> f32 {
        self.spring.x(time)
    }
    fn dx(&self, time: f32) -> f32 {
        self.spring.dx(time)
    }
    fn is_done(&self, time: f32) -> bool {
        self.spring.is_done(time)
    }
    fn tolerance(&self) -> Tolerance {
        self.spring.tolerance()
    }
}

/// Wraps another simulation, clamping its position to `[x_min, x_max]` and its
/// velocity to `[dx_min, dx_max]`. Mirrors Flutter's `ClampedSimulation`.
pub struct ClampedSimulation {
    inner: Box<dyn Simulation>,
    x_min: f32,
    x_max: f32,
    dx_min: f32,
    dx_max: f32,
}

impl ClampedSimulation {
    /// Wraps `inner`, clamping position to `[x_min, x_max]` and velocity to
    /// `[dx_min, dx_max]`.
    #[must_use]
    pub fn new(
        inner: Box<dyn Simulation>,
        x_min: f32,
        x_max: f32,
        dx_min: f32,
        dx_max: f32,
    ) -> Self {
        Self {
            inner,
            x_min,
            x_max,
            dx_min,
            dx_max,
        }
    }
}

impl std::fmt::Debug for ClampedSimulation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClampedSimulation")
            .field("x_min", &self.x_min)
            .field("x_max", &self.x_max)
            .field("dx_min", &self.dx_min)
            .field("dx_max", &self.dx_max)
            .finish_non_exhaustive()
    }
}

impl Simulation for ClampedSimulation {
    fn x(&self, time: f32) -> f32 {
        self.inner.x(time).clamp(self.x_min, self.x_max)
    }
    fn dx(&self, time: f32) -> f32 {
        self.inner.dx(time).clamp(self.dx_min, self.dx_max)
    }
    fn is_done(&self, time: f32) -> bool {
        self.inner.is_done(time)
    }
    fn tolerance(&self) -> Tolerance {
        self.inner.tolerance()
    }
}

/// A [`FrictionSimulation`] clamped to a position range, finishing when the
/// friction settles *or* the object reaches a bound. Mirrors Flutter's
/// `BoundedFrictionSimulation` — used by scrollables so a fling that overshoots
/// the content extent stops cleanly at the edge.
#[derive(Debug, Clone)]
pub struct BoundedFrictionSimulation {
    friction: FrictionSimulation,
    min_x: f32,
    max_x: f32,
}

impl BoundedFrictionSimulation {
    /// Creates a bounded friction simulation confined to `[min_x, max_x]`.
    ///
    /// # Panics
    /// Panics if `drag` is not in `(0, 1)` (see [`FrictionSimulation::new`]).
    #[must_use]
    pub fn new(drag: f32, position: f32, velocity: f32, min_x: f32, max_x: f32) -> Self {
        Self {
            friction: FrictionSimulation::new(drag, position, velocity),
            min_x,
            max_x,
        }
    }
}

impl Simulation for BoundedFrictionSimulation {
    fn x(&self, time: f32) -> f32 {
        self.friction.x(time).clamp(self.min_x, self.max_x)
    }
    fn dx(&self, time: f32) -> f32 {
        self.friction.dx(time)
    }
    fn is_done(&self, time: f32) -> bool {
        let distance = self.friction.tolerance().distance;
        self.friction.is_done(time)
            || near_zero(self.friction.x(time) - self.min_x, distance)
            || near_zero(self.friction.x(time) - self.max_x, distance)
    }
    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "drag >= 1 never decelerates")]
    fn friction_drag_ge_one_panics_instead_of_hanging() {
        // drag >= 1 would make dx grow forever; construction must reject it.
        let _ = FrictionSimulation::new(1.5, 0.0, 100.0);
    }

    #[test]
    fn friction_settles_with_valid_drag() {
        let sim = FrictionSimulation::new(0.135, 0.0, 1000.0);
        // dx decays toward zero, so is_done eventually holds.
        assert!(sim.is_done(100.0), "friction with drag<1 must come to rest");
    }

    #[test]
    fn friction_through_lands_at_target() {
        // Fling from x=0 (v=800) to rest (v=0) at x=500: the resting (asymptotic)
        // position must be the requested 500.
        let sim = FrictionSimulation::through(0.0, 500.0, 800.0, 0.0);
        assert!((sim.final_x() - 500.0).abs() < 0.5, "final_x={}", sim.final_x());
        // It approaches 500 over time and the (fixed) time_at_x of an
        // intermediate point is positive and finite.
        assert!((sim.x(100.0) - 500.0).abs() < 1.0, "x(100)={}", sim.x(100.0));
        let t_mid = sim.time_at_x(250.0);
        assert!(t_mid.is_finite() && t_mid > 0.0, "t_mid={t_mid}");
    }

    #[test]
    fn clamped_simulation_bounds_position_and_velocity() {
        let inner = Box::new(FrictionSimulation::new(0.135, 0.0, 5000.0));
        let clamped = ClampedSimulation::new(inner, -10.0, 100.0, -50.0, 50.0);
        // At t=0: position 0 is within [-10, 100]; velocity 5000 clamps to 50.
        assert_eq!(clamped.x(0.0), 0.0);
        assert_eq!(clamped.dx(0.0), 50.0);
        // Later, the friction position would exceed 100 but stays clamped.
        assert!(clamped.x(10.0) <= 100.0 && clamped.x(10.0) >= -10.0);
    }

    #[test]
    fn bounded_friction_stops_at_bound() {
        // A fast fling that would overshoot 100 must report done at the bound.
        let sim = BoundedFrictionSimulation::new(0.135, 0.0, 5000.0, -10.0, 100.0);
        assert!(sim.x(10.0) <= 100.0, "clamped to max bound");
        assert!(sim.is_done(10.0), "done once the bound is reached");
    }

    #[test]
    fn scroll_spring_does_not_snap() {
        // An underdamped scroll spring overshoots its target (bounce), unlike a
        // snap-to-end spring which would clamp at the end.
        let spring = SpringDescription::with_damping_ratio(1.0, 200.0, 0.4);
        let sim = ScrollSpringSimulation::new(spring, 0.0, 100.0, 0.0);
        let overshot = (0..200).any(|i| sim.x(i as f32 / 60.0) > 100.5);
        assert!(overshot, "underdamped scroll spring should overshoot");
    }

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
