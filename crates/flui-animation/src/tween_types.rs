//! Tween types for animating values.
//!
//! This module provides types for animating values between a beginning and ending state.
//! The core abstraction is the [`Animatable`] trait, which maps a progress value (0.0 to 1.0)
//! to an output value of any type. A value that moves through several states
//! over time is a [`Keyframes`](crate::Keyframes) track.
//!
//! # Examples
//!
//! ```
//! use flui_animation::{Tween, Animatable};
//!
//! let tween = Tween::new(0.0, 100.0);
//! assert_eq!(tween.transform(0.5), 50.0);
//!
//! let reversed = Tween::new(tween.end, tween.begin);
//! assert_eq!(reversed.transform(0.0), 100.0);
//! ```

use flui_foundation::geometry::Lerp;

/// A value that can be animated.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be animated",
    label = "implement `Animatable` or use a `Tween<T>`"
)]
pub trait Animatable {
    /// The value produced by this mapping.
    type Value;
    /// Returns the value of this object at the given animation value.
    fn transform(&self, t: f64) -> Self::Value;

    /// Observe this mapping at a live parent animation's progress.
    ///
    /// The result retains its mapping and the parent's `Rc`; shared subscriptions
    /// are removed with its last clone. Retaining a built-in observer handle does
    /// not retain the separate [`crate::DrivenController`] owner of its run/seat.
    /// Creating an observer moves the mapping; only cloning that observer
    /// requires the mapping to implement `Clone`.
    #[must_use]
    fn animate(self, parent: std::rc::Rc<dyn crate::Animation<f64>>) -> crate::TweenAnimation<Self>
    where
        Self: Sized + 'static,
        Self::Value: Clone + 'static,
    {
        crate::TweenAnimation::new(self, parent)
    }
}

/// A tween that linearly interpolates between a `begin` and `end` value of any
/// [`Lerp`] type, including logical geometry, color and transform matrices.
///
/// `transform` does **not** clamp `t`: bouncy/elastic/spring curves emit
/// `t > 1` (or `t < 0`) and the overshoot must reach the value. The exact
/// endpoints (`t == 0`, `t == 1`) are returned verbatim without interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tween<V> {
    /// The value at the start of the animation.
    pub begin: V,
    /// The value at the end of the animation.
    pub end: V,
}

impl<V> Tween<V> {
    /// Creates a new tween between `begin` and `end`.
    #[must_use]
    pub const fn new(begin: V, end: V) -> Self {
        Self { begin, end }
    }
}

impl<V: Lerp> Animatable for Tween<V> {
    type Value = V;
    fn transform(&self, t: f64) -> V {
        if t == 0.0 {
            return self.begin.clone();
        }
        if t == 1.0 {
            return self.end.clone();
        }
        self.begin.lerp_to(&self.end, t)
    }
}

// ============================================================================
// Concrete Tweens
// ============================================================================

/// How an [`IntTween`] turns its interpolated value into an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Rounding {
    /// The nearest integer; a half rounds toward positive infinity
    /// (−2.5 → −2, 2.5 → 3), as CSS Values 4 §5.2.1 interpolates integers.
    #[default]
    Nearest,
    /// The integer at or below (toward negative infinity): a step that
    /// advances once the value reaches it.
    Floor,
    /// The integer at or above (toward positive infinity).
    Ceil,
    /// The integer part, dropping the fraction (toward zero).
    TowardZero,
}

impl Rounding {
    fn apply(self, value: f64) -> f64 {
        match self {
            // `value + 0.5` would round the largest value below a half up to
            // 1.0; `value - floor(value)` is exact, so compare the fraction.
            Self::Nearest => {
                let floor = value.floor();
                if value - floor >= 0.5 {
                    floor + 1.0
                } else {
                    floor
                }
            }
            Self::Floor => value.floor(),
            Self::Ceil => value.ceil(),
            Self::TowardZero => value.trunc(),
        }
    }
}

/// A tween that linearly interpolates between two integers and rounds the
/// result by its [`Rounding`]. `t` is clamped to `[0, 1]`; a NaN `t` returns
/// `begin`.
///
/// ```
/// use flui_animation::{Animatable, IntTween, Rounding};
///
/// assert_eq!(IntTween::new(-5, 0).transform(0.5), -2);
/// assert_eq!(IntTween::new(0, 10).with_rounding(Rounding::Floor).transform(0.55), 5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntTween {
    /// The beginning value.
    pub begin: i32,
    /// The ending value.
    pub end: i32,
    /// How the interpolated value becomes an integer.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rounding: Rounding,
}

impl IntTween {
    /// An integer tween that rounds to the nearest integer.
    #[must_use]
    pub const fn new(begin: i32, end: i32) -> Self {
        Self {
            begin,
            end,
            rounding: Rounding::Nearest,
        }
    }

    /// The same tween with another rounding.
    #[must_use]
    pub const fn with_rounding(mut self, rounding: Rounding) -> Self {
        self.rounding = rounding;
        self
    }
}

impl Animatable for IntTween {
    type Value = i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a rounded value within the endpoints' i32 range"
    )]
    fn transform(&self, t: f64) -> i32 {
        if t.is_nan() {
            return self.begin;
        }
        let t = t.clamp(0.0, 1.0);
        let value = f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t;
        self.rounding.apply(value) as i32
    }
}
