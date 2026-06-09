//! Text span types for rich text.

use std::sync::Arc;

use super::{TextBaseline, TextStyle};

/// Trait for inline spans that can be embedded in text.
///
/// Implementors include `TextSpan` (styled text) and `PlaceholderSpan`
/// (inline objects like images).
pub trait InlineSpanTrait: std::fmt::Debug {
    /// Returns the style for this span, if any.
    #[inline]
    fn style(&self) -> Option<&TextStyle> {
        None
    }

    /// Visits this span and its children.
    ///
    /// The visitor returns false to stop traversal.
    #[inline]
    fn visit(&self, visitor: &mut dyn FnMut(&dyn InlineSpanTrait) -> bool)
    where
        Self: Sized,
    {
        visitor(self);
    }

    /// Returns the text content of this span, if any.
    #[inline]
    fn to_plain_text(&self) -> String {
        String::new()
    }

    /// Returns true if this span contains semantic labels.
    #[inline]
    fn has_semantics(&self) -> bool {
        false
    }
}

/// Type-erased inline span wrapper.
///
/// Allows storing different types of inline spans (text, placeholders)
/// in a uniform container.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InlineSpan {
    /// A run of styled text (with its own child spans).
    ///
    /// `Arc`-wrapped so `InlineSpan::clone` is an O(1) refcount bump rather
    /// than an O(N) deep copy of the span tree — display lists clone
    /// `DrawCommand`s freely (compositing, `with_opacity`/transform ops), and
    /// rich text can carry many child spans. The `Arc` indirection also keeps
    /// the enum small (no `large_enum_variant`).
    Text(Arc<TextSpan>),
    /// An inline placeholder reserving space for an embedded box.
    Placeholder(PlaceholderSpan),
}

impl InlineSpan {
    /// Creates an inline span from any type convertible into one — i.e.
    /// [`TextSpan`] or [`PlaceholderSpan`], the closed set of span kinds.
    #[must_use]
    #[inline]
    pub fn new(span: impl Into<InlineSpan>) -> Self {
        span.into()
    }

    /// Returns this span as its [`InlineSpanTrait`] object for shared behavior.
    #[must_use]
    #[inline]
    pub fn as_trait(&self) -> &(dyn InlineSpanTrait + Send + Sync) {
        match self {
            Self::Text(span) => span.as_ref(),
            Self::Placeholder(span) => span,
        }
    }

    /// Returns the style for this span, if any.
    #[must_use]
    #[inline]
    pub fn style(&self) -> Option<&TextStyle> {
        self.as_trait().style()
    }

    /// Returns the plain text content.
    #[must_use]
    #[inline]
    pub fn to_plain_text(&self) -> String {
        self.as_trait().to_plain_text()
    }

    /// Returns true if this span has semantic labels.
    #[must_use]
    #[inline]
    pub fn has_semantics(&self) -> bool {
        self.as_trait().has_semantics()
    }
}

impl From<TextSpan> for InlineSpan {
    #[inline]
    fn from(span: TextSpan) -> Self {
        Self::Text(Arc::new(span))
    }
}

impl From<PlaceholderSpan> for InlineSpan {
    #[inline]
    fn from(span: PlaceholderSpan) -> Self {
        Self::Placeholder(span)
    }
}

/// A span of styled text with optional children and interactivity.
///
/// Represents a portion of text with associated styling, child spans,
/// and optional semantic labels and event handlers for accessibility
/// and user interaction.
#[derive(Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Tap callback handler.
    ///
    /// Skipped by serde: a live `Arc<dyn Fn>` callback cannot be serialized,
    /// and a deserialized span legitimately carries no handler (it defaults to
    /// `None`). This is the universal "callbacks don't survive serialization"
    /// rule, not a loss of styling/text content.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub on_tap: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for TextSpan {
    #[inline]
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
    #[inline]
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
    /// Creates a new text span with the given text.
    #[must_use]
    #[inline]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// Creates a new text span with text and style.
    #[must_use]
    #[inline]
    pub fn styled(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: Some(text.into()),
            style: Some(style),
            ..Default::default()
        }
    }

    /// Creates a span with only children (no direct text).
    #[must_use]
    #[inline]
    pub fn with_children(children: Vec<TextSpan>) -> Self {
        Self {
            text: None,
            children,
            ..Default::default()
        }
    }

    /// Adds a style to this span.
    #[must_use]
    #[inline]
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Adds a child span.
    #[must_use]
    #[inline]
    pub fn with_child(mut self, child: TextSpan) -> Self {
        self.children.push(child);
        self
    }

    /// Adds a semantic label for accessibility.
    #[must_use]
    #[inline]
    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// Sets the mouse cursor for this span.
    #[must_use]
    #[inline]
    pub fn with_mouse_cursor(mut self, cursor: MouseCursor) -> Self {
        self.mouse_cursor = Some(cursor);
        self
    }

    /// Adds a tap callback handler.
    #[must_use]
    #[inline]
    pub fn with_on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Returns the plain text content of this span and all children.
    #[must_use]
    #[inline]
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
    ///
    /// The visitor returns false to stop traversal.
    #[inline]
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

    /// Returns the number of direct children.
    #[must_use]
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns true if this span has no children.
    #[must_use]
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns the total number of spans (this span + all descendants).
    #[must_use]
    #[inline]
    pub fn total_span_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(TextSpan::total_span_count)
            .sum::<usize>()
    }

    /// Returns the total text length including children.
    #[must_use]
    #[inline]
    pub fn text_length(&self) -> usize {
        self.to_plain_text().len()
    }

    /// Returns true if this span has a tap handler.
    #[must_use]
    #[inline]
    pub fn is_interactive(&self) -> bool {
        self.on_tap.is_some()
    }

    /// Returns true if this span has semantic labels.
    #[must_use]
    #[inline]
    pub fn has_semantics(&self) -> bool {
        self.semantics_label.is_some()
    }

    /// Returns the text content, if any.
    #[must_use]
    #[inline]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Returns the style, if any.
    #[must_use]
    #[inline]
    pub fn style(&self) -> Option<&TextStyle> {
        self.style.as_ref()
    }
}

impl InlineSpanTrait for TextSpan {
    #[inline]
    fn style(&self) -> Option<&TextStyle> {
        self.style.as_ref()
    }

    #[inline]
    fn to_plain_text(&self) -> String {
        self.to_plain_text()
    }

    #[inline]
    fn has_semantics(&self) -> bool {
        self.semantics_label.is_some()
    }
}

/// Mouse cursor types for interactive text.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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

/// Placeholder for inline objects (images, widgets, etc.) in text.
///
/// Reserves space in a text layout for non-text content with specified
/// dimensions and alignment.
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
    #[must_use]
    #[inline]
    pub fn new(width: f64, height: f64, alignment: PlaceholderAlignment) -> Self {
        Self {
            width,
            height,
            alignment,
            baseline: None,
            baseline_offset: 0.0,
        }
    }

    /// Sets the baseline alignment.
    #[must_use]
    #[inline]
    pub fn with_baseline(mut self, baseline: TextBaseline, offset: f64) -> Self {
        self.baseline = Some(baseline);
        self.baseline_offset = offset;
        self
    }

    /// Returns the width.
    #[must_use]
    #[inline]
    pub const fn width(&self) -> f64 {
        self.width
    }

    /// Returns the height.
    #[must_use]
    #[inline]
    pub const fn height(&self) -> f64 {
        self.height
    }

    /// Returns the alignment.
    #[must_use]
    #[inline]
    pub const fn alignment(&self) -> PlaceholderAlignment {
        self.alignment
    }

    /// Returns the area (width * height).
    #[must_use]
    #[inline]
    pub const fn area(&self) -> f64 {
        self.width * self.height
    }

    /// Returns the aspect ratio (width / height).
    #[must_use]
    #[inline]
    pub fn aspect_ratio(&self) -> f64 {
        if self.height == 0.0 {
            f64::INFINITY
        } else {
            self.width / self.height
        }
    }
}

impl InlineSpanTrait for PlaceholderSpan {
    #[inline]
    fn to_plain_text(&self) -> String {
        "\u{FFFC}".to_string() // Object replacement character
    }
}

/// Computed dimensions for a placeholder in laid-out text.
///
/// Similar to `PlaceholderSpan`, but used to represent the actual
/// computed dimensions after text layout.
#[derive(Debug, Clone, PartialEq)]
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
    #[must_use]
    #[inline]
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

    /// Returns the width.
    #[must_use]
    #[inline]
    pub const fn width(&self) -> f64 {
        self.width
    }

    /// Returns the height.
    #[must_use]
    #[inline]
    pub const fn height(&self) -> f64 {
        self.height
    }

    /// Returns the alignment.
    #[must_use]
    #[inline]
    pub const fn alignment(&self) -> PlaceholderAlignment {
        self.alignment
    }

    /// Returns the area (width * height).
    #[must_use]
    #[inline]
    pub const fn area(&self) -> f64 {
        self.width * self.height
    }

    /// Returns the aspect ratio (width / height).
    #[must_use]
    #[inline]
    pub fn aspect_ratio(&self) -> f64 {
        if self.height == 0.0 {
            f64::INFINITY
        } else {
            self.width / self.height
        }
    }
}

/// Vertical alignment for placeholders in text.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
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
mod tests {}
