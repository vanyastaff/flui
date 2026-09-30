//! `ConstantAnimation` - an animation that always returns the same value.

use crate::animation::{Animation, StatusCallback};
use crate::status::AnimationStatus;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counter for generating unique listener IDs (starts at 1, not 0).
///
/// These ids live in `ConstantAnimation`'s own namespace: they are only ever
/// handed back to [`ConstantAnimation::remove_listener`]-style no-ops and are
/// never valid for any `ChangeNotifier`-backed animation (which mints ids
/// from its own counter).
static LISTENER_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

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
/// # Examples
///
/// ```
/// use flui_animation::{ALWAYS_COMPLETE, Animation};
/// use flui_animation::AnimationStatus;
///
/// assert_eq!(ALWAYS_COMPLETE.value(), 1.0);
/// assert_eq!(ALWAYS_COMPLETE.status(), AnimationStatus::Completed);
/// ```
pub static ALWAYS_COMPLETE: ConstantAnimation<f64> = ConstantAnimation {
    value: 1.0,
    status: AnimationStatus::Completed,
};

/// An animation that is always dismissed (value = 0.0).
///
/// This is useful when an API expects an animation but you want to show
/// the initial state.
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
pub static ALWAYS_DISMISSED: ConstantAnimation<f64> = ConstantAnimation {
    value: 0.0,
    status: AnimationStatus::Dismissed,
};
