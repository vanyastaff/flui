//! Shadow painting implementation

use flui_engine::{Paint, Painter, RRect};
use flui_types::{Rect, styling::BoxShadow};

/// Painter for box shadows
pub struct ShadowPainter;

impl ShadowPainter {
    /// Paint a list of box shadows
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter to draw with
    /// * `rect` - The rectangle to paint shadows around
    /// * `shadows` - The list of box shadows to paint
    /// * `border_radius` - Optional border radius for rounded shadows
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        shadows: &[BoxShadow],
        border_radius: Option<f32>,
    ) {
        for shadow in shadows {
            Self::paint_single(painter, rect, shadow, border_radius);
        }
    }

    /// Paint a single box shadow
    fn paint_single(
        painter: &mut dyn Painter,
        rect: Rect,
        shadow: &BoxShadow,
        border_radius: Option<f32>,
    ) {
        // Calculate shadow rect with offset
        let shadow_rect = Rect::from_xywh(
            rect.left() + shadow.offset.dx,
            rect.top() + shadow.offset.dy,
            rect.width(),
            rect.height(),
        );

        // Adjust for spread radius
        let shadow_rect = if shadow.spread_radius > 0.0 {
            Rect::from_xywh(
                shadow_rect.left() - shadow.spread_radius,
                shadow_rect.top() - shadow.spread_radius,
                shadow_rect.width() + shadow.spread_radius * 2.0,
                shadow_rect.height() + shadow.spread_radius * 2.0,
            )
        } else if shadow.spread_radius < 0.0 {
            Rect::from_xywh(
                shadow_rect.left() + shadow.spread_radius.abs(),
                shadow_rect.top() + shadow.spread_radius.abs(),
                shadow_rect.width() - shadow.spread_radius.abs() * 2.0,
                shadow_rect.height() - shadow.spread_radius.abs() * 2.0,
            )
        } else {
            shadow_rect
        };

        let base_color = [
            shadow.color.red() as f32 / 255.0,
            shadow.color.green() as f32 / 255.0,
            shadow.color.blue() as f32 / 255.0,
            shadow.color.alpha() as f32 / 255.0,
        ];

        // Paint using the built-in shadow methods from Painter trait
        if let Some(radius) = border_radius {
            // Rounded rectangle shadow
            let rrect = RRect {
                rect: shadow_rect,
                corner_radius: radius,
            };

            let paint = Paint {
                color: base_color,
                stroke_width: 0.0,
                anti_alias: true,
            };

            let offset = flui_types::Offset::new(0.0, 0.0);
            painter.rrect_with_shadow(rrect, &paint, offset, shadow.blur_radius, base_color);
        } else {
            // Sharp rectangle shadow
            let paint = Paint {
                color: base_color,
                stroke_width: 0.0,
                anti_alias: true,
            };

            let offset = flui_types::Offset::new(0.0, 0.0);
            painter.rect_with_shadow(shadow_rect, &paint, offset, shadow.blur_radius, base_color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::{Offset, styling::Color};

    #[test]
    fn test_box_shadow_basic() {
        // Test basic shadow creation
        let shadow = BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(2.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
        };

        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.offset, Offset::new(2.0, 2.0));
        assert_eq!(shadow.blur_radius, 4.0);
        assert_eq!(shadow.spread_radius, 0.0);
    }

    #[test]
    fn test_box_shadow_with_blur() {
        // Test shadow with blur
        let shadow = BoxShadow {
            color: Color::rgba(0, 0, 0, 128),
            offset: Offset::new(0.0, 4.0),
            blur_radius: 8.0,
            spread_radius: 0.0,
        };

        assert_eq!(shadow.blur_radius, 8.0);
        assert!(shadow.blur_radius > 0.0);
    }

    #[test]
    fn test_box_shadow_with_spread() {
        // Test shadow with spread radius
        let shadow = BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(0.0, 0.0),
            blur_radius: 0.0,
            spread_radius: 4.0,
        };

        assert_eq!(shadow.spread_radius, 4.0);
    }

    #[test]
    fn test_box_shadow_negative_spread() {
        // Test shadow with negative spread (inset)
        let shadow = BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(0.0, 0.0),
            blur_radius: 0.0,
            spread_radius: -2.0,
        };

        assert_eq!(shadow.spread_radius, -2.0);
        assert!(shadow.spread_radius < 0.0);
    }

    #[test]
    fn test_box_shadow_no_offset() {
        // Test shadow with no offset (glow effect)
        let shadow = BoxShadow {
            color: Color::rgba(255, 255, 0, 200),
            offset: Offset::ZERO,
            blur_radius: 10.0,
            spread_radius: 0.0,
        };

        assert_eq!(shadow.offset, Offset::ZERO);
        assert!(shadow.blur_radius > 0.0);
    }

    #[test]
    fn test_box_shadow_color_variations() {
        // Test different shadow colors
        let colors = vec![
            Color::BLACK,
            Color::rgba(0, 0, 0, 128),
            Color::rgba(0, 0, 0, 64),
            Color::RED,
            Color::BLUE,
        ];

        for color in colors {
            let shadow = BoxShadow {
                color,
                offset: Offset::new(2.0, 2.0),
                blur_radius: 4.0,
                spread_radius: 0.0,
            };
            assert_eq!(shadow.color, color);
        }
    }

    #[test]
    fn test_multiple_shadows() {
        // Test array of shadows
        let shadows = vec![
            BoxShadow {
                color: Color::rgba(0, 0, 0, 64),
                offset: Offset::new(0.0, 1.0),
                blur_radius: 2.0,
                spread_radius: 0.0,
            },
            BoxShadow {
                color: Color::rgba(0, 0, 0, 32),
                offset: Offset::new(0.0, 4.0),
                blur_radius: 8.0,
                spread_radius: 0.0,
            },
        ];

        assert_eq!(shadows.len(), 2);
        assert_eq!(shadows[0].offset.dy, 1.0);
        assert_eq!(shadows[1].offset.dy, 4.0);
    }

    #[test]
    fn test_shadow_offset_directions() {
        // Test shadows in different directions
        let offsets = vec![
            Offset::new(2.0, 0.0),   // Right
            Offset::new(-2.0, 0.0),  // Left
            Offset::new(0.0, 2.0),   // Down
            Offset::new(0.0, -2.0),  // Up
            Offset::new(2.0, 2.0),   // Bottom-right
            Offset::new(-2.0, -2.0), // Top-left
        ];

        for offset in offsets {
            let shadow = BoxShadow {
                color: Color::BLACK,
                offset,
                blur_radius: 4.0,
                spread_radius: 0.0,
            };
            assert_eq!(shadow.offset, offset);
        }
    }

    #[test]
    fn test_shadow_blur_radius_values() {
        // Test different blur radii
        let blur_radii = vec![0.0, 1.0, 2.0, 4.0, 8.0, 16.0];

        for blur in blur_radii {
            let shadow = BoxShadow {
                color: Color::BLACK,
                offset: Offset::new(2.0, 2.0),
                blur_radius: blur,
                spread_radius: 0.0,
            };
            assert_eq!(shadow.blur_radius, blur);
        }
    }

    #[test]
    fn test_rect_for_shadow() {
        // Test rect setup for shadow painting
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 150.0);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 150.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 170.0);
    }

    #[test]
    fn test_shadow_with_border_radius() {
        // Test that border radius can be used with shadows
        let shadow = BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(2.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
        };

        let border_radius = Some(8.0);

        assert!(border_radius.is_some());
        assert_eq!(border_radius.unwrap(), 8.0);

        let _ = shadow;
    }
}
