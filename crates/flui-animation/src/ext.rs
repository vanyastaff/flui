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
/// use flui_scheduler::UpdateScheduler;
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
/// let animation = FloatTween::new(0.0, 100.0).animate(controller as Rc<dyn Animation<f64>>);
/// assert_eq!(animation.value(), 0.0);
/// ```
pub trait AnimatableExt<T>: Animatable<T> + Sized {
    /// Creates a [`TweenAnimation`] that reads this animatable at the
    /// parent's value.
    fn animate(self, parent: Rc<dyn Animation<f64>>) -> TweenAnimation<T, Self>
    where
        Self: fmt::Debug + Clone + 'static,
        T: Clone + fmt::Debug + 'static,
    {
        TweenAnimation::new(self, parent)
    }
}

impl<T, A: Animatable<T>> AnimatableExt<T> for A {}
