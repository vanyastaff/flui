//! Error types for the animation system.

/// Errors that can occur when using an [`AnimationController`](crate::AnimationController).
///
/// This enum represents all possible error conditions that can occur
/// during animation operations.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, AnimationError};
/// use std::time::Duration;
///
/// let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
/// let controller = owner.controller().clone();
///
/// // The owner closes the shared controller.
/// owner.dispose();
///
/// // Now operations will return AnimationError::Disposed
/// let result = controller.forward();
/// assert!(matches!(result, Err(AnimationError::Disposed)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationError {
    /// The [`AnimationController`](crate::AnimationController) has been disposed.
    ///
    /// This error occurs when attempting to use a controller after
    /// retiring its [`DrivenController`](crate::DrivenController) owner.
    #[error("AnimationController has been disposed")]
    Disposed,

    /// This controller permanently consumed its run or sample identities.
    /// New runs refuse before mutation; a final admitted run may still finish.
    #[error("AnimationController identities exhausted")]
    IdentityExhausted,

    /// User motion callbacks repeatedly changed the sampled controller before admission.
    /// The latest installed run remains intact; the caller can retry later.
    #[error("motion changed reentrantly while preparing a retarget")]
    ReentrantMotion,

    /// Invalid animation bounds, or an invalid `repeat`/`repeat_with` range,
    /// were provided.
    ///
    /// Returned by [`ValueRange::new`](crate::ValueRange::new)
    /// unless both bounds are finite, `lower_bound < upper_bound`, AND
    /// `upper_bound - lower_bound` itself fits in `f64` — two finite
    /// endpoints do not by themselves make a finite range
    /// (`(-f64::MAX, f64::MAX)` has a span of `f64::INFINITY`).
    ///
    /// Also returned by [`repeat_with`](crate::AnimationController::repeat_with)
    /// for a range-SHAPE error: a caller-supplied `min`/`max` that is `NaN`,
    /// or an inverted/equal pair, on ANY controller. Distinct from
    /// [`NonFiniteTarget`](Self::NonFiniteTarget), which `repeat_with`
    /// returns instead when the range shape is fine but its EFFECTIVE value
    /// (after defaulting an unset endpoint to this controller's own bound)
    /// is still non-finite.
    #[error("Invalid animation bounds: {0}")]
    InvalidBounds(String),

    /// Invalid spring configuration for fling animation.
    ///
    /// This error occurs when an underdamped spring (which oscillates)
    /// is used with [`AnimationController::fling()`](crate::AnimationController::fling).
    /// Use [`AnimationController::animate_with()`](crate::AnimationController::animate_with)
    /// for oscillating springs.
    #[error("Invalid spring configuration: {0}")]
    InvalidSpring(String),

    /// A caller-supplied value-space input (a `target`, a `from`, a fling
    /// `velocity`, or a simulation's initial sample) was not finite, or
    /// clamps to a bound this controller does not have.
    ///
    /// `NaN` is always refused — there is no finite value to repair toward.
    /// A `+-inf` input is refused only when the bound it would clamp to is
    /// itself non-finite (an [`unbounded`](crate::AnimationControllerBuilder::unbounded)
    /// controller); on a bounded controller it clamps to that bound instead
    /// (the "go to the end" idiom).
    ///
    /// Returned by [`forward`](crate::AnimationController::forward)/[`forward_from`](crate::AnimationController::forward_from),
    /// [`reverse`](crate::AnimationController::reverse)/[`reverse_from`](crate::AnimationController::reverse_from),
    /// [`animate_to`](crate::AnimationController::animate_to)/[`animate_back`](crate::AnimationController::animate_back)
    /// (and their `_curved` variants), [`fling`](crate::AnimationController::fling)/[`fling_with`](crate::AnimationController::fling_with),
    /// [`animate_with`](crate::AnimationController::animate_with)/[`animate_back_with`](crate::AnimationController::animate_back_with),
    /// and [`repeat`](crate::AnimationController::repeat)/[`repeat_with`](crate::AnimationController::repeat_with)
    /// when its *effective* range (after defaulting) is not finite. Every
    /// refusal is also emitted as a `tracing::warn!` — see
    /// `crates/flui-animation/docs/ARCHITECTURE.md`'s mapping entry for why.
    #[error("Non-finite target: {0}")]
    NonFiniteTarget(String),
}
