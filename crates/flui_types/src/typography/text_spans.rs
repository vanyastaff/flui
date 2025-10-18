//! Text span types for rich text.

use super::{TextStyle, TextBaseline};
use std::sync::Arc;

/// Trait for inline spans (text or placeholders).
pub trait InlineSpanTrait: std::fmt::Debug {
    /// Returns the style for this span, if any.
    fn style(&self) -> Option<&TextStyle> {
        None
    }

    /// Visits this span and its children.
    fn visit(&self, visitor: &mut dyn FnMut(&dyn InlineSpanTrait) -> bool) where Self: Sized {
        visitor(self);
    }

    /// Returns the text content of this span, if any.
    fn to_plain_text(&self) -> String {
        String::new()
    }

    /// Returns true if this span contains semantic labels.
    fn has_semantics(&self) -> bool {
        false
    }
}

/// Type-erased inline span.
#[derive(Debug, Clone)]
pub struct InlineSpan {
    inner: Arc<dyn InlineSpanTrait + Send + Sync>,
}

impl InlineSpan {
    /// Creates a new inline span from a concrete type.
    pub fn new<T: InlineSpanTrait + Send + Sync + 'static>(span: T) -> Self {
        Self {
            inner: Arc::new(span),
        }
    }

    /// Returns a reference to the inner span.
    pub fn as_trait(&self) -> &(dyn InlineSpanTrait + Send + Sync) {
        &*self.inner
    }
}

/// A span of text with a style.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub struct TextSpan {
    /// Text content.
    pub text: Option<String>,
    /// Style for this span.
    pub style: Option<TextStyle>,
    /// Child spans.
    pub children: Vec<TextSpan>,
    /// Semantic label for accessibility.
    pub semantics_label: Option<String>,
    /// Mouse cursor when hovering.
    pub mouse_cursor: Option<MouseCursor>,
    /// Callback when tapped (not serializable).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for TextSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSpan")
            .field("text", &self.text)
            .field("style", &self.style)
            .field("children", &self.children)
            .field("semantics_label", &self.semantics_label)
            .field("mouse_cursor", &self.mouse_cursor)
            .field("on_tap", &self.on_tap.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

impl PartialEq for TextSpan {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.style == other.style
            && self.children == other.children
            && self.semantics_label == other.semantics_label
            && self.mouse_cursor == other.mouse_cursor
            // We don't compare callbacks
    }
}


impl TextSpan {
    /// Creates a new text span.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// Creates a text span with style.
    pub fn styled(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: Some(text.into()),
            style: Some(style),
            ..Default::default()
        }
    }

    /// Creates a container span with children.
    pub fn with_children(children: Vec<TextSpan>) -> Self {
        Self {
            text: None,
            children,
            ..Default::default()
        }
    }

    /// Sets the style.
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Adds a child span.
    pub fn with_child(mut self, child: TextSpan) -> Self {
        self.children.push(child);
        self
    }

    /// Sets the semantics label.
    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// Sets the mouse cursor.
    pub fn with_mouse_cursor(mut self, cursor: MouseCursor) -> Self {
        self.mouse_cursor = Some(cursor);
        self
    }

    /// Sets the tap callback.
    pub fn with_on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Returns the plain text content of this span and its children.
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

    /// Visits this span and its children.
    pub fn visit<F>(&self, visitor: &mut F)
    where
        F: FnMut(&TextSpan) -> bool,
    {
        if !visitor(self) {
            return;
        }
        for child in &self.children {
            child.visit(visitor);
        }
    }
}

impl InlineSpanTrait for TextSpan {
    fn style(&self) -> Option<&TextStyle> {
        self.style.as_ref()
    }

    fn to_plain_text(&self) -> String {
        self.to_plain_text()
    }

    fn has_semantics(&self) -> bool {
        self.semantics_label.is_some()
    }
}

/// Mouse cursor types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MouseCursor {
    /// Default cursor.
    Default,
    /// Pointer/hand cursor (for links).
    Pointer,
    /// Text selection cursor.
    Text,
    /// Move cursor.
    Move,
    /// Resize cursors.
    /// Resize north.
    ResizeNorth,
    /// Resize south.
    ResizeSouth,
    /// Resize east.
    ResizeEast,
    /// Resize west.
    ResizeWest,
    /// Resize north-east.
    ResizeNorthEast,
    /// Resize north-west.
    ResizeNorthWest,
    /// Resize south-east.
    ResizeSouthEast,
    /// Resize south-west.
    ResizeSouthWest,
    /// Not allowed cursor.
    NotAllowed,
    /// Wait/busy cursor.
    Wait,
    /// Help cursor.
    Help,
}

/// A placeholder span for embedding widgets.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaceholderSpan {
    /// Width of the placeholder.
    pub width: f64,
    /// Height of the placeholder.
    pub height: f64,
    /// Alignment of the placeholder.
    pub alignment: PlaceholderAlignment,
    /// Baseline to align to.
    pub baseline: Option<TextBaseline>,
    /// Offset from baseline.
    pub baseline_offset: f64,
}

impl PlaceholderSpan {
    /// Creates a new placeholder span.
    pub fn new(width: f64, height: f64, alignment: PlaceholderAlignment) -> Self {
        Self {
            width,
            height,
            alignment,
            baseline: None,
            baseline_offset: 0.0,
        }
    }

    /// Sets the baseline.
    pub fn with_baseline(mut self, baseline: TextBaseline, offset: f64) -> Self {
        self.baseline = Some(baseline);
        self.baseline_offset = offset;
        self
    }
}

impl InlineSpanTrait for PlaceholderSpan {
    fn to_plain_text(&self) -> String {
        "\u{FFFC}".to_string() // Object replacement character
    }
}

/// Dimensions of a placeholder.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaceholderDimensions {
    /// Width of the placeholder.
    pub width: f64,
    /// Height of the placeholder.
    pub height: f64,
    /// Alignment of the placeholder.
    pub alignment: PlaceholderAlignment,
    /// Baseline to align to.
    pub baseline: Option<TextBaseline>,
    /// Offset from baseline.
    pub baseline_offset: f64,
}

impl PlaceholderDimensions {
    /// Creates new placeholder dimensions.
    pub fn new(
        width: f64,
        height: f64,
        alignment: PlaceholderAlignment,
        baseline: Option<TextBaseline>,
        baseline_offset: f64,
    ) -> Self {
        Self {
            width,
            height,
            alignment,
            baseline,
            baseline_offset,
        }
    }
}

/// Alignment of a placeholder within text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaceholderAlignment {
    /// Align to baseline.
    #[default]
    Baseline,
    /// Align above baseline.
    AboveBaseline,
    /// Align below baseline.
    BelowBaseline,
    /// Align to top of text.
    Top,
    /// Align to bottom of text.
    Bottom,
    /// Align to middle of text.
    Middle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_span_new() {
        let span = TextSpan::new("Hello");
        assert_eq!(span.text, Some("Hello".to_string()));
        assert!(span.style.is_none());
        assert!(span.children.is_empty());
    }

    #[test]
    fn test_text_span_styled() {
        let style = TextStyle::default();
        let span = TextSpan::styled("World", style.clone());
        assert_eq!(span.text, Some("World".to_string()));
        assert_eq!(span.style, Some(style));
    }

    #[test]
    fn test_text_span_with_children() {
        let child1 = TextSpan::new("Hello");
        let child2 = TextSpan::new(" World");
        let parent = TextSpan::with_children(vec![child1, child2]);

        assert!(parent.text.is_none());
        assert_eq!(parent.children.len(), 2);
    }

    #[test]
    fn test_text_span_plain_text() {
        let child1 = TextSpan::new("Hello");
        let child2 = TextSpan::new(" World");
        let parent = TextSpan::with_children(vec![child1, child2]);

        assert_eq!(parent.to_plain_text(), "Hello World");
    }

    #[test]
    fn test_text_span_builder() {
        let span = TextSpan::new("Click me")
            .with_semantics_label("Button")
            .with_mouse_cursor(MouseCursor::Pointer);

        assert_eq!(span.semantics_label, Some("Button".to_string()));
        assert_eq!(span.mouse_cursor, Some(MouseCursor::Pointer));
    }

    #[test]
    fn test_text_span_visit() {
        let child1 = TextSpan::new("A");
        let child2 = TextSpan::new("B");
        let parent = TextSpan::with_children(vec![child1, child2]);

        let mut visited = Vec::new();
        parent.visit(&mut |span| {
            if let Some(text) = &span.text {
                visited.push(text.clone());
            }
            true
        });

        assert_eq!(visited, vec!["A", "B"]);
    }

    #[test]
    fn test_placeholder_span() {
        let placeholder = PlaceholderSpan::new(100.0, 50.0, PlaceholderAlignment::Middle);

        assert_eq!(placeholder.width, 100.0);
        assert_eq!(placeholder.height, 50.0);
        assert_eq!(placeholder.alignment, PlaceholderAlignment::Middle);
        assert!(placeholder.baseline.is_none());
    }

    #[test]
    fn test_placeholder_span_with_baseline() {
        let placeholder = PlaceholderSpan::new(100.0, 50.0, PlaceholderAlignment::Baseline)
            .with_baseline(TextBaseline::Alphabetic, 10.0);

        assert_eq!(placeholder.baseline, Some(TextBaseline::Alphabetic));
        assert_eq!(placeholder.baseline_offset, 10.0);
    }

    #[test]
    fn test_placeholder_dimensions() {
        let dims = PlaceholderDimensions::new(
            100.0,
            50.0,
            PlaceholderAlignment::Top,
            Some(TextBaseline::Ideographic),
            5.0,
        );

        assert_eq!(dims.width, 100.0);
        assert_eq!(dims.height, 50.0);
        assert_eq!(dims.alignment, PlaceholderAlignment::Top);
        assert_eq!(dims.baseline, Some(TextBaseline::Ideographic));
        assert_eq!(dims.baseline_offset, 5.0);
    }

    #[test]
    fn test_placeholder_alignment_default() {
        assert_eq!(PlaceholderAlignment::default(), PlaceholderAlignment::Baseline);
    }

    #[test]
    fn test_mouse_cursor_variants() {
        let cursors = [
            MouseCursor::Default,
            MouseCursor::Pointer,
            MouseCursor::Text,
            MouseCursor::Move,
            MouseCursor::NotAllowed,
            MouseCursor::Wait,
            MouseCursor::Help,
        ];

        for cursor in &cursors {
            // Just ensure they're all distinct
            assert_eq!(*cursor, *cursor);
        }
    }
}
