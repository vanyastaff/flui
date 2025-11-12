//! Core animation trait and types.

use flui_core::foundation::{Listenable, ListenerId};
use flui_types::animation::AnimationStatus;
use std::fmt;
use std::sync::Arc;

/// Callback for animation status changes.
///
/// Called when an animation's status changes (e.g., from Forward to Completed).
pub type StatusCallback = Arc<dyn Fn(AnimationStatus) + Send + Sync>;

/// The direction an animation is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationDirection {
    /// Animation is running forward (from begin to end).
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
/// use flui_types::animation::AnimationStatus;
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

/// Type-erased animation trait object.
///
/// This allows storing animations of different types in collections.
///
/// # Examples
///
/// ```
/// use flui_animation::DynAnimation;
/// use std::sync::Arc;
///
/// let animations: Vec<Arc<dyn DynAnimation<f32>>> = vec![];
/// ```
pub trait DynAnimation<T: Clone + Send + Sync + 'static>: Animation<T> + Listenable {}

// Blanket implementation for all types that implement Animation
impl<T, A> DynAnimation<T> for A
where
    T: Clone + Send + Sync + 'static,
    A: Animation<T> + Listenable + ?Sized,
{
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
