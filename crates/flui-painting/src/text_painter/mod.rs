//! Text measurement and painting.
//!
//! Provides [`TextPainter`], which measures and paints styled text.
//! Rust equivalent of Flutter's `TextPainter` class.
//!
//! # Concern split (Mythos chain U7)
//!
//! The 990-LOC `text_painter.rs` god module was split into a
//! `text_painter/` directory. Four files:
//!
//! - This module (`mod.rs`) -- `TextPainter` struct + `TextLayoutCache` +
//!   `LayoutMetrics` + `Default` + builder API + getters + setters +
//!   `mark_needs_layout` / `did_layout` lifecycle.
//! - [`baseline`] -- `TextBaseline` enum.
//! - [`measure`]  -- `layout` + `compute_layout_metrics` +
//!   `compute_paint_offset` + `size`/`width`/`height` +
//!   `compute_distance_to_actual_baseline` + `did_exceed_max_lines`.
//! - [`paint`]    -- `paint` + cursor methods (`get_offset_for_caret`,
//!   `get_position_for_offset`, `get_line_metrics`,
//!   `get_boxes_for_selection`, `get_word_boundary`).

use flui_types::{
    geometry::{Offset, Pixels, Size},
    typography::{
        InlineSpan, PlaceholderDimensions, StrutStyle, TextAlign, TextDirection,
        TextHeightBehavior, TextWidthBasis,
    },
};

use crate::text_layout::TextLayout;

pub mod baseline;
pub mod measure;
pub mod paint;

pub use baseline::TextBaseline;

/// Default font size when none is specified.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// A painter that lays out and paints text.
///
/// This is the primary interface for measuring and rendering text in
/// FLUI. Wraps a styled text span and provides layout and painting
/// capabilities.
///
/// # Lifecycle
///
/// 1. Create with [`TextPainter::new`] or builder methods.
/// 2. Set text and configuration.
/// 3. Call [`layout`](TextPainter::layout) to compute metrics.
/// 4. Read metrics like [`width`](TextPainter::width),
///    [`height`](TextPainter::height).
/// 5. Call [`paint`](TextPainter::paint) to render.
///
/// # Thread Safety
///
/// `TextPainter` is `Send` but not `Sync` due to mutable layout
/// state.
#[derive(Debug)]
pub struct TextPainter {
    /// The styled text to paint.
    pub(super) text: Option<InlineSpan>,

    /// How text should be aligned horizontally.
    pub(super) text_align: TextAlign,

    /// The default text direction.
    pub(super) text_direction: Option<TextDirection>,

    /// Text scaling factor for accessibility.
    pub(super) text_scale_factor: f32,

    /// Maximum number of lines before truncation.
    pub(super) max_lines: Option<u32>,

    /// Ellipsis string for overflow.
    pub(super) ellipsis: Option<String>,

    /// Strut style for consistent line height.
    pub(super) strut_style: Option<StrutStyle>,

    /// How to measure text width.
    pub(super) text_width_basis: TextWidthBasis,

    /// Text height behavior.
    pub(super) text_height_behavior: Option<TextHeightBehavior>,

    /// Placeholder dimensions for inline widgets.
    pub(super) placeholder_dimensions: Vec<PlaceholderDimensions>,

    /// Cached layout result.
    pub(super) layout_cache: Option<TextLayoutCache>,
}

/// Cached layout information.
#[derive(Debug)]
pub(super) struct TextLayoutCache {
    /// The width constraint used for layout.
    pub(super) min_width: f32,
    /// The max width constraint used for layout.
    pub(super) max_width: f32,
    /// Computed size after layout.
    pub(super) size: Size<Pixels>,
    /// Distance to alphabetic baseline.
    pub(super) alphabetic_baseline: f32,
    /// Distance to ideographic baseline.
    pub(super) ideographic_baseline: f32,
    /// Whether layout did overflow.
    pub(super) did_exceed_max_lines: bool,
    /// Computed paint offset based on alignment.
    pub(super) paint_offset: Offset<Pixels>,
    /// The underlying text layout for cursor/hit testing.
    pub(super) layout: TextLayout,
}

/// Intermediate layout metrics returned by `compute_layout_metrics`.
pub(super) struct LayoutMetrics {
    pub(super) size: Size<Pixels>,
    pub(super) alphabetic_baseline: f32,
    pub(super) ideographic_baseline: f32,
    pub(super) did_exceed_max_lines: bool,
    pub(super) paint_offset: Offset<Pixels>,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    /// Creates a new text painter with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: None,
            text_align: TextAlign::Start,
            text_direction: None,
            text_scale_factor: 1.0,
            max_lines: None,
            ellipsis: None,
            strut_style: None,
            text_width_basis: TextWidthBasis::Parent,
            text_height_behavior: None,
            placeholder_dimensions: Vec::new(),
            layout_cache: None,
        }
    }

    // ===== Builder API =====

    /// Creates a text painter with the given text span.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<InlineSpan>) -> Self {
        self.set_text(Some(text.into()));
        self
    }

    /// Sets the text direction.
    #[must_use]
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.set_text_direction(Some(direction));
        self
    }

    /// Sets the text alignment.
    #[must_use]
    pub fn with_text_align(mut self, align: TextAlign) -> Self {
        self.set_text_align(align);
        self
    }

    /// Sets the text scale factor.
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.set_text_scale_factor(factor);
        self
    }

    /// Sets the maximum number of lines.
    #[must_use]
    pub fn with_max_lines(mut self, max_lines: Option<u32>) -> Self {
        self.set_max_lines(max_lines);
        self
    }

    /// Sets the ellipsis string.
    #[must_use]
    pub fn with_ellipsis(mut self, ellipsis: Option<String>) -> Self {
        self.set_ellipsis(ellipsis);
        self
    }

    /// Sets the strut style.
    #[must_use]
    pub fn with_strut_style(mut self, style: Option<StrutStyle>) -> Self {
        self.set_strut_style(style);
        self
    }

    // ===== Getters =====

    /// Returns the styled text to paint.
    #[inline]
    #[must_use]
    pub fn text(&self) -> Option<&InlineSpan> {
        self.text.as_ref()
    }

    /// Returns the text alignment.
    #[inline]
    #[must_use]
    pub fn text_align(&self) -> TextAlign {
        self.text_align
    }

    /// Returns the text direction.
    #[inline]
    #[must_use]
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    /// Returns the text scale factor.
    #[inline]
    #[must_use]
    pub fn text_scale_factor(&self) -> f32 {
        self.text_scale_factor
    }

    /// Returns the maximum number of lines.
    #[inline]
    #[must_use]
    pub fn max_lines(&self) -> Option<u32> {
        self.max_lines
    }

    /// Returns the ellipsis string.
    #[inline]
    #[must_use]
    pub fn ellipsis(&self) -> Option<&str> {
        self.ellipsis.as_deref()
    }

    /// Returns the strut style.
    #[inline]
    #[must_use]
    pub fn strut_style(&self) -> Option<&StrutStyle> {
        self.strut_style.as_ref()
    }

    /// Returns the text width basis.
    #[inline]
    #[must_use]
    pub fn text_width_basis(&self) -> TextWidthBasis {
        self.text_width_basis
    }

    /// Returns the text height behavior.
    #[inline]
    #[must_use]
    pub fn text_height_behavior(&self) -> Option<&TextHeightBehavior> {
        self.text_height_behavior.as_ref()
    }

    // ===== Setters =====

    /// Sets the text to paint.
    ///
    /// After calling this, you must call
    /// [`layout`](crate::TextPainter::layout) before painting.
    pub fn set_text(&mut self, text: Option<InlineSpan>) {
        if self.text != text {
            self.text = text;
            self.mark_needs_layout();
        }
    }

    /// Sets the text alignment.
    pub fn set_text_align(&mut self, align: TextAlign) {
        if self.text_align != align {
            self.text_align = align;
            self.mark_needs_layout();
        }
    }

    /// Sets the text direction.
    pub fn set_text_direction(&mut self, direction: Option<TextDirection>) {
        if self.text_direction != direction {
            self.text_direction = direction;
            self.mark_needs_layout();
        }
    }

    /// Sets the text scale factor.
    pub fn set_text_scale_factor(&mut self, factor: f32) {
        if (self.text_scale_factor - factor).abs() > f32::EPSILON {
            self.text_scale_factor = factor;
            self.mark_needs_layout();
        }
    }

    /// Sets the maximum number of lines.
    pub fn set_max_lines(&mut self, max_lines: Option<u32>) {
        if self.max_lines != max_lines {
            self.max_lines = max_lines;
            self.mark_needs_layout();
        }
    }

    /// Sets the ellipsis string.
    pub fn set_ellipsis(&mut self, ellipsis: Option<String>) {
        if self.ellipsis != ellipsis {
            self.ellipsis = ellipsis;
            self.mark_needs_layout();
        }
    }

    /// Sets the strut style.
    pub fn set_strut_style(&mut self, style: Option<StrutStyle>) {
        if self.strut_style != style {
            self.strut_style = style;
            self.mark_needs_layout();
        }
    }

    /// Sets the text width basis.
    pub fn set_text_width_basis(&mut self, basis: TextWidthBasis) {
        if self.text_width_basis != basis {
            self.text_width_basis = basis;
            self.mark_needs_layout();
        }
    }

    /// Sets the text height behavior.
    pub fn set_text_height_behavior(&mut self, behavior: Option<TextHeightBehavior>) {
        if self.text_height_behavior != behavior {
            self.text_height_behavior = behavior;
            self.mark_needs_layout();
        }
    }

    /// Sets placeholder dimensions for inline widgets.
    pub fn set_placeholder_dimensions(&mut self, dimensions: Vec<PlaceholderDimensions>) {
        if self.placeholder_dimensions != dimensions {
            self.placeholder_dimensions = dimensions;
            self.mark_needs_layout();
        }
    }

    // ===== Lifecycle =====

    /// Invalidates the layout cache.
    pub fn mark_needs_layout(&mut self) {
        self.layout_cache = None;
    }

    /// Returns true if layout has been computed.
    #[inline]
    #[must_use]
    pub fn did_layout(&self) -> bool {
        self.layout_cache.is_some()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::{
        geometry::{Offset, px},
        typography::{TextPosition, TextSpan},
    };

    use super::*;

    #[test]
    fn test_text_painter_new() {
        let painter = TextPainter::new();
        assert!(painter.text().is_none());
        assert_eq!(painter.text_align(), TextAlign::Start);
        assert!(painter.text_direction().is_none());
    }

    #[test]
    fn test_text_painter_builder() {
        let painter = TextPainter::new()
            .with_text(TextSpan::new("Hello"))
            .with_text_direction(TextDirection::Ltr)
            .with_text_align(TextAlign::Center);

        assert!(painter.text().is_some());
        assert_eq!(painter.text_align(), TextAlign::Center);
        assert_eq!(painter.text_direction(), Some(TextDirection::Ltr));
    }

    #[test]
    fn test_text_painter_layout() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello, World!"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        assert!(painter.did_layout());
        assert!(painter.width() > 0.0);
        assert!(painter.height() > 0.0);
    }

    #[test]
    fn test_text_painter_setters_invalidate_layout() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);
        assert!(painter.did_layout());

        painter.set_text_align(TextAlign::Center);
        assert!(!painter.did_layout());
    }

    #[test]
    fn test_text_painter_max_lines() {
        let painter = TextPainter::new().with_max_lines(Some(3));

        assert_eq!(painter.max_lines(), Some(3));
    }

    #[test]
    fn test_text_baseline() {
        assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
    }

    #[test]
    fn test_default_font_size() {
        assert!((DEFAULT_FONT_SIZE - 14.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_get_offset_for_caret() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello, World!"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let start = painter.get_offset_for_caret(TextPosition::upstream(0));
        let mid = painter.get_offset_for_caret(TextPosition::upstream(5));
        let end = painter.get_offset_for_caret(TextPosition::upstream(13));

        assert!(start.dx >= px(0.0));
        assert!(mid.dx > start.dx);
        assert!(end.dx > mid.dx);
    }

    #[test]
    fn test_get_position_for_offset() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let pos = painter.get_position_for_offset(Offset::new(px(0.0), px(5.0)));
        assert_eq!(pos.offset, 0);

        let pos = painter.get_position_for_offset(Offset::new(px(1000.0), px(5.0)));
        assert!(pos.offset <= 5);
    }

    #[test]
    fn test_get_line_metrics() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Line 1\nLine 2"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let metrics = painter.get_line_metrics();
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].line_number, 0);
        assert_eq!(metrics[1].line_number, 1);
    }

    #[test]
    fn test_get_boxes_for_selection() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello, World!"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let boxes = painter.get_boxes_for_selection(1, 5);
        assert!(!boxes.is_empty());

        let first_box = &boxes[0];
        assert!(first_box.rect.width() > px(0.0));
        assert!(first_box.rect.height() > px(0.0));
    }

    #[test]
    fn test_get_word_boundary() {
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new("Hello World"))
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let boundary = painter.get_word_boundary(TextPosition::upstream(2));
        assert!(boundary.start <= 2);
        assert!(boundary.end >= 2);
    }
}
