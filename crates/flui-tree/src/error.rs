//! Error types for tree operations.
//!
//! This module provides error types for generic tree operations that can fail,
//! such as accessing non-existent nodes or detecting cycles.
//!
//! # Design Philosophy
//!
//! `flui-tree` provides ONLY generic tree errors. Domain-specific errors
//! should be defined in their respective crates:
//!
//! - **flui_rendering**: `RenderError`, `LayoutError`, `PaintError`
//! - **flui-element**: `ElementError`, `LifecycleError`
//! - **flui-view**: `ViewError`, `BuildError`
//!
//! # Error Categories
//!
//! - **Structural errors**: `CycleDetected`, `InvalidParent`
//! - **Lookup errors**: `NotFound`, `AlreadyExists`
//! - **Constraint errors**: `MaxDepthExceeded`, `EmptyTree`
//! - **Runtime errors**: `ConcurrentModification`, `Internal`
//!
//! # ID Representation
//!
//! All node IDs are stored as `usize` for simplicity and to avoid generic
//! type parameters in error types. Callers can convert their ID types
//! to/from `usize` using `.get()` or similar methods.

use thiserror::Error;

/// Result type for tree operations.
pub type TreeResult<T> = Result<T, TreeError>;

/// Errors that can occur during generic tree operations.
///
/// This enum covers errors that apply to any tree structure,
/// regardless of the specific domain (UI, rendering, etc.).
///
/// Node IDs are stored as `usize` for simplicity. Convert your ID type
/// using `.get()` or similar methods.
///
/// # Non-exhaustive
///
/// This enum is marked `#[non_exhaustive]` to allow adding new
/// error variants in future versions without breaking changes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TreeError {
    /// Element not found in tree.
    ///
    /// Returned when attempting to access, modify, or navigate
    /// to an element that doesn't exist in the tree.
    #[error("element {0} not found in tree")]
    NotFound(usize),

    /// Element already exists in tree.
    ///
    /// Returned when attempting to insert an element with an ID
    /// that's already present in the tree.
    #[error("element {0} already exists in tree")]
    AlreadyExists(usize),

    /// Invalid parent reference.
    ///
    /// Returned when attempting to set a parent that would violate
    /// tree invariants (e.g., parent doesn't exist).
    #[error("invalid parent {parent} for element {child}")]
    InvalidParent {
        /// The child element ID.
        child: usize,
        /// The invalid parent ID.
        parent: usize,
    },

    /// Cycle detected in tree structure.
    ///
    /// Returned when an operation would create a cycle in the tree,
    /// which would violate the fundamental tree invariant.
    #[error("cycle detected: {0} would create a cycle")]
    CycleDetected(usize),

    /// Maximum tree depth exceeded.
    ///
    /// Returned when traversal exceeds the configured maximum depth,
    /// which may indicate infinite recursion or a corrupted structure.
    #[error("maximum tree depth {max} exceeded at element {element}")]
    MaxDepthExceeded {
        /// The element that exceeded depth.
        element: usize,
        /// The maximum allowed depth.
        max: usize,
    },

    /// Tree is empty (no root).
    ///
    /// Returned when an operation requires a non-empty tree
    /// but the tree has no elements.
    #[error("tree is empty")]
    EmptyTree,

    /// Operation not supported for this tree type.
    ///
    /// Returned when an operation is not implemented or not
    /// applicable for the specific tree implementation.
    #[error("operation not supported for element {0}: {1}")]
    NotSupported(usize, &'static str),

    /// Concurrent modification detected.
    ///
    /// Returned when a traversal detects that the tree was
    /// modified during iteration, which could lead to undefined behavior.
    #[error("concurrent modification detected during traversal")]
    ConcurrentModification,

    /// Internal error (should not happen).
    ///
    /// Indicates a bug in the tree implementation. If you encounter
    /// this error, please report it as a bug.
    #[error("internal error: {0}")]
    Internal(String),
}

impl TreeError {
    // ========================================================================
    // CONSTRUCTORS
    // ========================================================================

    /// Creates a `NotFound` error.
    #[inline]
    pub const fn not_found(id: usize) -> Self {
        Self::NotFound(id)
    }

    /// Creates an `AlreadyExists` error.
    #[inline]
    pub const fn already_exists(id: usize) -> Self {
        Self::AlreadyExists(id)
    }

    /// Creates an `InvalidParent` error.
    #[inline]
    pub const fn invalid_parent(child: usize, parent: usize) -> Self {
        Self::InvalidParent { child, parent }
    }

    /// Creates a `CycleDetected` error.
    #[inline]
    pub const fn cycle_detected(id: usize) -> Self {
        Self::CycleDetected(id)
    }

    /// Creates a `MaxDepthExceeded` error.
    #[inline]
    pub const fn max_depth_exceeded(element: usize, max: usize) -> Self {
        Self::MaxDepthExceeded { element, max }
    }

    /// Creates an `EmptyTree` error.
    #[inline]
    pub const fn empty_tree() -> Self {
        Self::EmptyTree
    }

    /// Creates a `NotSupported` error.
    #[inline]
    pub const fn not_supported(id: usize, reason: &'static str) -> Self {
        Self::NotSupported(id, reason)
    }

    /// Creates a `ConcurrentModification` error.
    #[inline]
    pub const fn concurrent_modification() -> Self {
        Self::ConcurrentModification
    }

    /// Creates an `Internal` error.
    #[inline]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    // ========================================================================
    // ERROR CLASSIFICATION
    // ========================================================================

    /// Returns the element ID associated with this error, if any.
    ///
    /// Most tree errors are associated with a specific element.
    /// This method extracts that element ID for logging, debugging,
    /// or error recovery purposes.
    pub const fn element_id(&self) -> Option<usize> {
        match self {
            Self::NotFound(id)
            | Self::AlreadyExists(id)
            | Self::CycleDetected(id)
            | Self::NotSupported(id, _) => Some(*id),

            Self::InvalidParent { child, .. } => Some(*child),
            Self::MaxDepthExceeded { element, .. } => Some(*element),

            Self::EmptyTree | Self::ConcurrentModification | Self::Internal(_) => None,
        }
    }

    /// Returns `true` if this is a recoverable error.
    ///
    /// Recoverable errors are those that don't indicate corruption
    /// or fundamental issues with the tree structure. They typically
    /// represent expected failure cases that can be handled gracefully.
    ///
    /// # Recoverable Errors
    ///
    /// - `NotFound` - Element doesn't exist (expected in some workflows)
    /// - `NotSupported` - Operation not available (can use alternative)
    ///
    /// # Non-Recoverable Errors
    ///
    /// - `CycleDetected` - Tree invariant violated
    /// - `ConcurrentModification` - Data race detected
    /// - `Internal` - Implementation bug
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::NotFound(_) | Self::NotSupported(_, _))
    }

    /// Returns `true` if this error indicates a structural problem.
    ///
    /// Structural errors indicate that the tree's fundamental invariants
    /// may be violated, requiring careful recovery or reconstruction.
    pub const fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::CycleDetected(_) | Self::InvalidParent { .. } | Self::ConcurrentModification
        )
    }

    /// Returns `true` if this error indicates a lookup failure.
    ///
    /// Lookup errors are typically benign and indicate that an
    /// element simply doesn't exist in the tree.
    pub const fn is_lookup_error(&self) -> bool {
        matches!(self, Self::NotFound(_) | Self::AlreadyExists(_))
    }

    /// Returns `true` if this error indicates an internal bug.
    ///
    /// If this returns `true`, please report the error as a bug.
    pub const fn is_internal(&self) -> bool {
        matches!(self, Self::Internal(_))
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
        let id = 42usize;

        let err = TreeError::not_found(id);
        assert!(err.to_string().contains("42"));
        assert!(err.to_string().contains("not found"));

        let err = TreeError::cycle_detected(id);
        assert!(err.to_string().contains("cycle"));

        let err = TreeError::max_depth_exceeded(id, 100);
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("depth"));
    }

    #[test]
    fn test_element_id_extraction() {
        let id = 42usize;
        let parent = 1usize;

        assert_eq!(TreeError::not_found(id).element_id(), Some(id));
        assert_eq!(TreeError::already_exists(id).element_id(), Some(id));
        assert_eq!(TreeError::cycle_detected(id).element_id(), Some(id));
        assert_eq!(TreeError::invalid_parent(id, parent).element_id(), Some(id));
        assert_eq!(
            TreeError::max_depth_exceeded(id, 100).element_id(),
            Some(id)
        );
        assert_eq!(TreeError::empty_tree().element_id(), None);
        assert_eq!(TreeError::concurrent_modification().element_id(), None);
        assert_eq!(TreeError::internal("bug").element_id(), None);
    }

    #[test]
    fn test_is_recoverable() {
        let id = 1usize;

        // Recoverable errors
        assert!(TreeError::not_found(id).is_recoverable());
        assert!(TreeError::not_supported(id, "test").is_recoverable());

        // Non-recoverable errors
        assert!(!TreeError::cycle_detected(id).is_recoverable());
        assert!(!TreeError::empty_tree().is_recoverable());
        assert!(!TreeError::concurrent_modification().is_recoverable());
        assert!(!TreeError::internal("bug").is_recoverable());
    }

    #[test]
    fn test_is_structural() {
        let id = 1usize;
        let parent = 2usize;

        // Structural errors
        assert!(TreeError::cycle_detected(id).is_structural());
        assert!(TreeError::invalid_parent(id, parent).is_structural());
        assert!(TreeError::concurrent_modification().is_structural());

        // Non-structural errors
        assert!(!TreeError::not_found(id).is_structural());
        assert!(!TreeError::empty_tree().is_structural());
    }

    #[test]
    fn test_is_lookup_error() {
        let id = 1usize;

        // Lookup errors
        assert!(TreeError::not_found(id).is_lookup_error());
        assert!(TreeError::already_exists(id).is_lookup_error());

        // Non-lookup errors
        assert!(!TreeError::cycle_detected(id).is_lookup_error());
        assert!(!TreeError::empty_tree().is_lookup_error());
    }

    #[test]
    fn test_is_internal() {
        let id = 1usize;

        assert!(TreeError::internal("bug").is_internal());
        assert!(!TreeError::not_found(id).is_internal());
        assert!(!TreeError::empty_tree().is_internal());
    }

    #[test]
    fn test_const_constructors() {
        let id = 1usize;
        let parent = 2usize;

        let not_found = TreeError::not_found(id);
        let already_exists = TreeError::already_exists(id);
        let invalid_parent = TreeError::invalid_parent(id, parent);
        let cycle = TreeError::cycle_detected(id);
        let max_depth = TreeError::max_depth_exceeded(id, 100);
        let not_supported = TreeError::not_supported(id, "reason");

        // Verify the errors were created correctly
        assert!(matches!(not_found, TreeError::NotFound(_)));
        assert!(matches!(already_exists, TreeError::AlreadyExists(_)));
        assert!(matches!(invalid_parent, TreeError::InvalidParent { .. }));
        assert!(matches!(cycle, TreeError::CycleDetected(_)));
        assert!(matches!(max_depth, TreeError::MaxDepthExceeded { .. }));
        assert!(matches!(not_supported, TreeError::NotSupported(_, _)));
    }
}
