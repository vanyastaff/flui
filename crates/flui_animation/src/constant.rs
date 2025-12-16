//! `ConstantAnimation` - an animation that always returns the same value.

use crate::animation::{Animation, StatusCallback};
use crate::status::AnimationStatus;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counter for generating unique listener IDs (starts at 1, not 0).
static LISTENER_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// An animation that always returns the same value.
///
/// This is useful when an API expects an animation but you don't actually
/// want to animate anything. Using a constant animation involves less overhead
/// than building an [`AnimationController`] with a fixed value.
///
/// Similar to Flutter's `AlwaysStoppedAnimation<T>`.
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
/// // Create a completed animation
/// let completed = ConstantAnimation::completed(1.0);
/// assert_eq!(completed.status(), AnimationStatus::Completed);
///
/// // Create a dismissed animation
/// let dismissed = ConstantAnimation::dismissed(0.0);
/// assert_eq!(dismissed.status(), AnimationStatus::Dismissed);
/// ```
///
/// [`AnimationController`]: crate::AnimationController
#[derive(Clone)]
pub struct ConstantAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    value: T,
    status: AnimationStatus,
}

impl<T> ConstantAnimation<T>
where
    T: Clone + Send + Sync + 'static,
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

    /// Creates a constant animation that is always completed.
    ///
    /// The status is [`AnimationStatus::Completed`].
    #[must_use]
    pub fn completed(value: T) -> Self {
        Self {
            value,
            status: AnimationStatus::Completed,
        }
    }

    /// Creates a constant animation that is always dismissed.
    ///
    /// The status is [`AnimationStatus::Dismissed`].
    #[must_use]
    pub fn dismissed(value: T) -> Self {
        Self {
            value,
            status: AnimationStatus::Dismissed,
        }
    }
}

impl<T> Animation<T> for ConstantAnimation<T>
where
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    #[inline]
    fn value(&self) -> T {
        self.value.clone()
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.status
    }

    fn add_status_listener(&self, _callback: StatusCallback) -> ListenerId {
        // Status never changes, so we don't need to store the listener.
        // Return a unique ID anyway for API consistency.
        ListenerId::new(LISTENER_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    fn remove_status_listener(&self, _id: ListenerId) {
        // No-op since we don't store listeners.
    }
}

impl<T> Listenable for ConstantAnimation<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn add_listener(&self, _callback: ListenerCallback) -> ListenerId {
        // Value never changes, so we don't need to store the listener.
        // Return a unique ID anyway for API consistency.
        ListenerId::new(LISTENER_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
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
    T: Clone + Send + Sync + fmt::Debug + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConstantAnimation")
            .field("value", &self.value)
            .field("status", &self.status)
            .finish()
    }
}

// ============================================================================
// Pre-defined Constants
// ============================================================================

/// An animation that is always complete (value = 1.0).
///
/// This is useful when an API expects an animation but you want to show
/// the final state immediately.
///
/// Similar to Flutter's `kAlwaysCompleteAnimation`.
///
/// # Examples
///
/// ```
/// use flui_animation::{ALWAYS_COMPLETE, Animation};
/// use flui_animation::AnimationStatus;
///
/// assert_eq!(ALWAYS_COMPLETE.value(), 1.0);
/// assert_eq!(ALWAYS_COMPLETE.status(), AnimationStatus::Completed);
/// ```
pub static ALWAYS_COMPLETE: ConstantAnimation<f32> = ConstantAnimation {
    value: 1.0,
    status: AnimationStatus::Completed,
};

/// An animation that is always dismissed (value = 0.0).
///
/// This is useful when an API expects an animation but you want to show
/// the initial state.
///
/// Similar to Flutter's `kAlwaysDismissedAnimation`.
///
/// # Examples
///
/// ```
/// use flui_animation::{ALWAYS_DISMISSED, Animation};
/// use flui_animation::AnimationStatus;
///
/// assert_eq!(ALWAYS_DISMISSED.value(), 0.0);
/// assert_eq!(ALWAYS_DISMISSED.status(), AnimationStatus::Dismissed);
/// ```
pub static ALWAYS_DISMISSED: ConstantAnimation<f32> = ConstantAnimation {
    value: 0.0,
    status: AnimationStatus::Dismissed,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_constant_animation_new() {
        let animation = ConstantAnimation::new(0.5);
        assert_eq!(animation.value(), 0.5);
        assert_eq!(animation.status(), AnimationStatus::Forward);
    }

    #[test]
    fn test_constant_animation_with_status() {
        let animation = ConstantAnimation::with_status(0.75, AnimationStatus::Reverse);
        assert_eq!(animation.value(), 0.75);
        assert_eq!(animation.status(), AnimationStatus::Reverse);
    }

    #[test]
    fn test_constant_animation_completed() {
        let animation = ConstantAnimation::completed(1.0);
        assert_eq!(animation.value(), 1.0);
        assert_eq!(animation.status(), AnimationStatus::Completed);
        assert!(animation.is_completed());
    }

    #[test]
    fn test_constant_animation_dismissed() {
        let animation = ConstantAnimation::dismissed(0.0);
        assert_eq!(animation.value(), 0.0);
        assert_eq!(animation.status(), AnimationStatus::Dismissed);
        assert!(animation.is_dismissed());
    }

    #[test]
    fn test_constant_animation_is_not_animating() {
        let animation = ConstantAnimation::new(0.5);
        // Forward status means is_animating returns true per Animation trait
        assert!(animation.is_animating());

        let completed = ConstantAnimation::completed(1.0);
        assert!(!completed.is_animating());

        let dismissed = ConstantAnimation::dismissed(0.0);
        assert!(!dismissed.is_animating());
    }

    #[test]
    fn test_constant_animation_generic_type() {
        // Test with a different type
        let animation = ConstantAnimation::new("hello".to_string());
        assert_eq!(animation.value(), "hello");
    }

    #[test]
    fn test_always_complete_constant() {
        assert_eq!(ALWAYS_COMPLETE.value(), 1.0);
        assert_eq!(ALWAYS_COMPLETE.status(), AnimationStatus::Completed);
    }

    #[test]
    fn test_always_dismissed_constant() {
        assert_eq!(ALWAYS_DISMISSED.value(), 0.0);
        assert_eq!(ALWAYS_DISMISSED.status(), AnimationStatus::Dismissed);
    }

    #[test]
    fn test_constant_animation_listeners_return_unique_ids() {
        let animation = ConstantAnimation::new(0.5);
        let callback1: ListenerCallback = Arc::new(|| {});
        let callback2: ListenerCallback = Arc::new(|| {});

        let id1 = animation.add_listener(callback1);
        let id2 = animation.add_listener(callback2);

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_constant_animation_debug() {
        let animation = ConstantAnimation::new(0.5);
        let debug_str = format!("{:?}", animation);
        assert!(debug_str.contains("ConstantAnimation"));
        assert!(debug_str.contains("0.5"));
    }
}
