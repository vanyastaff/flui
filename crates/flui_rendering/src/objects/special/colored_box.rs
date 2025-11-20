//! RenderColoredBox - simple solid color box

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{Leaf, RenderBox};
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

impl RenderBox<Leaf> for RenderColoredBox {
    fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;
        // Leaf renders have no children - fill available space
        let size = Size::new(constraints.max_width, constraints.max_height);
        self.size = size;
        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
        // Draw solid color rectangle
        let rect = Rect::from_min_size(flui_types::Point::ZERO, self.size);
        let mut paint = flui_painting::Paint::default();
        paint.color = self.color;
        paint.style = flui_painting::PaintStyle::Fill;

        ctx.canvas().draw_rect(rect, &paint);
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
