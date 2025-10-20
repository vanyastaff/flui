//! Text widget - displays styled text
//!
//! The Text widget displays a string with a single style. It's one of the most
//! fundamental widgets in any UI framework.
//!
//! # Example
//!
//! ```rust,ignore
//! // Simple text
//! Text::new("Hello, World!")
//!
//! // Styled text with builder
//! Text::builder()
//!     .data("Hello, World!")
//!     .size(24.0)
//!     .color(Color::rgb(255, 0, 0))
//!     .build()
//! ```

use bon::Builder;
use flui_core::{
    BoxConstraints, Element, LeafRenderObjectWidget, Offset, RenderObject,
    RenderObjectWidget, Size, Widget,
};
use flui_types::Color;

/// A widget that displays a string of text with a single style.
///
/// The Text widget displays a string with a uniform style. For text with
/// multiple styles, use RichText instead.
///
/// # Example
///
/// ```rust,ignore
/// Text::new("Hello, World!")
/// ```
///
/// # Implementation
///
/// Text is a LeafRenderObjectWidget that creates a RenderText object for
/// rendering. The actual text rendering is delegated to egui.
#[derive(Debug, Clone, Builder)]
pub struct Text {
    /// The text to display
    #[builder(into)]
    pub data: String,

    /// Text size in logical pixels
    #[builder(default = 14.0)]
    pub size: f32,

    /// Text color
    #[builder(default = Color::rgb(0, 0, 0))]
    pub color: Color,

    /// Optional key for widget identification
    pub key: Option<String>,
}

impl Text {
    /// Create a new Text widget with the given string
    ///
    /// # Parameters
    ///
    /// - `data`: The text to display
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text = Text::new("Hello, World!");
    /// ```
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            size: 14.0,
            color: Color::rgb(0, 0, 0),
            key: None,
        }
    }

    /// Create a text widget with a specific size
    ///
    /// # Parameters
    ///
    /// - `data`: The text to display
    /// - `size`: Font size in logical pixels
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text = Text::sized("Hello", 24.0);
    /// ```
    pub fn sized(data: impl Into<String>, size: f32) -> Self {
        Self {
            data: data.into(),
            size,
            color: Color::rgb(0, 0, 0),
            key: None,
        }
    }

    /// Create a colored text widget
    ///
    /// # Parameters
    ///
    /// - `data`: The text to display
    /// - `color`: Text color
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text = Text::colored("Error!", Color::rgb(255, 0, 0));
    /// ```
    pub fn colored(data: impl Into<String>, color: Color) -> Self {
        Self {
            data: data.into(),
            size: 14.0,
            color,
            key: None,
        }
    }
}

impl Widget for Text {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(flui_core::RenderObjectElement::new(self.clone()))
    }

    fn key(&self) -> Option<&dyn flui_core::foundation::Key> {
        None
    }
}

impl RenderObjectWidget for Text {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(RenderText::new(
            self.data.clone(),
            self.size,
            self.color,
        ))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        if let Some(render_text) = render_object.downcast_mut::<RenderText>() {
            render_text.set_text(self.data.clone());
            render_text.set_size(self.size);
            render_text.set_color(self.color);
        }
    }
}

impl LeafRenderObjectWidget for Text {}

/// RenderObject for Text widget
///
/// Handles layout and painting of text using egui's text rendering.
#[derive(Debug)]
pub struct RenderText {
    /// The text to display
    text: String,

    /// Font size in logical pixels
    size: f32,

    /// Text color
    color: Color,

    /// Computed size after layout
    computed_size: Size,

    /// Whether layout is needed
    needs_layout_flag: bool,

    /// Whether paint is needed
    needs_paint_flag: bool,
}

impl RenderText {
    /// Create a new RenderText
    pub fn new(text: String, size: f32, color: Color) -> Self {
        Self {
            text,
            size,
            color,
            computed_size: Size::zero(),
            needs_layout_flag: true,
            needs_paint_flag: true,
        }
    }

    /// Set the text
    pub fn set_text(&mut self, text: String) {
        if self.text != text {
            self.text = text;
            self.mark_needs_layout();
        }
    }

    /// Set the font size
    pub fn set_size(&mut self, size: f32) {
        if self.size != size {
            self.size = size;
            self.mark_needs_layout();
        }
    }

    /// Set the text color
    pub fn set_color(&mut self, color: Color) {
        if self.color != color {
            self.color = color;
            self.mark_needs_paint();
        }
    }

    /// Calculate text size using egui
    fn calculate_text_size(&self) -> Size {
        // For now, use a simple heuristic
        // In a real implementation, we'd use egui's text measurement
        let char_count = self.text.chars().count() as f32;
        let width = char_count * self.size * 0.6; // Rough estimate
        let height = self.size * 1.2; // Line height

        Size::new(width, height)
    }
}

impl RenderObject for RenderText {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Calculate text size
        let text_size = self.calculate_text_size();

        // Constrain to bounds
        self.computed_size = constraints.constrain(text_size);
        self.needs_layout_flag = false;

        self.computed_size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Convert Flui color to egui color
        let egui_color = egui::Color32::from_rgb(
            self.color.red(),
            self.color.green(),
            self.color.blue(),
        );

        // Create egui text
        let galley = painter.layout_no_wrap(
            self.text.clone(),
            egui::FontId::proportional(self.size),
            egui_color,
        );

        // Paint the text
        let pos = egui::pos2(offset.dx, offset.dy);
        painter.galley(pos, galley, egui_color);
    }

    fn size(&self) -> Size {
        self.computed_size
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
        self.needs_paint_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }
}

/// Declarative macro for creating Text widgets
///
/// # Example
///
/// ```rust,ignore
/// text! {
///     data: "Hello, World!",
///     size: 24.0,
///     color: Color::rgb(255, 0, 0),
/// }
/// ```
#[macro_export]
macro_rules! text {
    // Simple text: text!("Hello")
    ($data:expr) => {
        $crate::Text::new($data)
    };

    // With fields: text! { data: "Hello", size: 24.0 }
    {
        $( $field:ident : $value:expr ),* $(,)?
    } => {
        $crate::Text::builder()
            $(
                .$field($value)
            )*
            .build()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_new() {
        let text = Text::new("Hello, World!");
        assert_eq!(text.data, "Hello, World!");
        assert_eq!(text.size, 14.0);
        assert_eq!(text.color, Color::rgb(0, 0, 0));
    }

    #[test]
    fn test_text_sized() {
        let text = Text::sized("Hello", 24.0);
        assert_eq!(text.data, "Hello");
        assert_eq!(text.size, 24.0);
    }

    #[test]
    fn test_text_colored() {
        let color = Color::rgb(255, 0, 0);
        let text = Text::colored("Error!", color);
        assert_eq!(text.data, "Error!");
        assert_eq!(text.color, color);
    }

    #[test]
    fn test_text_builder() {
        let text = Text::builder()
            .data("Test")
            .size(20.0)
            .color(Color::rgb(100, 100, 100))
            .build();

        assert_eq!(text.data, "Test");
        assert_eq!(text.size, 20.0);
        assert_eq!(text.color, Color::rgb(100, 100, 100));
    }

    #[test]
    fn test_render_text_set_text() {
        let mut render_text = RenderText::new("Hello".to_string(), 14.0, Color::rgb(0, 0, 0));

        assert_eq!(render_text.text, "Hello");
        assert!(render_text.needs_layout());

        render_text.needs_layout_flag = false;
        render_text.set_text("World".to_string());

        assert_eq!(render_text.text, "World");
        assert!(render_text.needs_layout());
    }

    #[test]
    fn test_render_text_set_size() {
        let mut render_text = RenderText::new("Test".to_string(), 14.0, Color::rgb(0, 0, 0));

        render_text.needs_layout_flag = false;
        render_text.set_size(24.0);

        assert_eq!(render_text.size, 24.0);
        assert!(render_text.needs_layout());
    }

    #[test]
    fn test_render_text_set_color() {
        let mut render_text = RenderText::new("Test".to_string(), 14.0, Color::rgb(0, 0, 0));

        render_text.needs_paint_flag = false;
        let new_color = Color::rgb(255, 0, 0);
        render_text.set_color(new_color);

        assert_eq!(render_text.color, new_color);
        assert!(render_text.needs_paint());
    }

    #[test]
    fn test_render_text_layout() {
        let mut render_text = RenderText::new("Hello".to_string(), 14.0, Color::rgb(0, 0, 0));

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = render_text.layout(constraints);

        assert!(!render_text.needs_layout());
        assert_eq!(size, render_text.size());
    }

    #[test]
    fn test_text_macro_simple() {
        let text = Text::new("Hello");
        assert_eq!(text.data, "Hello");
    }

    #[test]
    fn test_text_macro_with_fields() {
        let text = Text::builder()
            .data("Test")
            .size(20.0)
            .build();
        assert_eq!(text.data, "Test");
        assert_eq!(text.size, 20.0);
    }
}
