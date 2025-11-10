//! Error recovery for graceful degradation
//!
//! Provides recovery policies for handling pipeline errors gracefully
//! in production environments.
//!
//! # Recovery Policies
//!
//! - **UseLastGoodFrame**: Display last successful frame (production default)
//! - **ShowErrorWidget**: Display error widget with details (development)
//! - **SkipFrame**: Skip current frame and continue (minimal impact)
//! - **Panic**: Panic immediately (testing/debugging)
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::{ErrorRecovery, RecoveryPolicy, PipelineError, PipelinePhase};
//!
//! let mut recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);
//!
//! // Save good frame
//! // recovery.save_good_frame(layer);
//!
//! // Handle error
//! match recovery.handle_error(error, PipelinePhase::Layout) {
//!     RecoveryAction::UseLastFrame(layer) => {
//!         // Use saved frame
//!     }
//!     RecoveryAction::SkipFrame => {
//!         // Skip this frame
//!     }
//!     _ => {}
//! }
//! ```

use super::error::{PipelineError, PipelinePhase};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Recovery policy for pipeline errors
///
/// Defines how the pipeline should respond to errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPolicy {
    /// Use last good frame
    ///
    /// When an error occurs, display the last successfully rendered frame.
    /// Best for production - users see frozen UI instead of crash.
    ///
    /// **Use when:**
    /// - Production environment
    /// - User experience is priority
    /// - Graceful degradation required
    UseLastGoodFrame,

    /// Show error widget
    ///
    /// When an error occurs, display a widget showing error details.
    /// Best for development - developers see what went wrong.
    ///
    /// **Use when:**
    /// - Development environment
    /// - Debugging errors
    /// - Need error visibility
    ShowErrorWidget,

    /// Skip frame
    ///
    /// When an error occurs, skip the current frame and continue.
    /// Best for animations - prefer dropped frame over freeze.
    ///
    /// **Use when:**
    /// - High FPS animations
    /// - Can tolerate dropped frames
    /// - Want minimal impact
    SkipFrame,

    /// Panic on error
    ///
    /// When an error occurs, panic immediately.
    /// Best for testing - fail fast on any error.
    ///
    /// **Use when:**
    /// - Testing environment
    /// - Want to catch all errors
    /// - Debugging issues
    Panic,
}

impl Default for RecoveryPolicy {
    /// Default policy is UseLastGoodFrame (production-safe)
    #[inline]
    fn default() -> Self {
        Self::UseLastGoodFrame
    }
}

/// Recovery action to take
///
/// Returned by `ErrorRecovery::handle_error()` to indicate
/// what action should be taken.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Use the last good frame
    ///
    /// Caller should use their saved frame.
    /// Note: Frame storage is caller's responsibility.
    UseLastFrame,

    /// Show error widget
    ShowError(PipelineError),

    /// Skip this frame
    SkipFrame,

    /// Panic with error
    Panic(PipelineError),
}

/// Error recovery handler
///
/// Manages error recovery with configurable policies.
///
/// # Thread Safety
///
/// ErrorRecovery is thread-safe and can be shared across threads.
///
/// # Frame Storage
///
/// Note: This struct does NOT store the actual frame.
/// Frame storage is the caller's responsibility (PipelineOwner).
/// This only tracks policy and error count.
#[derive(Debug)]
pub struct ErrorRecovery {
    /// Recovery policy
    policy: RecoveryPolicy,

    /// Error count
    ///
    /// Tracks total errors encountered.
    /// Used for metrics and debugging.
    error_count: AtomicUsize,

    /// Maximum errors before giving up
    ///
    /// If error_count exceeds this, switches to Panic.
    /// Prevents infinite error loops.
    max_errors: usize,
}

impl ErrorRecovery {
    /// Create new error recovery with policy
    ///
    /// # Parameters
    ///
    /// - `policy`: Recovery policy to use
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::{ErrorRecovery, RecoveryPolicy};
    ///
    /// // Production: use last good frame
    /// let recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);
    ///
    /// // Development: show error widget
    /// let recovery = ErrorRecovery::new(RecoveryPolicy::ShowErrorWidget);
    /// ```
    #[inline]
    pub fn new(policy: RecoveryPolicy) -> Self {
        Self {
            policy,
            error_count: AtomicUsize::new(0),
            max_errors: 100, // Reasonable default
        }
    }

    /// Create error recovery with max errors limit
    ///
    /// # Parameters
    ///
    /// - `policy`: Recovery policy to use
    /// - `max_errors`: Maximum errors before panic
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::{ErrorRecovery, RecoveryPolicy};
    ///
    /// // Allow up to 10 errors before panicking
    /// let recovery = ErrorRecovery::with_max_errors(
    ///     RecoveryPolicy::UseLastGoodFrame,
    ///     10
    /// );
    /// ```
    #[inline]
    pub fn with_max_errors(policy: RecoveryPolicy, max_errors: usize) -> Self {
        Self {
            policy,
            error_count: AtomicUsize::new(0),
            max_errors,
        }
    }

    /// Handle pipeline error
    ///
    /// Applies recovery policy to the error and returns appropriate action.
    ///
    /// # Parameters
    ///
    /// - `error`: Pipeline error that occurred
    /// - `phase`: Phase where error occurred
    ///
    /// # Returns
    ///
    /// RecoveryAction indicating what to do
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// match recovery.handle_error(error, phase) {
    ///     RecoveryAction::UseLastFrame => {
    ///         // Use caller's saved frame
    ///         return Ok(saved_layer);
    ///     }
    ///     RecoveryAction::SkipFrame => {
    ///         return Err(error);
    ///     }
    ///     RecoveryAction::Panic(e) => {
    ///         panic!("Pipeline error: {}", e);
    ///     }
    ///     _ => {}
    /// }
    /// ```
    pub fn handle_error(&self, error: PipelineError, _phase: PipelinePhase) -> RecoveryAction {
        // Increment error count
        let count = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Check if exceeded max errors
        if count > self.max_errors {
            tracing::error!(
                error_count = count,
                max_errors = self.max_errors,
                "Exceeded maximum errors, panicking"
            );

            return RecoveryAction::Panic(
                PipelineError::invalid_state(format!(
                    "Exceeded maximum pipeline errors ({}/{})",
                    count, self.max_errors
                ))
                .expect("Non-empty error message"),
            );
        }

        // Log error
        tracing::warn!(
            error_count = count,
            error = %error,
            "Pipeline error"
        );

        // Apply recovery policy
        match self.policy {
            RecoveryPolicy::UseLastGoodFrame => RecoveryAction::UseLastFrame,
            RecoveryPolicy::ShowErrorWidget => RecoveryAction::ShowError(error),
            RecoveryPolicy::SkipFrame => RecoveryAction::SkipFrame,
            RecoveryPolicy::Panic => RecoveryAction::Panic(error),
        }
    }

    /// Get error count
    ///
    /// Returns total number of errors encountered.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::ErrorRecovery;
    ///
    /// let recovery = ErrorRecovery::default();
    /// assert_eq!(recovery.error_count(), 0);
    /// ```
    #[inline]
    pub fn error_count(&self) -> usize {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Reset error count
    ///
    /// Clears the error counter.
    /// Useful after recovering from temporary issues.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::ErrorRecovery;
    ///
    /// let mut recovery = ErrorRecovery::default();
    /// // ... errors occur ...
    /// recovery.reset_error_count();
    /// assert_eq!(recovery.error_count(), 0);
    /// ```
    #[inline]
    pub fn reset_error_count(&mut self) {
        self.error_count.store(0, Ordering::Relaxed);
    }

    /// Get current policy
    #[inline]
    pub fn policy(&self) -> RecoveryPolicy {
        self.policy
    }

    /// Set new policy
    #[inline]
    pub fn set_policy(&mut self, policy: RecoveryPolicy) {
        self.policy = policy;
    }
}

impl Default for ErrorRecovery {
    #[inline]
    fn default() -> Self {
        Self::new(RecoveryPolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_recovery_creation() {
        let recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);
        assert_eq!(recovery.policy(), RecoveryPolicy::UseLastGoodFrame);
        assert_eq!(recovery.error_count(), 0);
    }

    #[test]
    fn test_error_count() {
        use crate::ElementId;
        let mut recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

        let error = PipelineError::layout_error(ElementId::new(42), "test").unwrap();

        recovery.handle_error(error.clone(), PipelinePhase::Layout);
        assert_eq!(recovery.error_count(), 1);

        recovery.handle_error(error.clone(), PipelinePhase::Layout);
        assert_eq!(recovery.error_count(), 2);

        recovery.reset_error_count();
        assert_eq!(recovery.error_count(), 0);
    }

    #[test]
    fn test_skip_frame_policy() {
        use crate::ElementId;
        let recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

        let error = PipelineError::layout_error(ElementId::new(42), "test").unwrap();

        match recovery.handle_error(error, PipelinePhase::Layout) {
            RecoveryAction::SkipFrame => {
                // Expected
            }
            _ => panic!("Expected SkipFrame action"),
        }
    }

    #[test]
    fn test_show_error_policy() {
        use crate::ElementId;
        let recovery = ErrorRecovery::new(RecoveryPolicy::ShowErrorWidget);

        let error = PipelineError::paint_error(ElementId::new(42), "test").unwrap();

        match recovery.handle_error(error.clone(), PipelinePhase::Paint) {
            RecoveryAction::ShowError(e) => {
                assert_eq!(e.phase(), PipelinePhase::Paint);
            }
            _ => panic!("Expected ShowError action"),
        }
    }

    #[test]
    #[should_panic(expected = "maximum pipeline errors")]
    fn test_max_errors() {
        use crate::ElementId;
        let recovery = ErrorRecovery::with_max_errors(RecoveryPolicy::SkipFrame, 3);

        let error = PipelineError::layout_error(ElementId::new(42), "test").unwrap();

        // Should panic on 4th error
        for _ in 0..5 {
            if let RecoveryAction::Panic(e) =
                recovery.handle_error(error.clone(), PipelinePhase::Layout)
            {
                panic!("{}", e)
            }
        }
    }

    #[test]
    fn test_set_policy() {
        let mut recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);
        assert_eq!(recovery.policy(), RecoveryPolicy::UseLastGoodFrame);

        recovery.set_policy(RecoveryPolicy::SkipFrame);
        assert_eq!(recovery.policy(), RecoveryPolicy::SkipFrame);
    }
}
