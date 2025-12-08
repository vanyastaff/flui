//! Error recovery policies for pipeline failures
//!
//! Provides configurable strategies for handling errors during
//! build, layout, and paint phases.
//!
//! # Recovery Policies
//!
//! - `UseLastGoodFrame` - Production default, show last successful frame
//! - `ShowErrorWidget` - Development mode, show error overlay
//! - `SkipFrame` - Skip the failed frame, continue
//! - `Panic` - Testing mode, fail fast
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::{ErrorRecovery, RecoveryPolicy, RecoveryAction, PipelineError, PipelinePhase};
//!
//! let recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);
//! let error = PipelineError::layout_failed(1, "test");
//!
//! match recovery.handle_error(error, PipelinePhase::Layout) {
//!     RecoveryAction::SkipFrame => println!("Skipping frame"),
//!     RecoveryAction::UseLastFrame => println!("Using cached frame"),
//!     RecoveryAction::ShowError(e) => println!("Show error: {}", e),
//!     RecoveryAction::Panic(e) => panic!("Fatal: {}", e),
//! }
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{PipelineError, PipelinePhase};

/// Default maximum errors before forced panic.
const DEFAULT_MAX_ERRORS: usize = 100;

/// Recovery policy for pipeline errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RecoveryPolicy {
    /// Use last successfully rendered frame (production default)
    #[default]
    UseLastGoodFrame,

    /// Show error widget overlay (development mode)
    ShowErrorWidget,

    /// Skip the failed frame and continue
    SkipFrame,

    /// Panic on error (testing mode)
    Panic,
}

impl RecoveryPolicy {
    /// Returns the policy name as a static string.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UseLastGoodFrame => "use_last_good_frame",
            Self::ShowErrorWidget => "show_error_widget",
            Self::SkipFrame => "skip_frame",
            Self::Panic => "panic",
        }
    }

    /// Returns `true` if this is a graceful recovery policy.
    #[inline]
    #[must_use]
    pub const fn is_graceful(self) -> bool {
        matches!(self, Self::UseLastGoodFrame | Self::SkipFrame)
    }

    /// Returns `true` if this policy shows errors to users.
    #[inline]
    #[must_use]
    pub const fn shows_error(self) -> bool {
        matches!(self, Self::ShowErrorWidget | Self::Panic)
    }
}

/// Action to take after error recovery
#[derive(Debug, Clone)]
#[must_use]
pub enum RecoveryAction {
    /// Use the last successfully rendered frame
    UseLastFrame,

    /// Show an error widget with the error details
    ShowError(PipelineError),

    /// Skip this frame entirely
    SkipFrame,

    /// Panic with the error (testing/debugging)
    Panic(PipelineError),
}

impl RecoveryAction {
    /// Returns `true` if this action allows the pipeline to continue.
    #[inline]
    #[must_use]
    pub const fn can_continue(&self) -> bool {
        matches!(
            self,
            Self::UseLastFrame | Self::SkipFrame | Self::ShowError(_)
        )
    }

    /// Returns `true` if this is a skip frame action.
    #[inline]
    #[must_use]
    pub const fn is_skip(&self) -> bool {
        matches!(self, Self::SkipFrame)
    }

    /// Returns `true` if this is a panic action.
    #[inline]
    #[must_use]
    pub const fn is_panic(&self) -> bool {
        matches!(self, Self::Panic(_))
    }

    /// Get the error if this action contains one.
    #[must_use]
    pub fn error(&self) -> Option<&PipelineError> {
        match self {
            Self::ShowError(e) | Self::Panic(e) => Some(e),
            _ => None,
        }
    }

    /// Consume self and return the error if present.
    #[must_use]
    pub fn into_error(self) -> Option<PipelineError> {
        match self {
            Self::ShowError(e) | Self::Panic(e) => Some(e),
            _ => None,
        }
    }
}

/// Error recovery manager
///
/// Handles pipeline errors according to the configured policy.
/// Tracks error counts and can trigger panic after too many errors.
#[derive(Debug)]
pub struct ErrorRecovery {
    /// Recovery policy
    policy: RecoveryPolicy,

    /// Error count (atomic for thread safety)
    error_count: AtomicUsize,

    /// Maximum errors before forced panic
    max_errors: usize,
}

impl ErrorRecovery {
    /// Create new error recovery with specified policy
    #[must_use]
    pub fn new(policy: RecoveryPolicy) -> Self {
        Self {
            policy,
            error_count: AtomicUsize::new(0),
            max_errors: DEFAULT_MAX_ERRORS,
        }
    }

    /// Create error recovery with custom max errors
    #[must_use]
    pub fn with_max_errors(policy: RecoveryPolicy, max_errors: usize) -> Self {
        Self {
            policy,
            error_count: AtomicUsize::new(0),
            max_errors,
        }
    }

    /// Handle a pipeline error
    ///
    /// Applies the recovery policy and returns the appropriate action.
    /// Increments error count and may trigger panic if exceeded.
    ///
    /// # Parameters
    ///
    /// - `error`: Pipeline error that occurred
    /// - `phase`: Phase where error occurred
    ///
    /// # Returns
    ///
    /// `RecoveryAction` indicating what to do
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

            return RecoveryAction::Panic(PipelineError::invalid_state(format!(
                "Exceeded maximum pipeline errors ({}/{})",
                count, self.max_errors
            )));
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
    #[inline]
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Reset error count
    #[inline]
    pub fn reset_error_count(&mut self) {
        self.error_count.store(0, Ordering::Relaxed);
    }

    /// Get current policy
    #[inline]
    #[must_use]
    pub const fn policy(&self) -> RecoveryPolicy {
        self.policy
    }

    /// Set new policy
    #[inline]
    pub fn set_policy(&mut self, policy: RecoveryPolicy) {
        self.policy = policy;
    }

    /// Get max errors
    #[inline]
    #[must_use]
    pub const fn max_errors(&self) -> usize {
        self.max_errors
    }

    /// Set max errors
    #[inline]
    pub fn set_max_errors(&mut self, max: usize) {
        self.max_errors = max;
    }

    /// Returns `true` if any errors have occurred.
    #[inline]
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.error_count.load(Ordering::Relaxed) > 0
    }

    /// Returns `true` if error count has reached the maximum.
    #[inline]
    #[must_use]
    pub fn is_at_limit(&self) -> bool {
        self.error_count.load(Ordering::Relaxed) >= self.max_errors
    }
}

impl Default for ErrorRecovery {
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
        assert!(!recovery.has_errors());
    }

    #[test]
    fn test_error_count() {
        let mut recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

        let error = PipelineError::layout_failed(42, "test");

        let _ = recovery.handle_error(error.clone(), PipelinePhase::Layout);
        assert_eq!(recovery.error_count(), 1);
        assert!(recovery.has_errors());

        let _ = recovery.handle_error(error.clone(), PipelinePhase::Layout);
        assert_eq!(recovery.error_count(), 2);

        recovery.reset_error_count();
        assert_eq!(recovery.error_count(), 0);
        assert!(!recovery.has_errors());
    }

    #[test]
    fn test_skip_frame_policy() {
        let recovery = ErrorRecovery::new(RecoveryPolicy::SkipFrame);

        let error = PipelineError::layout_failed(42, "test");
        let action = recovery.handle_error(error, PipelinePhase::Layout);

        assert!(action.is_skip());
        assert!(action.can_continue());
        assert!(!action.is_panic());
    }

    #[test]
    fn test_show_error_policy() {
        let recovery = ErrorRecovery::new(RecoveryPolicy::ShowErrorWidget);

        let error = PipelineError::paint_failed(42, "test");
        let action = recovery.handle_error(error, PipelinePhase::Paint);

        assert!(action.can_continue());
        assert!(action.error().is_some());
        assert_eq!(action.error().unwrap().phase(), PipelinePhase::Paint);
    }

    #[test]
    fn test_use_last_frame_policy() {
        let recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);

        let error = PipelineError::build_failed(1, "test");
        let action = recovery.handle_error(error, PipelinePhase::Build);

        match action {
            RecoveryAction::UseLastFrame => {}
            _ => panic!("Expected UseLastFrame action"),
        }
    }

    #[test]
    fn test_set_policy() {
        let mut recovery = ErrorRecovery::new(RecoveryPolicy::UseLastGoodFrame);
        assert_eq!(recovery.policy(), RecoveryPolicy::UseLastGoodFrame);

        recovery.set_policy(RecoveryPolicy::SkipFrame);
        assert_eq!(recovery.policy(), RecoveryPolicy::SkipFrame);
    }

    #[test]
    fn test_max_errors_exceeded() {
        let recovery = ErrorRecovery::with_max_errors(RecoveryPolicy::SkipFrame, 3);

        let error = PipelineError::layout_failed(42, "test");

        // First 3 should return SkipFrame
        for _ in 0..3 {
            let action = recovery.handle_error(error.clone(), PipelinePhase::Layout);
            assert!(action.is_skip());
        }

        assert!(recovery.is_at_limit());

        // 4th should return Panic
        let action = recovery.handle_error(error, PipelinePhase::Layout);
        assert!(action.is_panic());
    }

    #[test]
    fn test_policy_predicates() {
        assert!(RecoveryPolicy::UseLastGoodFrame.is_graceful());
        assert!(RecoveryPolicy::SkipFrame.is_graceful());
        assert!(!RecoveryPolicy::ShowErrorWidget.is_graceful());
        assert!(!RecoveryPolicy::Panic.is_graceful());

        assert!(RecoveryPolicy::ShowErrorWidget.shows_error());
        assert!(RecoveryPolicy::Panic.shows_error());
        assert!(!RecoveryPolicy::UseLastGoodFrame.shows_error());
        assert!(!RecoveryPolicy::SkipFrame.shows_error());
    }

    #[test]
    fn test_action_into_error() {
        let error = PipelineError::build_failed(1, "test");
        let action = RecoveryAction::ShowError(error);

        let err = action.into_error().unwrap();
        assert!(err.is_build_error());
    }
}
