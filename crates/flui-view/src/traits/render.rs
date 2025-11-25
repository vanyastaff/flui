//! Render view trait for creating render objects.
//!
//! For views that create render objects for layout and painting.

use crate::update::UpdateResult;
use std::fmt::Debug;

// ============================================================================
// RENDER VIEW TRAIT
// ============================================================================

/// Render view - views that create render objects.
///
/// Similar to Flutter's `RenderObjectWidget`. This is a widget that
/// stores configuration and creates render objects for layout and painting.
///
/// # Type Parameters
///
/// - `R`: The render object type this view creates
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: Child,
/// }
///
/// impl RenderView for Padding {
///     type RenderObject = RenderPadding;
///
///     fn create_render_object(&self) -> RenderPadding {
///         RenderPadding::new(self.padding)
///     }
///
///     fn update_render_object(&self, render: &mut RenderPadding) -> UpdateResult {
///         if render.padding == self.padding {
///             return UpdateResult::Unchanged;
///         }
///         render.padding = self.padding;
///         UpdateResult::NeedsLayout
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Need custom layout logic
/// - Need custom painting
/// - Building basic widgets (Container, Padding, etc)
/// - Need to participate in hit testing
pub trait RenderView: Clone + Send + 'static {
    /// Associated render object type.
    type RenderObject: Send + Sync + Debug + 'static;

    /// Create render object from this view configuration.
    ///
    /// Called once when element is first mounted.
    fn create_render_object(&self) -> Self::RenderObject;

    /// Update render object when view configuration changes.
    ///
    /// Returns what kind of update is needed:
    /// - `Unchanged` - nothing changed, skip work
    /// - `NeedsLayout` - layout-affecting properties changed
    /// - `NeedsPaint` - only visual properties changed
    ///
    /// # Default
    ///
    /// Returns `Unchanged` (immutable render object).
    #[allow(unused_variables)]
    fn update_render_object(&self, render: &mut Self::RenderObject) -> UpdateResult {
        UpdateResult::Unchanged
    }

    /// Cleanup when element is unmounted (optional).
    ///
    /// Override to dispose resources held by render object.
    #[allow(unused_variables)]
    fn dispose_render_object(&self, render: &mut Self::RenderObject) {}
}

// ============================================================================
// RENDER VIEW WITH CHILDREN
// ============================================================================

/// Marker trait for render views with no children (leaf).
pub trait LeafRenderView: RenderView {}

/// Marker trait for render views with a single child.
pub trait SingleChildRenderView: RenderView {
    /// Child type.
    type Child: Send + 'static;

    /// Returns the child.
    fn child(&self) -> &Self::Child;
}

/// Marker trait for render views with optional child.
pub trait OptionalChildRenderView: RenderView {
    /// Child type.
    type Child: Send + 'static;

    /// Returns the optional child.
    fn child(&self) -> Option<&Self::Child>;
}

/// Marker trait for render views with multiple children.
pub trait MultiChildRenderView: RenderView {
    /// Child type.
    type Child: Send + 'static;

    /// Returns the children.
    fn children(&self) -> &[Self::Child];
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct TestRenderObject {
        value: i32,
    }

    #[derive(Clone)]
    struct TestRenderView {
        value: i32,
    }

    impl RenderView for TestRenderView {
        type RenderObject = TestRenderObject;

        fn create_render_object(&self) -> TestRenderObject {
            TestRenderObject { value: self.value }
        }

        fn update_render_object(&self, render: &mut TestRenderObject) -> UpdateResult {
            if render.value == self.value {
                return UpdateResult::Unchanged;
            }
            render.value = self.value;
            UpdateResult::NeedsLayout
        }
    }

    #[test]
    fn test_render_view_create() {
        let view = TestRenderView { value: 42 };
        let render = view.create_render_object();
        assert_eq!(render.value, 42);
    }

    #[test]
    fn test_render_view_update_unchanged() {
        let view = TestRenderView { value: 42 };
        let mut render = view.create_render_object();
        let result = view.update_render_object(&mut render);
        assert_eq!(result, UpdateResult::Unchanged);
    }

    #[test]
    fn test_render_view_update_changed() {
        let view1 = TestRenderView { value: 42 };
        let view2 = TestRenderView { value: 100 };

        let mut render = view1.create_render_object();
        let result = view2.update_render_object(&mut render);

        assert_eq!(result, UpdateResult::NeedsLayout);
        assert_eq!(render.value, 100);
    }
}
