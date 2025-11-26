//! BuildContext - Abstract context trait for view building
//!
//! This module defines the `BuildContext` trait - an abstraction that allows
//! views to access framework services during build without coupling to
//! concrete implementation.
//!
//! # Architecture
//!
//! ```text
//! flui-element (this crate)
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
//! - flui-element defines the trait
//! - flui-pipeline implements it
//! - No cycle!

use std::any::{Any, TypeId};
use std::sync::Arc;

use crate::ElementId;

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

    /// Look up inherited value by TypeId (low-level API).
    ///
    /// Walks up the tree to find the nearest provider element that provides
    /// a value of the given type. Registers a dependency so that when the
    /// provider updates, this element will be rebuilt.
    ///
    /// Returns `Arc<dyn Any>` that can be downcast to the actual type.
    ///
    /// # Note
    ///
    /// Use the type-safe `depend_on<T>()` extension method instead of calling
    /// this directly.
    fn depend_on_raw(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>>;

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

    /// Look up inherited value from nearest provider (type-safe API).
    ///
    /// Walks up the element tree to find the nearest provider element that
    /// provides a value of type `T`. Registers a dependency relationship so
    /// that when the provider's value changes, the current element will be
    /// automatically rebuilt.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The type of value to look up. Must be `Send + Sync + 'static`.
    ///
    /// # Returns
    ///
    /// - `Some(Arc<T>)` if a provider is found
    /// - `None` if no provider of type `T` exists in the ancestor chain
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    ///
    /// struct Theme {
    ///     primary_color: Color,
    /// }
    ///
    /// impl StatelessView for ThemedButton {
    ///     fn build(self, ctx: &dyn BuildContext) -> impl IntoElement {
    ///         // Look up theme from provider
    ///         let theme = ctx.depend_on::<Theme>()
    ///             .expect("Theme provider not found");
    ///
    ///         Button::new("Click")
    ///             .color(theme.primary_color)
    ///     }
    /// }
    /// ```
    ///
    /// # Architecture
    ///
    /// This method:
    /// 1. Walks up the parent chain from current element
    /// 2. Checks each ancestor to see if it's a Provider<T>
    /// 3. When found, registers current element as dependent
    /// 4. Returns Arc<T> to the provided value
    ///
    /// # Performance
    ///
    /// Tree walking is O(depth), but results can be cached in the element.
    /// Dependency registration is O(1).
    pub fn depend_on<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let type_id = TypeId::of::<T>();
        let any_arc = self.depend_on_raw(type_id)?;

        // Downcast Arc<dyn Any> to Arc<T>
        // This is safe because depend_on_raw ensures type matches
        any_arc.downcast::<T>().ok()
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

        fn depend_on_raw(&self, _type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
            // Mock: no providers
            None
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
