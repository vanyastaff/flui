//! RenderColoredBox - simple solid color box

use flui_types::{Color, Size, Rect};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, PictureLayer, Paint};

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
}

impl RenderColoredBox {
    /// Create new RenderColoredBox with specified color
    pub fn new(color: Color) -> Self {
        Self { color }
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

impl RenderObject for RenderColoredBox {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();
        let constraints = cx.constraints();

        // SingleArity always has exactly one child
        // Pass through constraints
        cx.layout_child(child, constraints)
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let child = cx.child();

        // Create picture layer for background color
        let mut picture = PictureLayer::new();

        // TODO: Get actual size from layout context instead of using placeholder
        // The PaintCx should provide access to the laid-out size from the layout phase
        let size = 1000.0;
        let rect = Rect::from_xywh(0.0, 0.0, size, size);

        // Create paint for the color
        let mut paint = Paint::default();
        let (r, g, b, a) = self.color.to_rgba_f32();
        paint.color = [r, g, b, a];

        // Draw the background rectangle
        picture.draw_rect(rect, paint);

        // SingleArity always has exactly one child
        let child_layer = cx.capture_child_layer(child);

        // Use ContainerLayer to stack background + child - use pool for efficiency
        let mut container = flui_engine::layer::pool::acquire_container();
        container.add_child(Box::new(picture));
        container.add_child(child_layer);

        Box::new(container)
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
