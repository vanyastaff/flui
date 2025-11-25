//! Pipeline error types
//!
//! Provides error types for all pipeline phases (build, layout, paint).
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::{PipelineError, PipelinePhase, PipelineResult};
//! use flui_foundation::ElementId;
//!
//! fn do_layout(id: ElementId) -> PipelineResult<()> {
//!     // ... layout logic ...
//!     Err(PipelineError::layout_failed(id, "invalid constraints"))
//! }
//! ```

use flui_foundation::ElementId;
use std::fmt;
use thiserror::Error;

/// Result type for pipeline operations
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Pipeline phase identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PipelinePhase {
    /// Build phase - widget tree construction
    Build,
    /// Layout phase - size computation
    Layout,
    /// Paint phase - layer generation
    Paint,
}

impl fmt::Display for PipelinePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build => write!(f, "build"),
            Self::Layout => write!(f, "layout"),
            Self::Paint => write!(f, "paint"),
        }
    }
}

/// Pipeline error
#[derive(Debug, Clone, Error)]
pub enum PipelineError {
    /// Build phase failed for an element
    #[error("Build failed for element {element_id}: {message}")]
    BuildFailed {
        /// Element that failed to build
        element_id: ElementId,
        /// Error message
        message: String,
    },

    /// Layout phase failed for an element
    #[error("Layout failed for element {element_id}: {message}")]
    LayoutFailed {
        /// Element that failed layout
        element_id: ElementId,
        /// Error message
        message: String,
    },

    /// Paint phase failed for an element
    #[error("Paint failed for element {element_id}: {message}")]
    PaintFailed {
        /// Element that failed paint
        element_id: ElementId,
        /// Error message
        message: String,
    },

    /// Element not found in tree
    #[error("Element not found: {0}")]
    ElementNotFound(ElementId),

    /// Invalid state detected
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Operation cancelled (timeout or manual)
    #[error("Operation cancelled: {0}")]
    Cancelled(String),

    /// No root element attached
    #[error("No root element attached")]
    NoRoot,

    /// Constraint violation
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
}

impl PipelineError {
    /// Create build failed error
    pub fn build_failed(element_id: ElementId, message: impl Into<String>) -> Self {
        Self::BuildFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create layout failed error
    pub fn layout_failed(element_id: ElementId, message: impl Into<String>) -> Self {
        Self::LayoutFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create paint failed error
    pub fn paint_failed(element_id: ElementId, message: impl Into<String>) -> Self {
        Self::PaintFailed {
            element_id,
            message: message.into(),
        }
    }

    /// Create element not found error
    pub fn element_not_found(element_id: ElementId) -> Self {
        Self::ElementNotFound(element_id)
    }

    /// Create invalid state error
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }

    /// Create cancelled error
    pub fn cancelled(reason: impl Into<String>) -> Self {
        Self::Cancelled(reason.into())
    }

    /// Get the pipeline phase this error occurred in
    pub fn phase(&self) -> PipelinePhase {
        match self {
            Self::BuildFailed { .. } => PipelinePhase::Build,
            Self::LayoutFailed { .. } | Self::ConstraintViolation(_) => PipelinePhase::Layout,
            Self::PaintFailed { .. } => PipelinePhase::Paint,
            // Default to build for generic errors
            Self::ElementNotFound(_)
            | Self::InvalidState(_)
            | Self::Cancelled(_)
            | Self::NoRoot => PipelinePhase::Build,
        }
    }

    /// Get element ID if error is element-specific
    pub fn element_id(&self) -> Option<ElementId> {
        match self {
            Self::BuildFailed { element_id, .. }
            | Self::LayoutFailed { element_id, .. }
            | Self::PaintFailed { element_id, .. }
            | Self::ElementNotFound(element_id) => Some(*element_id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_failed() {
        let id = ElementId::new(42);
        let err = PipelineError::build_failed(id, "test error");

        assert_eq!(err.phase(), PipelinePhase::Build);
        assert_eq!(err.element_id(), Some(id));
        assert!(err.to_string().contains("42"));
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_layout_failed() {
        let id = ElementId::new(10);
        let err = PipelineError::layout_failed(id, "bad constraints");

        assert_eq!(err.phase(), PipelinePhase::Layout);
        assert_eq!(err.element_id(), Some(id));
    }

    #[test]
    fn test_paint_failed() {
        let id = ElementId::new(5);
        let err = PipelineError::paint_failed(id, "canvas error");

        assert_eq!(err.phase(), PipelinePhase::Paint);
        assert_eq!(err.element_id(), Some(id));
    }

    #[test]
    fn test_phase_display() {
        assert_eq!(format!("{}", PipelinePhase::Build), "build");
        assert_eq!(format!("{}", PipelinePhase::Layout), "layout");
        assert_eq!(format!("{}", PipelinePhase::Paint), "paint");
    }
}
