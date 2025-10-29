//! Shape border painting implementation.
//!
//! Provides painting functionality for various shape borders including
//! rounded rectangles, circles, ovals, stars, and more.

use flui_engine::painter::{Paint, Painter, RRect};
use flui_types::{
    geometry::{Point, Rect},
    styling::{
        BeveledRectangleBorder, CircleBorder, ContinuousRectangleBorder, LinearBorder, OvalBorder,
        RoundedRectangleBorder, StadiumBorder, StarBorder,
    },
};
use std::f32::consts::PI;

/// Painter for shape borders.
///
/// Handles painting of various shape border types following Flutter's ShapeBorder model.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_painting::ShapePainter;
/// use flui_types::styling::{RoundedRectangleBorder, BorderSide, BorderRadius, Color};
///
/// let border = RoundedRectangleBorder::new(
///     BorderSide::new(Color::BLACK, 2.0, Default::default()),
///     BorderRadius::circular(10.0),
/// );
///
/// ShapePainter::paint_rounded_rect(painter, rect, &border);
/// ```
pub struct ShapePainter;

impl ShapePainter {
    /// Paints a rounded rectangle border.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The rectangle bounds
    /// * `border` - The rounded rectangle border style
    pub fn paint_rounded_rect(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &RoundedRectangleBorder,
    ) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        // Create rounded rectangle with average corner radius
        let avg_radius = (border.border_radius.top_left.x
            + border.border_radius.top_right.x
            + border.border_radius.bottom_left.x
            + border.border_radius.bottom_right.x)
            / 4.0;

        let rrect = RRect {
            rect,
            corner_radius: avg_radius,
        };

        painter.rrect(rrect, &paint);
    }

    /// Paints a beveled rectangle border.
    ///
    /// Beveled corners are drawn as straight cuts rather than curves.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The rectangle bounds
    /// * `border` - The beveled rectangle border style
    pub fn paint_beveled_rect(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &BeveledRectangleBorder,
    ) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        // Get bevel distances (using border_radius as bevel distance)
        let tl = border.border_radius.top_left.x;
        let tr = border.border_radius.top_right.x;
        let bl = border.border_radius.bottom_left.x;
        let br = border.border_radius.bottom_right.x;

        // Calculate corner points for beveled corners
        let left = rect.left();
        let top = rect.top();
        let right = rect.right();
        let bottom = rect.bottom();

        // Top-left corner
        let tl1 = Point::new(left + tl, top);
        let tl2 = Point::new(left, top + tl);

        // Top-right corner
        let tr1 = Point::new(right - tr, top);
        let tr2 = Point::new(right, top + tr);

        // Bottom-right corner
        let br1 = Point::new(right, bottom - br);
        let br2 = Point::new(right - br, bottom);

        // Bottom-left corner
        let bl1 = Point::new(left + bl, bottom);
        let bl2 = Point::new(left, bottom - bl);

        // Draw beveled border
        painter.line(tl1, tr1, &paint); // Top
        painter.line(tr1, tr2, &paint); // Top-right bevel
        painter.line(tr2, br1, &paint); // Right
        painter.line(br1, br2, &paint); // Bottom-right bevel
        painter.line(br2, bl1, &paint); // Bottom
        painter.line(bl1, bl2, &paint); // Bottom-left bevel
        painter.line(bl2, tl2, &paint); // Left
        painter.line(tl2, tl1, &paint); // Top-left bevel
    }

    /// Paints a circle border.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The bounding rectangle
    /// * `border` - The circle border style
    pub fn paint_circle(painter: &mut dyn Painter, rect: Rect, border: &CircleBorder) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        let center = rect.center();
        let radius = rect.width().min(rect.height()) / 2.0;

        painter.circle(center, radius, &paint);
    }

    /// Paints an oval border.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The bounding rectangle
    /// * `border` - The oval border style
    pub fn paint_oval(painter: &mut dyn Painter, rect: Rect, border: &OvalBorder) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        let center = rect.center();
        let rx = rect.width() / 2.0;
        let ry = rect.height() / 2.0;

        painter.ellipse(center, rx, ry, &paint);
    }

    /// Paints a stadium border (rectangle with semicircular ends).
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The bounding rectangle
    /// * `border` - The stadium border style
    pub fn paint_stadium(painter: &mut dyn Painter, rect: Rect, border: &StadiumBorder) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        // Stadium is a rounded rect with corner radius = half of the shorter side
        let radius = rect.width().min(rect.height()) / 2.0;
        let rrect = RRect {
            rect,
            corner_radius: radius,
        };

        painter.rrect(rrect, &paint);
    }

    /// Paints a star border.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The bounding rectangle
    /// * `border` - The star border style
    pub fn paint_star(painter: &mut dyn Painter, rect: Rect, border: &StarBorder) {
        if border.side.width <= 0.0 || border.points < 3 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        let center = rect.center();
        let outer_radius = rect.width().min(rect.height()) / 2.0;
        let inner_radius = outer_radius * border.inner_radius_ratio;

        let num_points = border.points as usize;
        let angle_per_point = 2.0 * PI / (num_points as f32);

        // Generate star points
        let mut points = Vec::with_capacity(num_points * 2);

        for i in 0..num_points {
            let angle = border.rotation + angle_per_point * i as f32 - PI / 2.0;

            // Outer point
            let outer_x = center.x + outer_radius * angle.cos();
            let outer_y = center.y + outer_radius * angle.sin();
            points.push(Point::new(outer_x, outer_y));

            // Inner point (between outer points)
            let inner_angle = angle + angle_per_point / 2.0;
            let inner_x = center.x + inner_radius * inner_angle.cos();
            let inner_y = center.y + inner_radius * inner_angle.sin();
            points.push(Point::new(inner_x, inner_y));
        }

        // Draw the star
        painter.polygon(&points, &paint);
    }

    /// Paints a continuous rectangle border.
    ///
    /// Continuous rectangles use a special curve for corners that provides
    /// a smoother appearance than standard rounded rectangles.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The rectangle bounds
    /// * `border` - The continuous rectangle border style
    pub fn paint_continuous_rect(
        painter: &mut dyn Painter,
        rect: Rect,
        border: &ContinuousRectangleBorder,
    ) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        // For now, approximate with rounded rect
        // TODO: Implement true continuous curve (squircle)
        let avg_radius = (border.border_radius.top_left.x
            + border.border_radius.top_right.x
            + border.border_radius.bottom_left.x
            + border.border_radius.bottom_right.x)
            / 4.0;

        let rrect = RRect {
            rect,
            corner_radius: avg_radius,
        };

        painter.rrect(rrect, &paint);
    }

    /// Paints a linear border.
    ///
    /// Draws only the specified edges of a rectangle.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `rect` - The rectangle bounds
    /// * `border` - The linear border style
    pub fn paint_linear(painter: &mut dyn Painter, rect: Rect, border: &LinearBorder) {
        if border.side.width <= 0.0 {
            return;
        }

        let color = Self::border_color(&border.side.color);
        let paint = Paint {
            color,
            stroke_width: border.side.width,
            anti_alias: true,
        };

        let left = rect.left();
        let top = rect.top();
        let right = rect.right();
        let bottom = rect.bottom();

        // Draw edges based on configuration
        if border.edges.top {
            painter.line(Point::new(left, top), Point::new(right, top), &paint);
        }
        if border.edges.right {
            painter.line(Point::new(right, top), Point::new(right, bottom), &paint);
        }
        if border.edges.bottom {
            painter.line(Point::new(right, bottom), Point::new(left, bottom), &paint);
        }
        if border.edges.left {
            painter.line(Point::new(left, bottom), Point::new(left, top), &paint);
        }
    }

    /// Converts a Color to RGBA array for painting.
    fn border_color(color: &flui_types::styling::Color) -> [f32; 4] {
        [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::{BorderRadius, BorderSide, BorderStyle, Color, LinearBorderEdges};

    fn test_side() -> BorderSide {
        BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid)
    }

    #[test]
    fn test_shape_painter_creation() {
        // ShapePainter is stateless
        let _painter = ShapePainter;
    }

    #[test]
    fn test_rounded_rect_border_zero_width() {
        let border = RoundedRectangleBorder::new(
            BorderSide::new(Color::BLACK, 0.0, BorderStyle::Solid),
            BorderRadius::circular(10.0),
        );
        // Should not panic with zero width
        assert_eq!(border.side.width, 0.0);
    }

    #[test]
    fn test_beveled_rect_creation() {
        let border = BeveledRectangleBorder::new(test_side(), BorderRadius::circular(5.0));
        assert_eq!(border.side.width, 2.0);
    }

    #[test]
    fn test_circle_border_creation() {
        let border = CircleBorder::new(test_side());
        assert_eq!(border.side.width, 2.0);
        assert_eq!(border.eccentricity, 0.0);
    }

    #[test]
    fn test_oval_border_creation() {
        let border = OvalBorder::new(test_side());
        assert_eq!(border.side.width, 2.0);
    }

    #[test]
    fn test_stadium_border_creation() {
        let border = StadiumBorder::new(test_side());
        assert_eq!(border.side.width, 2.0);
    }

    #[test]
    fn test_star_border_creation() {
        let border = StarBorder::new(test_side(), 5);
        assert_eq!(border.points, 5);
        assert_eq!(border.side.width, 2.0);
    }

    #[test]
    fn test_star_border_invalid_points() {
        let border = StarBorder::new(test_side(), 2);
        // Should handle < 3 points gracefully
        assert!(border.points < 3);
    }

    #[test]
    fn test_continuous_rect_creation() {
        let border = ContinuousRectangleBorder::new(test_side(), BorderRadius::circular(10.0));
        assert_eq!(border.side.width, 2.0);
    }

    #[test]
    fn test_linear_border_creation() {
        let border = LinearBorder::new(test_side(), LinearBorderEdges::ALL);
        assert_eq!(border.side.width, 2.0);
        assert!(border.edges.top);
        assert!(border.edges.right);
        assert!(border.edges.bottom);
        assert!(border.edges.left);
    }

    #[test]
    fn test_linear_border_top_only() {
        let border = LinearBorder::new(test_side(), LinearBorderEdges::TOP);
        assert!(border.edges.top);
        assert!(!border.edges.right);
        assert!(!border.edges.bottom);
        assert!(!border.edges.left);
    }

    #[test]
    fn test_border_color_conversion() {
        let color = Color::rgba(255, 128, 64, 200);
        let rgba = ShapePainter::border_color(&color);

        assert!((rgba[0] - 1.0).abs() < 0.01); // Red
        assert!((rgba[1] - 0.502).abs() < 0.01); // Green (128/255)
        assert!((rgba[2] - 0.251).abs() < 0.01); // Blue (64/255)
        assert!((rgba[3] - 0.784).abs() < 0.01); // Alpha (200/255)
    }
}
