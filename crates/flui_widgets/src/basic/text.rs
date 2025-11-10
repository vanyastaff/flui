//! Text widget - displays styled text
//!
//! The Text widget displays a string with a single style. It's one of the most
//! fundamental widgets in any UI framework.
//!
//! # Usage Patterns
//!
//! ## 1. Simple Constructor
//! ```rust,ignore
//! Text::new("Hello, World!")
//! ```
//!
//! ## 2. Convenience Methods
//! ```rust,ignore
//! // Sized text
//! Text::sized("Hello", 24.0)
//!
//! // Colored text
//! Text::colored("Error!", Color::RED)
//!
//! // Typography presets
//! Text::headline("Main Title")      // 32px bold
//! Text::title("Section Title")      // 24px
//! Text::body("Regular text")        // 16px
//! Text::caption("Small text")       // 12px
//! ```
//!
//! ## 3. Builder Pattern
//! ```rust,ignore
//! Text::builder()
//!     .data("Styled text")
//!     .size(20.0)
//!     .color(Color::BLUE)
//!     .text_align(TextAlign::Center)
//!     .build()
//! ```
//!
//! ## 4. Macro
//! ```rust,ignore
//! text!("Hello")
//! text!(data: "Hello", size: 24.0, color: Color::RED)
//! ```

use bon::Builder;
use flui_core::view::{IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::{ParagraphData, RenderParagraph};
use flui_types::{
    typography::{TextAlign, TextDirection, TextOverflow},
    Color,
};

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
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Text {
    /// The text to display
    #[builder(default)]
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
    /// Create a new Text widget with the given string.
    ///
    /// Uses default styling: 14px black text, left-aligned.
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

    /// Create a text widget with a specific size.
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

    /// Create a colored text widget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text = Text::colored("Error!", Color::RED);
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

    /// Create headline text (large, prominent) - 32px.
    ///
    /// Perfect for page titles and main headings.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let title = Text::headline("Welcome to FLUI");
    /// ```
    pub fn headline(data: impl Into<String>) -> Self {
        Self::sized(data, 32.0)
    }

    /// Create title text (section heading) - 24px.
    ///
    /// Perfect for section titles and subheadings.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let section = Text::title("Getting Started");
    /// ```
    pub fn title(data: impl Into<String>) -> Self {
        Self::sized(data, 24.0)
    }

    /// Create body text (normal reading) - 16px.
    ///
    /// Perfect for paragraphs and regular content.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let content = Text::body("This is some regular text content.");
    /// ```
    pub fn body(data: impl Into<String>) -> Self {
        Self::sized(data, 16.0)
    }

    /// Create caption text (small, secondary) - 12px.
    ///
    /// Perfect for labels, captions, and metadata.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let label = Text::caption("Last updated: 2024");
    /// ```
    pub fn caption(data: impl Into<String>) -> Self {
        Self::sized(data, 12.0)
    }

    /// Create text with custom size and color.
    ///
    /// Convenience method combining both common customizations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let styled = Text::styled("Warning", 18.0, Color::ORANGE);
    /// ```
    pub fn styled(data: impl Into<String>, size: f32, color: Color) -> Self {
        Self {
            data: data.into(),
            size,
            color,
            text_align: TextAlign::Left,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            key: None,
        }
    }

    /// Create centered text.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let centered = Text::centered("Middle");
    /// ```
    pub fn centered(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            size: 14.0,
            color: Color::BLACK,
            text_align: TextAlign::Center,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            key: None,
        }
    }

    /// Create right-aligned text.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let right = Text::right_aligned("End");
    /// ```
    pub fn right_aligned(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            size: 14.0,
            color: Color::BLACK,
            text_align: TextAlign::Right,
            text_direction: TextDirection::Ltr,
            max_lines: None,
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            key: None,
        }
    }
}

// bon Builder Extensions
use text_builder::State;

impl<S: State> TextBuilder<S> {
    /// Builds the Text widget.
    pub fn build(self) -> Text {
        self.build_internal()
    }
}

// Implement View for Text - Simplified API
impl View for Text {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create paragraph data
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

        // Create and return RenderParagraph via LeafRenderBuilder
        RenderBuilder::new(RenderParagraph::new(data))
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
    fn test_text_headline() {
        let text = Text::headline("Main Title");
        assert_eq!(text.data, "Main Title");
        assert_eq!(text.size, 32.0);
    }

    #[test]
    fn test_text_title() {
        let text = Text::title("Section");
        assert_eq!(text.data, "Section");
        assert_eq!(text.size, 24.0);
    }

    #[test]
    fn test_text_body() {
        let text = Text::body("Content");
        assert_eq!(text.data, "Content");
        assert_eq!(text.size, 16.0);
    }

    #[test]
    fn test_text_caption() {
        let text = Text::caption("Small");
        assert_eq!(text.data, "Small");
        assert_eq!(text.size, 12.0);
    }

    #[test]
    fn test_text_styled() {
        let color = Color::rgb(255, 128, 0);
        let text = Text::styled("Warning", 18.0, color);
        assert_eq!(text.data, "Warning");
        assert_eq!(text.size, 18.0);
        assert_eq!(text.color, color);
    }

    #[test]
    fn test_text_centered() {
        let text = Text::centered("Middle");
        assert_eq!(text.data, "Middle");
        assert_eq!(text.text_align, TextAlign::Center);
    }

    #[test]
    fn test_text_right_aligned() {
        let text = Text::right_aligned("End");
        assert_eq!(text.data, "End");
        assert_eq!(text.text_align, TextAlign::Right);
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
        let text = Text::builder().data("Test").size(20.0).build();
        assert_eq!(text.data, "Test");
        assert_eq!(text.size, 20.0);
    }
}

// Text now implements View trait directly - no need for IntoWidget wrapper
