//! RenderOpacity - applies opacity to a child using OpacityLayer

use crate::core::ElementId;
use crate::core::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;

/// RenderObject that applies opacity to its child
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOpacity;
///
/// let opacity = RenderOpacity::new(0.5);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
}

impl RenderOpacity {
    /// Create new RenderOpacity
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    /// Set new opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl RenderBox<Single> for RenderOpacity {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // If fully transparent, don't paint anything
        if self.opacity <= 0.0 {
            return;
        }

        // If fully opaque, paint child directly (zero-cost fast path)
        if self.opacity >= 1.0 {
            ctx.paint_child(child_id, ctx.offset);
            return;
        }

        // Paint child to its own canvas
        let offset = ctx.offset; // Read offset before mutable borrow
        let child_canvas = ctx
            .tree_mut()
            .perform_paint(ElementId::new(child_id.get()), offset)
            .expect("Paint failed - this is a framework bug");

        // Append child canvas with opacity
        ctx.canvas()
            .append_canvas_with_opacity(child_canvas, self.opacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert_eq!(opacity.opacity, 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = RenderOpacity::new(-0.5);
        assert_eq!(opacity1.opacity, 0.0);

        let opacity2 = RenderOpacity::new(1.5);
        assert_eq!(opacity2.opacity, 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = RenderOpacity::new(0.5);
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity, 0.8);
    }
}
