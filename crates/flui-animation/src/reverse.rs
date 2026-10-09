//! `ReverseAnimation` - inverts another animation's values.

use crate::animation::{Animation, ParentLinks, StatusCallback};
use crate::status::AnimationStatus;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::rc::Rc;

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
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = Rc::new(AnimationController::builder(Duration::from_millis(300)).build());
///
/// let reversed = ReverseAnimation::new(controller.clone() as Rc<dyn Animation<f64>>);
///
/// controller.set_value(0.25);
/// assert_eq!(reversed.value(), 0.75);  // 1.0 - 0.25
/// ```
#[derive(Clone)]
pub struct ReverseAnimation {
    links: Rc<ParentLinks>,
}

impl ReverseAnimation {
    /// Subscribe to this animation's status until the returned guard is dropped.
    pub fn subscribe_status(&self, callback: StatusCallback) -> crate::StatusSubscription {
        self.links.subscribe_status(callback)
    }

    /// Create a new reverse animation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent animation to reverse
    #[must_use]
    pub fn new(parent: Rc<dyn Animation<f64>>) -> Self {
        Self {
            links: ParentLinks::new(parent, reverse_status),
        }
    }

    /// Get the parent animation.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> &Rc<dyn Animation<f64>> {
        &self.links.parent
    }
}

impl Animation<f64> for ReverseAnimation {
    #[inline]
    fn value(&self) -> f64 {
        1.0 - self.links.parent.value()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        reverse_status(self.links.parent.status())
    }

    fn is_animating(&self) -> bool {
        self.links.parent.is_animating()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.links
            .status_notifier
            .add(Rc::new(move |status| callback(*status)))
    }

    fn add_status_observer(&self, observer: crate::animation::StatusObserver) -> ListenerId {
        self.links
            .status_notifier
            .add_with_recovery(Rc::new(move |status, recovery| observer(*status, recovery)))
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.links.status_notifier.remove_even_if_disposed(id);
    }
}

fn reverse_status(status: AnimationStatus) -> AnimationStatus {
    match status {
        AnimationStatus::Forward => AnimationStatus::Reverse,
        AnimationStatus::Reverse => AnimationStatus::Forward,
        AnimationStatus::Dismissed => AnimationStatus::Completed,
        AnimationStatus::Completed => AnimationStatus::Dismissed,
    }
}

impl Listenable for ReverseAnimation {
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.links.notifier.add_observer(observer)
    }
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.links.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.links.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.links.notifier.remove_all_listeners();
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
