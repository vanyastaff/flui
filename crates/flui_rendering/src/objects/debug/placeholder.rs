//! RenderPlaceholder - Debug placeholder visualization

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{Leaf, RenderBox};
use flui_painting::{Canvas, Paint};
use flui_types::prelude::{Color, TextStyle};
use flui_types::{Rect, Size};

/// RenderObject that displays a placeholder rectangle
///
/// Used for debugging and prototyping to visualize widget boundaries
/// and sizes. Shows a colored rectangle with optional text label.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderPlaceholder;
/// use flui_types::Size;
///
/// let placeholder = RenderPlaceholder::new(Size::new(100.0, 50.0))
///     .with_label("Placeholder");
/// ```
#[derive(Debug)]
pub struct RenderPlaceholder {
    /// Fallback size when unconstrained
    pub fallback_width: f32,
    /// Fallback height when unconstrained
    pub fallback_height: f32,
    /// Border color
    pub stroke_color: Color,
    /// Fill color
    pub fill_color: Color,
    /// Border width
    pub stroke_width: f32,
    /// Optional text label
    pub label: Option<String>,

    // Cache for layout
    size: Size,
}

impl RenderPlaceholder {
    /// Create new placeholder with fallback size
    pub fn new(fallback_size: Size) -> Self {
        Self {
            fallback_width: fallback_size.width,
            fallback_height: fallback_size.height,
            stroke_color: Color::rgba(100, 100, 100, 255), // Gray
            fill_color: Color::rgba(200, 200, 200, 128),   // Light gray, semi-transparent
            stroke_width: 2.0,
            label: None,
            size: Size::ZERO,
        }
    }

    /// Create with width and height
    pub fn with_size(width: f32, height: f32) -> Self {
        Self::new(Size::new(width, height))
    }

    /// Create square placeholder
    pub fn square(size: f32) -> Self {
        Self::new(Size::new(size, size))
    }

    /// Set fallback size
    pub fn set_fallback_size(&mut self, size: Size) {
        self.fallback_width = size.width;
        self.fallback_height = size.height;
    }

    /// Set stroke color
    pub fn set_stroke_color(&mut self, color: Color) {
        self.stroke_color = color;
    }

    /// Set fill color
    pub fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }

    /// Set stroke width
    pub fn set_stroke_width(&mut self, width: f32) {
        self.stroke_width = width;
    }

    /// Set label text
    pub fn set_label(&mut self, label: Option<String>) {
        self.label = label;
    }

    /// Create with custom colors
    pub fn with_colors(mut self, stroke: Color, fill: Color) -> Self {
        self.stroke_color = stroke;
        self.fill_color = fill;
        self
    }

    /// Create with label
    pub fn with_label<S: Into<String>>(mut self, label: S) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Create with stroke width
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }
}

impl Default for RenderPlaceholder {
    fn default() -> Self {
        Self::new(Size::new(100.0, 100.0))
    }
}

impl RenderBox<Leaf> for RenderPlaceholder {
    fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;

        // Use fallback size if unconstrained, otherwise use constraints
        let width = if constraints.max_width.is_finite() {
            self.fallback_width.min(constraints.max_width)
        } else {
            self.fallback_width
        };

        let height = if constraints.max_height.is_finite() {
            self.fallback_height.min(constraints.max_height)
        } else {
            self.fallback_height
        };

        let size = Size::new(width, height);
        self.size = size;
        size
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        let offset = _ctx.offset;
        let mut canvas = Canvas::new();

        let rect = Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Draw fill
        let mut fill_paint = Paint::default();
        fill_paint.color = self.fill_color;
        canvas.draw_rect(rect, &fill_paint);

        // Draw border
        let mut stroke_paint = Paint::default();
        stroke_paint.color = self.stroke_color;
        stroke_paint.style = flui_painting::PaintStyle::Stroke;
        stroke_paint.stroke_width = self.stroke_width;
        canvas.draw_rect(rect, &stroke_paint);

        // Draw label if present
        if let Some(ref label) = self.label {
            let text_style = TextStyle::default()
                .with_color(Color::BLACK)
                .with_font_size(14.0);

            // Center the text
            let text_offset =
                flui_types::Offset::new(offset.dx + 10.0, offset.dy + self.size.height / 2.0);

            let mut text_paint = Paint::default();
            text_paint.color = Color::BLACK;

            canvas.draw_text(label, text_offset, &text_style, &text_paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_placeholder_new() {
        let placeholder = RenderPlaceholder::new(Size::new(100.0, 50.0));

        assert_eq!(placeholder.fallback_width, 100.0);
        assert_eq!(placeholder.fallback_height, 50.0);
        assert_eq!(placeholder.stroke_width, 2.0);
        assert!(placeholder.label.is_none());
    }

    #[test]
    fn test_render_placeholder_with_size() {
        let placeholder = RenderPlaceholder::with_size(200.0, 150.0);

        assert_eq!(placeholder.fallback_width, 200.0);
        assert_eq!(placeholder.fallback_height, 150.0);
    }

    #[test]
    fn test_render_placeholder_square() {
        let placeholder = RenderPlaceholder::square(100.0);

        assert_eq!(placeholder.fallback_width, 100.0);
        assert_eq!(placeholder.fallback_height, 100.0);
    }

    #[test]
    fn test_render_placeholder_default() {
        let placeholder = RenderPlaceholder::default();

        assert_eq!(placeholder.fallback_width, 100.0);
        assert_eq!(placeholder.fallback_height, 100.0);
    }

    #[test]
    fn test_set_fallback_size() {
        let mut placeholder = RenderPlaceholder::default();
        placeholder.set_fallback_size(Size::new(50.0, 25.0));

        assert_eq!(placeholder.fallback_width, 50.0);
        assert_eq!(placeholder.fallback_height, 25.0);
    }

    #[test]
    fn test_set_stroke_color() {
        let mut placeholder = RenderPlaceholder::default();
        placeholder.set_stroke_color(Color::RED);

        assert_eq!(placeholder.stroke_color, Color::RED);
    }

    #[test]
    fn test_set_fill_color() {
        let mut placeholder = RenderPlaceholder::default();
        placeholder.set_fill_color(Color::BLUE);

        assert_eq!(placeholder.fill_color, Color::BLUE);
    }

    #[test]
    fn test_set_stroke_width() {
        let mut placeholder = RenderPlaceholder::default();
        placeholder.set_stroke_width(5.0);

        assert_eq!(placeholder.stroke_width, 5.0);
    }

    #[test]
    fn test_set_label() {
        let mut placeholder = RenderPlaceholder::default();
        placeholder.set_label(Some("Test".to_string()));

        assert_eq!(placeholder.label, Some("Test".to_string()));
    }

    #[test]
    fn test_with_colors() {
        let placeholder = RenderPlaceholder::default().with_colors(Color::RED, Color::BLUE);

        assert_eq!(placeholder.stroke_color, Color::RED);
        assert_eq!(placeholder.fill_color, Color::BLUE);
    }

    #[test]
    fn test_with_label() {
        let placeholder = RenderPlaceholder::default().with_label("Placeholder");

        assert_eq!(placeholder.label, Some("Placeholder".to_string()));
    }

    #[test]
    fn test_with_stroke_width() {
        let placeholder = RenderPlaceholder::default().with_stroke_width(3.0);

        assert_eq!(placeholder.stroke_width, 3.0);
    }

    #[test]
    fn test_arity_is_leaf() {
        let placeholder = RenderPlaceholder::default();

        assert_eq!(placeholder.arity(), RuntimeArity::Exact(0));
    }
}
