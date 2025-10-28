//! Border painting implementation

use flui_types::{Rect, Point, styling::{Border, BorderRadius}};
use flui_engine::{Painter, Paint, RRect};

/// Painter for borders
pub struct BorderPainter;

impl BorderPainter {
    /// Paint a border
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter to draw with
    /// * `rect` - The rectangle to paint the border around
    /// * `border` - The border to paint
    /// * `border_radius` - Optional border radius for rounded corners
    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &Border,
        border_radius: Option<BorderRadius>,
    ) {
        // Check if all sides are uniform
        let is_uniform = border.top == border.right
            && border.right == border.bottom
            && border.bottom == border.left;

        if is_uniform && border.top.map_or(false, |s| s.is_visible()) {
            // Simple uniform border
            Self::paint_uniform(painter, rect, border, border_radius);
        } else {
            // Paint each side separately
            Self::paint_sides(painter, rect, border, border_radius);
        }
    }

    /// Paint a uniform border (all sides the same)
    fn paint_uniform(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &Border,
        border_radius: Option<BorderRadius>,
    ) {
        let Some(side) = border.top else { return };

        let color = [
            side.color.red() as f32 / 255.0,
            side.color.green() as f32 / 255.0,
            side.color.blue() as f32 / 255.0,
            side.color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color,
            stroke_width: side.width,
            anti_alias: true,
        };

        // Adjust rect for stroke alignment
        let adjusted_rect = if side.stroke_align == 0.0 {
            // Inside stroke - shrink rect by half width
            Rect::from_xywh(
                rect.left() + side.width / 2.0,
                rect.top() + side.width / 2.0,
                rect.width() - side.width,
                rect.height() - side.width,
            )
        } else if side.stroke_align == 1.0 {
            // Outside stroke - expand rect by half width
            Rect::from_xywh(
                rect.left() - side.width / 2.0,
                rect.top() - side.width / 2.0,
                rect.width() + side.width,
                rect.height() + side.width,
            )
        } else {
            // Center stroke (0.5) or custom - no adjustment needed
            rect
        };

        // Draw border based on whether it has rounded corners
        if let Some(radius) = border_radius {
            // Use the first corner radius (assuming uniform for now)
            let corner_radius = radius.top_left.x;
            let rrect = RRect {
                rect: adjusted_rect,
                corner_radius,
            };
            painter.rrect(rrect, &paint);
        } else {
            painter.rect(adjusted_rect, &paint);
        }
    }

    /// Paint border sides separately (when they differ)
    fn paint_sides(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &Border,
        _border_radius: Option<BorderRadius>,
    ) {
        // Paint each side individually
        // This is a simplified implementation - proper implementation would
        // handle corners and stroke alignment more carefully

        // Top
        if let Some(top) = border.top {
            if top.is_visible() {
                let color = [
                    top.color.red() as f32 / 255.0,
                    top.color.green() as f32 / 255.0,
                    top.color.blue() as f32 / 255.0,
                    top.color.alpha() as f32 / 255.0,
                ];

                let paint = Paint {
                    color,
                    stroke_width: top.width,
                    anti_alias: true,
                };

                painter.line(
                    Point::new(rect.left(), rect.top()),
                    Point::new(rect.right(), rect.top()),
                    &paint,
                );
            }
        }

        // Right
        if let Some(right) = border.right {
            if right.is_visible() {
                let color = [
                    right.color.red() as f32 / 255.0,
                    right.color.green() as f32 / 255.0,
                    right.color.blue() as f32 / 255.0,
                    right.color.alpha() as f32 / 255.0,
                ];

                let paint = Paint {
                    color,
                    stroke_width: right.width,
                    anti_alias: true,
                };

                painter.line(
                    Point::new(rect.right(), rect.top()),
                    Point::new(rect.right(), rect.bottom()),
                    &paint,
                );
            }
        }

        // Bottom
        if let Some(bottom) = border.bottom {
            if bottom.is_visible() {
                let color = [
                    bottom.color.red() as f32 / 255.0,
                    bottom.color.green() as f32 / 255.0,
                    bottom.color.blue() as f32 / 255.0,
                    bottom.color.alpha() as f32 / 255.0,
                ];

                let paint = Paint {
                    color,
                    stroke_width: bottom.width,
                    anti_alias: true,
                };

                painter.line(
                    Point::new(rect.right(), rect.bottom()),
                    Point::new(rect.left(), rect.bottom()),
                    &paint,
                );
            }
        }

        // Left
        if let Some(left) = border.left {
            if left.is_visible() {
                let color = [
                    left.color.red() as f32 / 255.0,
                    left.color.green() as f32 / 255.0,
                    left.color.blue() as f32 / 255.0,
                    left.color.alpha() as f32 / 255.0,
                ];

                let paint = Paint {
                    color,
                    stroke_width: left.width,
                    anti_alias: true,
                };

                painter.line(
                    Point::new(rect.left(), rect.bottom()),
                    Point::new(rect.left(), rect.top()),
                    &paint,
                );
            }
        }

        // TODO: Properly handle rounded corners when sides differ
        // This would require path generation and custom rendering
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::{BorderSide, BorderStyle, Color};

    #[test]
    fn test_border_all_uniform() {
        // Test uniform border creation
        let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        let border = Border::all(side);

        assert_eq!(border.top, Some(side));
        assert_eq!(border.right, Some(side));
        assert_eq!(border.bottom, Some(side));
        assert_eq!(border.left, Some(side));
    }

    #[test]
    fn test_border_side_visibility() {
        // Test visible border
        let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        assert!(side.is_visible());

        // Test invisible border (zero width)
        let invisible = BorderSide::new(Color::BLACK, 0.0, BorderStyle::Solid);
        assert!(!invisible.is_visible());
    }

    #[test]
    fn test_border_stroke_align_default() {
        // Test default stroke alignment (inside)
        let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        assert_eq!(side.stroke_align, 0.0); // Default is inside
    }

    #[test]
    fn test_border_stroke_align_inside() {
        // Test inside stroke alignment
        let mut side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        side.stroke_align = 0.0; // Inside
        assert_eq!(side.stroke_align, 0.0);
    }

    #[test]
    fn test_border_stroke_align_center() {
        // Test center stroke alignment
        let mut side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        side.stroke_align = 0.5; // Center
        assert_eq!(side.stroke_align, 0.5);
    }

    #[test]
    fn test_border_stroke_align_outside() {
        // Test outside stroke alignment
        let mut side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        side.stroke_align = 1.0; // Outside
        assert_eq!(side.stroke_align, 1.0);
    }

    #[test]
    fn test_border_with_radius() {
        // Test border with rounded corners
        let side = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);
        let border = Border::all(side);
        let radius = BorderRadius::circular(10.0);

        assert_eq!(radius.top_left.x, 10.0);
        assert_eq!(radius.top_right.x, 10.0);
        assert_eq!(radius.bottom_left.x, 10.0);
        assert_eq!(radius.bottom_right.x, 10.0);

        let _ = border;
    }

    #[test]
    fn test_border_different_sides() {
        // Test border with different sides
        let top = BorderSide::new(Color::RED, 1.0, BorderStyle::Solid);
        let right = BorderSide::new(Color::GREEN, 2.0, BorderStyle::Solid);
        let bottom = BorderSide::new(Color::BLUE, 3.0, BorderStyle::Solid);
        let left = BorderSide::new(Color::YELLOW, 4.0, BorderStyle::Solid);

        let border = Border {
            top: Some(top),
            right: Some(right),
            bottom: Some(bottom),
            left: Some(left),
        };

        assert_eq!(border.top.unwrap().width, 1.0);
        assert_eq!(border.right.unwrap().width, 2.0);
        assert_eq!(border.bottom.unwrap().width, 3.0);
        assert_eq!(border.left.unwrap().width, 4.0);
    }

    #[test]
    fn test_border_partial_sides() {
        // Test border with only some sides
        let top = BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid);

        let border = Border {
            top: Some(top),
            right: None,
            bottom: None,
            left: None,
        };

        assert!(border.top.is_some());
        assert!(border.right.is_none());
        assert!(border.bottom.is_none());
        assert!(border.left.is_none());
    }

    #[test]
    fn test_border_color_variations() {
        // Test different colors
        let colors = [
            Color::BLACK,
            Color::WHITE,
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::TRANSPARENT,
        ];

        for color in colors {
            let side = BorderSide::new(color, 1.0, BorderStyle::Solid);
            assert_eq!(side.color, color);
        }
    }

    #[test]
    fn test_border_width_variations() {
        // Test different widths
        let widths = [0.0, 0.5, 1.0, 2.0, 5.0, 10.0];

        for width in widths {
            let side = BorderSide::new(Color::BLACK, width, BorderStyle::Solid);
            assert_eq!(side.width, width);
        }
    }

    #[test]
    fn test_rect_conversion() {
        // Test rect conversion for painting
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 150.0);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 150.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 170.0);
    }

    #[test]
    fn test_border_radius_uniform() {
        // Test uniform border radius
        let radius = BorderRadius::circular(15.0);

        assert_eq!(radius.top_left.x, 15.0);
        assert_eq!(radius.top_left.y, 15.0);
        assert_eq!(radius.top_right.x, 15.0);
        assert_eq!(radius.top_right.y, 15.0);
        assert_eq!(radius.bottom_left.x, 15.0);
        assert_eq!(radius.bottom_left.y, 15.0);
        assert_eq!(radius.bottom_right.x, 15.0);
        assert_eq!(radius.bottom_right.y, 15.0);
    }
}
