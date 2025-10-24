//! Typed RenderObject trait with compile-time arity constraints

use flui_types::Size;

use super::arity::RenderArity;
use super::context::{LayoutCx, PaintCx};

/// Typed RenderObject trait with compile-time child count guarantees
///
/// This is the new, typed version of `DynRenderObject`. Key differences:
///
/// 1. **Associated Arity type**: The RenderObject declares how many children it has
///    at compile time through `type Arity: RenderArity`
///
/// 2. **Generic contexts**: `LayoutCx` and `PaintCx` are parameterized by the
///    RenderObject type, allowing them to provide type-safe child access methods
///
/// 3. **No downcast**: The concrete type is known at compile time, enabling
///    zero-cost abstractions and inlining
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::typed::{RenderObject, SingleArity, LayoutCx, PaintCx};
/// use flui_types::Size;
///
/// #[derive(Debug)]
/// pub struct RenderOpacity {
///     pub opacity: f32,
/// }
///
/// impl RenderObject for RenderOpacity {
///     type Arity = SingleArity;
///
///     fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
///         // Type-safe: cx.child() is available because Arity = SingleArity
///         let child_id = cx.child();
///         let child_size = cx.layout_child(child_id, cx.constraints());
///         child_size
///     }
///
///     fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
///         if self.opacity > 0.0 {
///             let child_id = cx.child();
///             cx.paint_child(child_id);
///         }
///     }
/// }
/// ```
pub trait RenderObject: Send + Sync + Sized + 'static {
    /// The arity constraint for this RenderObject
    ///
    /// This determines what methods are available on `LayoutCx` and `PaintCx`:
    /// - `LeafArity`: No child access methods
    /// - `SingleArity`: `.child()` method available
    /// - `MultiArity`: `.children()` iterator available
    type Arity: RenderArity;

    /// Compute layout for this RenderObject
    ///
    /// The context `cx` is specialized based on `Self::Arity`:
    /// - For `LeafArity`: Only has `.constraints()` and state access
    /// - For `SingleArity`: Has `.child()` to get the single child
    /// - For `MultiArity`: Has `.children()` to iterate all children
    ///
    /// Returns the computed size for this RenderObject.
    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size;

    /// Paint this RenderObject
    ///
    /// Similar to `layout()`, the context is specialized by arity:
    /// - For `LeafArity`: Direct painting only
    /// - For `SingleArity`: Can paint one child via `.child()`
    /// - For `MultiArity`: Can paint children via `.children()`
    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>);

    /// Optional: compute intrinsic width
    ///
    /// Default implementation returns None (no intrinsic width).
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: compute intrinsic height
    ///
    /// Default implementation returns None (no intrinsic height).
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: debug name for this RenderObject
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Trait for RenderObjects that can handle hit testing
///
/// This is an optional trait that RenderObjects can implement
/// if they need custom hit testing logic.
pub trait HitTestable: RenderObject {
    /// Test if a point hits this RenderObject
    ///
    /// Returns `true` if the point (relative to this object's origin)
    /// is considered a "hit" for this object.
    fn hit_test(&self, cx: &PaintCx<'_, Self>, local_position: flui_types::Offset) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::arity::{LeafArity, SingleArity};
    use flui_types::{BoxConstraints, Size};

    // Example Leaf RenderObject
    #[derive(Debug)]
    struct TestLeafRender {
        size: Size,
    }

    impl RenderObject for TestLeafRender {
        type Arity = LeafArity;

        fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
            // Leaf render: just return our fixed size
            cx.constraints().constrain(self.size)
        }

        fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
            // Paint logic here
        }
    }

    // Example Single-child RenderObject
    #[derive(Debug)]
    struct TestSingleRender;

    impl RenderObject for TestSingleRender {
        type Arity = SingleArity;

        fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
            // Single-child render: layout our child
            let child = cx.child();
            cx.layout_child(child, cx.constraints())
        }

        fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
            let child = cx.child();
            cx.paint_child(child);
        }
    }

    #[test]
    fn test_render_object_arity() {
        // Just checking that types compile
        assert_eq!(
            <TestLeafRender as RenderObject>::Arity::name(),
            "Leaf"
        );
        assert_eq!(
            <TestSingleRender as RenderObject>::Arity::name(),
            "Single"
        );
    }
}
