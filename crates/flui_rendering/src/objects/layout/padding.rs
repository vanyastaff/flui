//! RenderPadding - adds padding around a child

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::{EdgeInsets, Size};

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

impl RenderBox<Single> for RenderPadding {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        let padding = self.padding;

        // Deflate constraints by padding
        let child_constraints = ctx.constraints.deflate(&padding);

        // Layout child with deflated constraints
        let child_size = ctx
            .layout_child(child_id, child_constraints)
            ;

        // Add padding to child size
        Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        )
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Apply padding offset and paint child
        let child_offset = flui_types::Offset::new(self.padding.left, self.padding.top);
        ctx.paint_child(child_id, ctx.offset + child_offset);
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
