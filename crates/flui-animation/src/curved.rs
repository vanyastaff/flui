//! `CurvedAnimation` - applies easing curves to animations.

use crate::animation::{Animation, ParentSubscription, StatusCallback, link_parent};
use crate::curve::Curve;
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::Mutex;
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
    /// The running direction captured at run start; `None` at rest.
    ///
    /// Flutter parity (`CurvedAnimation._curveDirection`): the active curve is
    /// locked to the direction the run *entered* with, so flipping direction
    /// mid-run does not swap curves underneath the value and cause a visual
    /// discontinuity.
    curve_direction: Arc<Mutex<Option<AnimationStatus>>>,
    /// Re-emits parent value changes to our listeners; removed on last drop.
    _parent_sub: Arc<ParentSubscription>,
    /// Keeps `curve_direction` in sync with the parent's status transitions;
    /// removed on last drop.
    _status_sub: Arc<ParentSubscription>,
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

        let curve_direction = Arc::new(Mutex::new(None));
        let weak_direction = Arc::downgrade(&curve_direction);
        let status_id = parent.add_status_listener(Arc::new(move |status| {
            if let Some(direction) = weak_direction.upgrade() {
                let mut direction = direction.lock();
                match status {
                    // At rest the lock is released; the next run re-captures.
                    AnimationStatus::Dismissed | AnimationStatus::Completed => *direction = None,
                    // First running transition wins; mid-run flips keep it.
                    AnimationStatus::Forward | AnimationStatus::Reverse => {
                        direction.get_or_insert(status);
                    }
                }
            }
        }));
        let status_parent = Arc::clone(&parent);
        let status_sub = ParentSubscription::new(move || {
            status_parent.remove_status_listener(status_id);
        });

        Self {
            parent,
            curve,
            reverse_curve: None,
            notifier,
            curve_direction,
            _parent_sub: parent_sub,
            _status_sub: status_sub,
        }
    }

    /// Set a different curve for reverse animation.
    #[must_use]
    pub fn with_reverse_curve(mut self, reverse_curve: C) -> Self {
        self.reverse_curve = Some(reverse_curve);
        self
    }

    /// Get the current curve being used (respects reverse).
    ///
    /// Uses the direction captured at run start when running (so a mid-run
    /// direction flip keeps the entry curve), falling back to the parent's
    /// instantaneous status at rest — Flutter's `_useForwardCurve`.
    #[inline]
    fn current_curve(&self) -> &C {
        let captured: Option<AnimationStatus> = *self.curve_direction.lock();
        let effective = captured.unwrap_or_else(|| self.parent.status());
        match effective {
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
    use crate::curve::{Cubic, Curves};
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
    fn reverse_curve_locked_to_run_entry_direction() {
        // Flutter `_curveDirection` parity: a run that entered Forward keeps
        // the forward curve even if the parent's status flips to Reverse
        // mid-run; the reverse curve only applies to a run entered in
        // Reverse. Without the lock, a mid-run `reverse()` would swap curves
        // underneath the value and cause a visual jump.
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));
        // Forward curve is the identity cubic; the reverse curve is strongly
        // sub-linear at t=0.5, so any curve swap is observable there.
        let curved = CurvedAnimation::new(
            controller.clone() as Arc<dyn Animation<f32>>,
            Cubic::new(0.0, 0.0, 1.0, 1.0), // y(x) = x
        )
        .with_reverse_curve(Curves::EaseInQuint);

        controller.set_value(0.5);
        let _ = controller.forward();
        let during_forward = curved.value();

        // Flip direction mid-run: the captured Forward direction must keep
        // the (≈linear) forward curve active.
        let _ = controller.reverse();
        let during_flip = curved.value();
        assert!(
            (during_forward - during_flip).abs() < 1e-3,
            "mid-run direction flip must not swap curves (forward {during_forward} vs flipped {during_flip})"
        );

        // Settle the run, then start a fresh run in Reverse: now the reverse
        // curve applies from the start.
        controller.set_value(1.0); // Completed -> direction lock cleared
        let _ = controller.reverse();
        controller.set_value(0.5);
        let reverse_run = curved.value();
        let expected = Curves::EaseInQuint.transform(0.5);
        assert!(
            (reverse_run - expected).abs() < 1e-3,
            "a run entered in Reverse must use the reverse curve ({reverse_run} vs {expected})"
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
