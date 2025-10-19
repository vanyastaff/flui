//! Error types for flui_core
//!
//! This module defines all error types that can occur in the core framework.
//! We use `thiserror` for ergonomic error handling.

use thiserror::Error;

use crate::ElementId;

/// Core framework error type
///
/// All fallible operations in flui_core return `Result<T, CoreError>`.
#[derive(Error, Debug, Clone)]
pub enum CoreError {
    /// Element not found in tree
    #[error("Element {0} not found in tree")]
    ElementNotFound(ElementId),

    /// Invalid parent-child relationship
    #[error("Invalid parent-child relationship: parent={parent}, child={child}")]
    InvalidHierarchy {
        parent: ElementId,
        child: ElementId,
    },

    /// Element is not mounted in tree
    #[error("Element {0} is not mounted")]
    NotMounted(ElementId),

    /// Cannot update element due to type mismatch
    #[error("Cannot update element {id}: widget type mismatch")]
    TypeMismatch {
        id: ElementId,
    },

    /// Rebuild operation failed
    #[error("Rebuild failed for element {id}: {reason}")]
    RebuildFailed {
        id: ElementId,
        reason: String,
    },

    /// Element is already mounted
    #[error("Element {0} is already mounted")]
    AlreadyMounted(ElementId),

    /// Slot index out of bounds
    #[error("Slot index {slot} out of bounds for element {element}")]
    SlotOutOfBounds {
        element: ElementId,
        slot: usize,
    },

    /// Invalid operation on element in current state
    #[error("Invalid operation on element {id}: {reason}")]
    InvalidOperation {
        id: ElementId,
        reason: String,
    },

    /// Tree is in invalid state
    #[error("Element tree in invalid state: {0}")]
    InvalidTreeState(String),
}

/// Result type for core operations
pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    /// Create an element not found error
    pub fn element_not_found(id: ElementId) -> Self {
        Self::ElementNotFound(id)
    }

    /// Create an invalid hierarchy error
    pub fn invalid_hierarchy(parent: ElementId, child: ElementId) -> Self {
        Self::InvalidHierarchy { parent, child }
    }

    /// Create a not mounted error
    pub fn not_mounted(id: ElementId) -> Self {
        Self::NotMounted(id)
    }

    /// Create a type mismatch error
    pub fn type_mismatch(id: ElementId) -> Self {
        Self::TypeMismatch { id }
    }

    /// Create a rebuild failed error
    pub fn rebuild_failed(id: ElementId, reason: impl Into<String>) -> Self {
        Self::RebuildFailed {
            id,
            reason: reason.into(),
        }
    }

    /// Create an already mounted error
    pub fn already_mounted(id: ElementId) -> Self {
        Self::AlreadyMounted(id)
    }

    /// Create a slot out of bounds error
    pub fn slot_out_of_bounds(element: ElementId, slot: usize) -> Self {
        Self::SlotOutOfBounds { element, slot }
    }

    /// Create an invalid operation error
    pub fn invalid_operation(id: ElementId, reason: impl Into<String>) -> Self {
        Self::InvalidOperation {
            id,
            reason: reason.into(),
        }
    }

    /// Create an invalid tree state error
    pub fn invalid_tree_state(reason: impl Into<String>) -> Self {
        Self::InvalidTreeState(reason.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let id = ElementId::new();
        let err = CoreError::element_not_found(id);
        assert!(err.to_string().contains(&id.to_string()));
    }

    #[test]
    fn test_error_creation() {
        let id = ElementId::new();

        let _err1 = CoreError::element_not_found(id);
        let _err2 = CoreError::not_mounted(id);
        let _err3 = CoreError::type_mismatch(id);
        let _err4 = CoreError::rebuild_failed(id, "test reason");
        let _err5 = CoreError::already_mounted(id);
        let _err6 = CoreError::slot_out_of_bounds(id, 5);
        let _err7 = CoreError::invalid_operation(id, "test");
        let _err8 = CoreError::invalid_tree_state("test");
    }

    #[test]
    fn test_invalid_hierarchy() {
        let parent = ElementId::new();
        let child = ElementId::new();
        let err = CoreError::invalid_hierarchy(parent, child);

        let msg = err.to_string();
        assert!(msg.contains("Invalid parent-child"));
    }
}
