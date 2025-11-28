//! RenderErrorBox - Debug error visualization

use crate::core::{BoxProtocol, LayoutContext, PaintContext};
use crate::core::{Leaf, RenderBox};
use flui_painting::Paint;
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

impl<T: FullRenderTree> RenderBox<T, Leaf> for RenderErrorBox {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;

        // Error box takes up all available space
        let size = Size::new(constraints.max_width, constraints.max_height);

        self.size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: crate::core::PaintTree,
    {
        let rect = Rect::from_min_size(flui_types::Point::ZERO, self.size);

        // Draw background
        let bg_paint = Paint::fill(self.background_color);
        ctx.canvas().rect(rect, &bg_paint);

        // Draw diagonal stripes if enabled
        if self.show_stripes {
            let stripe_paint = Paint::stroke(Color::rgba(255, 255, 255, 50), 2.0);

            let stripe_spacing = 20.0;
            let mut x = 0.0;
            while x < self.size.width + self.size.height {
                ctx.canvas().line(
                    flui_types::Point::new(x, 0.0),
                    flui_types::Point::new(x - self.size.height, self.size.height),
                    &stripe_paint,
                );
                x += stripe_spacing;
            }
        }

        // Draw error message
        let text_paint = Paint::fill(self.text_color);
        let text_style = TextStyle::default()
            .with_font_size(14.0)
            .with_color(self.text_color);

        ctx.canvas().text(
            &self.message,
            flui_types::Offset::new(10.0, self.size.height / 2.0),
            &text_style,
            &text_paint,
        );
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
}
