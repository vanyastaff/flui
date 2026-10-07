//! Interruptible, velocity-preserving animation of any vector-like value.
//!
//! [`AnimatedValue<T>`] animates any [`TwoWayConverter`] type one scalar
//! component at a time, by a spring or a curve ([`MotionSpec`]).
//! Re-targeting mid-flight ([`AnimatedValue::animate_to`]) starts each
//! component's next segment from its *current position and velocity*, so
//! motion is continuous in value and velocity — no snap, no restart (see
//! [`retarget`](crate::retarget)). Both modes have an analytic velocity, so
//! the hand-off is exact.
//!
//! This is the engine primitive. It advances on an externally supplied `dt`
//! (the widget layer drives it from a ticker); it owns no ticker or listeners,
//! which keeps it trivially testable and composable.

use crate::AnimationError;
use crate::curve::Curve;
use crate::retarget::{MotionSpec, Segment};
use crate::simulation::{SimulationError, SpringDescription};
use flui_foundation::geometry::{Offset, Size};
use flui_painting::styling::{Color, PremultipliedOklab};
use smallvec::SmallVec;
use std::time::Duration;

/// A value that can be decomposed into, and rebuilt from, a fixed-width vector
/// of scalar components, so each component can be animated by its own spring.
///
/// Mirrors the role of Jetpack Compose's `TwoWayConverter`. Implement it (or, in
/// future, derive it) for any type you want to spring-animate.
pub trait TwoWayConverter: Clone {
    /// The scalar-component representation, e.g. `[f64; 4]` for a colour.
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

/// Springs a colour in premultiplied Oklab (`L·α`, `a·α`, `b·α`, `α`), the space
/// `Tween<Color>` mixes in (ADR-0149): each component's spring carries its own
/// velocity, and a fade to transparent keeps the opaque end's hue.
impl TwoWayConverter for Color {
    type Vector = [f64; 4];
    #[inline]
    fn to_vector(&self) -> Self::Vector {
        let p = self.to_premultiplied_oklab();
        [p.l, p.a, p.b, p.alpha].map(f64::from)
    }
    #[inline]
    fn from_vector(v: Self::Vector) -> Self {
        // f32 is the colour math's precision (ADR-0098 §2); an overshoot past
        // f32's range saturates like any out-of-gamut channel.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spring components narrow to the f32 colour math"
        )]
        let [l, a, b, alpha] = v.map(|c| c as f32);
        Color::from_premultiplied_oklab(PremultipliedOklab { l, a, b, alpha })
    }
}

/// An interruptible animated value. See the module docs.
///
/// Advance it with [`advance`](Self::advance), read it with
/// [`value`](Self::value) and [`velocity`](Self::velocity), retarget it with
/// [`animate_to`](Self::animate_to), and check for rest with
/// [`is_settled`](Self::is_settled).
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimatedValue, SpringDescription};
/// use std::time::Duration;
///
/// let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 1.0);
/// let mut value = AnimatedValue::new(0.0_f64, spring).expect("finite value");
/// value.animate_to(1.0).expect("finite target");
/// value.advance(Duration::from_millis(50));
/// let (x, v) = (value.value(), value.velocity()[0]);
/// // Interrupting keeps both the value and the velocity.
/// value.animate_to(-1.0).expect("finite target");
/// assert!((value.value() - x).abs() < 1e-12);
/// assert!((value.velocity()[0] - v).abs() < 1e-9);
/// ```
#[derive(Debug, Clone)]
pub struct AnimatedValue<T: TwoWayConverter> {
    motion: MotionSpec,
    /// One segment per scalar component (inline for the common 1–4 component case).
    components: SmallVec<[Segment; 4]>,
    /// The current target value (also serves as a correctly-sized scratch buffer).
    target: T,
    /// Seconds elapsed since the most recent (re)target.
    elapsed: Duration,
    /// Where a reversal must head for the current segment to be shortened,
    /// and by how much the current segment already was.
    reversal: Reversal<T::Vector>,
}

/// The CSS Transitions reversal state of a curve segment: its
/// reversing-adjusted start value and its shortening factor.
#[derive(Debug, Clone, Copy)]
struct Reversal<V> {
    adjusted_start: V,
    shortening: f64,
}

impl<T: TwoWayConverter> AnimatedValue<T> {
    /// Create a value resting at `initial`, animated by `spring`.
    ///
    /// # Errors
    /// Returns [`AnimationError::NonFiniteTarget`] for a non-finite component.
    pub fn new(initial: T, spring: SpringDescription) -> Result<Self, AnimationError> {
        Self::with_motion(initial, MotionSpec::Spring(spring))
    }

    /// Create a value resting at `initial`, animated as `motion` says.
    ///
    /// # Errors
    /// Returns [`AnimationError::NonFiniteTarget`] for a non-finite component.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimatedValue, ArcCurve, Curves, MotionSpec};
    /// use std::time::Duration;
    ///
    /// let motion = MotionSpec::Curve {
    ///     duration: Duration::from_millis(200),
    ///     curve: ArcCurve::new(Curves::EaseInOut),
    /// };
    /// let mut value = AnimatedValue::with_motion(0.0_f64, motion).expect("finite value");
    /// value.animate_to(10.0).expect("finite target");
    /// value.advance(Duration::from_millis(200));
    /// assert_eq!(value.value(), 10.0);
    /// assert!(value.is_settled());
    /// ```
    pub fn with_motion(initial: T, motion: MotionSpec) -> Result<Self, AnimationError> {
        let vector = initial.to_vector();
        validate_vector(&vector)?;
        let components = vector.as_ref().iter().map(|&c| Segment::Rest(c)).collect();
        Ok(Self {
            motion,
            components,
            target: initial,
            elapsed: Duration::ZERO,
            reversal: Reversal {
                adjusted_start: vector,
                shortening: 1.0,
            },
        })
    }

    /// Retarget toward `target`, keeping each component's current value and
    /// velocity.
    ///
    /// Each component's next segment starts from its analytic position and
    /// velocity at the current time, so an in-flight animation flows into the
    /// new one without snapping or losing momentum. In curve mode, heading
    /// back to the current segment's reversing-adjusted start (where the
    /// segment before it was headed from) shortens the new segment by the
    /// eased fraction already travelled. The current target leaves
    /// the motion untouched: a curve keeps its schedule instead of restarting
    /// its duration.
    ///
    /// # Errors
    /// Returns [`AnimationError::NonFiniteTarget`] when a target component
    /// or derived simulation constant is unrepresentable. Every component,
    /// the target, elapsed time and reversal state remain unchanged.
    pub fn animate_to(&mut self, target: T) -> Result<(), AnimationError> {
        let goal = target.to_vector();
        validate_vector(&goal)?;
        if goal.as_ref() == self.target.to_vector().as_ref() {
            self.target = target;
            return Ok(());
        }
        let shortening = self.reversal_shortening(&goal);
        let current = self.current_vector();
        self.restart(&goal, None, shortening)?;
        self.reversal = match shortening {
            Some(shortening) => Reversal {
                adjusted_start: self.target.to_vector(),
                shortening,
            },
            None => Reversal {
                adjusted_start: current,
                shortening: 1.0,
            },
        };
        self.target = target;
        Ok(())
    }

    /// Change how the value moves from now on.
    ///
    /// The current segment is replaced by one toward the same target under
    /// `motion`, starting from the current value and velocity.
    ///
    /// # Errors
    /// Returns [`AnimationError::NonFiniteTarget`] if the replacement cannot
    /// represent the current motion; the previous schedule remains intact.
    pub fn set_motion(&mut self, motion: MotionSpec) -> Result<(), AnimationError> {
        let goal = self.target.to_vector();
        let reversal = Reversal {
            adjusted_start: self.current_vector(),
            shortening: 1.0,
        };
        self.restart(&goal, Some(&motion), None)?;
        self.motion = motion;
        self.reversal = reversal;
        Ok(())
    }

    /// Jump immediately to `value`, cancelling any motion (zero velocity).
    ///
    /// # Errors
    /// Returns [`AnimationError::NonFiniteTarget`] for a non-finite component,
    /// leaving the previous value and motion unchanged.
    pub fn set_value(&mut self, value: T) -> Result<(), AnimationError> {
        let vector = value.to_vector();
        validate_vector(&vector)?;
        for (segment, &c) in self.components.iter_mut().zip(vector.as_ref()) {
            *segment = Segment::Rest(c);
        }
        self.elapsed = Duration::ZERO;
        self.reversal = Reversal {
            adjusted_start: vector,
            shortening: 1.0,
        };
        self.target = value;
        Ok(())
    }

    /// Advance by `dt`, saturating at [`Duration::MAX`].
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed = self.elapsed.saturating_add(dt);
    }

    /// The current animated value.
    #[must_use]
    pub fn value(&self) -> T {
        // Completed curve and constant segments retain the exact target, including
        // components a converter cannot recover (transparent color channels).
        // A settled spring still publishes its analytic convergence.
        if self.components.iter().all(|segment| {
            !matches!(segment, Segment::Spring { .. }) && segment.is_done(self.elapsed_seconds())
        }) {
            return self.target.clone();
        }
        T::from_vector(self.current_vector())
    }

    /// The current velocity of each component, in component units per
    /// second. Zero at rest.
    #[must_use]
    pub fn velocity(&self) -> T::Vector {
        let mut buffer = self.target.to_vector();
        for (slot, segment) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = segment.dx(self.elapsed_seconds());
        }
        buffer
    }

    /// The current target value.
    #[must_use]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Whether every component has settled: a curve segment has arrived, a
    /// spring is within its distance tolerance of the target.
    ///
    /// Settled means within tolerance; the value keeps converging
    /// continuously, with no final jump. A settled spring's
    /// [`value`](Self::value) may still differ from the target by up to the
    /// tolerance and keeps approaching it, so stop driving frames on
    /// `is_settled`, not on `value() == target`.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.components
            .iter()
            .all(|segment| segment.is_done(self.elapsed_seconds()))
    }

    fn current_vector(&self) -> T::Vector {
        let mut buffer = self.target.to_vector();
        for (slot, segment) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = segment.x(self.elapsed_seconds());
        }
        buffer
    }

    /// Starts every component's next segment toward `goal` from its current
    /// value and velocity.
    fn restart(
        &mut self,
        goal: &T::Vector,
        motion: Option<&MotionSpec>,
        shortening: Option<f64>,
    ) -> Result<(), AnimationError> {
        let shortening = shortening.unwrap_or(1.0);
        let time = self.elapsed_seconds();
        let motion = motion.unwrap_or(&self.motion);
        let components = self
            .components
            .iter()
            .zip(goal.as_ref())
            .map(|(segment, &goal)| {
                Segment::start(segment.x(time), segment.dx(time), goal, motion, shortening)
            })
            .collect::<Result<SmallVec<[Segment; 4]>, _>>()
            .map_err(refused)?;
        self.components = components;
        self.elapsed = Duration::ZERO;
        Ok(())
    }

    fn elapsed_seconds(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    /// The new shortening factor when `goal` reverses a curve segment in
    /// flight, `None` otherwise: `|c(τ)·S + (1 − S)|` clamped to `[0, 1]`,
    /// with `τ` the old segment's progress and `S` its shortening.
    fn reversal_shortening(&self, goal: &T::Vector) -> Option<f64> {
        let MotionSpec::Curve { duration, curve } = &self.motion else {
            return None;
        };
        if self.is_settled() || goal.as_ref() != self.reversal.adjusted_start.as_ref() {
            return None;
        }
        let old_duration = duration.as_secs_f64() * self.reversal.shortening;
        if old_duration <= 0.0 {
            return None;
        }
        let progress = (self.elapsed_seconds() / old_duration).clamp(0.0, 1.0);
        let old = self.reversal.shortening;
        Some(
            (curve.transform(progress) * old + (1.0 - old))
                .abs()
                .clamp(0.0, 1.0),
        )
    }
}

fn validate_vector(vector: &impl AsRef<[f64]>) -> Result<(), AnimationError> {
    if vector
        .as_ref()
        .iter()
        .all(|component| component.is_finite())
    {
        Ok(())
    } else {
        Err(AnimationError::NonFiniteTarget(
            "animated value components must be finite".into(),
        ))
    }
}

fn refused(error: SimulationError) -> AnimationError {
    AnimationError::NonFiniteTarget(format!("animated value component refused: {error}"))
}
