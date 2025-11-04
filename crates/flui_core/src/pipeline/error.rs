//! Pipeline error types
//!
//! Defines error types for pipeline operations with detailed context
//! for debugging and recovery.

use std::fmt;

/// Pipeline error type
///
/// Represents errors that can occur during pipeline operations.
///
/// # Error Handling Strategy
///
/// Errors are categorized by severity:
/// - **Recoverable**: Can continue with fallback (layout timeout, paint error)
/// - **Fatal**: Cannot continue (tree corruption, invalid state)
///
/// # Example
///
/// ```rust
/// use flui_core::pipeline::PipelineError;
///
/// fn handle_error(error: PipelineError) {
///     match error {
///         PipelineError::Timeout { phase, .. } => {
///             // Use last good frame
///         }
///         PipelineError::LayoutError { .. } => {
///             // Show error widget
///         }
///         _ => {
///             // Fatal error - panic in development
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum PipelineError {
    /// Operation timed out
    ///
    /// Occurs when an operation exceeds its deadline.
    /// Typically recoverable by using last good frame.
    Timeout {
        /// Pipeline phase that timed out
        phase: PipelinePhase,
        /// Actual elapsed time in milliseconds
        elapsed_ms: u64,
        /// Deadline that was exceeded in milliseconds
        deadline_ms: u64,
    },

    /// Layout error
    ///
    /// Occurs during layout phase (size computation, constraint violation).
    /// Recoverable by skipping frame or showing error widget.
    LayoutError {
        /// Element that caused the layout error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Paint error
    ///
    /// Occurs during paint phase (layer generation failure).
    /// Recoverable by using last good frame.
    PaintError {
        /// Element that caused the paint error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Build error
    ///
    /// Occurs during build phase (widget rebuild failure).
    /// Recoverable by showing error widget.
    BuildError {
        /// Element that caused the build error
        element_id: crate::element::ElementId,
        /// Error message describing what went wrong
        message: String,
    },

    /// Tree corruption
    ///
    /// Fatal error - element tree is in invalid state.
    /// Not recoverable - must panic in development.
    TreeCorruption {
        /// Description of the tree corruption
        message: String,
    },

    /// Invalid state
    ///
    /// Fatal error - pipeline is in invalid state.
    /// Not recoverable - must panic in development.
    InvalidState {
        /// Description of the invalid state
        message: String,
    },
}

impl PipelineError {
    /// Check if error is recoverable
    ///
    /// Returns `true` for errors that can be handled gracefully
    /// (timeout, layout error, paint error, build error).
    ///
    /// Returns `false` for fatal errors
    /// (tree corruption, invalid state).
    #[inline]
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Timeout { .. } => true,
            Self::LayoutError { .. } => true,
            Self::PaintError { .. } => true,
            Self::BuildError { .. } => true,
            Self::TreeCorruption { .. } => false,
            Self::InvalidState { .. } => false,
        }
    }

    /// Get error phase
    ///
    /// Returns the pipeline phase where the error occurred.
    #[inline]
    pub fn phase(&self) -> PipelinePhase {
        match self {
            Self::Timeout { phase, .. } => *phase,
            Self::LayoutError { .. } => PipelinePhase::Layout,
            Self::PaintError { .. } => PipelinePhase::Paint,
            Self::BuildError { .. } => PipelinePhase::Build,
            Self::TreeCorruption { .. } => PipelinePhase::Build,
            Self::InvalidState { .. } => PipelinePhase::Build,
        }
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout { phase, elapsed_ms, deadline_ms } => {
                write!(
                    f,
                    "Pipeline timeout in {:?} phase: {}ms elapsed (deadline: {}ms)",
                    phase, elapsed_ms, deadline_ms
                )
            }
            Self::LayoutError { element_id, message } => {
                write!(f, "Layout error for element {:?}: {}", element_id, message)
            }
            Self::PaintError { element_id, message } => {
                write!(f, "Paint error for element {:?}: {}", element_id, message)
            }
            Self::BuildError { element_id, message } => {
                write!(f, "Build error for element {:?}: {}", element_id, message)
            }
            Self::TreeCorruption { message } => {
                write!(f, "Element tree corruption: {}", message)
            }
            Self::InvalidState { message } => {
                write!(f, "Invalid pipeline state: {}", message)
            }
        }
    }
}

impl std::error::Error for PipelineError {}

/// Pipeline phase identifier
///
/// Identifies which phase of the pipeline an error occurred in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelinePhase {
    /// Build phase (widget rebuild)
    Build,
    /// Layout phase (size computation)
    Layout,
    /// Paint phase (layer generation)
    Paint,
}

impl fmt::Display for PipelinePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build => write!(f, "Build"),
            Self::Layout => write!(f, "Layout"),
            Self::Paint => write!(f, "Paint"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_is_recoverable() {
        let timeout = PipelineError::Timeout {
            phase: PipelinePhase::Layout,
            elapsed_ms: 20,
            deadline_ms: 16,
        };
        assert!(timeout.is_recoverable());

        let corruption = PipelineError::TreeCorruption {
            message: "Invalid element reference".to_string(),
        };
        assert!(!corruption.is_recoverable());
    }

    #[test]
    fn test_error_phase() {
        let layout_error = PipelineError::LayoutError {
            element_id: 42,
            message: "Constraint violation".to_string(),
        };
        assert_eq!(layout_error.phase(), PipelinePhase::Layout);

        let paint_error = PipelineError::PaintError {
            element_id: 42,
            message: "Layer creation failed".to_string(),
        };
        assert_eq!(paint_error.phase(), PipelinePhase::Paint);
    }

    #[test]
    fn test_error_display() {
        let error = PipelineError::Timeout {
            phase: PipelinePhase::Layout,
            elapsed_ms: 20,
            deadline_ms: 16,
        };
        let display = format!("{}", error);
        assert!(display.contains("timeout"));
        assert!(display.contains("Layout"));
    }
}
