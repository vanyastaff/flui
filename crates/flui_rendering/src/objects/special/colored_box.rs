//! RenderColoredBox - simple solid color box

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_types::{Color, Rect, Size};

/// RenderObject that paints a solid color background
///
/// A simplified version of RenderDecoratedBox that only handles solid colors.
/// More efficient than DecoratedBox when you only need a background color.
///
/// If it has a child, the child is painted on top of the color.
/// If it has no child, it fills the available space with the color.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderColoredBox;
/// use flui_types::Color;
///
/// // Create a red background
/// let colored = RenderColoredBox::new(Color::RED);
/// ```
#[derive(Debug)]
pub struct RenderColoredBox {
    /// Background color
    pub color: Color,
    /// Cached size from layout
    size: Size,
}

impl RenderColoredBox {
    /// Create new RenderColoredBox with specified color
    pub fn new(color: Color) -> Self {
        Self {
            color,
            size: Size::ZERO,
        }
    }

    /// Set new color
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }
}

impl Default for RenderColoredBox {
    fn default() -> Self {
        Self::new(Color::TRANSPARENT)
    }
}

impl Render for RenderColoredBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // SingleArity always has exactly one child
        // Pass through constraints
        let size = tree.layout_child(child_id, constraints);
        // Cache size for paint
        self.size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> flui_painting::Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderColoredBox::paint: color={:?}, size={:?}, offset={:?}",
            self.color,
            self.size,
            offset
        );

        // Create canvas for drawing
        let mut canvas = flui_painting::Canvas::new();

        // Apply offset directly to background rect
        let rect = Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        #[cfg(debug_assertions)]
        tracing::debug!("RenderColoredBox::paint: drawing rect={:?}", rect);

        // Create paint for the color
        let paint = flui_painting::Paint::fill(self.color);

        // Draw the background rectangle
        canvas.draw_rect(rect, &paint);

        // SingleArity always has exactly one child
        // Paint child at the same offset and append to our canvas
        let child_canvas = tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);

        canvas
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
    fn test_render_colored_box_new() {
        let colored = RenderColoredBox::new(Color::BLUE);
        assert_eq!(colored.color, Color::BLUE);
    }

    #[test]
    fn test_render_colored_box_default() {
        let colored = RenderColoredBox::default();
        assert_eq!(colored.color, Color::TRANSPARENT);
    }

    #[test]
    fn test_render_colored_box_set_color() {
        let mut colored = RenderColoredBox::new(Color::RED);
        colored.set_color(Color::GREEN);
        assert_eq!(colored.color, Color::GREEN);

        
    }
}
