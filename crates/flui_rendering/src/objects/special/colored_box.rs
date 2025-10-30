//! RenderColoredBox - simple solid color box

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::{BoxedLayer, Paint, PictureLayer};
use flui_types::{Color, Rect, Size, Offset, constraints::BoxConstraints};

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

impl SingleRender for RenderColoredBox {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
                        // SingleArity always has exactly one child
        // Pass through constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
                // Create picture layer for background color
        let mut picture = PictureLayer::new();

        // TODO: Get actual size from layout context instead of using placeholder
        // The PaintCx should provide access to the laid-out size from the layout phase
        let size = 1000.0;
        let rect = Rect::from_xywh(0.0, 0.0, size, size);

        // Create paint for the color
        let mut paint = Paint::default();
        paint.color = self.color;

        // Draw the background rectangle
        picture.draw_rect(rect, paint);

        // SingleArity always has exactly one child
        let child_layer = tree.paint_child(child_id, offset);

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
