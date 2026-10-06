//! Interruptible, velocity-preserving spring animation.
//!
//! [`AnimatedValue<T>`] animates any [`TwoWayConverter`] type with an
//! independent spring per scalar component. Re-targeting mid-flight
//! ([`AnimatedValue::animate_to`]) re-seeds each component spring from its
//! *current position and velocity*, so motion is continuous — no snap, no
//! restart. FLUI's spring simulation returns the analytic velocity `dx(t)`
//! exactly, so this hand-off is O(1) and exact, unlike libraries that
//! numerically sample velocity.
//!
//! This is the engine primitive. It advances on an externally supplied `dt`
//! (the widget layer drives it from a ticker); it owns no ticker or listeners,
//! which keeps it trivially testable and composable.

use crate::error::AnimationError;
use crate::simulation::{
    Simulation, SimulationError, SpringDescription, SpringSimulation, Tolerance,
};
use std::time::Duration;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::styling::Color;
use smallvec::SmallVec;

/// A value that can be decomposed into, and rebuilt from, a fixed-width vector
/// of scalar components, so each component can be animated by its own spring.
///
/// Mirrors the role of Jetpack Compose's `TwoWayConverter`. Implement it (or, in
/// future, derive it) for any type you want to spring-animate.
pub trait TwoWayConverter: Clone {
    /// The scalar-component representation, e.g. `[f64; 4]` for an RGBA color.
    /// `Copy` so it can be used as a scratch buffer; `AsRef`/`AsMut<[f64]>` so
    /// the spring core can iterate components generically.
    type Vector: AsRef<[f64]> + AsMut<[f64]> + Copy;

    /// Decompose into scalar components.
    fn to_vector(&self) -> Self::Vector;

    /// Rebuild from scalar components.
    fn from_vector(v: Self::Vector) -> Self;
}

impl TwoWayConverter for f64 {
    type Vector = [f64; 1];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [*self]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        v[0]
    }
}

impl TwoWayConverter for Offset<f64> {
    type Vector = [f64; 2];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.dx, self.dy]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Offset::new(v[0], v[1])
    }
}

impl TwoWayConverter for Size<f64> {
    type Vector = [f64; 2];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [self.width, self.height]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        Size::new(v[0], v[1])
    }
}

impl TwoWayConverter for Color {
    type Vector = [f64; 4];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        [
            f64::from(self.r),
            f64::from(self.g),
            f64::from(self.b),
            f64::from(self.a),
        ]
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        // The `clamp(0.0, 255.0).round()` pins the value into the exact u8 range
        // before the cast, so the truncation/sign-loss lints do not apply.
        let to_u8 = |c: f64| c.clamp(0.0, 255.0).round() as u8;
        Color::rgba(to_u8(v[0]), to_u8(v[1]), to_u8(v[2]), to_u8(v[3]))
    }
}

/// One spring per scalar component (inline for the common 1–4 component case).
type Components = SmallVec<[SpringSimulation; 4]>;

/// An interruptible spring-animated value. See the module docs.
///
/// Advance it with [`advance`](Self::advance), read it with [`value`](Self::value),
/// retarget it with [`animate_to`](Self::animate_to), and check for rest with
/// [`is_settled`](Self::is_settled). Time is an exact [`Duration`] sum, so `N`
/// calls of `advance(dt)` land on the same value as one `advance(N · dt)`.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimatedValue, simulation::SpringDescription};
/// use std::time::Duration;
///
/// let spring = SpringDescription::with_response_and_damping(Duration::from_millis(300), 1.0)?;
/// let mut value = AnimatedValue::new(0.0_f64, spring).expect("finite initial value");
/// value.animate_to(100.0).expect("finite target");
/// value.advance(Duration::from_millis(100));
/// assert!(value.value() > 0.0 && value.value() < 100.0);
/// value.advance(Duration::from_secs(10));
/// assert!(value.is_settled());
/// assert_eq!(value.value(), 100.0);
/// # Ok::<(), flui_animation::simulation::SimulationError>(())
/// ```
#[derive(Debug, Clone)]
pub struct AnimatedValue<T: TwoWayConverter> {
    spring: SpringDescription,
    components: Components,
    /// The current target value (also serves as a correctly-sized scratch buffer).
    target: T,
    /// Time since the most recent (re)target.
    elapsed: Duration,
}

impl<T: TwoWayConverter> AnimatedValue<T> {
    /// A value resting at `initial`, animated by `spring`.
    ///
    /// # Errors
    ///
    /// [`AnimationError::NonFiniteTarget`] when a component of `initial` is
    /// not finite.
    pub fn new(initial: T, spring: SpringDescription) -> Result<Self, AnimationError> {
        let components = resting(spring, &initial)?;
        Ok(Self {
            spring,
            components,
            target: initial,
            elapsed: Duration::ZERO,
        })
    }

    /// Retarget toward `target`, preserving each component's current
    /// position and velocity, so an in-flight animation flows into the new
    /// one without a jump in either. A component already at rest starts
    /// from its previous target with zero velocity.
    ///
    /// # Errors
    ///
    /// [`AnimationError::NonFiniteTarget`] when a component of `target` is
    /// not finite, or its distance from the current value overflows; the
    /// value is then left unchanged.
    pub fn animate_to(&mut self, target: T) -> Result<(), AnimationError> {
        let goal = target.to_vector();
        let t = self.elapsed.as_secs_f64();
        let components = self
            .components
            .iter()
            .zip(goal.as_ref())
            .map(|(sim, &goal)| {
                SpringSimulation::try_new(self.spring, sim.x(t), goal, sim.dx(t), Tolerance::DEFAULT)
            })
            .collect::<Result<Components, _>>()
            .map_err(refused)?;
        self.components = components;
        self.elapsed = Duration::ZERO;
        self.target = target;
        Ok(())
    }

    /// Jump immediately to `value`, cancelling any motion (zero velocity).
    ///
    /// # Errors
    ///
    /// [`AnimationError::NonFiniteTarget`] when a component of `value` is
    /// not finite; the value is then left unchanged.
    pub fn set_value(&mut self, value: T) -> Result<(), AnimationError> {
        self.components = resting(self.spring, &value)?;
        self.elapsed = Duration::ZERO;
        self.target = value;
        Ok(())
    }

    /// Advance time by `dt`, saturating at [`Duration::MAX`].
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed = self.elapsed.saturating_add(dt);
    }

    /// The current animated value.
    #[must_use]
    pub fn value(&self) -> T {
        let t = self.elapsed.as_secs_f64();
        let mut buffer = self.target.to_vector();
        for (slot, sim) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = sim.x(t);
        }
        T::from_vector(buffer)
    }

    /// The current target value.
    #[must_use]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Whether every component spring has come to rest at its target.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        let t = self.elapsed.as_secs_f64();
        self.components.iter().all(|sim| sim.is_done(t))
    }
}

/// Springs resting at each component of `value`.
fn resting<T: TwoWayConverter>(
    spring: SpringDescription,
    value: &T,
) -> Result<Components, AnimationError> {
    value
        .to_vector()
        .as_ref()
        .iter()
        .map(|&c| SpringSimulation::try_new(spring, c, c, 0.0, Tolerance::DEFAULT))
        .collect::<Result<Components, _>>()
        .map_err(refused)
}

fn refused(error: SimulationError) -> AnimationError {
    AnimationError::NonFiniteTarget(format!("animated value component refused: {error}"))
}
