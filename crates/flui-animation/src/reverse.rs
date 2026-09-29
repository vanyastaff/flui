//! `ReverseAnimation` - inverts another animation's values.

use crate::animation::{Animation, ParentSubscription, StatusCallback, link_parent};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::sync::Arc;

/// An animation that inverts another animation.
///
/// `ReverseAnimation` runs in the opposite direction from its parent:
/// - When parent = 0.0, `ReverseAnimation` = 1.0
/// - When parent = 0.5, `ReverseAnimation` = 0.5
/// - When parent = 1.0, `ReverseAnimation` = 0.0
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
/// use flui_scheduler::UpdateScheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// ));
///
/// let reversed = ReverseAnimation::new(controller.clone() as Arc<dyn Animation<f64>>);
///
/// controller.set_value(0.25);
/// assert_eq!(reversed.value(), 0.75);  // 1.0 - 0.25
/// ```
#[derive(Clone)]
pub struct ReverseAnimation {
    parent: Arc<dyn Animation<f64>>,
    notifier: Arc<ChangeNotifier>,
    /// Re-emits parent value changes to our listeners; removed on last drop.
    _parent_sub: Arc<ParentSubscription>,
}

impl ReverseAnimation {
    /// Create a new reverse animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation to reverse
    #[must_use]
    pub fn new(parent: Arc<dyn Animation<f64>>) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());
        let parent_sub = link_parent(&parent, &notifier);

        Self {
            parent,
            notifier,
            _parent_sub: parent_sub,
        }
    }

    /// Get the parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> &Arc<dyn Animation<f64>> {
        &self.parent
    }
}

impl Animation<f64> for ReverseAnimation {
    #[inline]
    fn value(&self) -> f64 {
        1.0 - self.parent.value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        match self.parent.status() {
            AnimationStatus::Forward => AnimationStatus::Reverse,
            AnimationStatus::Reverse => AnimationStatus::Forward,
            AnimationStatus::Dismissed => AnimationStatus::Completed,
            AnimationStatus::Completed => AnimationStatus::Dismissed,
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
            };
            callback(reversed_status);
        });

        self.parent.add_status_listener(reversed_callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.parent.remove_status_listener(id);
    }
}

impl Listenable for ReverseAnimation {
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

impl fmt::Debug for ReverseAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReverseAnimation")
            .field("value", &self.value())
            .field("status", &self.status())
            .finish()
    }
}
