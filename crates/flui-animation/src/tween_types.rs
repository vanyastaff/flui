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

/// A tween that linearly interpolates between two integers, rounding to the
/// nearest integer (half away from zero). `t` is clamped to `[0, 1]`; a NaN `t`
/// returns `begin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntTween {
    /// The beginning value.
    pub begin: i32,
    /// The ending value.
    pub end: i32,
}

impl IntTween {
    /// Creates a new integer tween.
    #[must_use]
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable for IntTween {
    type Value = i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "rounded f64 to i32; the cast saturates"
    )]
    fn transform(&self, t: f64) -> i32 {
        if t.is_nan() {
            return self.begin;
        }
        let t = t.clamp(0.0, 1.0);
        (f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t).round() as i32
    }
}

/// A tween that linearly interpolates between two integers, flooring to the
/// integer below (toward negative infinity). `t` is clamped to `[0, 1]`; a NaN `t`
/// returns `begin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StepTween {
    /// The beginning value.
    pub begin: i32,
    /// The ending value.
    pub end: i32,
}

impl StepTween {
    /// Creates a new step tween.
    #[must_use]
    pub const fn new(begin: i32, end: i32) -> Self {
        Self { begin, end }
    }
}

impl Animatable for StepTween {
    type Value = i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "floored f64 to i32; the cast saturates"
    )]
    fn transform(&self, t: f64) -> i32 {
        if t.is_nan() {
            return self.begin;
        }
        let t = t.clamp(0.0, 1.0);
        (f64::from(self.begin) + (f64::from(self.end) - f64::from(self.begin)) * t).floor() as i32
    }
}
