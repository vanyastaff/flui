//! Extension traits for animation composition and convenience methods.
//!
//! This module provides extension traits that add fluent API methods to animation types,
//! following the Rust API Guidelines for extension traits (C-CONV-SPECIFIC).

use crate::animation::Animation;
use crate::compound::{AnimationOperator, CompoundAnimation};
use crate::curve::Curve;
use crate::curved::CurvedAnimation;
use crate::reverse::ReverseAnimation;
use crate::tween::TweenAnimation;
use crate::tween_types::Animatable;
use std::fmt;
use std::sync::Arc;

/// Extension trait for creating animations from [`Animatable`] types.
///
/// This trait provides a fluent API for creating [`TweenAnimation`]s from any type
/// that implements [`Animatable`].
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, Animation};
/// use flui_animation::ext::AnimatableExt;
/// use flui_animation::FloatTween;
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     scheduler,
/// ));
///
/// // Fluent API using extension trait
/// let tween = FloatTween::new(0.0, 100.0);
/// let animation = tween.animate(controller as Arc<dyn Animation<f32>>);
///
/// assert_eq!(animation.value(), 0.0);
/// ```
pub trait AnimatableExt<T>: Animatable<T> + Clone + Send + Sync + 'static
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    /// Create a [`TweenAnimation`] from this animatable and a parent animation.
    ///
    /// This is a convenience method that is equivalent to calling
    /// `TweenAnimation::new(self, parent)`.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimatableExt;
    /// use flui_animation::FloatTween;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = Arc::new(AnimationController::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// ));
    ///
    /// let animation = FloatTween::new(0.0, 100.0)
    ///     .animate(controller as Arc<dyn Animation<f32>>);
    /// ```
    fn animate(self, parent: Arc<dyn Animation<f32>>) -> TweenAnimation<T, Self>
    where
        Self: fmt::Debug,
    {
        TweenAnimation::new(self, parent)
    }
}

// Blanket implementation for all types that implement Animatable
impl<T, A> AnimatableExt<T> for A
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
    A: Animatable<T> + Clone + Send + Sync + 'static,
{
}

/// Extension trait for composing animations.
///
/// This trait provides a fluent API for composing animations with curves,
/// reversal, and mathematical operators.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, Animation};
/// use flui_animation::ext::AnimationExt;
/// use flui_animation::Curves;
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     scheduler,
/// ));
///
/// // Apply a curve using the fluent API
/// let curved = controller.curved(Curves::EaseInOut);
/// ```
pub trait AnimationExt: Animation<f32> + Sized + 'static {
    /// Apply a curve to this animation.
    ///
    /// Creates a [`CurvedAnimation`] that transforms the linear 0.0..1.0 progression
    /// into a non-linear progression based on the provided curve.
    ///
    /// # Arguments
    ///
    /// * `curve` - The curve to apply
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_animation::Curves;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = Arc::new(AnimationController::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// ));
    ///
    /// let curved = controller.curved(Curves::EaseIn);
    /// ```
    fn curved<C>(self: Arc<Self>, curve: C) -> CurvedAnimation<C>
    where
        C: Curve + Clone + Send + Sync + fmt::Debug + 'static,
    {
        CurvedAnimation::new(self as Arc<dyn Animation<f32>>, curve)
    }

    /// Reverse this animation.
    ///
    /// Creates a [`ReverseAnimation`] that inverts the animation values:
    /// - When parent = 0.0, reversed = 1.0
    /// - When parent = 1.0, reversed = 0.0
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = Arc::new(AnimationController::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// ));
    ///
    /// controller.set_value(0.25);
    /// let reversed = controller.reversed();
    /// assert_eq!(reversed.value(), 0.75);
    /// ```
    fn reversed(self: Arc<Self>) -> ReverseAnimation {
        ReverseAnimation::new(self as Arc<dyn Animation<f32>>)
    }

    /// Combine with another animation using an operator.
    ///
    /// Creates a [`CompoundAnimation`] that combines two animations
    /// using the specified operator.
    ///
    /// # Arguments
    ///
    /// * `other` - The other animation to combine with
    /// * `op` - The operator to use for combining
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation, AnimationOperator};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller1 = Arc::new(AnimationController::new(
    ///     Duration::from_millis(300),
    ///     scheduler.clone(),
    /// ));
    /// let controller2 = Arc::new(AnimationController::new(
    ///     Duration::from_millis(300),
    ///     scheduler,
    /// ));
    ///
    /// controller1.set_value(0.5);
    /// controller2.set_value(0.3);
    ///
    /// let combined = controller1.combine(
    ///     controller2 as Arc<dyn Animation<f32>>,
    ///     AnimationOperator::Add,
    /// );
    /// assert_eq!(combined.value(), 0.8);
    /// ```
    fn combine(
        self: Arc<Self>,
        other: Arc<dyn Animation<f32>>,
        op: AnimationOperator,
    ) -> CompoundAnimation {
        CompoundAnimation::new(self as Arc<dyn Animation<f32>>, other, op)
    }

    /// Add another animation to this one.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Add)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let c1 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler.clone()));
    /// let c2 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler));
    ///
    /// c1.set_value(0.5);
    /// c2.set_value(0.3);
    ///
    /// let sum = c1.add(c2 as Arc<dyn Animation<f32>>);
    /// assert_eq!(sum.value(), 0.8);
    /// ```
    fn add(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::add(self as Arc<dyn Animation<f32>>, other)
    }

    /// Multiply with another animation.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Multiply)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let c1 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler.clone()));
    /// let c2 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler));
    ///
    /// c1.set_value(0.5);
    /// c2.set_value(0.4);
    ///
    /// let product = c1.multiply(c2 as Arc<dyn Animation<f32>>);
    /// assert!((product.value() - 0.2).abs() < 1e-6);
    /// ```
    fn multiply(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::multiply(self as Arc<dyn Animation<f32>>, other)
    }

    /// Subtract another animation from this one.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Subtract)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let c1 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler.clone()));
    /// let c2 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler));
    ///
    /// c1.set_value(0.8);
    /// c2.set_value(0.3);
    ///
    /// let diff = c1.subtract(c2 as Arc<dyn Animation<f32>>);
    /// assert!((diff.value() - 0.5).abs() < 1e-6);
    /// ```
    fn subtract(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::subtract(self as Arc<dyn Animation<f32>>, other)
    }

    /// Divide this animation by another.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Divide)`.
    ///
    /// Note: Division by zero will produce infinity or NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_animation::{AnimationController, Animation};
    /// use flui_animation::ext::AnimationExt;
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let c1 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler.clone()));
    /// let c2 = Arc::new(AnimationController::new(Duration::from_millis(300), scheduler));
    ///
    /// c1.set_value(0.8);
    /// c2.set_value(0.4);
    ///
    /// let quotient = c1.divide(c2 as Arc<dyn Animation<f32>>);
    /// assert!((quotient.value() - 2.0).abs() < 1e-6);
    /// ```
    fn divide(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::divide(self as Arc<dyn Animation<f32>>, other)
    }

    /// Return the minimum of this animation and another.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Min)`.
    fn min(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::min(self as Arc<dyn Animation<f32>>, other)
    }

    /// Return the maximum of this animation and another.
    ///
    /// This is a convenience method equivalent to `combine(other, AnimationOperator::Max)`.
    fn max(self: Arc<Self>, other: Arc<dyn Animation<f32>>) -> CompoundAnimation {
        CompoundAnimation::max(self as Arc<dyn Animation<f32>>, other)
    }
}

// Blanket implementation for all types that implement Animation<f32>
impl<A: Animation<f32> + 'static> AnimationExt for A {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Curves;
    use crate::tween_types::FloatTween;
    use crate::AnimationController;
    use flui_scheduler::Scheduler;
    use std::time::Duration;

    #[test]
    fn test_animatable_ext() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let tween = FloatTween::new(0.0, 100.0);
        let animation = tween.animate(controller.clone() as Arc<dyn Animation<f32>>);

        controller.set_value(0.5);
        assert_eq!(animation.value(), 50.0);

        controller.dispose();
    }

    #[test]
    fn test_animation_ext_curved() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let curved = controller.clone().curved(Curves::EaseIn);

        controller.set_value(0.5);
        // EaseIn makes 0.5 appear slower (less than 0.5)
        assert!(curved.value() < 0.5);

        controller.dispose();
    }

    #[test]
    fn test_animation_ext_reversed() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        controller.set_value(0.25);
        let reversed = controller.clone().reversed();
        assert_eq!(reversed.value(), 0.75);

        controller.dispose();
    }

    #[test]
    fn test_animation_ext_add() {
        let scheduler = Arc::new(Scheduler::new());
        let c1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let c2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        c1.set_value(0.5);
        c2.set_value(0.3);

        let sum = c1.clone().add(c2.clone() as Arc<dyn Animation<f32>>);
        assert_eq!(sum.value(), 0.8);

        c1.dispose();
        c2.dispose();
    }

    #[test]
    fn test_animation_ext_multiply() {
        let scheduler = Arc::new(Scheduler::new());
        let c1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let c2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        c1.set_value(0.5);
        c2.set_value(0.4);

        let product = c1.clone().multiply(c2.clone() as Arc<dyn Animation<f32>>);
        assert!((product.value() - 0.2).abs() < 1e-6);

        c1.dispose();
        c2.dispose();
    }

    #[test]
    fn test_animation_ext_subtract() {
        let scheduler = Arc::new(Scheduler::new());
        let c1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let c2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        c1.set_value(0.8);
        c2.set_value(0.3);

        let diff = c1.clone().subtract(c2.clone() as Arc<dyn Animation<f32>>);
        assert!((diff.value() - 0.5).abs() < 1e-6);

        c1.dispose();
        c2.dispose();
    }

    #[test]
    fn test_animation_ext_divide() {
        let scheduler = Arc::new(Scheduler::new());
        let c1 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler.clone(),
        ));
        let c2 = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        c1.set_value(0.8);
        c2.set_value(0.4);

        let quotient = c1.clone().divide(c2.clone() as Arc<dyn Animation<f32>>);
        assert!((quotient.value() - 2.0).abs() < 1e-6);

        c1.dispose();
        c2.dispose();
    }
}
