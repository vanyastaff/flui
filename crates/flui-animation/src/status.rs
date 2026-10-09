//! Animation status and behavior types.

/// The status of an animation.
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

/// Selects a controller's timeline and response to presentation motion policy.
///
/// The controller builder stores this immutable configuration. Its presentation's
/// registry selects the corresponding timeline and applies reduced-motion
/// settlement when delivering a frame.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_animation::{AnimationBehavior, AnimationController};
///
/// // A display timer keeps its authored duration under reduced motion.
/// let timer = AnimationController::builder(Duration::from_secs(3))
///     .behavior(AnimationBehavior::Preserve)
///     .build();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationBehavior {
    /// Honor reduced motion and the host duration scale.
    #[default]
    Normal,

    /// Keep authored timing under any host motion setting or application policy.
    /// Debug playback controls and registry muting still apply.
    Preserve,
}
