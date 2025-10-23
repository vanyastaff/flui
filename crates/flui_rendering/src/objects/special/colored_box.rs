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
        if self.data().color != color {
            self.data_mut().color = color;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderColoredBox {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Layout child with same constraints (pass-through)
        let children_ids = ctx.children();
        let size =
        if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            // No child: fill available space or shrink to zero
            constraints.biggest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        if let Some(size) = *state.size.lock() {
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
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
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
        use flui_core::testing::mock_render_context;

        let colored = SingleRenderBox::new(ColoredBoxData::new(Color::RED));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = colored.layout(constraints, &ctx);

        // No child, should fill available space
        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_render_colored_box_layout_tight_constraints() {
        use flui_core::testing::mock_render_context;

        let colored = SingleRenderBox::new(ColoredBoxData::new(Color::BLUE));
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

        let (_tree, ctx) = mock_render_context();
        let size = colored.layout(constraints, &ctx);

        assert_eq!(size, Size::new(50.0, 50.0));
    }
}
