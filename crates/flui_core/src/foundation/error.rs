//! Error types for flui_core
//!
//! This module defines error types that can occur in the core framework.
//! It uses the `thiserror` crate to provide clear Display messages and ergonomic
//! pattern matching when handling failures.
//!
//! # Examples
//!
//! Creating and returning a CoreError using the Result alias:
//!
//! ```rust
//! use flui_core::{CoreError, Result, ElementId};
//!
//! fn find_element(id: ElementId) -> Result<()> {
//!     // pretend lookup in a tree
//!     let found = false;
//!     if !found {
//!         return Err(CoreError::element_not_found(id));
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Matching on specific variants:
//!
//! ```rust
//! # use flui_core::{CoreError, ElementId};
//! let id = 0; // ElementId
//! let err = CoreError::not_mounted(id);
//! match err {
//!     CoreError::NotMounted(eid) => assert_eq!(eid, id),
//!     _ => unreachable!(),
//! }
//! ```

use std::borrow::Cow;
use std::sync::Arc;
use thiserror::Error;

use crate::element::{ElementId, ElementLifecycle};

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
        /// Parent element ID
        parent: ElementId,
        /// Child element ID
        child: ElementId,
    },

    /// Element is not mounted in tree
    #[error("Element {0} is not mounted")]
    NotMounted(ElementId),

    /// Cannot update element due to type mismatch
    #[error("Cannot update element {id}: widget type mismatch")]
    TypeMismatch {
        /// Element ID
        id: ElementId,
    },

    /// Rebuild operation failed
    #[error("Rebuild failed for element {id}: {reason}")]
    RebuildFailed {
        /// Element ID
        id: ElementId,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Element is already mounted
    #[error("Element {0} is already mounted")]
    AlreadyMounted(ElementId),

    /// Slot index out of bounds
    #[error("Slot index {slot} out of bounds for element {element}")]
    SlotOutOfBounds {
        /// Element ID
        element: ElementId,
        /// Slot index
        slot: usize,
    },

    /// Invalid operation on element in current state
    #[error("Invalid operation on element {id}: {reason}")]
    InvalidOperation {
        /// Element ID
        id: ElementId,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Tree is in invalid state
    #[error("Element tree in invalid state: {0}")]
    InvalidTreeState(Cow<'static, str>),

    /// View build failed with source error
    #[error("Failed to build view '{widget_type}' (element {element_id}): {source}")]
    BuildFailed {
        /// View type name
        widget_type: &'static str,
        /// Element ID
        element_id: ElementId,
        /// Underlying error
        source: Arc<dyn std::error::Error + Send + Sync>,
    },

    /// Lifecycle violation (debug only)
    #[error(
        "Lifecycle violation for element {element_id}: Cannot {operation} in state {actual_state:?} (expected {expected_state:?})"
    )]
    LifecycleViolation {
        /// Element ID
        element_id: ElementId,
        /// Expected lifecycle state
        expected_state: ElementLifecycle,
        /// Actual lifecycle state
        actual_state: ElementLifecycle,
        /// Operation that was attempted
        operation: &'static str,
    },

    /// Provider not found
    #[error(
        "No Provider of type '{widget_type}' found in ancestor tree of element {context_element_id}. Did you forget to wrap your app with the provider?"
    )]
    InheritedWidgetNotFound {
        /// Provider type name
        widget_type: &'static str,
        /// Context element ID
        context_element_id: ElementId,
    },

    /// Layout operation failed
    #[error("Layout failed for element {element_id}: {reason}")]
    LayoutFailed {
        /// Element ID
        element_id: ElementId,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Paint operation failed
    #[error("Paint failed for element {element_id}: {reason}")]
    PaintFailed {
        /// Element ID
        element_id: ElementId,
        /// Failure reason
        reason: Cow<'static, str>,
    },

    /// Type downcast failed
    #[error("Type downcast failed: expected {expected}, got {actual}")]
    DowncastFailed {
        /// Expected type name
        expected: &'static str,
        /// Actual type name
        actual: &'static str,
    },

    /// Render tree is invalid
    #[error("Render tree invalid: {0}")]
    InvalidRenderTree(Cow<'static, str>),

    /// Cache corruption
    #[error("Layout cache corrupted for element {0}")]
    CacheCorrupted(ElementId),
}

/// Result type for core operations
pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    /// Create an element not found error
    #[must_use]
    pub fn element_not_found(id: ElementId) -> Self {
        Self::ElementNotFound(id)
    }

    /// Create an invalid hierarchy error
    #[must_use]
    pub fn invalid_hierarchy(parent: ElementId, child: ElementId) -> Self {
        Self::InvalidHierarchy { parent, child }
    }

    /// Create a not mounted error
    #[must_use]
    pub fn not_mounted(id: ElementId) -> Self {
        Self::NotMounted(id)
    }

    /// Create a type mismatch error
    #[must_use]
    pub fn type_mismatch(id: ElementId) -> Self {
        Self::TypeMismatch { id }
    }

    /// Create a rebuild failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated):
    ///
    /// ```rust
    /// use flui_core::{CoreError, ElementId};
    ///
    /// let id = 0; // ElementId
    ///
    /// // Static string - zero allocation!
    /// let err1 = CoreError::rebuild_failed(id, "static reason");
    ///
    /// // Dynamic string - allocated when needed
    /// let err2 = CoreError::rebuild_failed(id, format!("dynamic {}", 42));
    /// ```
    #[must_use]
    pub fn rebuild_failed(id: ElementId, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::RebuildFailed {
            id,
            reason: reason.into(),
        }
    }

    /// Create an already mounted error
    #[must_use]
    pub fn already_mounted(id: ElementId) -> Self {
        Self::AlreadyMounted(id)
    }

    /// Create a slot out of bounds error
    #[must_use]
    pub fn slot_out_of_bounds(element: ElementId, slot: usize) -> Self {
        Self::SlotOutOfBounds { element, slot }
    }

    /// Create an invalid operation error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_operation(id: ElementId, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidOperation {
            id,
            reason: reason.into(),
        }
    }

    /// Create an invalid tree state error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_tree_state(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidTreeState(reason.into())
    }

    /// Create a build failed error
    #[must_use]
    pub fn build_failed(
        widget_type: &'static str,
        element_id: ElementId,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::BuildFailed {
            widget_type,
            element_id,
            source: Arc::new(source),
        }
    }

    /// Create a lifecycle violation error
    #[must_use]
    pub fn lifecycle_violation(
        element_id: ElementId,
        expected_state: ElementLifecycle,
        actual_state: ElementLifecycle,
        operation: &'static str,
    ) -> Self {
        Self::LifecycleViolation {
            element_id,
            expected_state,
            actual_state,
            operation,
        }
    }

    /// Create an inherited widget not found error
    #[must_use]
    pub fn inherited_widget_not_found(
        widget_type: &'static str,
        context_element_id: ElementId,
    ) -> Self {
        Self::InheritedWidgetNotFound {
            widget_type,
            context_element_id,
        }
    }

    /// Create a layout failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn layout_failed(element_id: ElementId, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::LayoutFailed {
            element_id,
            reason: reason.into(),
        }
    }

    /// Create a paint failed error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn paint_failed(element_id: ElementId, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::PaintFailed {
            element_id,
            reason: reason.into(),
        }
    }

    /// Create a downcast failed error
    #[must_use]
    pub fn downcast_failed(expected: &'static str, actual: &'static str) -> Self {
        Self::DowncastFailed { expected, actual }
    }

    /// Create an invalid render tree error
    ///
    /// Accepts both static strings (zero-cost) and dynamic strings (allocated).
    #[must_use]
    pub fn invalid_render_tree(reason: impl Into<Cow<'static, str>>) -> Self {
        Self::InvalidRenderTree(reason.into())
    }

    /// Create a cache corrupted error
    #[must_use]
    pub fn cache_corrupted(id: ElementId) -> Self {
        Self::CacheCorrupted(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        use crate::ElementId;
        let id = ElementId::new(42);
        let err = CoreError::element_not_found(id);
        assert!(err.to_string().contains(&id.get().to_string()));
    }

    #[test]
    fn test_error_creation() {
        use crate::ElementId;
        let id = ElementId::new(1);

        let _err1 = CoreError::element_not_found(id);
        let _err2 = CoreError::not_mounted(id);
        let _err3 = CoreError::type_mismatch(id);
        let _err4 = CoreError::rebuild_failed(id, "test reason");
        let _err5 = CoreError::already_mounted(id);
        let _err6 = CoreError::slot_out_of_bounds(id, 5);
        let _err7 = CoreError::invalid_operation(id, "test");
        let _err8 = CoreError::invalid_tree_state("test");
        let _err9 = CoreError::layout_failed(id, "constraint violation");
        let _err10 = CoreError::paint_failed(id, "rendering error");
    }

    #[test]
    fn test_invalid_hierarchy() {
        use crate::ElementId;
        let parent = ElementId::new(1);
        let child = ElementId::new(2);
        let err = CoreError::invalid_hierarchy(parent, child);

        let msg = err.to_string();
        assert!(msg.contains("Invalid parent-child"));
    }

    #[test]
    fn test_inherited_widget_not_found_error() {
        use crate::ElementId;
        let error = CoreError::inherited_widget_not_found("Theme", ElementId::new(5));
        let msg = error.to_string();
        assert!(msg.contains("Theme"));
        assert!(msg.contains("Did you forget"));
    }

    #[test]
    fn test_lifecycle_violation_error() {
        use crate::ElementId;
        let error = CoreError::lifecycle_violation(
            ElementId::new(1),
            ElementLifecycle::Active,
            ElementLifecycle::Defunct,
            "update",
        );
        let msg = error.to_string();
        assert!(msg.contains("Cannot update"));
        assert!(msg.contains("Defunct"));
    }

    #[test]
    fn test_cow_string_static() {
        use crate::ElementId;
        let err = CoreError::rebuild_failed(ElementId::new(1), "static string");
        match err {
            CoreError::RebuildFailed { reason, .. } => {
                // Static strings should not allocate
                assert_eq!(reason, "static string");
            }
            _ => panic!("Wrong error variant"),
        }
    }

    #[test]
    fn test_cow_string_dynamic() {
        use crate::ElementId;
        let dynamic = format!("dynamic {}", 42);
        let err = CoreError::rebuild_failed(ElementId::new(1), dynamic.clone());
        match err {
            CoreError::RebuildFailed { reason, .. } => {
                assert_eq!(reason.as_ref(), &dynamic);
            }
            _ => panic!("Wrong error variant"),
        }
    }

    #[test]
    fn test_layout_failed() {
        use crate::ElementId;
        let err = CoreError::layout_failed(ElementId::new(1), "width constraint violated");
        let msg = err.to_string();
        assert!(msg.contains("Layout failed"));
        assert!(msg.contains("width constraint violated"));
    }

    #[test]
    fn test_paint_failed() {
        use crate::ElementId;
        let err = CoreError::paint_failed(ElementId::new(1), "shader compilation failed");
        let msg = err.to_string();
        assert!(msg.contains("Paint failed"));
        assert!(msg.contains("shader compilation failed"));
    }
}
