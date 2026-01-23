//! Text measurement and painting.
//!
//! This module provides [`TextPainter`], which measures and paints styled text.
//! It's the Rust equivalent of Flutter's `TextPainter` class.
//!
//! # Architecture
//!
//! `TextPainter` operates at the abstraction layer:
//! - It stores text configuration (spans, styles, constraints)
//! - It computes layout metrics (width, height, baselines)
//! - It records text drawing commands to [`Canvas`]
//!
//! The actual GPU text rendering is handled by `flui_engine` using glyphon/cosmic-text.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_painting::{TextPainter, Canvas};
//! use flui_types::typography::{TextSpan, TextStyle, TextAlign, TextDirection};
//!
//! // Create a text painter
//! let mut painter = TextPainter::new()
//!     .with_text(TextSpan::new("Hello, World!"))
//!     .with_text_direction(TextDirection::Ltr);
//!
//! // Layout with constraints
//! painter.layout(0.0, 200.0);
//!
//! // Get metrics
//! println!("Size: {:?}", painter.size());
//!
//! // Paint to canvas
//! let mut canvas = Canvas::new();
//! painter.paint(&mut canvas, Offset::ZERO);
//! ```

use flui_types::geometry::{Offset, Pixels, Size};
use flui_types::typography::{
    InlineSpan, LineMetrics, PlaceholderDimensions, StrutStyle, TextAlign, TextBox, TextDirection,
    TextHeightBehavior, TextPosition, TextRange, TextWidthBasis,
};

use crate::text_layout::TextLayout;
use crate::Canvas;

/// Default font size when none is specified.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// A painter that lays out and paints text.
///
/// This is the primary interface for measuring and rendering text in FLUI.
/// It wraps a styled text span and provides layout and painting capabilities.
///
/// # Lifecycle
///
/// 1. Create with [`TextPainter::new`] or builder methods
/// 2. Set text and configuration
/// 3. Call [`layout`](TextPainter::layout) to compute metrics
/// 4. Read metrics like [`width`](TextPainter::width), [`height`](TextPainter::height)
/// 5. Call [`paint`](TextPainter::paint) to render
///
/// # Thread Safety
///
/// `TextPainter` is `Send` but not `Sync` due to mutable layout state.
#[derive(Debug)]
pub struct TextPainter {
    /// The styled text to paint.
    text: Option<InlineSpan>,

    /// How text should be aligned horizontally.
    text_align: TextAlign,

    /// The default text direction.
    text_direction: Option<TextDirection>,

    /// Text scaling factor for accessibility.
    text_scale_factor: f32,

    /// Maximum number of lines before truncation.
    max_lines: Option<u32>,

    /// Ellipsis string for overflow.
    ellipsis: Option<String>,

    /// Strut style for consistent line height.
    strut_style: Option<StrutStyle>,

    /// How to measure text width.
    text_width_basis: TextWidthBasis,

    /// Text height behavior.
    text_height_behavior: Option<TextHeightBehavior>,

    /// Placeholder dimensions for inline widgets.
    placeholder_dimensions: Vec<PlaceholderDimensions>,

    /// Cached layout result.
    layout_cache: Option<TextLayoutCache>,
}

/// Cached layout information.
#[derive(Debug)]
struct TextLayoutCache {
    /// The width constraint used for layout.
    min_width: f32,
    /// The max width constraint used for layout.
    max_width: f32,
    /// Computed size after layout.
    size: Size<Pixels>,
    /// Distance to alphabetic baseline.
    alphabetic_baseline: f32,
    /// Distance to ideographic baseline.
    ideographic_baseline: f32,
    /// Whether layout did overflow.
    did_exceed_max_lines: bool,
    /// Computed paint offset based on alignment.
    paint_offset: Offset<Pixels>,
    /// The underlying text layout for cursor/hit testing.
    layout: TextLayout,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    /// Creates a new text painter with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let painter = TextPainter::new();
    /// ```
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
    /// After calling this, you must call [`layout`](Self::layout) before painting.
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

    // ===== Layout =====

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

    /// Computes the text layout within the given width constraints.
    ///
    /// # Arguments
    ///
    /// * `min_width` - Minimum width for the text box.
    /// * `max_width` - Maximum width before wrapping.
    ///
    /// # Panics
    ///
    /// Panics if `text` or `text_direction` is not set.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// println!("Width: {}", painter.width());
    /// ```
    pub fn layout(&mut self, min_width: f32, max_width: f32) {
        assert!(
            !max_width.is_nan() && !min_width.is_nan(),
            "Width constraints must not be NaN"
        );

        // Check if we can reuse the cached layout
        if let Some(cache) = &self.layout_cache {
            if (cache.min_width - min_width).abs() < f32::EPSILON
                && (cache.max_width - max_width).abs() < f32::EPSILON
            {
                return;
            }
        }

        let text = self
            .text
            .as_ref()
            .expect("TextPainter.text must be set before layout");
        let _text_direction = self
            .text_direction
            .expect("TextPainter.text_direction must be set before layout");

        // Compute layout metrics using cosmic-text
        let (metrics, layout) = self.compute_layout_metrics(text, min_width, max_width);

        self.layout_cache = Some(TextLayoutCache {
            min_width,
            max_width,
            size: metrics.size,
            alphabetic_baseline: metrics.alphabetic_baseline,
            ideographic_baseline: metrics.ideographic_baseline,
            did_exceed_max_lines: metrics.did_exceed_max_lines,
            paint_offset: metrics.paint_offset,
            layout,
        });
    }

    /// Computes layout metrics for the text using cosmic-text.
    fn compute_layout_metrics(
        &self,
        text: &InlineSpan,
        min_width: f32,
        max_width: f32,
    ) -> (LayoutMetrics, TextLayout) {
        // Get font size from style or use default
        let font_size = text
            .style()
            .and_then(|s| s.font_size.map(|f| f as f32))
            .unwrap_or(DEFAULT_FONT_SIZE);

        let scaled_font_size = font_size * self.text_scale_factor;
        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);

        // Use cosmic-text for measurement
        let max_width_opt = if max_width.is_finite() {
            Some(max_width)
        } else {
            None
        };

        // Create the full TextLayout for cursor support
        let plain_text = text.to_plain_text();
        let layout = TextLayout::new(
            &plain_text,
            text.style(),
            scaled_font_size,
            max_width_opt,
            None,
            direction,
        );

        let layout_result = layout.metrics();

        // Check max lines constraint
        let line_count = layout_result.line_count as u32;
        let did_exceed_max_lines = self.max_lines.map_or(false, |max| line_count > max);

        // Apply min_width constraint
        let width = layout_result.width.max(min_width);

        // Approximate ideographic baseline
        let ideographic_baseline = layout_result.alphabetic_baseline * 1.125;

        // Compute paint offset based on alignment
        let paint_offset = self.compute_paint_offset(width, max_width);

        let metrics = LayoutMetrics {
            size: Size::new(width, layout_result.height),
            alphabetic_baseline: layout_result.alphabetic_baseline,
            ideographic_baseline,
            did_exceed_max_lines,
            paint_offset,
        };

        (metrics, layout)
    }

    /// Computes the paint offset based on text alignment.
    fn compute_paint_offset(&self, content_width: f32, max_width: f32) -> Offset<Pixels> {
        if !max_width.is_finite() {
            return Offset::ZERO;
        }

        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);
        let extra_space = max_width - content_width;

        let dx = match self.text_align {
            TextAlign::Left => 0.0,
            TextAlign::Right => extra_space,
            TextAlign::Center => extra_space / 2.0,
            TextAlign::Justify => 0.0,
            TextAlign::Start => match direction {
                TextDirection::Ltr => 0.0,
                TextDirection::Rtl => extra_space,
            },
            TextAlign::End => match direction {
                TextDirection::Ltr => extra_space,
                TextDirection::Rtl => 0.0,
            },
        };

        Offset::new(dx, 0.0)
    }

    // ===== Metrics =====

    /// Returns the computed size after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    pub fn size(&self) -> Size<Pixels> {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing size")
            .size
    }

    /// Returns the computed width after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.size().width
    }

    /// Returns the computed height after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.size().height
    }

    /// Returns the distance from the top to the alphabetic baseline.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    pub fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> f32 {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before accessing baseline");

        match baseline {
            TextBaseline::Alphabetic => cache.alphabetic_baseline,
            TextBaseline::Ideographic => cache.ideographic_baseline,
        }
    }

    /// Returns whether the text exceeded the maximum number of lines.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    pub fn did_exceed_max_lines(&self) -> bool {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing did_exceed_max_lines")
            .did_exceed_max_lines
    }

    // ===== Cursor and Selection =====

    /// Returns the screen offset for a caret at the given text position.
    ///
    /// This is used for drawing the text cursor.
    ///
    /// # Arguments
    ///
    /// * `position` - The text position (offset + affinity).
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// let caret_offset = painter.get_offset_for_caret(TextPosition::upstream(5));
    /// // Draw cursor at caret_offset
    /// ```
    #[must_use]
    pub fn get_offset_for_caret(&mut self, position: TextPosition) -> Offset<Pixels> {
        let cache = self
            .layout_cache
            .as_mut()
            .expect("layout() must be called before get_offset_for_caret()");

        let offset = cache.layout.get_offset_for_caret(position);
        let combined = offset + cache.paint_offset;
        combined.map(Pixels)
    }

    /// Returns the text position for a screen offset.
    ///
    /// This is used for hit testing (e.g., converting mouse clicks to text positions).
    ///
    /// # Arguments
    ///
    /// * `offset` - The screen offset relative to the text painter's origin.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// let position = painter.get_position_for_offset(Offset::new(50.0, 10.0));
    /// // position.offset is the character index
    /// ```
    #[must_use]
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_position_for_offset()");

        // Convert to f32 and adjust for paint offset
        let offset_f32 = offset.map(|p| p.0);
        let adjusted = offset_f32 - cache.paint_offset;
        cache.layout.get_position_for_offset(adjusted)
    }

    /// Returns metrics for each line in the laid out text.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// for line in painter.get_line_metrics() {
    ///     println!("Line {}: height={}", line.line_number, line.height);
    /// }
    /// ```
    #[must_use]
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_line_metrics()");

        cache.layout.get_line_metrics()
    }

    /// Returns bounding boxes for a text selection.
    ///
    /// Used for rendering selection highlights.
    ///
    /// # Arguments
    ///
    /// * `start` - Start offset of selection.
    /// * `end` - End offset of selection (exclusive).
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// let boxes = painter.get_boxes_for_selection(5, 15);
    /// for text_box in boxes {
    ///     // Draw selection highlight at text_box.rect
    /// }
    /// ```
    #[must_use]
    pub fn get_boxes_for_selection(&self, start: usize, end: usize) -> Vec<TextBox> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_boxes_for_selection()");

        let mut boxes = cache.layout.get_boxes_for_range(TextRange::new(start, end));

        // Adjust boxes for paint offset
        for text_box in &mut boxes {
            text_box.rect = text_box.rect.translate_offset(cache.paint_offset);
        }

        boxes
    }

    /// Returns the word boundary at the given text position.
    ///
    /// Used for double-click word selection.
    ///
    /// # Arguments
    ///
    /// * `position` - The text position to find word boundary for.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// let boundary = painter.get_word_boundary(TextPosition::upstream(10));
    /// // Select text from boundary.start to boundary.end
    /// ```
    #[must_use]
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_word_boundary()");

        cache.layout.get_word_boundary(position)
    }

    // ===== Painting =====

    /// Paints the text onto the canvas at the given offset.
    ///
    /// # Arguments
    ///
    /// * `canvas` - The canvas to paint on.
    /// * `offset` - The offset from the canvas origin.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// painter.layout(0.0, 200.0);
    /// painter.paint(&mut canvas, Offset::new(10.0, 10.0));
    /// ```
    pub fn paint(&self, canvas: &mut Canvas, offset: Offset<Pixels>) {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before paint()");

        let text = self
            .text
            .as_ref()
            .expect("TextPainter.text must be set before paint");

        let paint_offset = offset + cache.paint_offset;

        // Record text drawing command
        canvas.draw_text_span(text, paint_offset, self.text_scale_factor as f64);
    }
}

/// Intermediate layout metrics.
struct LayoutMetrics {
    size: Size<Pixels>,
    alphabetic_baseline: f32,
    ideographic_baseline: f32,
    did_exceed_max_lines: bool,
    paint_offset: Offset<Pixels>,
}

/// The baseline to use for aligning text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    /// The alphabetic baseline (bottom of letters like 'x').
    #[default]
    Alphabetic,
    /// The ideographic baseline (bottom of CJK characters).
    Ideographic,
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::typography::TextSpan;

    #[test]
    fn test_text_painter_new() {
        let painter = TextPainter::new();
        assert!(painter.text().is_none());
        assert_eq!(painter.text_align(), TextAlign::Start);
        assert!(painter.text_direction().is_none());
        assert!((painter.text_scale_factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_text_painter_builder() {
        let span = TextSpan::new("Hello");
        let painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr)
            .with_text_align(TextAlign::Center)
            .with_text_scale_factor(1.5);

        assert!(painter.text().is_some());
        assert_eq!(painter.text_direction(), Some(TextDirection::Ltr));
        assert_eq!(painter.text_align(), TextAlign::Center);
        assert!((painter.text_scale_factor() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_text_painter_layout() {
        let span = TextSpan::new("Hello, World!");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        assert!(!painter.did_layout());

        painter.layout(0.0, 200.0);

        assert!(painter.did_layout());
        assert!(painter.width() > 0.0);
        assert!(painter.height() > 0.0);
    }

    #[test]
    fn test_text_painter_setters_invalidate_layout() {
        let span = TextSpan::new("Hello");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);
        assert!(painter.did_layout());

        painter.set_text_align(TextAlign::Right);
        assert!(!painter.did_layout());
    }

    #[test]
    fn test_text_painter_max_lines() {
        let long_text =
            TextSpan::new("This is a very long text that should wrap to multiple lines");
        let mut painter = TextPainter::new()
            .with_text(long_text)
            .with_text_direction(TextDirection::Ltr)
            .with_max_lines(Some(2));

        painter.layout(0.0, 100.0);

        // With a narrow width, the text would wrap, but max_lines limits it
        assert!(painter.did_layout());
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
        let span = TextSpan::new("Hello, World!");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        use flui_types::typography::TextPosition;

        // Caret at start
        let start = painter.get_offset_for_caret(TextPosition::upstream(0));
        assert!(start.dx >= 0.0);

        // Caret in middle
        let mid = painter.get_offset_for_caret(TextPosition::upstream(6));
        assert!(mid.dx > start.dx);

        // Caret at end
        let end = painter.get_offset_for_caret(TextPosition::upstream(13));
        assert!(end.dx >= mid.dx);
    }

    #[test]
    fn test_get_position_for_offset() {
        let span = TextSpan::new("Hello");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        use flui_types::geometry::Offset;

        // Hit test at start
        let pos = painter.get_position_for_offset(Offset::new(0.0, 5.0));
        assert_eq!(pos.offset, 0);

        // Hit test past end
        let pos = painter.get_position_for_offset(Offset::new(1000.0, 5.0));
        assert!(pos.offset <= 5);
    }

    #[test]
    fn test_get_line_metrics() {
        let span = TextSpan::new("Line 1\nLine 2\nLine 3");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        let metrics = painter.get_line_metrics();
        assert_eq!(metrics.len(), 3);

        // Check line numbers are correct
        assert_eq!(metrics[0].line_number, 0);
        assert_eq!(metrics[1].line_number, 1);
        assert_eq!(metrics[2].line_number, 2);
    }

    #[test]
    fn test_get_boxes_for_selection() {
        let span = TextSpan::new("Hello, World!");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        // Select "ello"
        let boxes = painter.get_boxes_for_selection(1, 5);
        assert!(!boxes.is_empty());

        // Box should have positive dimensions
        assert!(boxes[0].rect.width() > 0.0);
        assert!(boxes[0].rect.height() > 0.0);
    }

    #[test]
    fn test_get_word_boundary() {
        let span = TextSpan::new("Hello World");
        let mut painter = TextPainter::new()
            .with_text(span)
            .with_text_direction(TextDirection::Ltr);

        painter.layout(0.0, 200.0);

        use flui_types::typography::TextPosition;

        let boundary = painter.get_word_boundary(TextPosition::upstream(2));
        // Should contain position 2
        assert!(boundary.start <= 2);
        assert!(boundary.end >= 2);
    }
}
