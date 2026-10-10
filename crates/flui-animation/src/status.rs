//! Animation status and behavior types.

/// An animation's directional phase, independent of run ownership and value.
///
/// Manual position changes and constant values may have a directional phase
/// without a run. A stopped run can have a settled phase at an interior value.
/// Use [`crate::Animation::is_animating`] for an installed run, the value for
/// position and [`crate::AnimationRunFuture`] for its completion or cancellation.
///
/// # Examples
///
/// ```
/// use flui_animation::AnimationStatus;
///
/// let status = AnimationStatus::Forward;
/// assert!(status.is_directional());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
#[non_exhaustive]
pub enum AnimationStatus {
    /// The reverse-settled phase, also used for the lower bound.
    #[default]
    Dismissed,

    /// The forward directional phase; it does not imply an installed run.
    Forward,

    /// The reverse directional phase; it does not imply an installed run.
    Reverse,

    /// The forward-settled phase, also used for the upper bound.
    Completed,
}

impl AnimationStatus {
    /// Whether the phase is [`Forward`](Self::Forward) or [`Reverse`](Self::Reverse).
    /// This does not imply an installed run.
    #[inline]
    #[must_use]
    pub const fn is_directional(&self) -> bool {
        matches!(self, AnimationStatus::Forward | AnimationStatus::Reverse)
    }

    /// Whether the phase is [`Dismissed`](Self::Dismissed) or [`Completed`](Self::Completed).
    /// This does not imply arrival at a value's endpoint or successful completion.
    #[inline]
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        matches!(
            self,
            AnimationStatus::Dismissed | AnimationStatus::Completed
        )
    }

    /// Whether the phase is [`Dismissed`](Self::Dismissed).
    #[inline]
    #[must_use]
    pub const fn is_dismissed(&self) -> bool {
        matches!(self, AnimationStatus::Dismissed)
    }

    /// Whether the phase is [`Completed`](Self::Completed).
    #[inline]
    #[must_use]
    pub const fn is_completed(&self) -> bool {
        matches!(self, AnimationStatus::Completed)
    }

    /// Whether the phase is [`Forward`](Self::Forward).
    #[inline]
    #[must_use]
    pub const fn is_forward(&self) -> bool {
        matches!(self, AnimationStatus::Forward)
    }

    /// Whether the phase is [`Reverse`](Self::Reverse).
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
