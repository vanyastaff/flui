//! ReverseAnimation - inverts another animation's values.

use crate::animation::{Animation, StatusCallback};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_types::animation::AnimationStatus;
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// An animation that inverts another animation.
///
/// ReverseAnimation runs in the opposite direction from its parent:
/// - When parent = 0.0, ReverseAnimation = 1.0
/// - When parent = 0.5, ReverseAnimation = 0.5
/// - When parent = 1.0, ReverseAnimation = 0.0
///
/// The status is also reversed:
/// - Forward becomes Reverse
/// - Reverse becomes Forward
/// - Dismissed becomes Completed
/// - Completed becomes Dismissed
///
/// # Examples
///
/// ```
/// use flui_animation::{ReverseAnimation, AnimationController, Animation};
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
/// let reversed = ReverseAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);
///
/// controller.set_value(0.25);
/// assert_eq!(reversed.value(), 0.75);  // 1.0 - 0.25
/// ```
#[derive(Clone)]
pub struct ReverseAnimation {
    parent: Arc<dyn Animation<f32>>,
    notifier: Arc<ChangeNotifier>,
    _parent_listener_id: Arc<Mutex<Option<ListenerId>>>,
}

impl ReverseAnimation {
    /// Create a new reverse animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation to reverse
    #[must_use]
    pub fn new(parent: Arc<dyn Animation<f32>>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        Self {
            parent,
            notifier,
            _parent_listener_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> &Arc<dyn Animation<f32>> {
        &self.parent
    }
}

impl Animation<f32> for ReverseAnimation {
    #[inline]
    fn value(&self) -> f32 {
        1.0 - self.parent.value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        match self.parent.status() {
            AnimationStatus::Forward => AnimationStatus::Reverse,
            AnimationStatus::Reverse => AnimationStatus::Forward,
            AnimationStatus::Dismissed => AnimationStatus::Completed,
            AnimationStatus::Completed => AnimationStatus::Dismissed,
            // Handle future variants (AnimationStatus is #[non_exhaustive])
            _ => self.parent.status(),
        }
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        // Wrap the callback to reverse the status
        let reversed_callback = Arc::new(move |status: AnimationStatus| {
            let reversed_status = match status {
                AnimationStatus::Forward => AnimationStatus::Reverse,
                AnimationStatus::Reverse => AnimationStatus::Forward,
                AnimationStatus::Dismissed => AnimationStatus::Completed,
                AnimationStatus::Completed => AnimationStatus::Dismissed,
                // Handle future variants (AnimationStatus is #[non_exhaustive])
                _ => status,
            };
            callback(reversed_status);
        });

        self.parent.add_status_listener(reversed_callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.remove_status_listener(id)
    }
}

impl Listenable for ReverseAnimation {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier
            
            .add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier
            
            .remove_listener(id)
    }

    fn remove_all_listeners(&self) {
        self.notifier
            
            .remove_all_listeners()
    }
}

impl fmt::Debug for ReverseAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReverseAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_scheduler::Scheduler;
    use std::time::Duration;

    #[test]
    fn test_reverse_animation_value() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let reversed = ReverseAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);

        controller.set_value(0.0);
        assert_eq!(reversed.value(), 1.0);

        controller.set_value(0.25);
        assert_eq!(reversed.value(), 0.75);

        controller.set_value(0.5);
        assert_eq!(reversed.value(), 0.5);

        controller.set_value(0.75);
        assert_eq!(reversed.value(), 0.25);

        controller.set_value(1.0);
        assert_eq!(reversed.value(), 0.0);

        controller.dispose();
    }

    #[test]
    fn test_reverse_animation_status() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let reversed = ReverseAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);

        // Dismissed → Completed
        assert_eq!(controller.status(), AnimationStatus::Dismissed);
        assert_eq!(reversed.status(), AnimationStatus::Completed);

        // Forward → Reverse
        controller.forward();
        assert_eq!(controller.status(), AnimationStatus::Forward);
        assert_eq!(reversed.status(), AnimationStatus::Reverse);

        controller.stop();

        // Reverse → Forward
        controller.reverse();
        assert_eq!(controller.status(), AnimationStatus::Reverse);
        assert_eq!(reversed.status(), AnimationStatus::Forward);

        controller.dispose();
    }

    #[test]
    fn test_reverse_animation_at_extremes() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            scheduler,
        ));

        let reversed = ReverseAnimation::new(controller.clone() as Arc<dyn Animation<f32>>);

        // At lower bound
        controller.set_value(0.0);
        assert_eq!(reversed.value(), 1.0);

        // At upper bound
        controller.set_value(1.0);
        assert_eq!(reversed.value(), 0.0);

        controller.dispose();
    }
}
