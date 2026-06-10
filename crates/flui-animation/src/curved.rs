//! `CurvedAnimation` - applies easing curves to animations.

use crate::animation::{Animation, ParentSubscription, StatusCallback, link_parent};
use crate::curve::Curve;
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::sync::Arc;

/// An animation that applies a curve to another animation.
///
/// Takes an `Animation<f32>` (typically an `AnimationController`) and applies
/// an easing curve to transform the linear 0.0..1.0 progression into a
/// non-linear progression.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, CurvedAnimation};
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
/// let curved = CurvedAnimation::new(controller, Curves::EaseInOut);
/// ```
#[derive(Clone)]
pub struct CurvedAnimation<C: Curve + Clone + Send + Sync> {
    parent: Arc<dyn Animation<f32>>,
    curve: C,
    reverse_curve: Option<C>,
    notifier: Arc<ChangeNotifier>,
    /// Re-emits parent value changes to our listeners; removed on last drop.
    _parent_sub: Arc<ParentSubscription>,
}

impl<C: Curve + Clone + Send + Sync> CurvedAnimation<C> {
    /// Create a new curved animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation (typically 0.0 to 1.0)
    /// * `curve` - The curve to apply
    #[must_use]
    pub fn new(parent: Arc<dyn Animation<f32>>, curve: C) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);

        Self {
            parent,
            curve,
            reverse_curve: None,
            notifier,
            _parent_sub: parent_sub,
        }
    }

    /// Set a different curve for reverse animation.
    #[must_use]
    pub fn with_reverse_curve(mut self, reverse_curve: C) -> Self {
        self.reverse_curve = Some(reverse_curve);
        self
    }

    /// Get the current curve being used (respects reverse).
    #[inline]
    fn current_curve(&self) -> &C {
        match self.parent.status() {
            AnimationStatus::Reverse => self.reverse_curve.as_ref().unwrap_or(&self.curve),
            _ => &self.curve,
        }
    }
}

impl<C: Curve + Clone + Send + Sync + fmt::Debug + 'static> Animation<f32> for CurvedAnimation<C> {
    #[inline]
    fn value(&self) -> f32 {
        let t = self.parent.value();
        let curve = self.current_curve();
        curve.transform(t)
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

impl<C: Curve + Clone + Send + Sync> Listenable for CurvedAnimation<C> {
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

impl<C: Curve + Clone + Send + Sync + fmt::Debug + 'static> fmt::Debug for CurvedAnimation<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value_str = format!("{:.3}", self.value());
        f.debug_struct("CurvedAnimation")
            .field("value", &value_str)
            .field("status", &self.status())
            .field("curve", &self.curve)
            .field("has_reverse_curve", &self.reverse_curve.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use crate::curve::Curves;
    use flui_scheduler::Scheduler;
    use std::time::Duration;

    #[test]
    fn test_curved_animation() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let curved = CurvedAnimation::new(
            controller.clone() as Arc<dyn Animation<f32>>,
            Curves::EaseIn,
        );

        controller.set_value(0.5);
        let curved_value = curved.value();

        // Ease-in should make 0.5 appear slower (less than 0.5)
        assert!(curved_value < 0.5);

        controller.dispose();
    }

    #[test]
    fn test_curved_animation_status() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let curved = CurvedAnimation::new(
            controller.clone() as Arc<dyn Animation<f32>>,
            Curves::Linear,
        );

        assert_eq!(curved.status(), AnimationStatus::Dismissed);

        let _ = controller.forward();
        assert_eq!(curved.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn curved_reemits_parent_value_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // B2 regression: a listener on a CurvedAnimation must fire when the
        // parent's value changes. Previously the combinator never subscribed to
        // its parent, so AnimatedBuilder-on-a-curve silently never rebuilt.
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        let curved = CurvedAnimation::new(
            controller.clone() as Arc<dyn Animation<f32>>,
            Curves::Linear,
        );

        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = Arc::clone(&hits);
        let _id = curved.add_listener(Arc::new(move || {
            hits2.fetch_add(1, Ordering::SeqCst);
        }));

        controller.set_value(0.5);
        controller.set_value(0.7);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "curved listener must re-emit each parent change"
        );

        controller.dispose();
    }

    #[test]
    fn dropping_curved_removes_parent_subscription() {
        // The shared ParentSubscription must remove its listener from the parent
        // when the last clone drops, so a long-lived controller does not
        // accumulate dead callbacks.
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        let before = controller.debug_value_listener_count();
        {
            let _curved = CurvedAnimation::new(
                controller.clone() as Arc<dyn Animation<f32>>,
                Curves::Linear,
            );
            assert_eq!(
                controller.debug_value_listener_count(),
                before + 1,
                "constructing a combinator subscribes once to the parent"
            );
        }
        assert_eq!(
            controller.debug_value_listener_count(),
            before,
            "dropping the combinator removes its parent subscription"
        );
        controller.dispose();
    }
}
