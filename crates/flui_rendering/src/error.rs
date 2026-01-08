//! Error types for the rendering system.
//!
//! This module provides a unified error type for render operations,
//! replacing panics with recoverable errors where appropriate.

use flui_foundation::RenderId;
use thiserror::Error;

/// Errors that can occur during rendering operations.
#[derive(Error, Debug, Clone)]
pub enum RenderError {
    // ========================================================================
    // Attachment Errors
    // ========================================================================
    /// Render object is not attached to a pipeline owner.
    #[error("render object is not attached to a pipeline owner")]
    NotAttached,

    /// Render object is already attached to a pipeline owner.
    #[error("render object is already attached to a pipeline owner")]
    AlreadyAttached,

    /// View configuration not set before use.
    #[error("view configuration not set")]
    ConfigurationNotSet,

    // ========================================================================
    // Tree Errors
    // ========================================================================
    /// Render node not found in tree.
    #[error("render node not found: {0:?}")]
    NodeNotFound(RenderId),

    /// Invalid parent-child relationship.
    #[error("invalid parent-child relationship")]
    InvalidParentChild,

    /// Cycle detected in render tree.
    #[error("cycle detected in render tree")]
    CycleDetected,

    // ========================================================================
    // Layout Errors
    // ========================================================================
    /// Invalid constraints provided to layout.
    #[error("invalid constraints: {message}")]
    InvalidConstraints {
        /// Description of the constraint violation.
        message: String,
    },

    /// Layout performed during paint phase.
    #[error("layout cannot be performed during paint phase")]
    LayoutDuringPaint,

    /// Layout performed on detached node.
    #[error("cannot layout detached render object")]
    LayoutDetached,

    /// Relayout boundary violation.
    #[error("relayout boundary violated: {message}")]
    RelayoutBoundaryViolation {
        /// Description of the violation.
        message: String,
    },

    // ========================================================================
    // Paint Errors
    // ========================================================================
    /// Paint performed before layout.
    #[error("paint performed before layout")]
    PaintBeforeLayout,

    /// Paint performed on detached node.
    #[error("cannot paint detached render object")]
    PaintDetached,

    /// Layer operation failed.
    #[error("layer operation failed: {message}")]
    LayerError {
        /// Description of the layer error.
        message: String,
    },

    // ========================================================================
    // Pipeline Errors
    // ========================================================================
    /// Pipeline phase executed in wrong order.
    #[error("pipeline phase {phase} executed out of order")]
    PhaseOrderViolation {
        /// The phase that was executed incorrectly.
        phase: &'static str,
    },

    /// Root render object not set.
    #[error("root render object not set")]
    RootNotSet,

    // ========================================================================
    // Compositing Errors
    // ========================================================================
    /// Compositing bits update failed.
    #[error("compositing bits update failed: {message}")]
    CompositingError {
        /// Description of the compositing error.
        message: String,
    },

    // ========================================================================
    // Semantics Errors
    // ========================================================================
    /// Semantics operation failed.
    #[error("semantics operation failed: {message}")]
    SemanticsError {
        /// Description of the semantics error.
        message: String,
    },

    /// Semantics not enabled.
    #[error("semantics system not enabled")]
    SemanticsNotEnabled,

    // ========================================================================
    // Child Handle Errors
    // ========================================================================
    /// Child handle operation failed.
    #[error("child handle operation failed: {message}")]
    ChildHandleError {
        /// Description of the error.
        message: String,
    },
}

/// Result type alias for render operations.
pub type RenderResult<T> = Result<T, RenderError>;

impl RenderError {
    /// Creates an invalid constraints error with a message.
    pub fn invalid_constraints(message: impl Into<String>) -> Self {
        Self::InvalidConstraints {
            message: message.into(),
        }
    }

    /// Creates a relayout boundary violation error.
    pub fn relayout_boundary(message: impl Into<String>) -> Self {
        Self::RelayoutBoundaryViolation {
            message: message.into(),
        }
    }

    /// Creates a layer error.
    pub fn layer(message: impl Into<String>) -> Self {
        Self::LayerError {
            message: message.into(),
        }
    }

    /// Creates a compositing error.
    pub fn compositing(message: impl Into<String>) -> Self {
        Self::CompositingError {
            message: message.into(),
        }
    }

    /// Creates a semantics error.
    pub fn semantics(message: impl Into<String>) -> Self {
        Self::SemanticsError {
            message: message.into(),
        }
    }

    /// Creates a child handle error.
    pub fn child_handle(message: impl Into<String>) -> Self {
        Self::ChildHandleError {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RenderError::NotAttached;
        assert_eq!(
            err.to_string(),
            "render object is not attached to a pipeline owner"
        );

        let err = RenderError::invalid_constraints("min > max");
        assert_eq!(err.to_string(), "invalid constraints: min > max");
    }

    #[test]
    fn test_error_helpers() {
        let err = RenderError::layer("buffer overflow");
        assert!(matches!(err, RenderError::LayerError { .. }));

        let err = RenderError::compositing("invalid bit state");
        assert!(matches!(err, RenderError::CompositingError { .. }));
    }
}
