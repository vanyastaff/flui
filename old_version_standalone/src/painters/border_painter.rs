//! Border painting utilities.
//!
//! This module provides utilities for rendering borders with support for
//! individual side colors, widths, and styles.

use crate::types::styling::border::{Border, BorderSide, BorderStyle};
use crate::types::styling::BorderRadius;
use egui::{self, Painter, Rect, Stroke, Color32};

/// Utility for painting borders.
pub struct BorderPainter;

impl BorderPainter {
    /// Paint a border with potentially different styles on each side.
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to draw the border around
    /// * `border` - The border configuration
    /// * `border_radius` - Optional corner rounding
    pub fn paint_border(
        painter: &Painter,
        rect: Rect,
        border: &Border,
        border_radius: Option<BorderRadius>,
    ) {
        // Check if all sides are the same (optimization)
        if Self::is_uniform_border(border) {
            Self::paint_uniform_border(painter, rect, &border.top, border_radius);
        } else {
            Self::paint_individual_sides(painter, rect, border, border_radius);
        }
    }

    /// Check if all border sides are identical.
    fn is_uniform_border(border: &Border) -> bool {
        border.top == border.right
            && border.top == border.bottom
            && border.top == border.left
    }

    /// Paint a uniform border (all sides the same).
    fn paint_uniform_border(
        painter: &Painter,
        rect: Rect,
        side: &BorderSide,
        border_radius: Option<BorderRadius>,
    ) {
        if side.is_none() {
            return;
        }

        let stroke = Self::border_side_to_stroke(side);
        let rounding = border_radius
            .map(|br| br.top_left.x) // Use top-left for uniform rounding
            .unwrap_or(0.0);

        // Use Outside stroke kind to draw border outside the rect
        painter.rect_stroke(
            rect,
            rounding,
            stroke,
            egui::epaint::StrokeKind::Outside,
        );
    }

    /// Paint individual border sides with potentially different styles.
    fn paint_individual_sides(
        painter: &Painter,
        rect: Rect,
        border: &Border,
        border_radius: Option<BorderRadius>,
    ) {
        // For now, approximate with uniform border using top side
        // TODO: Implement proper per-side border rendering with mitered corners
        if !border.top.is_none() {
            Self::paint_uniform_border(painter, rect, &border.top, border_radius);
        }
    }

    /// Convert a BorderSide to an egui Stroke.
    fn border_side_to_stroke(side: &BorderSide) -> Stroke {
        let color = Self::convert_color(side.color);

        match side.style {
            BorderStyle::Solid => Stroke::new(side.width, color),
            BorderStyle::None => Stroke::NONE,
        }
    }

    /// Convert nebula Color to egui Color32.
    fn convert_color(color: crate::types::core::Color) -> Color32 {
        Color32::from_rgba_premultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        )
    }

    /// Paint a simple rectangular border (no rounding).
    pub fn paint_rect_border(
        painter: &Painter,
        rect: Rect,
        width: f32,
        color: crate::types::core::Color,
    ) {
        let stroke = Stroke::new(width, Self::convert_color(color));
        painter.rect_stroke(rect, 0.0, stroke, egui::epaint::StrokeKind::Outside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::Color;

    #[test]
    fn test_is_uniform_border() {
        let uniform = Border::all(BorderSide::solid(Color::BLACK, 1.0));
        assert!(BorderPainter::is_uniform_border(&uniform));

        let non_uniform = Border {
            top: BorderSide::solid(Color::BLACK, 1.0),
            right: BorderSide::solid(Color::RED, 1.0),
            bottom: BorderSide::solid(Color::BLACK, 1.0),
            left: BorderSide::solid(Color::BLACK, 1.0),
        };
        assert!(!BorderPainter::is_uniform_border(&non_uniform));
    }

    #[test]
    fn test_border_side_to_stroke() {
        let side = BorderSide::solid(Color::from_rgba(255, 0, 0, 255), 2.0);
        let stroke = BorderPainter::border_side_to_stroke(&side);

        assert_eq!(stroke.width, 2.0);
        assert_eq!(stroke.color.r(), 255);
        assert_eq!(stroke.color.g(), 0);
        assert_eq!(stroke.color.b(), 0);
    }

    #[test]
    fn test_convert_color() {
        let color = Color::from_rgba(100, 150, 200, 250);
        let egui_color = BorderPainter::convert_color(color);

        assert_eq!(egui_color.r(), 100);
        assert_eq!(egui_color.g(), 150);
        assert_eq!(egui_color.b(), 200);
        assert_eq!(egui_color.a(), 250);
    }
}
