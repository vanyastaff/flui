//! The method that drives an [`Animatable`] with a parent animation.

use crate::animation::Animation;
use crate::tween::TweenAnimation;
use crate::tween_types::Animatable;
use std::fmt;
use std::sync::Arc;

/// Drives an [`Animatable`] with a parent animation: `tween.animate(parent)`
/// is [`TweenAnimation::new`]`(tween, parent)` in method position.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimatableExt, Animation, AnimationController, FloatTween};
/// use flui_scheduler::UpdateScheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
/// let animation = FloatTween::new(0.0, 100.0).animate(controller as Arc<dyn Animation<f64>>);
/// assert_eq!(animation.value(), 0.0);
/// ```
pub trait AnimatableExt<T>: Animatable<T> + Sized {
    /// Creates a [`TweenAnimation`] that reads this animatable at the
    /// parent's value.
    fn animate(self, parent: Arc<dyn Animation<f64>>) -> TweenAnimation<T, Self>
    where
        Self: fmt::Debug + Clone + Send + Sync + 'static,
        T: Clone + Send + Sync + fmt::Debug + 'static,
    {
        TweenAnimation::new(self, parent)
    }
}

impl<T, A: Animatable<T>> AnimatableExt<T> for A {}
