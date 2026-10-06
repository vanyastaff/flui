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

use crate::curve::Curve;
use crate::retarget::{MotionSpec, Segment};
use crate::simulation::SpringDescription;
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
///
/// let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 1.0);
/// let mut value = AnimatedValue::new(0.0_f64, spring);
/// value.animate_to(1.0);
/// value.advance(0.05);
/// let (x, v) = (value.value(), value.velocity()[0]);
/// // Interrupting keeps both the value and the velocity.
/// value.animate_to(-1.0);
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
    elapsed: f64,
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
    #[must_use]
    pub fn new(initial: T, spring: SpringDescription) -> Self {
        Self::with_motion(initial, MotionSpec::Spring(spring))
    }

    /// Create a value resting at `initial`, animated as `motion` says.
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
    /// let mut value = AnimatedValue::with_motion(0.0_f64, motion);
    /// value.animate_to(10.0);
    /// value.advance(0.2);
    /// assert_eq!(value.value(), 10.0);
    /// assert!(value.is_settled());
    /// ```
    #[must_use]
    pub fn with_motion(initial: T, motion: MotionSpec) -> Self {
        let vector = initial.to_vector();
        let components = vector.as_ref().iter().map(|&c| Segment::Rest(c)).collect();
        Self {
            motion,
            components,
            target: initial,
            elapsed: 0.0,
            reversal: Reversal {
                adjusted_start: vector,
                shortening: 1.0,
            },
        }
    }

    /// Retarget toward `target`, keeping each component's current value and
    /// velocity.
    ///
    /// Each component's next segment starts from its analytic position and
    /// velocity at the current time, so an in-flight animation flows into the
    /// new one without snapping or losing momentum. In curve mode, heading
    /// back to the current segment's reversing-adjusted start (where the
    /// segment before it was headed from) shortens the new segment by the
    /// eased fraction already travelled. A non-finite target component
    /// leaves that component at rest where it is.
    pub fn animate_to(&mut self, target: T) {
        let goal = target.to_vector();
        let shortening = self.reversal_shortening(&goal);
        let current = self.current_vector();
        self.restart(&goal, shortening);
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
    }

    /// Change how the value moves from now on.
    ///
    /// The current segment is replaced by one toward the same target under
    /// `motion`, starting from the current value and velocity.
    pub fn set_motion(&mut self, motion: MotionSpec) {
        self.motion = motion;
        let goal = self.target.to_vector();
        self.reversal = Reversal {
            adjusted_start: self.current_vector(),
            shortening: 1.0,
        };
        self.restart(&goal, None);
    }

    /// Jump immediately to `value`, cancelling any motion (zero velocity).
    pub fn set_value(&mut self, value: T) {
        let vector = value.to_vector();
        for (segment, &c) in self.components.iter_mut().zip(vector.as_ref()) {
            *segment = Segment::Rest(c);
        }
        self.elapsed = 0.0;
        self.reversal = Reversal {
            adjusted_start: vector,
            shortening: 1.0,
        };
        self.target = value;
    }

    /// Advance time by `dt` seconds. Zero, negative or non-finite `dt` does
    /// not move time.
    pub fn advance(&mut self, dt: f64) {
        if dt.is_finite() && dt > 0.0 {
            self.elapsed += dt;
        }
    }

    /// The current animated value.
    #[must_use]
    pub fn value(&self) -> T {
        T::from_vector(self.current_vector())
    }

    /// The current velocity of each component, in component units per
    /// second. Zero at rest.
    #[must_use]
    pub fn velocity(&self) -> T::Vector {
        let mut buffer = self.target.to_vector();
        for (slot, segment) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = segment.dx(self.elapsed);
        }
        buffer
    }

    /// The current target value.
    #[must_use]
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Whether every component has come to rest at its target.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.components
            .iter()
            .all(|segment| segment.is_done(self.elapsed))
    }

    fn current_vector(&self) -> T::Vector {
        let mut buffer = self.target.to_vector();
        for (slot, segment) in buffer.as_mut().iter_mut().zip(&self.components) {
            *slot = segment.x(self.elapsed);
        }
        buffer
    }

    /// Starts every component's next segment toward `goal` from its current
    /// value and velocity.
    fn restart(&mut self, goal: &T::Vector, shortening: Option<f64>) {
        let shortening = shortening.unwrap_or(1.0);
        for (segment, &goal) in self.components.iter_mut().zip(goal.as_ref()) {
            let position = segment.x(self.elapsed);
            let velocity = segment.dx(self.elapsed);
            *segment = Segment::start(position, velocity, goal, &self.motion, shortening);
        }
        self.elapsed = 0.0;
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
        let progress = (self.elapsed / old_duration).clamp(0.0, 1.0);
        let old = self.reversal.shortening;
        Some(
            (curve.transform(progress) * old + (1.0 - old))
                .abs()
                .clamp(0.0, 1.0),
        )
    }
}
