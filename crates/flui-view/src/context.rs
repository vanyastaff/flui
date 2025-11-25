//! Build context traits for view construction.
//!
//! Defines abstract traits for the context provided to views during build.
//! Concrete implementations live in `flui_core`.

use flui_foundation::ElementId;

// ============================================================================
// VIEW CONTEXT TRAIT
// ============================================================================

/// Abstract context for view operations.
///
/// This trait defines what a view can access during build. Concrete
/// implementations provide access to element tree, hooks, and rebuild scheduling.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` for thread-safe UI updates.
pub trait ViewContext: Send + Sync {
    /// Returns the current element ID.
    fn element_id(&self) -> ElementId;

    /// Returns the parent element ID, if any.
    fn parent(&self) -> Option<ElementId>;

    /// Returns `true` if this is the root element.
    fn is_root(&self) -> bool {
        self.parent().is_none()
    }

    /// Returns the depth in the tree. Root has depth 0.
    fn depth(&self) -> usize;

    /// Returns `true` if the element still exists in the tree.
    fn is_valid(&self) -> bool;

    /// Schedule a rebuild for the current element.
    fn mark_needs_build(&self);
}

// ============================================================================
// BUILD CONTEXT TRAIT
// ============================================================================

/// Extended context with hook support.
///
/// This trait extends `ViewContext` with hook-related functionality.
/// Used by stateful and animated views.
pub trait BuildContext: ViewContext {
    /// Execute a function with hook context access.
    ///
    /// Used internally by hook implementations.
    fn with_hooks<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut dyn HookAccess) -> R;
}

// ============================================================================
// HOOK ACCESS TRAIT
// ============================================================================

/// Abstract access to hook state.
///
/// This trait is implemented by hook context types and provides
/// the minimal interface needed for hooks to function.
pub trait HookAccess {
    /// Get the current hook index.
    fn current_index(&self) -> usize;

    /// Advance to the next hook.
    fn advance(&mut self);

    /// Reset hook index for new build.
    fn reset(&mut self);

    /// Get the number of hooks registered.
    fn hook_count(&self) -> usize;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockContext {
        element_id: ElementId,
        parent: Option<ElementId>,
        depth: usize,
    }

    impl ViewContext for MockContext {
        fn element_id(&self) -> ElementId {
            self.element_id
        }

        fn parent(&self) -> Option<ElementId> {
            self.parent
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn is_valid(&self) -> bool {
            true
        }

        fn mark_needs_build(&self) {
            // No-op for mock
        }
    }

    #[test]
    fn test_is_root() {
        let root = MockContext {
            element_id: ElementId::new(1),
            parent: None,
            depth: 0,
        };
        assert!(root.is_root());

        let child = MockContext {
            element_id: ElementId::new(2),
            parent: Some(ElementId::new(1)),
            depth: 1,
        };
        assert!(!child.is_root());
    }
}
