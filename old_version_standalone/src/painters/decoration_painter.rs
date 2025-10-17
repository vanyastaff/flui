//! Decoration painting utilities.
//!
//! This module provides utilities for rendering BoxDecoration with all features:
//! background color, borders, shadows, gradients, and images.

use crate::types::styling::BoxDecoration;
use egui::{self, Painter, Rect, Color32};

use super::{GlowShadowPainter, BorderPainter};

/// Utility for painting box decorations.
///
/// Handles the complete rendering of a BoxDecoration including:
/// - Box shadows (painted first, behind everything)
/// - Background color/gradient
/// - Border
pub struct DecorationPainter;

impl DecorationPainter {
    /// Paint a complete box decoration.
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to paint the decoration in
    /// * `decoration` - The box decoration to paint
    ///
    /// # Rendering Order
    ///
    /// 1. Box shadows (behind)
    /// 2. Background color/gradient
    /// 3. Border (on top)
    pub fn paint(
        painter: &Painter,
        rect: Rect,
        decoration: &BoxDecoration,
    ) {
        let rounding = Self::get_rounding(decoration);

        // 1. Paint box shadows first (behind everything)
        // Use GPU-accelerated painter for better quality gaussian blur
        if !decoration.box_shadows.is_empty() {
            GlowShadowPainter::paint_shadows(painter, rect, &decoration.box_shadows, rounding);
        }

        // 2. Paint background color
        if let Some(color) = decoration.color {
            let bg_color = Self::convert_color(color);
            painter.rect_filled(rect, rounding, bg_color);
        }

        // 3. Paint gradient (if any)
        if let Some(gradient) = &decoration.gradient {
            // TODO: Implement gradient rendering
            // For now, gradients are not supported
            // Will need to use egui's mesh API or shaders
            let _ = gradient; // Silence unused warning
        }

        // 4. Paint border on top
        if let Some(border) = &decoration.border {
            BorderPainter::paint_border(
                painter,
                rect,
                border,
                decoration.border_radius,
            );
        }

        // Note: Images are handled separately by the Container widget
        // as they require texture management
    }

    /// Get the corner rounding from decoration.
    fn get_rounding(decoration: &BoxDecoration) -> f32 {
        decoration.border_radius
            .map(|br| br.top_left.x) // Use top-left for uniform rounding
            .unwrap_or(0.0)
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

    /// Paint a simple colored box (convenience method).
    pub fn paint_colored_box(
        painter: &Painter,
        rect: Rect,
        color: crate::types::core::Color,
        rounding: f32,
    ) {
        let egui_color = Self::convert_color(color);
        painter.rect_filled(rect, rounding, egui_color);
    }

    /// Paint with elevation shadow (Material Design style).
    pub fn paint_with_elevation(
        painter: &Painter,
        rect: Rect,
        color: crate::types::core::Color,
        elevation: f32,
        rounding: f32,
    ) {
        // Paint elevation shadow with GPU-accelerated gaussian blur
        GlowShadowPainter::paint_elevation(painter, rect, elevation, rounding);

        // Paint background
        Self::paint_colored_box(painter, rect, color, rounding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::Color;
    use crate::types::styling::BorderRadius;

    #[test]
    fn test_get_rounding() {
        let mut decoration = BoxDecoration::new();
        assert_eq!(DecorationPainter::get_rounding(&decoration), 0.0);

        decoration = decoration.with_border_radius(BorderRadius::circular(8.0));
        assert_eq!(DecorationPainter::get_rounding(&decoration), 8.0);
    }

    #[test]
    fn test_convert_color() {
        let color = Color::from_rgba(255, 128, 64, 200);
        let egui_color = DecorationPainter::convert_color(color);

        assert_eq!(egui_color.r(), 255);
        assert_eq!(egui_color.g(), 128);
        assert_eq!(egui_color.b(), 64);
        assert_eq!(egui_color.a(), 200);
    }
}
