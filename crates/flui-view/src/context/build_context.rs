//! BuildContext - Abstract context trait for view building
//!
//! This module defines the `BuildContext` trait - an abstraction that allows
//! views to access framework services during build without coupling to
//! concrete implementation.
//!
//! # Architecture
//!
//! ```text
//! flui-view (this crate)
//! ├── BuildContext trait (abstraction)
//! └── ViewObject uses &dyn BuildContext
//!
//!          ↓ depends on
//!
//! flui-pipeline
//! └── PipelineBuildContext: BuildContext (concrete impl)
//! ```
//!
//! This design avoids circular dependencies:
//! - flui-view defines the trait
//! - flui-pipeline implements it
//! - No cycle!

use std::any::Any;

use flui_foundation::ElementId;

// ============================================================================
// BuildContext TRAIT
// ============================================================================

/// BuildContext - Abstract context for view building
///
/// Passed to `build()` methods to provide:
/// - Current element's position in tree
/// - Methods to look up inherited data
/// - Ability to schedule rebuilds
///
/// # Implementors
///
/// The concrete implementation `PipelineBuildContext` lives in `flui-pipeline`.
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(self, ctx: &dyn BuildContext) -> impl IntoElement {
///         let id = ctx.element_id();
///         // ...
///     }
/// }
/// ```
///
/// # Dyn Compatibility
///
/// This trait is dyn-compatible. All methods use concrete types or `&dyn`
/// to ensure it can be used as `&dyn BuildContext`.
pub trait BuildContext: Send + Sync {
    /// Get the current element's ID being built.
    fn element_id(&self) -> ElementId;

    /// Get the parent element's ID, if any.
    fn parent_id(&self) -> Option<ElementId>;

    /// Get depth of current element in tree (0 = root).
    fn depth(&self) -> usize;

    /// Mark current element as needing rebuild.
    ///
    /// Called by signals/state when value changes.
    fn mark_dirty(&self);

    /// Schedule a rebuild for a specific element.
    fn schedule_rebuild(&self, element_id: ElementId);

    /// Downcast to concrete type for advanced usage.
    fn as_any(&self) -> &dyn Any;
}

// ============================================================================
// HELPER METHODS
// ============================================================================

impl dyn BuildContext {
    /// Try to downcast to a specific BuildContext implementation.
    pub fn downcast_ref<T: BuildContext + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockBuildContext {
        element_id: ElementId,
    }

    impl BuildContext for MockBuildContext {
        fn element_id(&self) -> ElementId {
            self.element_id
        }

        fn parent_id(&self) -> Option<ElementId> {
            None
        }

        fn depth(&self) -> usize {
            0
        }

        fn mark_dirty(&self) {
            // no-op for mock
        }

        fn schedule_rebuild(&self, _element_id: ElementId) {
            // no-op for mock
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_mock_context() {
        let id = ElementId::new(1);
        let ctx = MockBuildContext { element_id: id };

        assert_eq!(ctx.element_id(), id);
        assert_eq!(ctx.parent_id(), None);
        assert_eq!(ctx.depth(), 0);
    }

    #[test]
    fn test_downcast() {
        let id = ElementId::new(1);
        let ctx: &dyn BuildContext = &MockBuildContext { element_id: id };

        let downcasted = ctx.downcast_ref::<MockBuildContext>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().element_id, id);
    }
}
