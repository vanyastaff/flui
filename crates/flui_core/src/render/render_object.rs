//! Typed RenderObject trait
//!
//! Implementation from idea.md Chapter 2.3

use flui_types::Size;
use flui_engine::BoxedLayer;

use crate::render::arity::Arity;
use super::layout_cx::LayoutCx;
use super::paint_cx::PaintCx;

/// Typed RenderObject with compile-time arity guarantees
///
/// This is the core trait for all render objects in FLUI. Key features:
///
/// 1. **Associated Arity**: Declares child count at compile time
/// 2. **Typed Contexts**: `LayoutCx<Arity>` and `PaintCx<Arity>` provide only valid methods
/// 3. **Layer Return**: `paint()` returns a Layer for compositing
/// 4. **Zero-Cost**: No `Box<dyn>`, no downcasts, full inlining
///
/// # Example: Leaf RenderObject
///
/// ```rust,ignore
/// use flui_core::{RenderObject, LeafArity, LayoutCx, PaintCx};
/// use flui_engine::{PictureLayer, Paint};
/// use flui_types::{Size, Rect};
///
/// pub struct RenderParagraph {
///     pub text: String,
///     pub font_size: f32,
/// }
///
/// impl RenderObject for RenderParagraph {
///     type Arity = LeafArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
///         // LeafArity: NO .child() or .children() methods available!
///         let constraints = cx.constraints();
///
///         // Calculate text size
///         let width = self.text.len() as f32 * self.font_size * 0.6;
///         let height = self.font_size * 1.2;
///
///         constraints.constrain(Size::new(width, height))
///     }
///
///     fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
///         // LeafArity: NO .child() or .children() methods available!
///         let mut picture = PictureLayer::new();
///
///         picture.draw_text(
///             Rect::from_xywh(0.0, 0.0, 100.0, 20.0),
///             &self.text,
///             self.font_size,
///             Paint::default(),
///         );
///
///         Box::new(picture)
///     }
/// }
/// ```
///
/// # Example: Single-Child RenderObject
///
/// ```rust,ignore
/// use flui_core::{RenderObject, SingleArity, LayoutCx, PaintCx};
/// use flui_engine::{OpacityLayer, BoxedLayer};
///
/// pub struct RenderOpacity {
///     pub opacity: f32,
/// }
///
/// impl RenderObject for RenderOpacity {
///     type Arity = SingleArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
///         // SingleArity: .child() method available!
///         let child = cx.child();
///         cx.layout_child(child, cx.constraints())
///     }
///
///     fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
///         // SingleArity: .capture_child_layer() available!
///         let child_layer = cx.capture_child_layer(cx.child());
///
///         Box::new(OpacityLayer::new(child_layer, self.opacity))
///     }
/// }
/// ```
///
/// # Example: Multi-Child RenderObject
///
/// ```rust,ignore
/// use flui_core::{RenderObject, MultiArity, LayoutCx, PaintCx};
/// use flui_engine::{ContainerLayer, BoxedLayer};
///
/// pub struct RenderFlex {
///     pub spacing: f32,
/// }
///
/// impl RenderObject for RenderFlex {
///     type Arity = MultiArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
///         // MultiArity: .children() method available!
///         let mut total_width = 0.0;
///         let mut max_height = 0.0;
///
///         for &child in cx.children() {
///             let child_size = cx.layout_child(child, cx.constraints());
///             total_width += child_size.width + self.spacing;
///             max_height = max_height.max(child_size.height);
///         }
///
///         Size::new(total_width, max_height)
///     }
///
///     fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
///         // MultiArity: .capture_child_layers() available!
///         let mut container = ContainerLayer::new();
///
///         for &child in cx.children() {
///             let child_layer = cx.capture_child_layer(child);
///             container.add_child(child_layer);
///         }
///
///         Box::new(container)
///     }
/// }
/// ```
pub trait RenderObject: Send + Sync + Sized + 'static {
    /// The arity constraint for this RenderObject
    ///
    /// Determines the type of `LayoutCx` and `PaintCx`:
    /// - `LeafArity`: Contexts have NO child methods
    /// - `SingleArity`: Contexts have `.child()` method
    /// - `MultiArity`: Contexts have `.children()` method
    type Arity: Arity;

    /// Compute layout for this RenderObject
    ///
    /// The context is typed by arity:
    /// - `LayoutCx<LeafArity>`: Only `.constraints()` available
    /// - `LayoutCx<SingleArity>`: `.child()` and `.layout_child()` available
    /// - `LayoutCx<MultiArity>`: `.children()` and `.layout_child()` available
    ///
    /// # Returns
    ///
    /// The size chosen by this RenderObject, which must satisfy the constraints:
    /// `cx.constraints().is_satisfied_by(returned_size)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
    ///     let child = cx.child(); // Only works for SingleArity!
    ///     cx.layout_child(child, cx.constraints())
    /// }
    /// ```
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size;

    /// Paint this RenderObject and return a Layer
    ///
    /// The context is typed by arity:
    /// - `PaintCx<LeafArity>`: Only `.painter()`, `.offset()` available
    /// - `PaintCx<SingleArity>`: `.child()` and `.capture_child_layer()` available
    /// - `PaintCx<MultiArity>`: `.children()` and `.capture_child_layers()` available
    ///
    /// # Returns
    ///
    /// A Layer from flui_engine that represents this RenderObject's visual output.
    /// This can be:
    /// - `PictureLayer` for leaf objects (direct drawing)
    /// - `OpacityLayer`, `TransformLayer`, etc. for effects
    /// - `ContainerLayer` for multi-child layouts
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
    ///     let child_layer = cx.capture_child_layer(cx.child());
    ///     Box::new(OpacityLayer::new(child_layer, 0.5))
    /// }
    /// ```
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer;

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

    /// Debug name for this RenderObject
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::arity::{LeafArity, SingleArity};
    use crate::render::layout_cx::SingleChild;
    use crate::render::paint_cx::SingleChildPaint;
    use flui_types::constraints::BoxConstraints;
    use flui_engine::ContainerLayer;

    // Example Leaf RenderObject
    #[derive(Debug)]
    struct TestLeaf {
        size: Size,
    }

    impl RenderObject for TestLeaf {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            // Leaf: no child methods available
            cx.constraints().constrain(self.size)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            // Leaf: no child methods available
            Box::new(ContainerLayer::new())
        }
    }

    // Example Single-child RenderObject
    #[derive(Debug)]
    struct TestSingle;

    impl RenderObject for TestSingle {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            // Single: .child() method available
            let child = cx.child();
            cx.layout_child(child, cx.constraints())
        }

        fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            // Single: .capture_child_layer() available
            let child = cx.child();
            cx.capture_child_layer(child)
        }
    }

    #[test]
    fn test_render_object_types_compile() {
        // Just verify types compile
        let _leaf = TestLeaf { size: Size::ZERO };
        let _single = TestSingle;
    }
}
