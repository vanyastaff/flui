//! RenderErrorBox - Debug error visualization

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::{Canvas, Paint};
use flui_types::prelude::{Color, TextStyle};
use flui_types::{Rect, Size};

/// RenderObject that displays an error message in a red box
///
/// Used by Flutter to display errors in the UI when widgets fail to build
/// or render. Shows the error message in a distinctive red box with diagonal
/// stripes pattern.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderErrorBox;
///
/// let error_box = RenderErrorBox::new("Widget failed to build".to_string());
/// ```
#[derive(Debug)]
pub struct RenderErrorBox {
    /// Error message to display
    pub message: String,
    /// Background color (default: red)
    pub background_color: Color,
    /// Text color (default: white)
    pub text_color: Color,
    /// Whether to show diagonal stripes
    pub show_stripes: bool,

    // Cache for layout
    size: Size,
}

impl RenderErrorBox {
    /// Create new error box with message
    pub fn new(message: String) -> Self {
        Self {
            message,
            background_color: Color::rgba(198, 40, 40, 255), // Dark red
            text_color: Color::WHITE,
            show_stripes: true,
            size: Size::ZERO,
        }
    }

    /// Create with default "Error" message
    pub fn default_message() -> Self {
        Self::new("Error".to_string())
    }

    /// Set error message
    pub fn set_message(&mut self, message: String) {
        self.message = message;
    }

    /// Set background color
    pub fn set_background_color(&mut self, color: Color) {
        self.background_color = color;
    }

    /// Set text color
    pub fn set_text_color(&mut self, color: Color) {
        self.text_color = color;
    }

    /// Set whether to show stripes
    pub fn set_show_stripes(&mut self, show: bool) {
        self.show_stripes = show;
    }

    /// Create with custom colors
    pub fn with_colors(mut self, bg: Color, text: Color) -> Self {
        self.background_color = bg;
        self.text_color = text;
        self
    }

    /// Create without stripes
    pub fn without_stripes(mut self) -> Self {
        self.show_stripes = false;
        self
    }
}

impl Default for RenderErrorBox {
    fn default() -> Self {
        Self::default_message()
    }
}

impl Render for RenderErrorBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // Error box takes up all available space
        let size = constraints.biggest();

        self.size = size;
        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let offset = ctx.offset;
        let mut canvas = Canvas::new();

        let rect = Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Draw background
        let mut bg_paint = Paint::default();
        bg_paint.color = self.background_color;
        canvas.draw_rect(rect, &bg_paint);

        // Draw diagonal stripes for visual distinction
        if self.show_stripes {
            let stripe_color = Color::rgba(0, 0, 0, 50); // Semi-transparent black
            let mut stripe_paint = Paint::default();
            stripe_paint.color = stripe_color;

            let _stripe_width = 10.0;
            let stripe_spacing = 20.0;

            // Draw diagonal stripes from top-left to bottom-right
            let mut x = -self.size.height; // Start off-screen to cover the whole area
            while x < self.size.width {
                let x1 = offset.dx + x;
                let y1 = offset.dy;
                let x2 = offset.dx + x + self.size.height;
                let y2 = offset.dy + self.size.height;

                // Draw stripe as a thin rotated rectangle
                // For simplicity, we draw it as a line with width
                use flui_types::Point;
                canvas.draw_line(Point::new(x1, y1), Point::new(x2, y2), &stripe_paint);

                x += stripe_spacing;
            }
        }

        // Draw error message text
        let text_style = TextStyle::default()
            .with_color(self.text_color)
            .with_font_size(16.0);

        let text_offset = flui_types::Offset::new(offset.dx + 10.0, offset.dy + 10.0);

        let mut text_paint = Paint::default();
        text_paint.color = self.text_color;

        canvas.draw_text(&self.message, text_offset, &text_style, &text_paint);

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_error_box_new() {
        let error_box = RenderErrorBox::new("Test error".to_string());

        assert_eq!(error_box.message, "Test error");
        assert_eq!(error_box.background_color, Color::rgba(198, 40, 40, 255));
        assert_eq!(error_box.text_color, Color::WHITE);
        assert!(error_box.show_stripes);
    }

    #[test]
    fn test_render_error_box_default_message() {
        let error_box = RenderErrorBox::default_message();

        assert_eq!(error_box.message, "Error");
    }

    #[test]
    fn test_render_error_box_default() {
        let error_box = RenderErrorBox::default();

        assert_eq!(error_box.message, "Error");
    }

    #[test]
    fn test_set_message() {
        let mut error_box = RenderErrorBox::default();
        error_box.set_message("New error".to_string());

        assert_eq!(error_box.message, "New error");
    }

    #[test]
    fn test_set_background_color() {
        let mut error_box = RenderErrorBox::default();
        error_box.set_background_color(Color::BLUE);

        assert_eq!(error_box.background_color, Color::BLUE);
    }

    #[test]
    fn test_set_text_color() {
        let mut error_box = RenderErrorBox::default();
        error_box.set_text_color(Color::BLACK);

        assert_eq!(error_box.text_color, Color::BLACK);
    }

    #[test]
    fn test_set_show_stripes() {
        let mut error_box = RenderErrorBox::default();
        error_box.set_show_stripes(false);

        assert!(!error_box.show_stripes);
    }

    #[test]
    fn test_with_colors() {
        let error_box =
            RenderErrorBox::default().with_colors(Color::rgb(255, 0, 0), Color::rgb(0, 0, 0));

        assert_eq!(error_box.background_color, Color::rgb(255, 0, 0));
        assert_eq!(error_box.text_color, Color::rgb(0, 0, 0));
    }

    #[test]
    fn test_without_stripes() {
        let error_box = RenderErrorBox::default().without_stripes();

        assert!(!error_box.show_stripes);
    }

    #[test]
    fn test_arity_is_leaf() {
        let error_box = RenderErrorBox::default();

        assert_eq!(error_box.arity(), Arity::Exact(0));
    }
}
