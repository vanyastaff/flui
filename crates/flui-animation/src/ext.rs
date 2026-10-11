//! The method that drives an [`Animatable`] with a parent animation.

use crate::animation::Animation;
use crate::tween::TweenAnimation;
use crate::tween_types::Animatable;
use std::fmt;
use std::rc::Rc;

/// Drives an [`Animatable`] with a parent animation: `tween.animate(parent)`
/// is [`TweenAnimation::new`]`(tween, parent)` in method position.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimatableExt, Animation, AnimationController, FloatTween};
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let controller = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
/// let animation = FloatTween::new(0.0, 100.0).animate(controller as Rc<dyn Animation<f64>>);
/// assert_eq!(animation.value(), 0.0);
/// ```
pub trait AnimatableExt: Animatable + Sized {
    /// Creates a [`TweenAnimation`] that reads this animatable at the
    /// parent's value.
    fn animate(self, parent: Rc<dyn Animation<f64>>) -> TweenAnimation<Self>
    where
        Self: fmt::Debug + Clone + 'static,
        Self::Value: Clone + fmt::Debug + 'static,
    {
        TweenAnimation::new(self, parent)
    }
}

impl<A: Animatable> AnimatableExt for A {}
