//! RenderPadding - adds padding around a child

use flui_types::{EdgeInsets, Offset, Size};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{TransformLayer, BoxedLayer};

/// RenderObject that adds padding around its child
///
/// Padding increases the size of the widget by the padding amount.
/// The child is laid out with constraints deflated by the padding,
/// then the final size includes the padding.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
/// ```
#[derive(Debug, Clone)]
pub struct RenderPadding {
    /// The padding to apply
    pub padding: EdgeInsets,
}

impl RenderPadding {
    /// Create new RenderPadding
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    /// Set new padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }
}

impl RenderObject for RenderPadding {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let constraints = cx.constraints();
        let padding = self.padding;

        // Deflate constraints by padding
        let child_constraints = constraints.deflate(&padding);

        // Layout child with deflated constraints
        let child = cx.child();
        let child_size = cx.layout_child(child, child_constraints);

        // Add padding to child size
        Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        )
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Capture child layer
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // Apply padding offset via TransformLayer
        let offset = Offset::new(self.padding.left, self.padding.top);
        Box::new(TransformLayer::translate(child_layer, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_padding_new() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_padding_set() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        padding.set_padding(EdgeInsets::all(20.0));
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }
}
