//! Error types for tree operations.
//!
//! This module provides error types for tree operations that can fail,
//! such as accessing non-existent nodes or detecting cycles.

use flui_foundation::ElementId;
use thiserror::Error;

/// Result type for tree operations.
pub type TreeResult<T> = Result<T, TreeError>;

/// Errors that can occur during tree operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TreeError {
    /// Element not found in tree.
    #[error("element {0} not found in tree")]
    NotFound(ElementId),

    /// Element already exists in tree.
    #[error("element {0} already exists in tree")]
    AlreadyExists(ElementId),

    /// Invalid parent reference.
    #[error("invalid parent {parent} for element {child}")]
    InvalidParent {
        /// The child element ID.
        child: ElementId,
        /// The invalid parent ID.
        parent: ElementId,
    },

    /// Cycle detected in tree structure.
    #[error("cycle detected: {0} would create a cycle")]
    CycleDetected(ElementId),

    /// Maximum tree depth exceeded.
    #[error("maximum tree depth {max} exceeded at element {element}")]
    MaxDepthExceeded {
        /// The element that exceeded depth.
        element: ElementId,
        /// The maximum allowed depth.
        max: usize,
    },

    /// Tree is empty (no root).
    #[error("tree is empty")]
    EmptyTree,

    /// Operation not supported for element type.
    #[error("operation not supported for element {0}: {1}")]
    NotSupported(ElementId, &'static str),

    /// Element is not a render element.
    #[error("element {0} is not a render element")]
    NotRenderElement(ElementId),

    /// Layout error during tree traversal.
    #[error("layout error at element {element}: {message}")]
    LayoutError {
        /// The element where layout failed.
        element: ElementId,
        /// Error message.
        message: String,
    },

    /// Paint error during tree traversal.
    #[error("paint error at element {element}: {message}")]
    PaintError {
        /// The element where paint failed.
        element: ElementId,
        /// Error message.
        message: String,
    },

    /// Concurrent modification detected.
    #[error("concurrent modification detected during traversal")]
    ConcurrentModification,

    /// Internal error (should not happen).
    #[error("internal error: {0}")]
    Internal(String),
}

impl TreeError {
    /// Creates a `NotFound` error.
    #[inline]
    pub fn not_found(id: ElementId) -> Self {
        Self::NotFound(id)
    }

    /// Creates an `AlreadyExists` error.
    #[inline]
    pub fn already_exists(id: ElementId) -> Self {
        Self::AlreadyExists(id)
    }

    /// Creates an `InvalidParent` error.
    #[inline]
    pub fn invalid_parent(child: ElementId, parent: ElementId) -> Self {
        Self::InvalidParent { child, parent }
    }

    /// Creates a `CycleDetected` error.
    #[inline]
    pub fn cycle_detected(id: ElementId) -> Self {
        Self::CycleDetected(id)
    }

    /// Creates a `MaxDepthExceeded` error.
    #[inline]
    pub fn max_depth_exceeded(element: ElementId, max: usize) -> Self {
        Self::MaxDepthExceeded { element, max }
    }

    /// Creates a `NotRenderElement` error.
    #[inline]
    pub fn not_render_element(id: ElementId) -> Self {
        Self::NotRenderElement(id)
    }

    /// Creates a `LayoutError`.
    #[inline]
    pub fn layout_error(element: ElementId, message: impl Into<String>) -> Self {
        Self::LayoutError {
            element,
            message: message.into(),
        }
    }

    /// Creates a `PaintError`.
    #[inline]
    pub fn paint_error(element: ElementId, message: impl Into<String>) -> Self {
        Self::PaintError {
            element,
            message: message.into(),
        }
    }

    /// Creates an `Internal` error.
    #[inline]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Returns the element ID associated with this error, if any.
    pub fn element_id(&self) -> Option<ElementId> {
        match self {
            Self::NotFound(id)
            | Self::AlreadyExists(id)
            | Self::CycleDetected(id)
            | Self::NotRenderElement(id)
            | Self::NotSupported(id, _) => Some(*id),

            Self::InvalidParent { child, .. } => Some(*child),
            Self::MaxDepthExceeded { element, .. }
            | Self::LayoutError { element, .. }
            | Self::PaintError { element, .. } => Some(*element),

            Self::EmptyTree | Self::ConcurrentModification | Self::Internal(_) => None,
        }
    }

    /// Returns `true` if this is a recoverable error.
    ///
    /// Recoverable errors are those that don't indicate corruption
    /// or fundamental issues with the tree structure.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_) | Self::NotRenderElement(_) | Self::NotSupported(_, _)
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
    fn test_error_display() {
        let id = ElementId::new(42);

        let err = TreeError::not_found(id);
        assert!(err.to_string().contains("42"));
        assert!(err.to_string().contains("not found"));

        let err = TreeError::cycle_detected(id);
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn test_element_id_extraction() {
        let id = ElementId::new(42);

        assert_eq!(TreeError::not_found(id).element_id(), Some(id));
        assert_eq!(TreeError::EmptyTree.element_id(), None);
    }

    #[test]
    fn test_is_recoverable() {
        let id = ElementId::new(1);

        assert!(TreeError::not_found(id).is_recoverable());
        assert!(TreeError::not_render_element(id).is_recoverable());
        assert!(!TreeError::cycle_detected(id).is_recoverable());
        assert!(!TreeError::EmptyTree.is_recoverable());
    }
}
