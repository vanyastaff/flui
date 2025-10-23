//! RenderColoredBox - simple solid color box

use flui_types::{Color, Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Data for RenderColoredBox
#[derive(Debug, Clone, Copy)]
pub struct ColoredBoxData {
    /// Background color
    pub color: Color,
}

impl ColoredBoxData {
    /// Create new colored box data
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

/// RenderObject that paints a solid color background
///
/// A simplified version of RenderDecoratedBox that only handles solid colors.
/// More efficient than DecoratedBox when you only need a background color.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::special::ColoredBoxData};
/// use flui_types::Color;
///
/// // Create a red background
/// let mut colored = SingleRenderBox::new(ColoredBoxData::new(Color::RED));
/// ```
pub type RenderColoredBox = SingleRenderBox<ColoredBoxData>;

// ===== Public API =====

impl RenderColoredBox {
    /// Get current color
    pub fn color(&self) -> Color {
        self.data().color
    }

    /// Set color
    pub fn set_color(&mut self, color: Color) {
        use crate::core::RenderBoxMixin;

        if self.data().color != color {
            self.data_mut().color = color;
            RenderBoxMixin::mark_needs_paint(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderColoredBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints (pass-through)
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            // No child: fill available space or shrink to zero
            constraints.biggest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        use crate::core::RenderBoxMixin;

        if let Some(size) = RenderBoxMixin::size(self) {
            // Paint background color
            let rect = egui::Rect::from_min_size(
                egui::pos2(offset.dx, offset.dy),
                egui::vec2(size.width, size.height),
            );

            let egui_color = egui::Color32::from_rgb(
                self.data().color.red(),
                self.data().color.green(),
                self.data().color.blue(),
            );

            painter.rect_filled(rect, 0.0, egui_color);
        }

        // Paint child on top
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colored_box_data_new() {
        let data = ColoredBoxData::new(Color::RED);
        assert_eq!(data.color, Color::RED);
    }

    #[test]
    fn test_render_colored_box_new() {
        let colored = SingleRenderBox::new(ColoredBoxData::new(Color::BLUE));
        assert_eq!(colored.color(), Color::BLUE);
    }

    #[test]
    fn test_render_colored_box_set_color() {
        use flui_core::DynRenderObject;

        let mut colored = SingleRenderBox::new(ColoredBoxData::new(Color::RED));

        colored.set_color(Color::GREEN);
        assert_eq!(colored.color(), Color::GREEN);
        assert!(DynRenderObject::needs_paint(&colored));
    }

    #[test]
    fn test_render_colored_box_set_same_color() {
        use flui_core::DynRenderObject;

        let mut colored = SingleRenderBox::new(ColoredBoxData::new(Color::RED));
        colored.clear_needs_paint();

        colored.set_color(Color::RED);
        assert!(!DynRenderObject::needs_paint(&colored));
    }

    #[test]
    fn test_render_colored_box_layout_no_child() {
        let mut colored = SingleRenderBox::new(ColoredBoxData::new(Color::RED));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = colored.layout(constraints);

        // No child, should fill available space
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_colored_box_layout_tight_constraints() {
        let mut colored = SingleRenderBox::new(ColoredBoxData::new(Color::BLUE));
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

        let size = colored.layout(constraints);

        assert_eq!(size, Size::new(50.0, 50.0));
    }
}
