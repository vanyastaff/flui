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

use std::f64::consts::PI;

/// Tolerance for determining when simulations are "done".
///
/// Specifies maximum allowable magnitudes for distances and velocities
/// to be considered at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Maximum distance from target to be considered "at rest".
    pub distance: f64,
    /// Maximum velocity to be considered "at rest".
    pub velocity: f64,
    /// Maximum time difference to be considered equal.
    pub time: f64,
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
    pub const fn new(distance: f64, velocity: f64, time: f64) -> Self {
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
    fn x(&self, time: f64) -> f64;

    /// The velocity of the object at the given time.
    fn dx(&self, time: f64) -> f64;

    /// Whether the simulation is "done" at the given time.
    ///
    /// Typically returns true when the object has come to rest
    /// within the specified tolerance.
    fn is_done(&self, time: f64) -> bool;

    /// The tolerance used to determine when the simulation is done.
    fn tolerance(&self) -> Tolerance;
}

/// Blanket implementation so `Box<dyn Simulation>` itself satisfies `Simulation`.
///
/// This lets callers that receive a `Box<dyn Simulation>` (e.g. from
/// `ScrollPhysics::create_ballistic_simulation`) pass it directly to
/// `AnimationController::animate_with`, which is generic over
/// `S: Simulation + 'static`.
impl<S: Simulation + ?Sized> Simulation for Box<S> {
    #[inline]
    fn x(&self, time: f64) -> f64 {
        (**self).x(time)
    }

    #[inline]
    fn dx(&self, time: f64) -> f64 {
        (**self).dx(time)
    }

    #[inline]
    fn is_done(&self, time: f64) -> bool {
        (**self).is_done(time)
    }

    #[inline]
    fn tolerance(&self) -> Tolerance {
        (**self).tolerance()
    }
}

/// Description of a spring's physical properties.
///
/// Used to configure [`SpringSimulation`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringDescription {
    /// The mass of the spring (m).
    pub mass: f64,
    /// The spring constant / stiffness (k).
    pub stiffness: f64,
    /// The damping coefficient (c).
    pub damping: f64,
}

impl SpringDescription {
    /// Creates a spring with explicit mass, stiffness, and damping.
    ///
    /// # Panics
    /// Panics if mass or stiffness is not finite and positive, or if damping is
    /// not finite and non-negative (`NaN` / `±Inf` are rejected).
    #[must_use]
    pub fn new(mass: f64, stiffness: f64, damping: f64) -> Self {
        assert!(mass.is_finite(), "Mass must be finite");
        assert!(mass > 0.0, "Mass must be positive");
        assert!(stiffness.is_finite(), "Stiffness must be finite");
        assert!(stiffness > 0.0, "Stiffness must be positive");
        assert!(damping.is_finite(), "Damping must be finite");
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
    /// Panics if mass or stiffness is not finite and positive, or if ratio is
    /// not finite and non-negative (`NaN` / `±Inf` are rejected).
    #[must_use]
    pub fn with_damping_ratio(mass: f64, stiffness: f64, ratio: f64) -> Self {
        assert!(mass.is_finite(), "Mass must be finite");
        assert!(mass > 0.0, "Mass must be positive");
        assert!(stiffness.is_finite(), "Stiffness must be finite");
        assert!(stiffness > 0.0, "Stiffness must be positive");
        assert!(ratio.is_finite(), "Damping ratio must be finite");
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
    pub fn with_duration_and_bounce(duration_secs: f64, bounce: f64) -> Self {
        const MASS: f64 = 1.0;

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
    pub fn with_response_and_damping(response: f64, damping_fraction: f64) -> Self {
        const MASS: f64 = 1.0;
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
    pub fn damping_ratio(&self) -> f64 {
        self.damping / (2.0 * (self.mass * self.stiffness).sqrt())
    }

    /// Returns the bounce value (inverse of damping ratio mapping).
    #[must_use]
    pub fn bounce(&self) -> f64 {
        let ratio = self.damping_ratio();
        if ratio < 1.0 {
            1.0 - ratio
        } else {
            (1.0 / ratio) - 1.0
        }
    }

    /// Returns the type of spring based on damping.
    ///
    /// Classification uses `f64`. The only round-trip correction is an exact
    /// match against the `f64` damping that `with_damping_ratio(..., 1.0)`
    /// stores (`2√(mk)` after `f64` arithmetic) — representable non-critical
    /// ratios keep their true under-/over-damped regime.
    #[must_use]
    pub fn spring_type(&self) -> SpringType {
        spring_regime(*self)
    }
}

/// `c² - 4mk` in `f64`. Internal analytic constants need this precision so
/// extreme but finite springs keep a usable slow root (see [`OverdampedSolution`]).
#[inline]
fn spring_discriminant(spring: SpringDescription) -> f64 {
    let mass = spring.mass;
    let stiffness = spring.stiffness;
    let damping = spring.damping;
    damping * damping - 4.0 * mass * stiffness
}

/// `f64` critical damping `2√(mk)`, matching [`SpringDescription::with_damping_ratio`]
/// at ratio `1.0`. Comparing against this exact bit pattern — not a relative
/// band — preserves representable non-critical ratios while still snapping the
/// ratio-`1.0` round-trip (whose `f64` discriminant is a tiny nonzero).
#[inline]
fn critical_damping_f32(mass: f64, stiffness: f64) -> f64 {
    2.0 * (mass * stiffness).sqrt()
}

#[inline]
fn spring_regime(spring: SpringDescription) -> SpringType {
    // Snap only the exact `with_damping_ratio(..., 1.0)` bit pattern.
    if spring.damping == critical_damping_f32(spring.mass, spring.stiffness) {
        return SpringType::CriticallyDamped;
    }

    let discriminant = spring_discriminant(spring);
    if discriminant > 0.0 {
        SpringType::Overdamped
    } else if discriminant < 0.0 {
        SpringType::Underdamped
    } else {
        SpringType::CriticallyDamped
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
    end_position: f64,
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
    pub fn new(spring: SpringDescription, start: f64, end: f64, velocity: f64) -> Self {
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
        start: f64,
        end: f64,
        velocity: f64,
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
    pub fn end_position(&self) -> f64 {
        self.end_position
    }
}

impl Simulation for SpringSimulation {
    fn x(&self, time: f64) -> f64 {
        if self.snap_to_end && self.is_done(time) {
            self.end_position
        } else {
            self.end_position + self.solution.x(time)
        }
    }

    fn dx(&self, time: f64) -> f64 {
        if self.snap_to_end && self.is_done(time) {
            0.0
        } else {
            self.solution.dx(time)
        }
    }

    fn is_done(&self, time: f64) -> bool {
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
    fn new(spring: SpringDescription, initial_position: f64, initial_velocity: f64) -> Self {
        match spring_regime(spring) {
            SpringType::Overdamped => SpringSolution::Overdamped(OverdampedSolution::new(
                spring,
                initial_position,
                initial_velocity,
            )),
            SpringType::Underdamped => SpringSolution::Underdamped(UnderdampedSolution::new(
                spring,
                initial_position,
                initial_velocity,
            )),
            SpringType::CriticallyDamped => SpringSolution::Critical(CriticalSolution::new(
                spring,
                initial_position,
                initial_velocity,
            )),
        }
    }

    fn x(&self, time: f64) -> f64 {
        match self {
            SpringSolution::Critical(s) => s.x(time),
            SpringSolution::Overdamped(s) => s.x(time),
            SpringSolution::Underdamped(s) => s.x(time),
        }
    }

    fn dx(&self, time: f64) -> f64 {
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
///
/// Analytic constants are `f64` so samples stay continuous with the neighboring
/// over-/under-damped regimes when public parameters are stored as `f64`.
#[derive(Debug, Clone)]
struct CriticalSolution {
    r: f64,
    c1: f64,
    c2: f64,
}

impl CriticalSolution {
    fn new(spring: SpringDescription, distance: f64, velocity: f64) -> Self {
        let mass = spring.mass;
        let damping = spring.damping;
        let r = -damping / (2.0 * mass);
        let c1 = distance;
        let c2 = velocity - (r * distance);
        Self { r, c1, c2 }
    }

    fn x(&self, time: f64) -> f64 {
        (self.c1 + self.c2 * time) * (self.r * time).exp()
    }

    fn dx(&self, time: f64) -> f64 {
        let power = (self.r * time).exp();

        self.r * (self.c1 + self.c2 * time) * power + self.c2 * power
    }
}

/// Overdamped spring solution.
///
/// Roots and coefficients are computed in `f64`. The slow root uses Vieta's
/// product relation (`r1·r2 = k/m`) instead of `-c + √(c²-4mk)`, which cancels
/// to exactly `0` in `f64` for heavy damping (e.g. mass=1, stiffness=1,
/// damping=10000) and leaves the simulation stuck away from the target.
#[derive(Debug, Clone)]
struct OverdampedSolution {
    r1: f64,
    r2: f64,
    c1: f64,
    c2: f64,
}

impl OverdampedSolution {
    fn new(spring: SpringDescription, distance: f64, velocity: f64) -> Self {
        let mass = spring.mass;
        let stiffness = spring.stiffness;
        let damping = spring.damping;

        let cmk = damping * damping - 4.0 * mass * stiffness;
        // Fast (more negative) root: both terms share sign, so no cancellation.
        let r1 = (-damping - cmk.sqrt()) / (2.0 * mass);
        // Slow root via Vieta — stable when `√(c²-4mk)` ≈ `c`.
        let r2 = (stiffness / mass) / r1;
        let c2 = (velocity - r1 * distance) / (r2 - r1);
        let c1 = distance - c2;
        Self { r1, r2, c1, c2 }
    }

    fn x(&self, time: f64) -> f64 {
        self.c1 * (self.r1 * time).exp() + self.c2 * (self.r2 * time).exp()
    }

    fn dx(&self, time: f64) -> f64 {
        self.c1 * self.r1 * (self.r1 * time).exp() + self.c2 * self.r2 * (self.r2 * time).exp()
    }
}

/// Underdamped spring solution.
///
/// Analytic constants are `f64` for the same continuity reasons as
/// [`CriticalSolution`] / [`OverdampedSolution`].
#[derive(Debug, Clone)]
struct UnderdampedSolution {
    w: f64,
    r: f64,
    c1: f64,
    c2: f64,
}

impl UnderdampedSolution {
    fn new(spring: SpringDescription, distance: f64, velocity: f64) -> Self {
        let mass = spring.mass;
        let stiffness = spring.stiffness;
        let damping = spring.damping;

        let w = (4.0 * mass * stiffness - damping * damping).sqrt() / (2.0 * mass);
        let r = -(damping / (2.0 * mass));
        let c1 = distance;
        let c2 = (velocity - r * distance) / w;
        Self { w, r, c1, c2 }
    }

    fn x(&self, time: f64) -> f64 {
        (self.r * time).exp() * (self.c1 * (self.w * time).cos() + self.c2 * (self.w * time).sin())
    }

    fn dx(&self, time: f64) -> f64 {
        let power = (self.r * time).exp();
        let cosine = (self.w * time).cos();
        let sine = (self.w * time).sin();

        power * (self.c2 * self.w * cosine - self.c1 * self.w * sine)
            + self.r * power * (self.c2 * sine + self.c1 * cosine)
    }
}

/// Checks if a value is near zero within the given threshold.
#[inline]
fn near_zero(value: f64, threshold: f64) -> bool {
    value.abs() < threshold
}

/// A friction simulation that slows an object by a constant deceleration.
#[derive(Debug, Clone)]
pub struct FrictionSimulation {
    drag: f64,
    drag_log: f64,
    initial_position: f64,
    initial_velocity: f64,
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
    pub fn new(drag: f64, position: f64, velocity: f64) -> Self {
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
    pub fn with_tolerance(drag: f64, position: f64, velocity: f64, tolerance: Tolerance) -> Self {
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
    pub fn final_x(&self) -> f64 {
        self.initial_position - self.initial_velocity / self.drag_log
    }

    /// Returns the time at which the simulation reaches the given position.
    #[must_use]
    pub fn time_at_x(&self, x: f64) -> f64 {
        if (x - self.initial_position).abs() < 1e-6 {
            0.0
        } else {
            // Solve x(t) = x for t. From `x(t) = x0 + v·(drag^t - 1)/ln(drag)`,
            //   drag^t = ln(drag)·(x - x0)/v + 1, so t = ln(that) / ln(drag).
            // (Previously used `(x0 - x)`, the wrong sign, which returned
            // negative times for forward motion; `time_at_x(final_x)` is `+inf`,
            // the asymptote.)
            (self.drag_log * (x - self.initial_position) / self.initial_velocity).ln_1p()
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
    /// simulation reports done at that velocity.
    ///
    /// # Panics
    /// Panics if the solved drag is not in `(0, 1)` — which happens only for
    /// physically impossible requests (e.g. asking to *speed up* over the span,
    /// or to move opposite the velocity).
    #[must_use]
    pub fn through(
        start_position: f64,
        end_position: f64,
        start_velocity: f64,
        end_velocity: f64,
    ) -> Self {
        // Zero travel distance puts a 0 (or ±0-signed) denominator under the
        // exponent and produces drag = e^±inf / NaN; fail with the physical
        // constraint instead of the downstream "drag must be in (0,1)" panic.
        assert!(
            (start_position - end_position).abs() > f64::EPSILON,
            "FrictionSimulation::through requires start_position != end_position: \
             zero travel distance has no finite drag solution"
        );
        // drag = e^((vStart - vEnd) / (xStart - xEnd)).
        let drag = std::f64::consts::E
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
    fn x(&self, time: f64) -> f64 {
        // exp_m1 retains small displacement for drag near one or time near
        // zero, where subtracting one from drag^time loses those bits.
        // Single-term form: pos + vel*(drag^t - 1)/drag_log
        // Algebraically equivalent to the two-term form for finite inputs, but
        // avoids INF - INF = NaN when |vel| → ∞ (e.g. before the 8 000 px/s
        // cap is applied).
        self.initial_position
            + self.initial_velocity * (self.drag_log * time).exp_m1() / self.drag_log
    }

    fn dx(&self, time: f64) -> f64 {
        self.initial_velocity * self.drag.powf(time)
    }

    fn is_done(&self, time: f64) -> bool {
        self.dx(time).abs() < self.tolerance.velocity
    }

    fn tolerance(&self) -> Tolerance {
        self.tolerance
    }
}

/// A gravity simulation with constant acceleration.
#[derive(Debug, Clone)]
pub struct GravitySimulation {
    acceleration: f64,
    initial_position: f64,
    initial_velocity: f64,
    end_position: f64,
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
    pub fn new(acceleration: f64, position: f64, velocity: f64, end: f64) -> Self {
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
        acceleration: f64,
        position: f64,
        velocity: f64,
        end: f64,
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
    fn x(&self, time: f64) -> f64 {
        self.initial_position + self.initial_velocity * time + 0.5 * self.acceleration * time * time
    }

    fn dx(&self, time: f64) -> f64 {
        self.initial_velocity + self.acceleration * time
    }

    fn is_done(&self, time: f64) -> bool {
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
#[derive(Debug, Clone)]
pub struct ScrollSpringSimulation {
    spring: SpringSimulation,
}

impl ScrollSpringSimulation {
    /// Creates a scroll spring from `start` toward `end` with an initial `velocity`.
    #[must_use]
    pub fn new(spring: SpringDescription, start: f64, end: f64, velocity: f64) -> Self {
        // `with_snap_to_end` is left at its default (false): overscroll bounce
        // requires the un-snapped value.
        Self {
            spring: SpringSimulation::new(spring, start, end, velocity),
        }
    }
}

impl Simulation for ScrollSpringSimulation {
    fn x(&self, time: f64) -> f64 {
        self.spring.x(time)
    }
    fn dx(&self, time: f64) -> f64 {
        self.spring.dx(time)
    }
    fn is_done(&self, time: f64) -> bool {
        self.spring.is_done(time)
    }
    fn tolerance(&self) -> Tolerance {
        self.spring.tolerance()
    }
}

/// Wraps another simulation, clamping its position to `[x_min, x_max]` and its
/// velocity to `[dx_min, dx_max]`.
pub struct ClampedSimulation {
    inner: Box<dyn Simulation>,
    x_min: f64,
    x_max: f64,
    dx_min: f64,
    dx_max: f64,
}

impl ClampedSimulation {
    /// Wraps `inner`, clamping position to `[x_min, x_max]` and velocity to
    /// `[dx_min, dx_max]`.
    #[must_use]
    pub fn new(
        inner: Box<dyn Simulation>,
        x_min: f64,
        x_max: f64,
        dx_min: f64,
        dx_max: f64,
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
    fn x(&self, time: f64) -> f64 {
        self.inner.x(time).clamp(self.x_min, self.x_max)
    }
    fn dx(&self, time: f64) -> f64 {
        self.inner.dx(time).clamp(self.dx_min, self.dx_max)
    }
    fn is_done(&self, time: f64) -> bool {
        self.inner.is_done(time)
    }
    fn tolerance(&self) -> Tolerance {
        self.inner.tolerance()
    }
}

/// A [`FrictionSimulation`] clamped to a position range, finishing when the
/// friction settles *or* the object reaches a bound. Used by scrollables so a fling that overshoots the
/// content extent stops cleanly at the edge.
#[derive(Debug, Clone)]
pub struct BoundedFrictionSimulation {
    friction: FrictionSimulation,
    min_x: f64,
    max_x: f64,
    /// Travel direction at construction: a fling with positive velocity heads
    /// toward `max_x` and finishes once the unclamped friction position reaches
    /// that bound; a negative one heads toward `min_x`.
    positive_direction: bool,
}

impl BoundedFrictionSimulation {
    /// Creates a bounded friction simulation confined to `[min_x, max_x]`.
    ///
    /// # Panics
    /// Panics if `drag` is not in `(0, 1)` (see [`FrictionSimulation::new`]).
    #[must_use]
    pub fn new(drag: f64, position: f64, velocity: f64, min_x: f64, max_x: f64) -> Self {
        Self {
            friction: FrictionSimulation::new(drag, position, velocity),
            min_x,
            max_x,
            positive_direction: velocity > 0.0,
        }
    }
}

impl Simulation for BoundedFrictionSimulation {
    fn x(&self, time: f64) -> f64 {
        self.friction.x(time).clamp(self.min_x, self.max_x)
    }
    fn dx(&self, time: f64) -> f64 {
        // Once pinned at the bound the object is at rest, so report zero velocity
        // — matching the clamped position — instead of the still-decaying
        // friction velocity a controller would otherwise keep sampling.
        if self.is_done(time) {
            0.0
        } else {
            self.friction.dx(time)
        }
    }
    fn is_done(&self, time: f64) -> bool {
        // Done when the friction settles OR the fling reaches the bound it is
        // travelling toward. The previous code tested whether the *clamped*
        // position was within tolerance of the exact bound, which a fast fling
        // skips entirely: its unclamped position can jump from inside the range
        // to far past the bound between frames, leaving the simulation "active"
        // (and the controller ticking) while x() is already pinned at the edge.
        if self.friction.is_done(time) {
            return true;
        }
        let fx = self.friction.x(time);
        if self.positive_direction {
            fx >= self.max_x
        } else {
            fx <= self.min_x
        }
    }
    fn tolerance(&self) -> Tolerance {
        self.friction.tolerance()
    }
}
