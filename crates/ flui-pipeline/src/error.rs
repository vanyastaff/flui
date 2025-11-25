//! Error types for the FLUI pipeline.

use flui_foundation::ElementId;
use thiserror::Error;

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, PipelineError>;

/// Errors that can occur during pipeline execution.
#[derive(Error, Debug, Clone)]
pub enum PipelineError {
    /// Build phase error - widget couldn't be inflated.
    #[error("Build error for element {element:?}: {message}")]
    BuildError { element: ElementId, message: String },

    /// Layout phase error - constraints couldn't be satisfied.
    #[error("Layout error for element {element:?}: {message}")]
    LayoutError { element: ElementId, message: String },

    /// Paint phase error - rendering failed.
    #[error("Paint error for element {element:?}: {message}")]
    PaintError { element: ElementId, message: String },

    /// Element not found in tree.
    #[error("Element {0:?} not found")]
    ElementNotFound(ElementId),

    /// Element is not a render element.
    #[error("Element {0:?} is not a render element")]
    NotRenderElement(ElementId),

    /// Invalid constraints provided.
    #[error("Invalid constraints for element {element:?}: {reason}")]
    InvalidConstraints { element: ElementId, reason: String },

    /// Cycle detected in layout.
    #[error("Layout cycle detected involving element {0:?}")]
    LayoutCycle(ElementId),

    /// Maximum layout iterations exceeded.
    #[error("Maximum layout iterations ({max}) exceeded for element {element:?}")]
    MaxLayoutIterations { element: ElementId, max: usize },

    /// Compositing error.
    #[error("Compositing error: {0}")]
    CompositingError(String),

    /// Pipeline is already running.
    #[error("Pipeline is already running")]
    PipelineRunning,

    /// Pipeline was aborted.
    #[error("Pipeline was aborted: {0}")]
    Aborted(String),

    /// Internal error - should not happen.
    #[error("Internal pipeline error: {0}")]
    Internal(String),
}

impl PipelineError {
    /// Creates a build error.
    pub fn build_error(element: ElementId, message: impl Into<String>) -> Self {
        Self::BuildError {
            element,
            message: message.into(),
        }
    }

    /// Creates a layout error.
    pub fn layout_error(element: ElementId, message: impl Into<String>) -> Self {
        Self::LayoutError {
            element,
            message: message.into(),
        }
    }

    /// Creates a paint error.
    pub fn paint_error(element: ElementId, message: impl Into<String>) -> Self {
        Self::PaintError {
            element,
            message: message.into(),
        }
    }

    /// Creates an invalid constraints error.
    pub fn invalid_constraints(element: ElementId, reason: impl Into<String>) -> Self {
        Self::InvalidConstraints {
            element,
            reason: reason.into(),
        }
    }

    /// Returns the element ID if this error is associated with one.
    pub fn element_id(&self) -> Option<ElementId> {
        match self {
            Self::BuildError { element, .. }
            | Self::LayoutError { element, .. }
            | Self::PaintError { element, .. }
            | Self::InvalidConstraints { element, .. }
            | Self::MaxLayoutIterations { element, .. } => Some(*element),
            Self::ElementNotFound(id) | Self::NotRenderElement(id) | Self::LayoutCycle(id) => {
                Some(*id)
            }
            _ => None,
        }
    }

    /// Returns true if this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        !matches!(
            self,
            Self::LayoutCycle(_) | Self::PipelineRunning | Self::Internal(_)
        )
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_error() {
        let id = ElementId::new(1);
        let err = PipelineError::build_error(id, "test error");

        assert_eq!(err.element_id(), Some(id));
        assert!(err.is_recoverable());
        assert!(err.to_string().contains("Build error"));
    }

    #[test]
    fn test_layout_error() {
        let id = ElementId::new(2);
        let err = PipelineError::layout_error(id, "constraint failed");

        assert_eq!(err.element_id(), Some(id));
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_layout_cycle_not_recoverable() {
        let id = ElementId::new(3);
        let err = PipelineError::LayoutCycle(id);

        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_element_id_extraction() {
        let id = ElementId::new(4);

        let errors = vec![
            PipelineError::BuildError {
                element: id,
                message: "test".into(),
            },
            PipelineError::ElementNotFound(id),
            PipelineError::NotRenderElement(id),
        ];

        for err in errors {
            assert_eq!(err.element_id(), Some(id));
        }
    }
}
