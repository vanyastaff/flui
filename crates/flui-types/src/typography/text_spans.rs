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

    /// Compares only the parts that affect SHAPING/LAYOUT: text
    /// content, layout-affecting style fields
    /// ([`TextStyle::layout_affecting_eq`]), and placeholder geometry.
    ///
    /// `true` means the two span trees produce identical glyph
    /// geometry — a text engine may keep its shaped layout and only
    /// re-emit paint commands (colors/shadows changed at most).
    /// Interaction-only fields (semantic labels, cursors, tap
    /// handlers) are ignored: they affect neither shaping nor paint.
    #[must_use]
    pub fn layout_affecting_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a.layout_affecting_eq(b),
            // Placeholder geometry IS layout: any change re-shapes.
            (Self::Placeholder(a), Self::Placeholder(b)) => a == b,
            _ => false,
        }
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
    /// Recursive layout-affecting comparison: text content, the
    /// layout-affecting style fields, and the children pairwise.
    ///
    /// Average and worst case O(total spans + total text bytes) — one
    /// walk over both trees, short-circuiting on the first difference.
    /// See [`InlineSpan::layout_affecting_eq`] for the contract.
    #[must_use]
    pub fn layout_affecting_eq(&self, other: &Self) -> bool {
        if self.text != other.text || self.children.len() != other.children.len() {
            return false;
        }
        let style_eq = match (&self.style, &other.style) {
            (None, None) => true,
            (Some(a), Some(b)) => a.layout_affecting_eq(b),
            // A style appearing/disappearing can change font selection
            // via inheritance — conservatively a layout change.
            _ => false,
        };
        style_eq
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(a, b)| a.layout_affecting_eq(b))
    }

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
mod tests {
    use super::*;
    use crate::Color;

    /// `root("a") -> [b -> [c], d]`, texts chosen so pre-order is "abcd".
    fn tree() -> TextSpan {
        TextSpan::new("a")
            .with_child(TextSpan::new("b").with_child(TextSpan::new("c")))
            .with_child(TextSpan::new("d"))
    }

    #[test]
    fn plain_text_counts_and_structure() {
        let t = tree();
        assert_eq!(t.to_plain_text(), "abcd");
        assert_eq!((t.child_count(), t.total_span_count()), (2, 4));
        assert!(!t.is_leaf() && t.children[1].is_leaf());
        assert_eq!(
            TextSpan::with_children(vec![TextSpan::new("x")]).text(),
            None
        );
        // Bytes, not characters.
        assert_eq!(TextSpan::new("é").text_length(), 2);
    }

    /// Pre-order; returning false skips that span's children only.
    #[test]
    fn visit_is_pre_order_and_prunes_per_span() {
        let mut seen = Vec::new();
        tree().visit(&mut |s| {
            seen.push(s.text().unwrap_or("").to_owned());
            true
        });
        assert_eq!(seen, ["a", "b", "c", "d"]);

        let mut pruned = Vec::new();
        tree().visit(&mut |s| {
            pruned.push(s.text().unwrap_or("").to_owned());
            s.text() != Some("b")
        });
        assert_eq!(pruned, ["a", "b", "d"]);
    }

    #[test]
    fn builders_and_queries() {
        let style = TextStyle::new().with_font_size(12.0);
        let span = TextSpan::styled("x", style.clone())
            .with_semantics_label("label")
            .with_mouse_cursor(MouseCursor::Pointer);
        assert_eq!(span.style(), Some(&style));
        assert_eq!(span.semantics_label.as_deref(), Some("label"));
        assert_eq!(span.mouse_cursor, Some(MouseCursor::Pointer));
        assert!(span.has_semantics() && !span.is_interactive());
        assert_eq!(
            TextSpan::new("x").with_style(style.clone()).style,
            Some(style)
        );

        let tappable = TextSpan::new("x").with_on_tap(|| {});
        assert!(tappable.is_interactive());
        assert!(format!("{tappable:?}").contains("<callback>"));
        // Equality ignores callbacks.
        assert_eq!(tappable, TextSpan::new("x"));
    }

    /// Layout equality ignores paint-only style changes but not text,
    /// structure, a style appearing, or a nested layout change.
    #[test]
    fn layout_affecting_eq() {
        let sized = |size| TextStyle::new().with_font_size(size);
        let base =
            TextSpan::styled("a", sized(12.0)).with_child(TextSpan::styled("b", sized(10.0)));
        let recolored = TextSpan::styled("a", sized(12.0).with_color(Color::RED))
            .with_child(TextSpan::styled("b", sized(10.0).with_color(Color::BLUE)));
        assert!(base.layout_affecting_eq(&recolored));
        assert!(TextSpan::new("a").layout_affecting_eq(&TextSpan::new("a")));

        let other_text =
            TextSpan::styled("z", sized(12.0)).with_child(TextSpan::styled("b", sized(10.0)));
        let extra_child = base.clone().with_child(TextSpan::new("c"));
        let resized =
            TextSpan::styled("a", sized(14.0)).with_child(TextSpan::styled("b", sized(10.0)));
        let unstyled = TextSpan::new("a").with_child(TextSpan::styled("b", sized(10.0)));
        let deep =
            TextSpan::styled("a", sized(12.0)).with_child(TextSpan::styled("b", sized(11.0)));
        for changed in [other_text, extra_child, resized, unstyled, deep] {
            assert!(!base.layout_affecting_eq(&changed), "{changed:?}");
            assert!(!changed.layout_affecting_eq(&base), "{changed:?}");
        }
    }

    #[test]
    fn inline_span_dispatch() {
        let style = TextStyle::new().with_font_size(12.0);
        let text = InlineSpan::new(TextSpan::styled("hi", style.clone()).with_semantics_label("l"));
        let placeholder =
            InlineSpan::new(PlaceholderSpan::new(4.0, 2.0, PlaceholderAlignment::Middle));
        assert_eq!(
            (text.style(), text.to_plain_text(), text.has_semantics()),
            (Some(&style), "hi".into(), true)
        );
        assert_eq!(
            (
                placeholder.style(),
                placeholder.to_plain_text(),
                placeholder.has_semantics()
            ),
            (None, "\u{FFFC}".into(), false)
        );

        assert!(text.layout_affecting_eq(&text.clone()));
        assert!(placeholder.layout_affecting_eq(&placeholder.clone()));
        assert!(!text.layout_affecting_eq(&placeholder) && !placeholder.layout_affecting_eq(&text));
        let wider = InlineSpan::new(PlaceholderSpan::new(5.0, 2.0, PlaceholderAlignment::Middle));
        assert!(!placeholder.layout_affecting_eq(&wider));
        let resized = InlineSpan::new(TextSpan::styled("hi", style.with_font_size(20.0)));
        assert!(!text.layout_affecting_eq(&resized));
    }

    #[test]
    fn placeholder_geometry() {
        let p = PlaceholderSpan::new(6.0, 3.0, PlaceholderAlignment::Top)
            .with_baseline(TextBaseline::Ideographic, 1.5);
        assert_eq!(
            (p.width(), p.height(), p.alignment()),
            (6.0, 3.0, PlaceholderAlignment::Top)
        );
        assert_eq!(
            (p.baseline, p.baseline_offset),
            (Some(TextBaseline::Ideographic), 1.5)
        );
        assert_eq!((p.area(), p.aspect_ratio()), (18.0, 2.0));
        assert_eq!(
            PlaceholderSpan::new(6.0, 0.0, PlaceholderAlignment::Top).aspect_ratio(),
            f64::INFINITY
        );

        let d = PlaceholderDimensions::new(6.0, 3.0, PlaceholderAlignment::Bottom, None, 0.5);
        assert_eq!(
            (d.width(), d.height(), d.alignment()),
            (6.0, 3.0, PlaceholderAlignment::Bottom)
        );
        assert_eq!((d.area(), d.aspect_ratio()), (18.0, 2.0));
        let flat = PlaceholderDimensions::new(6.0, 0.0, PlaceholderAlignment::Bottom, None, 0.0);
        assert_eq!(flat.aspect_ratio(), f64::INFINITY);
    }
}
