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
    DynRenderObject, LeafRenderObjectElement, LeafRenderObjectWidget,
    RenderObjectWidget, Widget,
};
use flui_types::{Color, typography::{TextAlign, TextDirection, TextOverflow}};
use flui_rendering::{RenderParagraph, ParagraphData};

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
/// Text is a LeafRenderObjectWidget that creates a RenderParagraph object for
/// rendering. The actual text rendering is delegated to flui_rendering's RenderParagraph.
#[derive(Debug, Clone, Builder)]
pub struct Text {
    /// The text to display
    #[builder(into)]
    pub data: String,

    /// Text size in logical pixels
    #[builder(default = 14.0)]
    pub size: f32,

    /// Text color
    #[builder(default = Color::BLACK)]
    pub color: Color,

    /// Text alignment
    #[builder(default = TextAlign::Left)]
    pub text_align: TextAlign,

    /// Text direction
    #[builder(default = TextDirection::Ltr)]
    pub text_direction: TextDirection,

    /// Maximum number of lines
    pub max_lines: Option<usize>,

    /// Text overflow behavior
    #[builder(default = TextOverflow::Clip)]
    pub overflow: TextOverflow,

    /// Whether to wrap text at word boundaries
    #[builder(default = true)]
    pub soft_wrap: bool,

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
            color: Color::BLACK,
            text_align: TextAlign::Left,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
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
            color: Color::BLACK,
            text_align: TextAlign::Left,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
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
            text_align: TextAlign::Left,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            key: None,
        }
    }
}

impl Widget for Text {
    type Element = LeafRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        LeafRenderObjectElement::new(self)
    }
}

impl RenderObjectWidget for Text {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        let data = ParagraphData::new(&self.data)
            .with_font_size(self.size)
            .with_color(self.color)
            .with_align(self.text_align)
            .with_overflow(self.overflow);

        let mut data = if let Some(max_lines) = self.max_lines {
            data.with_max_lines(max_lines)
        } else {
            data
        };

        data.text_direction = self.text_direction;
        data.soft_wrap = self.soft_wrap;

        Box::new(RenderParagraph::new(data))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        if let Some(paragraph) = render_object.downcast_mut::<RenderParagraph>() {
            paragraph.set_text(&self.data);
            paragraph.set_font_size(self.size);
            paragraph.set_color(self.color);
            paragraph.set_text_align(self.text_align);

            // Update data fields directly
            let data = paragraph.data_mut();
            data.text_direction = self.text_direction;
            data.max_lines = self.max_lines;
            data.overflow = self.overflow;
            data.soft_wrap = self.soft_wrap;
        }
    }
}

impl LeafRenderObjectWidget for Text {}

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

    // RenderParagraph tests are in flui_rendering crate

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
