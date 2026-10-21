//! Text span types for rich text.

use std::sync::Arc;

use cursor_icon::CursorIcon;

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

    /// Returns spoken text, replacing each span's own text with its authored
    /// accessibility label. Child spans retain their own labels and order.
    /// This does not change shaping, selection offsets or painted text.
    #[must_use]
    pub fn to_semantics_text(&self) -> String {
        let Self::Text(root) = self else {
            return self.to_plain_text();
        };
        let mut result = String::new();
        let mut pending = vec![root.as_ref()];
        while let Some(span) = pending.pop() {
            if let Some(text) = span.semantics_label.as_deref().or(span.text.as_deref()) {
                result.push_str(text);
            }
            pending.extend(span.children.iter().rev());
        }
        result
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
    /// Interaction-only fields (semantic labels, cursors, links) are
    /// ignored: they affect neither shaping nor paint.
    #[must_use]
    pub fn layout_affecting_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a.layout_affecting_eq(b),
            // Placeholder geometry IS layout: any change re-shapes.
            (Self::Placeholder(a), Self::Placeholder(b)) => a == b,
            _ => false,
        }
    }

    /// The links of this span tree in document order: each span whose link
    /// covers some text ([`TextSpan::link_covers_text`]), before its
    /// children. A link's position in this list is the identity a paragraph
    /// routes that link's taps, focus and activation by.
    #[must_use]
    pub fn links(&self) -> Vec<&TextLink> {
        fn collect<'a>(span: &'a TextSpan, links: &mut Vec<&'a TextLink>) {
            if span.link_covers_text() {
                links.extend(span.link.as_ref());
            }
            for child in &span.children {
                collect(child, links);
            }
        }
        let mut links = Vec::new();
        if let Self::Text(root) = self {
            collect(root, &mut links);
        }
        links
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

/// Where a link span leads: a URL or a tag the app defines.
///
/// A span carries its link as data. The widget that shows the paragraph
/// owns what activating it does: `flui_widgets::RichText::on_link` receives
/// the link of the span a tap or assistive technology activated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct TextLink(Arc<str>);

impl TextLink {
    /// A link to `target`.
    #[must_use]
    pub fn new(target: impl Into<Arc<str>>) -> Self {
        Self(target.into())
    }

    /// The link's target, as authored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TextLink {
    fn from(target: &str) -> Self {
        Self::new(target)
    }
}

impl From<String> for TextLink {
    fn from(target: String) -> Self {
        Self::new(target)
    }
}

impl AsRef<str> for TextLink {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for TextLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A span of styled text with optional children, a link and a cursor.
///
/// Represents a portion of text with associated styling, child spans,
/// an optional semantic label for accessibility, and the link and mouse
/// cursor its text has. A link and a cursor cover the span's children too,
/// unless a child sets its own.
#[derive(Debug, Default, Clone, PartialEq)]
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
    /// Mouse cursor over this span's text. Unset, a link that something
    /// handles shows [`CursorIcon::Pointer`] and other text defers to what
    /// is under it.
    pub mouse_cursor: Option<CursorIcon>,
    /// Where this span's text links to.
    pub link: Option<TextLink>,
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

    /// Sets the mouse cursor over this span's text.
    #[must_use]
    #[inline]
    pub fn with_mouse_cursor(mut self, cursor: CursorIcon) -> Self {
        self.mouse_cursor = Some(cursor);
        self
    }

    /// Makes this span's text a link to `link`.
    #[must_use]
    #[inline]
    pub fn with_link(mut self, link: impl Into<TextLink>) -> Self {
        self.link = Some(link.into());
        self
    }

    /// Whether this span sets a link that covers some text: its own, or a
    /// descendant's that sets no link in between. A link whose children
    /// all set their own, or carry no text, links nothing a reader could
    /// see, tap or focus.
    #[must_use]
    pub fn link_covers_text(&self) -> bool {
        fn has_unlinked_text(span: &TextSpan) -> bool {
            span.text.as_deref().is_some_and(|text| !text.is_empty())
                || span
                    .children
                    .iter()
                    .any(|child| child.link.is_none() && has_unlinked_text(child))
        }
        self.link.is_some() && has_unlinked_text(self)
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
