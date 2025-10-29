//! Path painting implementation.
//!
//! Provides PathPainter for rendering vector paths with various fill and stroke styles.

use flui_engine::painter::{Paint, Painter};
use flui_types::painting::path::Path;
use flui_types::styling::Color;

/// Painter for rendering vector paths.
///
/// Handles path rendering with fill and stroke operations, supporting various
/// path operations like lines, curves, and shapes.
///
/// # Examples
///
/// ```rust
/// use flui_painting::PathPainter;
/// use flui_types::painting::Path;
/// use flui_types::geometry::Point;
/// use flui_types::styling::Color;
///
/// let mut path = Path::new();
/// path.move_to(Point::new(10.0, 10.0));
/// path.line_to(Point::new(100.0, 10.0));
/// path.line_to(Point::new(100.0, 100.0));
/// path.close();
///
/// // PathPainter::fill(painter, &path, Color::rgb(255, 0, 0));
/// ```
pub struct PathPainter;

impl PathPainter {
    /// Fills a path with the given color.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `path` - The path to fill
    /// * `color` - The fill color
    pub fn fill(painter: &mut dyn Painter, path: &Path, color: Color) {
        let paint_color = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color: paint_color,
            stroke_width: 0.0,
            anti_alias: true,
        };

        Self::paint_path(painter, path, &paint, false);
    }

    /// Strokes a path with the given color and width.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `path` - The path to stroke
    /// * `color` - The stroke color
    /// * `stroke_width` - The width of the stroke
    pub fn stroke(painter: &mut dyn Painter, path: &Path, color: Color, stroke_width: f32) {
        let paint_color = [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ];

        let paint = Paint {
            color: paint_color,
            stroke_width,
            anti_alias: true,
        };

        Self::paint_path(painter, path, &paint, true);
    }

    /// Paints a path with custom paint settings.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `path` - The path to paint
    /// * `paint` - The paint settings
    /// * `_is_stroke` - Whether this is a stroke operation (unused, stroke info is in paint)
    fn paint_path(painter: &mut dyn Painter, path: &Path, paint: &Paint, _is_stroke: bool) {
        // Use the native path() method which has a default implementation
        // that decomposes paths into primitives. Backends can override this
        // for more efficient native path rendering.
        painter.path(path, paint);
    }

    /// Paints a path with both fill and stroke.
    ///
    /// # Arguments
    ///
    /// * `painter` - The painter to draw with
    /// * `path` - The path to paint
    /// * `fill_color` - Optional fill color
    /// * `stroke_color` - Optional stroke color
    /// * `stroke_width` - Width of the stroke
    pub fn fill_and_stroke(
        painter: &mut dyn Painter,
        path: &Path,
        fill_color: Option<Color>,
        stroke_color: Option<Color>,
        stroke_width: f32,
    ) {
        if let Some(color) = fill_color {
            Self::fill(painter, path, color);
        }

        if let Some(color) = stroke_color {
            Self::stroke(painter, path, color, stroke_width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::painting::Path;

    #[test]
    fn test_path_painter_creation() {
        // PathPainter is stateless, just verify we can create paths
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));

        assert!(!path.is_empty());
    }

    #[test]
    fn test_bezier_conversion() {
        // Test that quadratic to cubic conversion is reasonable
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.quadratic_to(Point::new(50.0, 50.0), Point::new(100.0, 0.0));

        assert_eq!(path.commands().len(), 2);
    }
}
