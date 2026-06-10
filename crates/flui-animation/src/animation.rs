//! Core animation trait and types.

use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use std::fmt;
use std::sync::Arc;

/// Callback for animation status changes.
///
/// Called when an animation's status changes (e.g., from Forward to Completed).
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

/// The direction an animation is running.
///
/// This enum represents whether an animation is progressing forward (from begin to end)
/// or in reverse (from end to begin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationDirection {
    /// Animation is running forward (from begin to end).
    #[default]
    Forward,
    /// Animation is running in reverse (from end to begin).
    Reverse,
}

impl AnimationDirection {
    /// Returns the opposite direction.
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            AnimationDirection::Forward => AnimationDirection::Reverse,
            AnimationDirection::Reverse => AnimationDirection::Forward,
        }
    }
}

/// An animation that progresses from 0.0 to 1.0, or from begin to end.
///
/// This is the foundation of FLUI's animation system. All animations implement
/// this trait and the `Listenable` trait, allowing widgets to rebuild when the
/// animation value changes.
///
/// # Type Parameter
///
/// * `T` - The type of value this animation produces (e.g., `f32`, `Color`, `Size`)
///
/// # Thread Safety
///
/// All animations must be thread-safe (`Send + Sync`).
///
/// # Examples
///
/// ```
/// use flui_animation::Animation;
/// use flui_animation::AnimationStatus;
///
/// fn use_animation<T: Clone + Send + Sync + 'static>(animation: &dyn Animation<T>) {
///     let value = animation.value();
///     let status = animation.status();
///
///     if status.is_running() {
///         // Animation is in progress
///     }
/// }
/// ```
pub trait Animation<T>: Listenable + Send + Sync + fmt::Debug
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns the current value of the animation.
    fn value(&self) -> T;

    /// Returns the current status of the animation.
    fn status(&self) -> AnimationStatus;

    /// Add a status listener (called when animation starts, completes, etc.).
    ///
    /// Returns a listener ID that can be used to remove the listener later.
    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId;

    /// Remove a status listener.
    fn remove_status_listener(&self, id: ListenerId);

    /// Whether the animation is currently running.
    #[inline]
    fn is_animating(&self) -> bool {
        self.status().is_running()
    }

    /// Whether the animation is completed.
    #[inline]
    fn is_completed(&self) -> bool {
        self.status().is_completed()
    }

    /// Whether the animation is dismissed (at beginning).
    #[inline]
    fn is_dismissed(&self) -> bool {
        self.status().is_dismissed()
    }

    /// Whether the animation is running forward.
    #[inline]
    fn is_forward(&self) -> bool {
        self.status().is_forward()
    }

    /// Whether the animation is running in reverse.
    #[inline]
    fn is_reverse(&self) -> bool {
        self.status().is_reverse()
    }
}

/// A shared handle that removes a combinator's value-subscription from its
/// parent animation when the **last** clone of the combinator is dropped.
///
/// Combinators (`CurvedAnimation`, `ReverseAnimation`, ...) re-emit their
/// parent's value changes to their own listeners by subscribing to the parent.
/// Because combinators derive `Clone` and share one `notifier` across clones,
/// the subscription is reference-counted here and torn down exactly once, on the
/// final drop — never while a sibling clone is still alive.
pub(crate) struct ParentSubscription {
    // `Mutex<Option<…>>` makes the `Send`-only teardown closure `Sync` so the
    // enclosing combinator stays `Send + Sync`; the closure runs once, on drop.
    teardown: parking_lot::Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl ParentSubscription {
    fn new(teardown: impl FnMut() + Send + 'static) -> Arc<Self> {
        Arc::new(Self {
            teardown: parking_lot::Mutex::new(Some(Box::new(teardown))),
        })
    }
}

impl fmt::Debug for ParentSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParentSubscription").finish_non_exhaustive()
    }
}

impl Drop for ParentSubscription {
    fn drop(&mut self) {
        if let Some(mut teardown) = self.teardown.lock().take() {
            teardown();
        }
    }
}

/// Subscribe `notifier` to re-emit whenever `parent`'s value changes, returning
/// a shared [`ParentSubscription`] that removes the subscription on the last
/// combinator clone's drop.
///
/// The parent's callback holds only a `Weak` reference to `notifier`, so the
/// subscription never keeps the combinator's notifier alive on its own.
pub(crate) fn link_parent<T>(
    parent: &Arc<dyn Animation<T>>,
    notifier: &Arc<ChangeNotifier>,
) -> Arc<ParentSubscription>
where
    T: Clone + Send + Sync + 'static,
{
    let weak = Arc::downgrade(notifier);
    let id = parent.add_listener(Arc::new(move || {
        if let Some(notifier) = weak.upgrade() {
            notifier.notify_listeners();
        }
    }));
    let parent = Arc::clone(parent);
    ParentSubscription::new(move || parent.remove_listener(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_direction_flip() {
        assert_eq!(
            AnimationDirection::Forward.flip(),
            AnimationDirection::Reverse
        );
        assert_eq!(
            AnimationDirection::Reverse.flip(),
            AnimationDirection::Forward
        );
    }

    #[test]
    fn test_animation_status_helpers() {
        assert!(AnimationStatus::Forward.is_running());
        assert!(AnimationStatus::Reverse.is_running());
        assert!(!AnimationStatus::Dismissed.is_running());
        assert!(!AnimationStatus::Completed.is_running());

        assert!(AnimationStatus::Completed.is_completed());
        assert!(!AnimationStatus::Forward.is_completed());

        assert!(AnimationStatus::Dismissed.is_dismissed());
        assert!(!AnimationStatus::Completed.is_dismissed());
    }
}
