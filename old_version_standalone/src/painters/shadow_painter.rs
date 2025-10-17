//! Shadow painting utilities.
//!
//! This module provides utilities for rendering box shadows with blur effects.

use crate::types::core::{Color, Offset};
use crate::types::styling::shadow::BoxShadow;
use egui::{self, Painter, Rect};

/// Utility for painting box shadows.
pub struct ShadowPainter;

impl ShadowPainter {
    /// Paint a single box shadow.
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to cast the shadow from
    /// * `shadow` - The box shadow configuration
    /// * `rounding` - Corner rounding radius
    pub fn paint_shadow(
        painter: &Painter,
        rect: Rect,
        shadow: &BoxShadow,
        rounding: f32,
    ) {
        if shadow.is_none() {
            return;
        }

        // Calculate shadow rect with offset and spread
        let shadow_rect = Self::calculate_shadow_rect(rect, shadow);

        // Convert color
        let shadow_color = Self::convert_color(shadow.color);

        // For now, use egui's built-in shadow (simplified)
        // TODO: Implement proper blur with spread radius and blur styles
        if shadow.blur_radius > 0.0 {
            // egui doesn't have native box shadow with blur, so we approximate
            // by drawing multiple layers with decreasing opacity
            let layers = (shadow.blur_radius / 2.0).ceil() as i32;
            let layers = layers.max(1).min(10); // Limit layers for performance

            for i in 0..layers {
                let progress = (i as f32) / (layers as f32);
                let layer_alpha = (shadow_color.a() as f32 * (1.0 - progress)) as u8;
                let layer_color = egui::Color32::from_rgba_premultiplied(
                    shadow_color.r(),
                    shadow_color.g(),
                    shadow_color.b(),
                    layer_alpha,
                );

                let expansion = progress * shadow.blur_radius;
                let layer_rect = shadow_rect.expand(expansion);

                painter.rect_filled(layer_rect, rounding + expansion, layer_color);
            }
        } else {
            // Hard shadow (no blur)
            painter.rect_filled(shadow_rect, rounding, shadow_color);
        }
    }

    /// Paint multiple box shadows (layered).
    ///
    /// Shadows are painted from back to front (first shadow in the list is painted first).
    pub fn paint_shadows(
        painter: &Painter,
        rect: Rect,
        shadows: &[BoxShadow],
        rounding: f32,
    ) {
        for shadow in shadows {
            Self::paint_shadow(painter, rect, shadow, rounding);
        }
    }

    /// Calculate the shadow rectangle with offset and spread.
    fn calculate_shadow_rect(rect: Rect, shadow: &BoxShadow) -> Rect {
        let offset = egui::vec2(shadow.offset.dx, shadow.offset.dy);
        let spread = shadow.spread_radius;

        rect.translate(offset).expand(spread)
    }

    /// Convert nebula Color to egui Color32.
    fn convert_color(color: Color) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        )
    }

    /// Paint a Material Design elevation shadow.
    ///
    /// This creates the typical two-layer shadow effect used in Material Design.
    pub fn paint_elevation(
        painter: &Painter,
        rect: Rect,
        elevation: f32,
        rounding: f32,
    ) {
        let (key_shadow, ambient_shadow) = BoxShadow::elevation_shadows(elevation);

        // Paint ambient shadow first (it's wider and behind)
        Self::paint_shadow(painter, rect, &ambient_shadow, rounding);

        // Paint key shadow on top
        Self::paint_shadow(painter, rect, &key_shadow, rounding);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_shadow_rect() {
        let rect = Rect::from_min_max(
            egui::pos2(10.0, 10.0),
            egui::pos2(50.0, 50.0),
        );

        let shadow = BoxShadow::simple(
            Color::BLACK,
            Offset::new(5.0, 5.0),
            10.0,
        ).with_spread_radius(2.0);

        let shadow_rect = ShadowPainter::calculate_shadow_rect(rect, &shadow);

        // Should be offset by (5, 5) and expanded by spread (2)
        assert_eq!(shadow_rect.min.x, 10.0 + 5.0 - 2.0); // 13.0
        assert_eq!(shadow_rect.min.y, 10.0 + 5.0 - 2.0); // 13.0
        assert_eq!(shadow_rect.max.x, 50.0 + 5.0 + 2.0); // 57.0
        assert_eq!(shadow_rect.max.y, 50.0 + 5.0 + 2.0); // 57.0
    }

    #[test]
    fn test_convert_color() {
        let color = Color::from_rgba(255, 128, 64, 200);
        let egui_color = ShadowPainter::convert_color(color);

        assert_eq!(egui_color.r(), 255);
        assert_eq!(egui_color.g(), 128);
        assert_eq!(egui_color.b(), 64);
        assert_eq!(egui_color.a(), 200);
    }
}
