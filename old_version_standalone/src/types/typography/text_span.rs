//! TextSpan and InlineSpan types for rich text rendering.
//!
//! Similar to Flutter's TextSpan, these types allow creating text with multiple styles
//! within a single text widget.

use super::{TextStyle, TextScaler};

/// A span of text or inline content.
///
/// This is the base trait for text content that can be rendered inline.
/// Similar to Flutter's InlineSpan.
pub trait InlineSpan: std::fmt::Debug {
    /// Convert this span to an egui LayoutJob.
    ///
    /// This is the main method for rendering inline spans with egui.
    fn to_layout_job(&self, scaler: &TextScaler) -> egui::text::LayoutJob;
}

/// An immutable span of text with optional styling and children.
///
/// A TextSpan can have:
/// - Plain text with a style
/// - Children TextSpan objects with their own styles
/// - Both text and children (text is treated as first child)
///
/// # Examples
///
/// ```ignore
/// use nebula_ui::types::typography::{TextSpan, TextStyle};
/// use nebula_ui::types::core::Color;
///
/// // Simple styled text
/// let span = TextSpan::new("Hello world!")
///     .with_style(TextStyle::body().with_color(Color::BLACK));
///
/// // Multiple styles
/// let span = TextSpan::builder()
///     .child(TextSpan::new("Bold ").with_style(TextStyle::body().bold()))
///     .child(TextSpan::new("Italic").with_style(TextStyle::body().italic()))
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    /// The text content of this span (optional if has children)
    pub text: Option<String>,

    /// Child spans (can contain styled sub-spans)
    pub children: Vec<TextSpan>,

    /// Style to apply to this span and its text
    pub style: Option<TextStyle>,

    /// Alternative semantics label for accessibility
    ///
    /// Note: Limited support in egui - mostly for documentation compatibility
    pub semantics_label: Option<String>,
}

impl TextSpan {
    /// Create a new TextSpan with the given text.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let span = TextSpan::new("Hello");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            children: Vec::new(),
            style: None,
            semantics_label: None,
        }
    }

    /// Create an empty TextSpan (container for children).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let span = TextSpan::empty()
    ///     .with_child(TextSpan::new("child1"))
    ///     .with_child(TextSpan::new("child2"));
    /// ```
    pub fn empty() -> Self {
        Self {
            text: None,
            children: Vec::new(),
            style: None,
            semantics_label: None,
        }
    }

    /// Set the style for this span.
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Add a child span.
    pub fn with_child(mut self, child: TextSpan) -> Self {
        self.children.push(child);
        self
    }

    /// Add multiple children.
    pub fn with_children(mut self, children: Vec<TextSpan>) -> Self {
        self.children.extend(children);
        self
    }

    /// Set the semantics label.
    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// Check if this span is empty (no text and no children).
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.children.is_empty()
    }

    /// Get the total text length (including children).
    pub fn text_len(&self) -> usize {
        let mut len = self.text.as_ref().map(|t| t.len()).unwrap_or(0);
        for child in &self.children {
            len += child.text_len();
        }
        len
    }

    /// Convert to plain text (strips all styling).
    pub fn to_plain_text(&self) -> String {
        let mut result = String::new();
        if let Some(text) = &self.text {
            result.push_str(text);
        }
        for child in &self.children {
            result.push_str(&child.to_plain_text());
        }
        result
    }
}

impl InlineSpan for TextSpan {
    fn to_layout_job(&self, scaler: &TextScaler) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob::default();
        self.append_to_layout_job(&mut job, scaler, None);
        job
    }
}

impl TextSpan {
    /// Append this span to a LayoutJob (internal helper).
    ///
    /// This recursively processes text and children, applying inherited styles.
    fn append_to_layout_job(
        &self,
        job: &mut egui::text::LayoutJob,
        scaler: &TextScaler,
        parent_style: Option<&TextStyle>,
    ) {
        // Merge this span's style with parent style
        let effective_style = match (&self.style, parent_style) {
            (Some(style), Some(parent)) => Some(parent.merge(style)),
            (Some(style), None) => Some(style.clone()),
            (None, Some(parent)) => Some(parent.clone()),
            (None, None) => None,
        };

        // Add this span's text if present
        if let Some(text) = &self.text {
            let format = if let Some(style) = &effective_style {
                style_to_text_format(style, scaler)
            } else {
                default_text_format(scaler)
            };

            job.append(text, 0.0, format);
        }

        // Add children
        for child in &self.children {
            child.append_to_layout_job(job, scaler, effective_style.as_ref());
        }
    }
}

impl Default for TextSpan {
    fn default() -> Self {
        Self::empty()
    }
}

// ============================================================================
// Helper functions for egui conversion
// ============================================================================

/// Convert TextStyle to egui TextFormat.
fn style_to_text_format(style: &TextStyle, scaler: &TextScaler) -> egui::text::TextFormat {
    use crate::types::typography::text_style::text_style_to_egui;

    let (font_id, color) = text_style_to_egui(style, scaler);

    egui::text::TextFormat {
        font_id,
        color,
        background: egui::Color32::TRANSPARENT,
        italics: style.italic,
        underline: egui::Stroke::NONE, // TODO: support underline from decoration
        strikethrough: egui::Stroke::NONE, // TODO: support strikethrough
        valign: egui::Align::BOTTOM,
        ..Default::default()
    }
}

/// Create default egui TextFormat.
fn default_text_format(scaler: &TextScaler) -> egui::text::TextFormat {
    use crate::types::typography::text_style::default_egui_style;

    let (font_id, color) = default_egui_style(scaler);

    egui::text::TextFormat {
        font_id,
        color,
        background: egui::Color32::TRANSPARENT,
        italics: false,
        underline: egui::Stroke::NONE,
        strikethrough: egui::Stroke::NONE,
        valign: egui::Align::BOTTOM,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::Color;

    #[test]
    fn test_text_span_new() {
        let span = TextSpan::new("Hello");
        assert_eq!(span.text, Some("Hello".to_string()));
        assert!(span.children.is_empty());
        assert!(span.style.is_none());
    }

    #[test]
    fn test_text_span_empty() {
        let span = TextSpan::empty();
        assert!(span.text.is_none());
        assert!(span.children.is_empty());
        assert!(span.is_empty());
    }

    #[test]
    fn test_text_span_with_style() {
        let style = TextStyle::body();
        let span = TextSpan::new("Hello").with_style(style.clone());
        assert_eq!(span.style, Some(style));
    }

    #[test]
    fn test_text_span_with_children() {
        let child1 = TextSpan::new("child1");
        let child2 = TextSpan::new("child2");
        let span = TextSpan::empty()
            .with_child(child1)
            .with_child(child2);

        assert_eq!(span.children.len(), 2);
        assert_eq!(span.children[0].text, Some("child1".to_string()));
        assert_eq!(span.children[1].text, Some("child2".to_string()));
    }

    #[test]
    fn test_text_span_text_len() {
        let span = TextSpan::new("Hello")
            .with_child(TextSpan::new(" "))
            .with_child(TextSpan::new("world"));

        assert_eq!(span.text_len(), 11); // "Hello" + " " + "world"
    }

    #[test]
    fn test_text_span_to_plain_text() {
        let span = TextSpan::new("Bold ")
            .with_style(TextStyle::body().bold())
            .with_child(TextSpan::new("Italic").with_style(TextStyle::body().italic()));

        assert_eq!(span.to_plain_text(), "Bold Italic");
    }

    #[test]
    fn test_text_span_is_empty() {
        assert!(TextSpan::empty().is_empty());
        assert!(!TextSpan::new("text").is_empty());
        assert!(!TextSpan::empty().with_child(TextSpan::new("child")).is_empty());
    }

    #[test]
    fn test_text_span_semantics() {
        let span = TextSpan::new("Hello")
            .with_semantics_label("Greeting");

        assert_eq!(span.semantics_label, Some("Greeting".to_string()));
    }

    #[test]
    fn test_text_span_to_layout_job() {
        let scaler = TextScaler::none();
        let span = TextSpan::new("Hello world");
        let job = span.to_layout_job(&scaler);

        assert_eq!(job.text, "Hello world");
    }

    #[test]
    fn test_text_span_complex() {
        let scaler = TextScaler::none();
        let span = TextSpan::empty()
            .with_child(TextSpan::new("Bold ").with_style(TextStyle::body().bold()))
            .with_child(TextSpan::new("Normal "))
            .with_child(TextSpan::new("Italic").with_style(TextStyle::body().italic()));

        let job = span.to_layout_job(&scaler);
        assert_eq!(job.text, "Bold Normal Italic");
    }
}
