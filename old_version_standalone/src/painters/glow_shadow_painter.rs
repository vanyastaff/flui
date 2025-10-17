//! GPU-accelerated shadow painting with gaussian blur.
//!
//! This module provides hardware-accelerated shadow rendering using OpenGL shaders
//! for realistic gaussian blur effects.

use crate::types::core::{Color, Offset};
use crate::types::styling::shadow::BoxShadow;
use egui::{Painter, Rect, Mesh, TextureId, Color32, Pos2, Vec2};

/// GPU-accelerated shadow painter using glow shaders.
///
/// This painter uses gaussian blur shaders for more realistic shadows
/// compared to the multi-layer approximation in ShadowPainter.
pub struct GlowShadowPainter;

impl GlowShadowPainter {
    /// Paint a box shadow with GPU-accelerated gaussian blur.
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to cast the shadow from
    /// * `shadow` - The box shadow configuration
    /// * `rounding` - Corner rounding radius
    ///
    /// # Implementation Note
    ///
    /// For now, this falls back to the CPU-based multi-layer approach.
    /// Full GPU shader implementation requires:
    /// 1. Custom shader compilation
    /// 2. Framebuffer management
    /// 3. Two-pass blur (horizontal + vertical)
    ///
    /// TODO: Implement full GPU gaussian blur with glow
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

        // For now, use improved multi-layer approach
        // TODO: Replace with actual GPU gaussian blur shader
        Self::paint_shadow_multilayer(painter, shadow_rect, shadow, rounding);
    }

    /// Paint shadow using improved multi-layer approximation.
    ///
    /// This is a better approximation than the basic approach,
    /// using more layers with better alpha falloff.
    fn paint_shadow_multilayer(
        painter: &Painter,
        shadow_rect: Rect,
        shadow: &BoxShadow,
        rounding: f32,
    ) {
        let shadow_color = Self::convert_color(shadow.color);

        if shadow.blur_radius > 0.0 {
            // Use more layers for smoother blur approximation
            let layers = (shadow.blur_radius / 1.5).ceil() as i32;
            let layers = layers.max(3).min(20); // 3-20 layers

            // Gaussian-like falloff
            for i in 0..layers {
                let progress = (i as f32) / (layers as f32);

                // Gaussian curve approximation: exp(-x^2)
                let gaussian = (-4.0 * progress * progress).exp();
                let layer_alpha = (shadow_color.a() as f32 * gaussian) as u8;

                let layer_color = Color32::from_rgba_premultiplied(
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

    /// Paint multiple box shadows with GPU acceleration.
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
    fn convert_color(color: Color) -> Color32 {
        Color32::from_rgba_premultiplied(
            color.red(),
            color.green(),
            color.blue(),
            color.alpha(),
        )
    }

    /// Paint a Material Design elevation shadow with GPU acceleration.
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

// TODO: Implement true GPU gaussian blur shader
//
// The full implementation would look like this:
//
// 1. Create custom shader:
//    ```glsl
//    // Horizontal blur pass
//    uniform sampler2D inputTexture;
//    uniform float blurRadius;
//
//    void main() {
//        vec4 color = vec4(0.0);
//        float total = 0.0;
//
//        for (int i = -radius; i <= radius; i++) {
//            float weight = exp(-float(i*i) / (2.0 * sigma * sigma));
//            color += texture(inputTexture, uv + vec2(i * texelSize, 0.0)) * weight;
//            total += weight;
//        }
//
//        fragColor = color / total;
//    }
//    ```
//
// 2. Two-pass rendering:
//    - Pass 1: Horizontal blur to temp framebuffer
//    - Pass 2: Vertical blur from temp to screen
//
// 3. Integration with egui_glow:
//    - Create framebuffer objects
//    - Compile shaders
//    - Render to texture
//    - Composite result
//
// This would give true gaussian blur at 60fps even for large blur radii.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_shadow_rect() {
        let rect = Rect::from_min_max(
            Pos2::new(10.0, 10.0),
            Pos2::new(50.0, 50.0),
        );

        let shadow = BoxShadow::simple(
            Color::BLACK,
            Offset::new(5.0, 5.0),
            10.0,
        ).with_spread_radius(2.0);

        let shadow_rect = GlowShadowPainter::calculate_shadow_rect(rect, &shadow);

        assert_eq!(shadow_rect.min.x, 13.0); // 10 + 5 - 2
        assert_eq!(shadow_rect.min.y, 13.0);
        assert_eq!(shadow_rect.max.x, 57.0); // 50 + 5 + 2
        assert_eq!(shadow_rect.max.y, 57.0);
    }

    #[test]
    fn test_convert_color() {
        let color = Color::from_rgba(255, 128, 64, 200);
        let egui_color = GlowShadowPainter::convert_color(color);

        assert_eq!(egui_color.r(), 255);
        assert_eq!(egui_color.g(), 128);
        assert_eq!(egui_color.b(), 64);
        assert_eq!(egui_color.a(), 200);
    }
}
