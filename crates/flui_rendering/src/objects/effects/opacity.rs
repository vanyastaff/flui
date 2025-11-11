//! RenderOpacity - applies opacity to a child using OpacityLayer

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_painting::Canvas;
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

impl Render for RenderOpacity {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // If fully transparent, return empty canvas
        if self.opacity <= 0.0 {
            return Canvas::new();
        }

        // Get child canvas
        let child_canvas = tree.paint_child(child_id, offset);

        // If fully opaque, return child canvas directly
        if self.opacity >= 1.0 {
            return child_canvas;
        }

        // For partial opacity, we need Canvas-level opacity support
        // TODO: Add Canvas::set_opacity() method or opacity parameter to append_canvas
        // For now, just return child canvas (opacity will need to be handled at layer level)
        child_canvas
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
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
