//! Decoration painting utilities for egui integration
//!
//! This module provides BoxPainter for painting decorations, similar to Flutter's BoxPainter.
//! BoxPainter is a stateful class that can paint a particular Decoration.

use flui_types::{Rect, styling::BoxDecoration};

/// A stateful class that can paint a BoxDecoration.
///
/// Similar to Flutter's BoxPainter, this class holds onto the decoration configuration
/// and provides efficient painting. It can potentially cache egui shapes for performance.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::decoration_painter::BoxDecorationPainter;
/// use flui_types::styling::{BoxDecoration, Color};
///
/// let decoration = BoxDecoration::with_color(Color::rgba(255, 0, 0, 255));
/// let painter = BoxDecorationPainter::new(decoration);
///
/// // Later, in your render code:
/// painter.paint(&egui_painter, rect);
/// ```
#[derive(Debug)]
pub struct BoxDecorationPainter {
    /// The decoration to paint
    decoration: BoxDecoration,
}

impl BoxDecorationPainter {
    /// Create a new BoxDecorationPainter with the given decoration
    pub fn new(decoration: BoxDecoration) -> Self {
        Self { decoration }
    }

    /// Paint the decoration on the given egui::Painter at the given rect
    pub fn paint(&self, painter: &egui::Painter, rect: Rect) {
        // Convert flui Rect to egui Rect
        let egui_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top()),
            egui::vec2(rect.width(), rect.height()),
        );

        // Paint background color
        if let Some(color) = self.decoration.color {
            let egui_color = egui::Color32::from_rgba_unmultiplied(
                color.r,
                color.g,
                color.b,
                color.a,
            );

            // Get border radius for rounded corners
            let rounding = if let Some(ref border_radius) = self.decoration.border_radius {
                egui::CornerRadius {
                    nw: border_radius.top_left.x as u8,
                    ne: border_radius.top_right.x as u8,
                    sw: border_radius.bottom_left.x as u8,
                    se: border_radius.bottom_right.x as u8,
                }
            } else {
                egui::CornerRadius::ZERO
            };

            painter.rect_filled(egui_rect, rounding, egui_color);
        }

        // Paint border
        if let Some(ref border) = self.decoration.border {
            // Get border radius for rounded corners
            let rounding = if let Some(ref border_radius) = self.decoration.border_radius {
                egui::CornerRadius {
                    nw: border_radius.top_left.x as u8,
                    ne: border_radius.top_right.x as u8,
                    sw: border_radius.bottom_left.x as u8,
                    se: border_radius.bottom_right.x as u8,
                }
            } else {
                egui::CornerRadius::ZERO
            };

            // For simplicity, use the top border for all sides if they're the same
            // In a full implementation, we'd need to draw each side separately
            if let Some(ref top_border) = border.top {
                let border_color = egui::Color32::from_rgba_unmultiplied(
                    top_border.color.r,
                    top_border.color.g,
                    top_border.color.b,
                    top_border.color.a,
                );

                let stroke = egui::Stroke::new(top_border.width, border_color);
                painter.rect_stroke(egui_rect, rounding, stroke, egui::StrokeKind::Outside);
            }
        }

        // TODO: Paint box shadows (requires more complex egui integration)
        // TODO: Paint gradients (requires shader support or manual drawing)
        // TODO: Paint decoration image
    }

    /// Get a reference to the decoration
    pub fn decoration(&self) -> &BoxDecoration {
        &self.decoration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::{Border, BorderSide, BorderStyle, Color, BorderRadius};

    #[test]
    fn test_box_decoration_painter_new() {
        let decoration = BoxDecoration::with_color(Color::rgba(255, 0, 0, 255));
        let painter = BoxDecorationPainter::new(decoration.clone());

        assert_eq!(painter.decoration().color, Some(Color::rgba(255, 0, 0, 255)));
    }

    #[test]
    fn test_decoration_with_color() {
        let decoration = BoxDecoration::with_color(Color::rgba(255, 0, 0, 255));
        assert!(decoration.color.is_some());

        let _painter = BoxDecorationPainter::new(decoration);
        assert!(_painter.decoration().color.is_some());
    }

    #[test]
    fn test_decoration_with_border() {
        let border = Border::all(BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid));
        let decoration = BoxDecoration::default().set_border(Some(border));

        assert!(decoration.border.is_some());

        let _painter = BoxDecorationPainter::new(decoration);
        assert!(_painter.decoration().border.is_some());
    }

    #[test]
    fn test_decoration_with_border_radius() {
        let radius = BorderRadius::circular(8.0);
        let decoration = BoxDecoration::default().set_border_radius(Some(radius));

        assert!(decoration.border_radius.is_some());

        let _painter = BoxDecorationPainter::new(decoration);
        assert!(_painter.decoration().border_radius.is_some());
    }

    #[test]
    fn test_decoration_combined() {
        let decoration = BoxDecoration::default()
            .set_color(Some(Color::rgba(255, 255, 255, 255)))
            .set_border(Some(Border::all(BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid))))
            .set_border_radius(Some(BorderRadius::circular(4.0)));

        assert!(decoration.color.is_some());
        assert!(decoration.border.is_some());
        assert!(decoration.border_radius.is_some());

        let painter = BoxDecorationPainter::new(decoration.clone());
        assert_eq!(painter.decoration(), &decoration);
    }

    #[test]
    fn test_painter_method_exists() {
        // This test verifies the painter has the correct method signature
        let decoration = BoxDecoration::default();
        let _painter = BoxDecorationPainter::new(decoration);
        let _rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        // We can't actually create egui::Painter without a full egui context,
        // so we just verify the method exists with correct signature
        let _can_call: fn(&BoxDecorationPainter, &egui::Painter, Rect) = BoxDecorationPainter::paint;
    }
}
