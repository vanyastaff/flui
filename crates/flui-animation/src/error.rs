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
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
///
/// // Dispose the controller
/// controller.dispose();
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
    /// calling [`AnimationController::dispose()`](crate::AnimationController::dispose).
    #[error("AnimationController has been disposed")]
    Disposed,

    /// Invalid animation bounds were provided.
    ///
    /// This error occurs when `lower_bound >= upper_bound` in
    /// [`AnimationController::with_bounds()`](crate::AnimationController::with_bounds).
    #[error("Invalid animation bounds: {0}")]
    InvalidBounds(String),

    /// Ticker is not available.
    ///
    /// This error occurs when the animation system cannot obtain
    /// a ticker for frame synchronization.
    #[error("Ticker not available")]
    TickerNotAvailable,

    /// Invalid spring configuration for fling animation.
    ///
    /// This error occurs when an underdamped spring (which oscillates)
    /// is used with [`AnimationController::fling()`](crate::AnimationController::fling).
    /// Use [`AnimationController::animate_with()`](crate::AnimationController::animate_with)
    /// for oscillating springs.
    #[error("Invalid spring configuration: {0}")]
    InvalidSpring(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        assert_eq!(
            AnimationError::Disposed.to_string(),
            "AnimationController has been disposed"
        );

        assert_eq!(
            AnimationError::InvalidBounds("lower (1.0) >= upper (0.0)".to_string()).to_string(),
            "Invalid animation bounds: lower (1.0) >= upper (0.0)"
        );

        assert_eq!(
            AnimationError::TickerNotAvailable.to_string(),
            "Ticker not available"
        );
    }

    #[test]
    fn test_error_clone() {
        let error = AnimationError::Disposed;
        let cloned = error.clone();
        assert_eq!(error, cloned);
    }

    #[test]
    fn test_error_debug() {
        let error = AnimationError::Disposed;
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("Disposed"));
    }
}
