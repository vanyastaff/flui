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
//! let id = ElementId::new();
//! let err = CoreError::not_mounted(id);
//! match err {
//!     CoreError::NotMounted(eid) => assert_eq!(eid, id),
//!     _ => unreachable!(),
//! }
//! ```

use thiserror::Error;
use std::any::TypeId;
use std::sync::Arc;
use std::borrow::Cow;

use crate::{ElementId, ElementLifecycle};

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
        reason: Cow<'static, str>,
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
        reason: Cow<'static, str>,
    },

    /// Tree is in invalid state
    #[error("Element tree in invalid state: {0}")]
    InvalidTreeState(Cow<'static, str>),

    /// Widget build failed with source error
    #[error("Failed to build widget '{widget_type}' (element {element_id}): {source}")]
    BuildFailed {
        widget_type: &'static str,
        element_id: ElementId,
        source: Arc<dyn std::error::Error + Send + Sync>,
    },

    /// Lifecycle violation (debug only)
    #[error("Lifecycle violation for element {element_id}: Cannot {operation} in state {actual_state:?} (expected {expected_state:?})")]
    LifecycleViolation {
        element_id: ElementId,
        expected_state: ElementLifecycle,
        actual_state: ElementLifecycle,
        operation: &'static str,
    },

    /// Global key error
    #[error("{0}")]
    KeyError(KeyError),

    /// InheritedWidget not found
    #[error("No InheritedWidget of type '{widget_type}' found in ancestor tree of element {context_element_id}. Did you forget to wrap your app with the widget?")]
    InheritedWidgetNotFound {
        widget_type: &'static str,
        context_element_id: ElementId,
    },
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
    /// let id = ElementId::new();
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

    /// Create a key error
    #[must_use]
    pub fn key_error(error: KeyError) -> Self {
        Self::KeyError(error)
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
}

/// Error types for global keys
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// Duplicate global key detected
    #[error("Duplicate GlobalKey detected: {key_id:?}. Each GlobalKey must be unique. Existing element: {existing_element}, New element: {new_element}")]
    DuplicateKey {
        key_id: TypeId,
        existing_element: ElementId,
        new_element: ElementId,
    },

    /// Global key not found
    #[error("GlobalKey not found: {key_id:?}")]
    KeyNotFound { key_id: TypeId },
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


    #[test]
    fn test_key_error_display() {
        let error = KeyError::KeyNotFound {
            key_id: TypeId::of::<()>(),
        };
        assert!(error.to_string().contains("GlobalKey"));
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn test_inherited_widget_not_found_error() {
        let error = CoreError::inherited_widget_not_found("Theme", unsafe { ElementId::from_raw(5) });
        let msg = error.to_string();
        assert!(msg.contains("Theme"));
        assert!(msg.contains("Did you forget"));
    }

    #[test]
    fn test_lifecycle_violation_error() {
        let error = CoreError::lifecycle_violation(
            unsafe { ElementId::from_raw(1) },
            ElementLifecycle::Active,
            ElementLifecycle::Defunct,
            "update",
        );
        let msg = error.to_string();
        assert!(msg.contains("Cannot update"));
        assert!(msg.contains("Defunct"));
    }

    #[test]
    fn test_duplicate_key_error() {
        let error = KeyError::DuplicateKey {
            key_id: TypeId::of::<()>(),
            existing_element: unsafe { ElementId::from_raw(1) },
            new_element: unsafe { ElementId::from_raw(2) },
        };
        let msg = error.to_string();
        assert!(msg.contains("Duplicate"));
        assert!(msg.contains("GlobalKey"));
    }
}
