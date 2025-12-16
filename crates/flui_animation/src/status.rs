//! Animation status and behavior types.

/// The status of an animation.
///
/// Similar to Flutter's `AnimationStatus`.
///
/// # Examples
///
/// ```
/// use flui_animation::AnimationStatus;
///
/// let status = AnimationStatus::Forward;
/// assert!(status.is_running());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
#[non_exhaustive]
pub enum AnimationStatus {
    /// The animation is stopped at the beginning.
    #[default]
    Dismissed,

    /// The animation is running from beginning to end.
    Forward,

    /// The animation is running backwards, from end to beginning.
    Reverse,

    /// The animation is stopped at the end.
    Completed,
}

impl AnimationStatus {
    /// Returns true if the animation is running (forward or reverse).
    #[inline]
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, AnimationStatus::Forward | AnimationStatus::Reverse)
    }

    /// Returns true if the animation is stopped (dismissed or completed).
    #[inline]
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        matches!(
            self,
            AnimationStatus::Dismissed | AnimationStatus::Completed
        )
    }

    /// Returns true if the animation is at the beginning (dismissed).
    #[inline]
    #[must_use]
    pub const fn is_dismissed(&self) -> bool {
        matches!(self, AnimationStatus::Dismissed)
    }

    /// Returns true if the animation is at the end (completed).
    #[inline]
    #[must_use]
    pub const fn is_completed(&self) -> bool {
        matches!(self, AnimationStatus::Completed)
    }

    /// Returns true if the animation is running forward.
    #[inline]
    #[must_use]
    pub const fn is_forward(&self) -> bool {
        matches!(self, AnimationStatus::Forward)
    }

    /// Returns true if the animation is running in reverse.
    #[inline]
    #[must_use]
    pub const fn is_reverse(&self) -> bool {
        matches!(self, AnimationStatus::Reverse)
    }

    /// Returns the opposite direction status.
    ///
    /// - Forward → Reverse
    /// - Reverse → Forward
    /// - Dismissed/Completed → unchanged
    #[inline]
    #[must_use]
    pub const fn flip(&self) -> Self {
        match self {
            AnimationStatus::Forward => AnimationStatus::Reverse,
            AnimationStatus::Reverse => AnimationStatus::Forward,
            AnimationStatus::Dismissed => AnimationStatus::Dismissed,
            AnimationStatus::Completed => AnimationStatus::Completed,
        }
    }
}

/// Configures how an animation should behave when not in view.
///
/// Similar to Flutter's `AnimationBehavior`.
///
/// # Examples
///
/// ```
/// use flui_animation::AnimationBehavior;
///
/// let behavior = AnimationBehavior::Normal;
/// assert!(!behavior.should_preserve());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationBehavior {
    /// The animation will run normally.
    #[default]
    Normal,

    /// The animation will preserve its state when not in view.
    ///
    /// This is useful for animations that should not tick when the widget
    /// is not visible, but should resume when it becomes visible again.
    Preserve,
}

impl AnimationBehavior {
    /// Returns true if the animation should preserve its state.
    #[inline]
    #[must_use]
    pub const fn should_preserve(&self) -> bool {
        matches!(self, AnimationBehavior::Preserve)
    }

    /// Returns true if the animation should run normally.
    #[inline]
    #[must_use]
    pub const fn is_normal(&self) -> bool {
        matches!(self, AnimationBehavior::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_status_default() {
        assert_eq!(AnimationStatus::default(), AnimationStatus::Dismissed);
    }

    #[test]
    fn test_animation_status_is_running() {
        assert!(AnimationStatus::Forward.is_running());
        assert!(AnimationStatus::Reverse.is_running());
        assert!(!AnimationStatus::Dismissed.is_running());
        assert!(!AnimationStatus::Completed.is_running());
    }

    #[test]
    fn test_animation_status_is_stopped() {
        assert!(!AnimationStatus::Forward.is_stopped());
        assert!(!AnimationStatus::Reverse.is_stopped());
        assert!(AnimationStatus::Dismissed.is_stopped());
        assert!(AnimationStatus::Completed.is_stopped());
    }

    #[test]
    fn test_animation_status_is_dismissed() {
        assert!(AnimationStatus::Dismissed.is_dismissed());
        assert!(!AnimationStatus::Forward.is_dismissed());
    }

    #[test]
    fn test_animation_status_is_completed() {
        assert!(AnimationStatus::Completed.is_completed());
        assert!(!AnimationStatus::Forward.is_completed());
    }

    #[test]
    fn test_animation_status_is_forward() {
        assert!(AnimationStatus::Forward.is_forward());
        assert!(!AnimationStatus::Reverse.is_forward());
    }

    #[test]
    fn test_animation_status_is_reverse() {
        assert!(AnimationStatus::Reverse.is_reverse());
        assert!(!AnimationStatus::Forward.is_reverse());
    }

    #[test]
    fn test_animation_behavior_default() {
        assert_eq!(AnimationBehavior::default(), AnimationBehavior::Normal);
    }

    #[test]
    fn test_animation_behavior_should_preserve() {
        assert!(AnimationBehavior::Preserve.should_preserve());
        assert!(!AnimationBehavior::Normal.should_preserve());
    }

    #[test]
    fn test_animation_behavior_is_normal() {
        assert!(AnimationBehavior::Normal.is_normal());
        assert!(!AnimationBehavior::Preserve.is_normal());
    }

    #[test]
    fn test_animation_status_flip() {
        assert_eq!(AnimationStatus::Forward.flip(), AnimationStatus::Reverse);
        assert_eq!(AnimationStatus::Reverse.flip(), AnimationStatus::Forward);
        assert_eq!(
            AnimationStatus::Dismissed.flip(),
            AnimationStatus::Dismissed
        );
        assert_eq!(
            AnimationStatus::Completed.flip(),
            AnimationStatus::Completed
        );
    }
}
