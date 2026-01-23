//! Text span types for rich text.

use super::{TextBaseline, TextStyle};
use std::sync::Arc;

/// Trait for inline spans (text or placeholders).
pub trait InlineSpanTrait: std::fmt::Debug {
    /// Returns the style for this span, if any.
    fn style(&self) -> Option<&TextStyle> {
        None
    }

    /// Visits this span and its children.
    fn visit(&self, visitor: &mut dyn FnMut(&dyn InlineSpanTrait) -> bool)
    where
        Self: Sized,
    {
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

#[derive(Debug, Clone)]
pub struct InlineSpan {
    inner: Arc<dyn InlineSpanTrait + Send + Sync>,
}

impl InlineSpan {
    #[must_use]
    pub fn new<T: InlineSpanTrait + Send + Sync + 'static>(span: T) -> Self {
        Self {
            inner: Arc::new(span),
        }
    }

    #[must_use]
    pub fn as_trait(&self) -> &(dyn InlineSpanTrait + Send + Sync) {
        &*self.inner
    }

    #[must_use]
    pub fn style(&self) -> Option<&TextStyle> {
        self.inner.style()
    }

    #[must_use]
    pub fn to_plain_text(&self) -> String {
        self.inner.to_plain_text()
    }

    #[must_use]
    pub fn has_semantics(&self) -> bool {
        self.inner.has_semantics()
    }
}

impl PartialEq for InlineSpan {
    fn eq(&self, other: &Self) -> bool {
        // Compare by pointer equality for Arc (identity comparison)
        // For deep comparison, use to_plain_text() or compare serialized forms
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl From<TextSpan> for InlineSpan {
    fn from(span: TextSpan) -> Self {
        Self::new(span)
    }
}

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
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn styled(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: Some(text.into()),
            style: Some(style),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_children(children: Vec<TextSpan>) -> Self {
        Self {
            text: None,
            children,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    #[must_use]
    pub fn with_child(mut self, child: TextSpan) -> Self {
        self.children.push(child);
        self
    }

    #[must_use]
    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn with_mouse_cursor(mut self, cursor: MouseCursor) -> Self {
        self.mouse_cursor = Some(cursor);
        self
    }

    #[must_use]
    pub fn with_on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    #[must_use]
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

    #[must_use]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    #[must_use]
    pub fn total_span_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(|c| c.total_span_count())
            .sum::<usize>()
    }

    #[must_use]
    pub fn text_length(&self) -> usize {
        self.to_plain_text().len()
    }

    #[must_use]
    pub fn is_interactive(&self) -> bool {
        self.on_tap.is_some()
    }

    #[must_use]
    pub fn has_semantics(&self) -> bool {
        self.semantics_label.is_some()
    }

    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    #[must_use]
    pub fn style(&self) -> Option<&TextStyle> {
        self.style.as_ref()
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
    #[must_use]
    pub fn new(width: f64, height: f64, alignment: PlaceholderAlignment) -> Self {
        Self {
            width,
            height,
            alignment,
            baseline: None,
            baseline_offset: 0.0,
        }
    }

    #[must_use]
    pub fn with_baseline(mut self, baseline: TextBaseline, offset: f64) -> Self {
        self.baseline = Some(baseline);
        self.baseline_offset = offset;
        self
    }

    #[must_use]
    pub const fn width(&self) -> f64 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> f64 {
        self.height
    }

    #[must_use]
    pub const fn alignment(&self) -> PlaceholderAlignment {
        self.alignment
    }

    #[must_use]
    pub const fn area(&self) -> f64 {
        self.width * self.height
    }

    #[must_use]
    pub fn aspect_ratio(&self) -> f64 {
        if self.height == 0.0 {
            f64::INFINITY
        } else {
            self.width / self.height
        }
    }
}

impl InlineSpanTrait for PlaceholderSpan {
    fn to_plain_text(&self) -> String {
        "\u{FFFC}".to_string() // Object replacement character
    }
}

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
    #[must_use]
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

    #[must_use]
    pub const fn width(&self) -> f64 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> f64 {
        self.height
    }

    #[must_use]
    pub const fn alignment(&self) -> PlaceholderAlignment {
        self.alignment
    }

    #[must_use]
    pub const fn area(&self) -> f64 {
        self.width * self.height
    }

    #[must_use]
    pub fn aspect_ratio(&self) -> f64 {
        if self.height == 0.0 {
            f64::INFINITY
        } else {
            self.width / self.height
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlaceholderAlignment {
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

}
