//! `ConstantAnimation` - an animation that always returns the same value.

use crate::animation::Animation;
use crate::status::AnimationStatus;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::fmt;

/// An animation that always returns the same value.
///
/// This is useful when an API expects an animation but you don't actually
/// want to animate anything. Using a constant animation involves less overhead
/// than building an [`AnimationController`] with a fixed value.
///
/// # Examples
///
/// ```
/// use flui_animation::{ConstantAnimation, Animation};
/// use flui_animation::AnimationStatus;
///
/// // Create a constant animation with value 0.5
/// let animation = ConstantAnimation::new(0.5);
/// assert_eq!(animation.value(), 0.5);
/// assert_eq!(animation.status(), AnimationStatus::Forward);
///
/// // A settled phase does not imply a particular value or a completed run.
/// let stopped = ConstantAnimation::with_status(0.5, AnimationStatus::ForwardSettled);
/// assert_eq!(stopped.status(), AnimationStatus::ForwardSettled);
/// assert_eq!(stopped.value(), 0.5);
///
/// let reverse = ConstantAnimation::with_status(0.5, AnimationStatus::ReverseSettled);
/// assert_eq!(reverse.status(), AnimationStatus::ReverseSettled);
/// ```
///
/// [`AnimationController`]: crate::AnimationController
#[derive(Clone)]
pub struct ConstantAnimation<T>
where
    T: Clone + 'static,
{
    value: T,
    status: AnimationStatus,
}

impl<T> ConstantAnimation<T>
where
    T: Clone + 'static,
{
    /// Creates a new constant animation with the given value.
    ///
    /// The status defaults to [`AnimationStatus::Forward`].
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value,
            status: AnimationStatus::Forward,
        }
    }

    /// Creates a constant animation with a custom status.
    #[must_use]
    pub fn with_status(value: T, status: AnimationStatus) -> Self {
        Self { value, status }
    }
}

impl<T> Animation<T> for ConstantAnimation<T>
where
    T: Clone + 'static,
{
    #[inline]
    fn value(&self) -> T {
        self.value.clone()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.status
    }

    fn is_animating(&self) -> bool {
        false
    }

    fn observe_status(&self, observer: crate::StatusObserver) -> crate::StatusSubscription {
        drop(observer);
        crate::StatusSubscription::default()
    }
}

impl<T> Listenable for ConstantAnimation<T>
where
    T: Clone + 'static,
{
    fn add_listener(&self, _callback: ListenerCallback) -> ListenerId {
        // No registration is admitted. Core notifiers check the successor
        // before minting an ID, so MAX is outside their allocated namespace.
        ListenerId::new(usize::MAX)
    }

    fn add_observer(&self, _observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        // Retire the inert relay directly, without the default wrapper allocation.
        ListenerId::new(usize::MAX)
    }

    fn remove_listener(&self, _id: ListenerId) {
        // No-op since we don't store listeners.
    }

    fn remove_all_listeners(&self) {
        // No-op since we don't store listeners.
    }
}

impl<T> fmt::Debug for ConstantAnimation<T>
where
    T: Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConstantAnimation")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Pre-defined Constants
// ============================================================================

/// A fixed value at the end of the normalized unit interval (`1.0`).
///
/// This is useful when an API expects an animation but you want to show
/// the final state immediately.
///
/// # Examples
///
/// ```
/// use flui_animation::{UNIT_END_ANIMATION, Animation};
/// use flui_animation::AnimationStatus;
///
/// assert_eq!(UNIT_END_ANIMATION.value(), 1.0);
/// assert_eq!(UNIT_END_ANIMATION.status(), AnimationStatus::ForwardSettled);
/// ```
pub const UNIT_END_ANIMATION: ConstantAnimation<f64> = ConstantAnimation {
    value: 1.0,
    status: AnimationStatus::ForwardSettled,
};

/// A fixed value at the start of the normalized unit interval (`0.0`).
///
/// This is useful when an API expects an animation but you want to show
/// the initial state.
///
/// # Examples
///
/// ```
/// use flui_animation::{UNIT_START_ANIMATION, Animation};
/// use flui_animation::AnimationStatus;
///
/// assert_eq!(UNIT_START_ANIMATION.value(), 0.0);
/// assert_eq!(UNIT_START_ANIMATION.status(), AnimationStatus::ReverseSettled);
/// ```
pub const UNIT_START_ANIMATION: ConstantAnimation<f64> = ConstantAnimation {
    value: 0.0,
    status: AnimationStatus::ReverseSettled,
};
