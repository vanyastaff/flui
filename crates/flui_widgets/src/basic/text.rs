//! Text widget - displays styled text.
//!
//! A widget that displays a string of text with a single style.
//! Similar to Flutter's Text widget.
//!
//! # Usage Patterns
//!
//! ## 1. Simple Text
//! ```rust,ignore
//! Text::new("Hello, World!")
//! ```
//!
//! ## 2. Styled Text
//! ```rust,ignore
//! Text::new("Hello")
//!     .style(TextStyle::default().with_font_size(24.0))
//!     .color(Color::BLUE)
//! ```
//!
//! ## 3. Multi-line Text
//! ```rust,ignore
//! Text::new("Long text that wraps...")
//!     .max_lines(3)
//!     .overflow(TextOverflow::Ellipsis)
//! ```
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `Text` widget which builds a `RichText`.

use flui_rendering::prelude::*;
use flui_types::styling::Color;
use flui_types::typography::{
    InlineSpan, StrutStyle, TextAlign, TextDirection, TextHeightBehavior, TextOverflow, TextSpan,
    TextStyle, TextWidthBasis,
};
use flui_view::{impl_render_view, RenderView};

/// A widget that displays a string of text with styling.
///
/// The text to display is described using a tree of [TextSpan] objects,
/// each of which has its own associated style.
///
/// ## Layout Behavior
///
/// - Text wraps at word boundaries by default (soft_wrap = true)
/// - Text is clipped or truncated based on overflow setting
/// - Text alignment can be configured
///
/// ## Flutter Equivalence
///
/// This corresponds to Flutter's `Text` widget.
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple text
/// Text::new("Hello, World!")
///
/// // Styled text
/// Text::new("Hello")
///     .font_size(24.0)
///     .color(Color::BLUE)
///     .text_align(TextAlign::Center)
///
/// // Multi-line with overflow
/// Text::new("This is a very long text that should wrap to multiple lines...")
///     .max_lines(2)
///     .overflow(TextOverflow::Ellipsis)
/// ```
#[derive(Debug)]
pub struct Text {
    /// The text to display (as InlineSpan for rich text support).
    text: InlineSpan,

    /// The style to apply to the text.
    style: Option<TextStyle>,

    /// How the text should be aligned horizontally.
    text_align: TextAlign,

    /// The directionality of the text.
    text_direction: TextDirection,

    /// Whether the text should break at soft line breaks.
    soft_wrap: bool,

    /// How visual overflow should be handled.
    overflow: TextOverflow,

    /// The text scale factor for accessibility.
    text_scale_factor: f32,

    /// Maximum number of lines before truncation.
    max_lines: Option<u32>,

    /// Strut style for consistent line heights.
    strut_style: Option<StrutStyle>,

    /// How to measure text width.
    text_width_basis: TextWidthBasis,

    /// Text height behavior.
    text_height_behavior: Option<TextHeightBehavior>,

    /// Selection color (for selectable text).
    selection_color: Option<Color>,
}

impl Clone for Text {
    fn clone(&self) -> Self {
        Self {
            text: self.text.clone(),
            style: self.style.clone(),
            text_align: self.text_align,
            text_direction: self.text_direction,
            soft_wrap: self.soft_wrap,
            overflow: self.overflow,
            text_scale_factor: self.text_scale_factor,
            max_lines: self.max_lines,
            strut_style: self.strut_style.clone(),
            text_width_basis: self.text_width_basis,
            text_height_behavior: self.text_height_behavior.clone(),
            selection_color: self.selection_color,
        }
    }
}

impl Text {
    /// Creates a new Text widget with the given string.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Text::new("Hello, World!")
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        let text_string = text.into();
        Self {
            text: InlineSpan::from(TextSpan::new(text_string)),
            style: None,
            text_align: TextAlign::Start,
            text_direction: TextDirection::Ltr,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            text_scale_factor: 1.0,
            max_lines: None,
            strut_style: None,
            text_width_basis: TextWidthBasis::Parent,
            text_height_behavior: None,
            selection_color: None,
        }
    }

    /// Creates a new Text widget with a rich text span.
    ///
    /// Use this for complex text with multiple styles.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let span = TextSpan::new("Hello ")
    ///     .add_child(TextSpan::new("World").with_style(bold_style));
    /// Text::rich(span)
    /// ```
    pub fn rich(text_span: impl Into<InlineSpan>) -> Self {
        Self {
            text: text_span.into(),
            style: None,
            text_align: TextAlign::Start,
            text_direction: TextDirection::Ltr,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            text_scale_factor: 1.0,
            max_lines: None,
            strut_style: None,
            text_width_basis: TextWidthBasis::Parent,
            text_height_behavior: None,
            selection_color: None,
        }
    }

    // ========================================================================
    // Builder Methods
    // ========================================================================

    /// Sets the text style.
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Sets the text color.
    ///
    /// Shorthand for `style(TextStyle::default().with_color(color))`.
    pub fn color(mut self, color: Color) -> Self {
        let style = self.style.take().unwrap_or_default();
        self.style = Some(style.with_color(color));
        self
    }

    /// Sets the font size.
    ///
    /// Shorthand for `style(TextStyle::default().with_font_size(size))`.
    pub fn font_size(mut self, size: f64) -> Self {
        let style = self.style.take().unwrap_or_default();
        self.style = Some(style.with_font_size(size));
        self
    }

    /// Sets the text alignment.
    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    /// Sets the text direction.
    pub fn text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = direction;
        self
    }

    /// Sets whether text should wrap at soft line breaks.
    pub fn soft_wrap(mut self, wrap: bool) -> Self {
        self.soft_wrap = wrap;
        self
    }

    /// Sets the overflow behavior.
    pub fn overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Sets the text scale factor.
    pub fn text_scale_factor(mut self, factor: f32) -> Self {
        self.text_scale_factor = factor;
        self
    }

    /// Sets the maximum number of lines.
    pub fn max_lines(mut self, max: u32) -> Self {
        self.max_lines = Some(max);
        self
    }

    /// Sets the strut style.
    pub fn strut_style(mut self, strut: StrutStyle) -> Self {
        self.strut_style = Some(strut);
        self
    }

    /// Sets the text width basis.
    pub fn text_width_basis(mut self, basis: TextWidthBasis) -> Self {
        self.text_width_basis = basis;
        self
    }

    /// Sets the text height behavior.
    pub fn text_height_behavior(mut self, behavior: TextHeightBehavior) -> Self {
        self.text_height_behavior = Some(behavior);
        self
    }

    /// Sets the selection color.
    pub fn selection_color(mut self, color: Color) -> Self {
        self.selection_color = Some(color);
        self
    }

    // ========================================================================
    // Getters
    // ========================================================================

    /// Returns the text span.
    pub fn text(&self) -> &InlineSpan {
        &self.text
    }

    /// Returns the effective inline span with merged style.
    ///
    /// If a style was set on the Text widget, it creates a new TextSpan
    /// with the merged style applied.
    fn effective_text(&self) -> InlineSpan {
        if let Some(ref widget_style) = self.style {
            // Create a new TextSpan with merged style
            let plain_text = self.text.to_plain_text();
            let base_style = self.text.style().cloned().unwrap_or_default();
            let merged_style = base_style.merge(widget_style);

            InlineSpan::from(TextSpan::new(plain_text).with_style(merged_style))
        } else {
            self.text.clone()
        }
    }
}

// Implement View trait via macro (creates create_element and as_any)
impl_render_view!(Text);

impl RenderView for Text {
    type RenderObject = RenderParagraph;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderParagraph::with_config(
            self.effective_text(),
            self.text_align,
            self.text_direction,
            self.soft_wrap,
            self.overflow,
            self.text_scale_factor,
            self.max_lines,
            self.strut_style.clone(),
            self.text_width_basis,
            self.text_height_behavior.clone(),
            self.selection_color,
        )
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        // Update text
        render_object.set_text(self.effective_text());

        // Update properties
        render_object.set_text_align(self.text_align);
        render_object.set_text_direction(self.text_direction);
        render_object.set_soft_wrap(self.soft_wrap);
        render_object.set_overflow(self.overflow);
        render_object.set_text_scale_factor(self.text_scale_factor);
        render_object.set_max_lines(self.max_lines);
        render_object.set_strut_style(self.strut_style.clone());
        render_object.set_text_width_basis(self.text_width_basis);
        render_object.set_text_height_behavior(self.text_height_behavior.clone());
        render_object.set_selection_color(self.selection_color);
    }

    fn has_children(&self) -> bool {
        false // Text is a leaf widget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_new() {
        let text = Text::new("Hello");
        assert_eq!(text.text().to_plain_text(), "Hello");
    }

    #[test]
    fn test_text_color() {
        let text = Text::new("Hello").color(Color::RED);
        assert!(text.style.is_some());
        assert_eq!(text.style.as_ref().unwrap().color, Some(Color::RED));
    }

    #[test]
    fn test_text_font_size() {
        let text = Text::new("Hello").font_size(24.0);
        assert!(text.style.is_some());
        assert_eq!(text.style.as_ref().unwrap().font_size, Some(24.0));
    }

    #[test]
    fn test_text_text_align() {
        let text = Text::new("Hello").text_align(TextAlign::Center);
        assert_eq!(text.text_align, TextAlign::Center);
    }

    #[test]
    fn test_text_overflow() {
        let text = Text::new("Hello").overflow(TextOverflow::Ellipsis);
        assert_eq!(text.overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn test_text_max_lines() {
        let text = Text::new("Hello").max_lines(3);
        assert_eq!(text.max_lines, Some(3));
    }

    #[test]
    fn test_text_soft_wrap() {
        let text = Text::new("Hello").soft_wrap(false);
        assert!(!text.soft_wrap);
    }

    #[test]
    fn test_render_view_create() {
        let text = Text::new("Hello, World!");
        let render = text.create_render_object();
        assert!(render.text().is_some());
    }

    #[test]
    fn test_text_chained_builders() {
        let text = Text::new("Hello")
            .color(Color::BLUE)
            .font_size(16.0)
            .text_align(TextAlign::Center)
            .max_lines(2)
            .overflow(TextOverflow::Ellipsis);

        assert!(text.style.is_some());
        assert_eq!(text.text_align, TextAlign::Center);
        assert_eq!(text.max_lines, Some(2));
        assert_eq!(text.overflow, TextOverflow::Ellipsis);
    }
}
