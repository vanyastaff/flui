//! `TweenAnimation` - maps f32 animations to any type T.

use crate::animation::{Animation, ParentSubscription, StatusCallback, link_parent};
use crate::status::AnimationStatus;
use crate::tween_types::Animatable;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::sync::Arc;

/// An animation that applies a Tween to a parent animation.
///
/// Takes an `Animation<f32>` (0.0 to 1.0) and applies a Tween to transform
/// it into an `Animation<T>` for any type T that implements `Animatable`.
///
/// # Type Parameters
///
/// * `T` - The output type (e.g., Color, Size, Offset)
/// * `A` - The Tween type that can transform f32 to T
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, TweenAnimation, Animation};
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
/// let tween = FloatTween::new(0.0, 100.0);
/// let float_animation = TweenAnimation::new(
///     tween,
///     controller as Arc<dyn Animation<f32>>,
/// );
/// ```
#[derive(Clone)]
pub struct TweenAnimation<T, A>
where
    T: Clone + Send + Sync + 'static,
    A: Animatable<T> + Clone + Send + Sync + 'static,
{
    tween: A,
    parent: Arc<dyn Animation<f32>>,
    notifier: Arc<ChangeNotifier>,
    /// Re-emits parent value changes to our listeners; removed on last drop.
    _parent_sub: Arc<ParentSubscription>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, A> TweenAnimation<T, A>
where
    T: Clone + Send + Sync + 'static,
    A: Animatable<T> + Clone + Send + Sync + 'static,
{
    /// Create a new tween animation.
    ///
    /// # Arguments
    ///
    /// * `tween` - The tween that maps f32 → T
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    #[must_use]
    pub fn new(tween: A, parent: Arc<dyn Animation<f32>>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);

        Self {
            tween,
            parent,
            notifier,
            _parent_sub: parent_sub,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get a reference to the tween.
    #[inline]
    #[must_use]
    pub fn tween(&self) -> &A {
        &self.tween
    }

    /// Get a reference to the parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> &Arc<dyn Animation<f32>> {
        &self.parent
    }
}

impl<T, A> Animation<T> for TweenAnimation<T, A>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
    A: Animatable<T> + Clone + Send + Sync + fmt::Debug + 'static,
{
    #[inline]
    fn value(&self) -> T {
        let t = self.parent.value();
        self.tween.transform(t)
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.parent.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.parent.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.remove_status_listener(id);
    }
}

impl<T, A> Listenable for TweenAnimation<T, A>
where
    T: Clone + Send + Sync + 'static,
    A: Animatable<T> + Clone + Send + Sync + 'static,
{
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

impl<T, A> fmt::Debug for TweenAnimation<T, A>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
    A: Animatable<T> + Clone + Send + Sync + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TweenAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .field("tween", &self.tween)
            .finish_non_exhaustive()
    }
}

/// Helper function to create a `TweenAnimation` from a Tween and parent animation.
///
/// This is a convenience function for the common case.
pub fn animate<T, A>(tween: A, parent: Arc<dyn Animation<f32>>) -> TweenAnimation<T, A>
where
    T: Clone + Send + Sync + 'static,
    A: Animatable<T> + Clone + Send + Sync + 'static,
{
    TweenAnimation::new(tween, parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use crate::tween_types::FloatTween;
    use flui_scheduler::Scheduler;
    use std::time::Duration;

    #[test]
    fn test_tween_animation() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let tween = FloatTween::new(0.0, 100.0);
        let animation = TweenAnimation::new(tween, controller.clone() as Arc<dyn Animation<f32>>);

        controller.set_value(0.0);
        assert_eq!(animation.value(), 0.0);

        controller.set_value(0.5);
        assert_eq!(animation.value(), 50.0);

        controller.set_value(1.0);
        assert_eq!(animation.value(), 100.0);

        controller.dispose();
    }

    #[test]
    fn test_tween_animation_status() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let tween = FloatTween::new(0.0, 100.0);
        let animation = TweenAnimation::new(tween, controller.clone() as Arc<dyn Animation<f32>>);

        assert_eq!(animation.status(), AnimationStatus::Dismissed);

        controller.forward().unwrap();
        assert_eq!(animation.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn tween_reemits_parent_value_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Same class of regression as CurvedAnimation B2: a listener on a
        // TweenAnimation must fire when the parent's value changes; previously
        // the combinator never subscribed to its parent, so tween-driven
        // rebuilds silently never happened.
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        let tween = FloatTween::new(0.0, 100.0);
        let animation = TweenAnimation::new(tween, controller.clone() as Arc<dyn Animation<f32>>);

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let _id = animation.add_listener(Arc::new(move || {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        controller.set_value(0.5);
        controller.set_value(0.7);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "tween listener must re-emit each parent change"
        );

        controller.dispose();
    }

    #[test]
    fn dropping_tween_removes_parent_subscription() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        let before = controller.debug_value_listener_count();
        {
            let _animation = TweenAnimation::new(
                FloatTween::new(0.0, 1.0),
                controller.clone() as Arc<dyn Animation<f32>>,
            );
            assert_eq!(
                controller.debug_value_listener_count(),
                before + 1,
                "constructing a tween combinator subscribes once to the parent"
            );
        }
        assert_eq!(
            controller.debug_value_listener_count(),
            before,
            "dropping the tween combinator removes its parent subscription"
        );
        controller.dispose();
    }

    #[test]
    fn test_animate_helper() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let tween = FloatTween::new(10.0, 20.0);
        let animation = animate(tween, controller.clone() as Arc<dyn Animation<f32>>);

        controller.set_value(0.5);
        assert_eq!(animation.value(), 15.0);

        controller.dispose();
    }
}
