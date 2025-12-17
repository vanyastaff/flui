//! RenderParagraph - A render object that displays a paragraph of text.
//!
//! This is the Rust equivalent of Flutter's `RenderParagraph` from
//! `package:flutter/src/rendering/paragraph.dart`.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderParagraph` | `RenderParagraph` |
//! | `text` | `text` (InlineSpan) |
//! | `text_align` | `textAlign` |
//! | `text_direction` | `textDirection` |
//! | `soft_wrap` | `softWrap` |
//! | `overflow` | `overflow` |
//! | `max_lines` | `maxLines` |
//! | `text_scaler` | `textScaler` |
//!
//! # Architecture
//!
//! `RenderParagraph` is a leaf render object (no children in the render tree sense,
//! though it can have inline widget children via `WidgetSpan`). It uses `TextPainter`
//! for painting and direct `measure_text` calls for layout measurement.
//!
//! # Layout Protocol
//!
//! 1. Layout inline children (WidgetSpans) to get placeholder dimensions
//! 2. Measure text directly using `measure_text`
//! 3. Constrain size to box constraints
//! 4. Position inline children based on placeholder boxes
//! 5. Determine overflow handling (clip, ellipsis, fade)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::prelude::*;
//! use flui_painting::TextPainter;
//! use flui_types::typography::{InlineSpan, TextSpan, TextAlign, TextDirection, TextOverflow};
//!
//! let text = InlineSpan::from(TextSpan::new("Hello, World!"));
//! let mut paragraph = RenderParagraph::new(
//!     text,
//!     TextDirection::Ltr,
//! );
//! paragraph.set_text_align(TextAlign::Center);
//! paragraph.set_max_lines(Some(2));
//! paragraph.set_overflow(TextOverflow::Ellipsis);
//! ```


use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_painting::{measure_text, TextPainter};

use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};
use flui_types::geometry::{Offset, Rect, Size};
use flui_types::styling::Color;
use flui_types::typography::{
    InlineSpan, LineMetrics, StrutStyle, TextAlign, TextBox, TextDirection, TextHeightBehavior,
    TextOverflow, TextPosition, TextRange, TextWidthBasis,
};

use crate::constraints::BoxConstraints;
use crate::lifecycle::BaseRenderObject;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{BoxHitTestResult, RenderBox, RenderObject, TextBaseline};

/// The ellipsis character used for text overflow.
const ELLIPSIS: &str = "\u{2026}";

/// A render object that displays a paragraph of text.
///
/// This corresponds to Flutter's `RenderParagraph` class. It renders styled,
/// multi-line text with support for:
///
/// - Text alignment and direction
/// - Soft wrapping
/// - Overflow handling (clip, ellipsis, fade, visible)
/// - Maximum line limits
/// - Text scaling
/// - Strut style for consistent line heights
/// - Inline widget children (WidgetSpan)
///
/// # Trait Hierarchy
///
/// ```text
/// RenderObject → RenderBox
/// ```
///
/// This is a leaf render object - it has no RenderBox children in the traditional
/// sense, though it can contain inline widgets via WidgetSpan.
#[derive(Debug)]
pub struct RenderParagraph {
    /// Base render object for lifecycle management.
    base: BaseRenderObject,

    /// The text painter that handles measurement and painting.
    text_painter: TextPainter,

    /// Whether the text should break at soft line breaks.
    soft_wrap: bool,

    /// How visual overflow should be handled.
    overflow: TextOverflow,

    /// The color to use when painting the selection.
    selection_color: Option<Color>,

    /// Cached size from last layout.
    size: Size,

    /// Whether clipping is needed for overflow.
    needs_clipping: bool,

    /// Overflow shader for fade effect (not fully implemented yet).
    _overflow_shader: Option<OverflowShader>,

    /// Cached constraints from last layout.
    _last_constraints: Option<BoxConstraints>,
}

/// Placeholder for overflow shader (fade effect).
#[derive(Debug, Clone)]
struct OverflowShader {
    // TODO: Implement gradient shader for fade overflow
}

impl RenderParagraph {
    /// Creates a new RenderParagraph with the given text and direction.
    ///
    /// # Arguments
    ///
    /// * `text` - The styled text to display (InlineSpan tree)
    /// * `text_direction` - The text direction (LTR or RTL)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let text = InlineSpan::from(TextSpan::new("Hello"));
    /// let paragraph = RenderParagraph::new(text, TextDirection::Ltr);
    /// ```
    pub fn new(text: InlineSpan, text_direction: TextDirection) -> Self {
        let mut text_painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(text_direction);

        Self {
            base: BaseRenderObject::new(),
            text_painter,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            selection_color: None,
            size: Size::ZERO,
            needs_clipping: false,
            _overflow_shader: None,
            _last_constraints: None,
        }
    }

    /// Creates a new RenderParagraph with full configuration.
    ///
    /// This matches Flutter's RenderParagraph constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn with_config(
        text: InlineSpan,
        text_align: TextAlign,
        text_direction: TextDirection,
        soft_wrap: bool,
        overflow: TextOverflow,
        text_scale_factor: f32,
        max_lines: Option<u32>,
        strut_style: Option<StrutStyle>,
        text_width_basis: TextWidthBasis,
        text_height_behavior: Option<TextHeightBehavior>,
        selection_color: Option<Color>,
    ) -> Self {
        let ellipsis = if overflow == TextOverflow::Ellipsis {
            Some(ELLIPSIS.to_string())
        } else {
            None
        };

        let mut text_painter = TextPainter::new()
            .with_text(text)
            .with_text_direction(text_direction)
            .with_text_align(text_align)
            .with_text_scale_factor(text_scale_factor)
            .with_max_lines(max_lines)
            .with_ellipsis(ellipsis)
            .with_strut_style(strut_style);

        text_painter.set_text_width_basis(text_width_basis);
        text_painter.set_text_height_behavior(text_height_behavior);

        Self {
            base: BaseRenderObject::new(),
            text_painter,
            soft_wrap,
            overflow,
            selection_color,
            size: Size::ZERO,
            needs_clipping: false,
            _overflow_shader: None,
            _last_constraints: None,
        }
    }

    // ========================================================================
    // Text Properties
    // ========================================================================

    /// Returns the styled text being displayed.
    pub fn text(&self) -> Option<&InlineSpan> {
        self.text_painter.text()
    }

    /// Sets the styled text to display.
    ///
    /// Triggers layout if the text changed.
    pub fn set_text(&mut self, text: InlineSpan) {
        // Compare and update
        let needs_update = self.text_painter.text().map(|t| t != &text).unwrap_or(true);

        if needs_update {
            let has_owner = self.base.owner().is_some();
            tracing::debug!(
                "RenderParagraph::set_text updating text (needs_update=true, has_owner={})",
                has_owner
            );
            self.text_painter.set_text(Some(text));
            self._overflow_shader = None;
            self.mark_needs_layout();
        } else {
            tracing::trace!("RenderParagraph::set_text text unchanged");
        }
    }

    /// Returns the text alignment.
    pub fn text_align(&self) -> TextAlign {
        self.text_painter.text_align()
    }

    /// Sets the text alignment.
    ///
    /// Triggers repaint (not relayout) since alignment doesn't affect size.
    pub fn set_text_align(&mut self, value: TextAlign) {
        if self.text_painter.text_align() != value {
            self.text_painter.set_text_align(value);
            self.mark_needs_paint();
        }
    }

    /// Returns the text direction.
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.text_painter.text_direction()
    }

    /// Sets the text direction.
    ///
    /// Triggers layout since direction affects text measurement.
    pub fn set_text_direction(&mut self, value: TextDirection) {
        if self.text_painter.text_direction() != Some(value) {
            self.text_painter.set_text_direction(Some(value));
            self.mark_needs_layout();
        }
    }

    /// Returns whether text should break at soft line breaks.
    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    /// Sets whether text should break at soft line breaks.
    ///
    /// If false, text will be laid out as if there was unlimited horizontal space.
    pub fn set_soft_wrap(&mut self, value: bool) {
        if self.soft_wrap != value {
            self.soft_wrap = value;
            self.mark_needs_layout();
        }
    }

    /// Returns how visual overflow should be handled.
    pub fn overflow(&self) -> TextOverflow {
        self.overflow
    }

    /// Sets how visual overflow should be handled.
    pub fn set_overflow(&mut self, value: TextOverflow) {
        if self.overflow != value {
            self.overflow = value;
            let ellipsis = if value == TextOverflow::Ellipsis {
                Some(ELLIPSIS.to_string())
            } else {
                None
            };
            self.text_painter.set_ellipsis(ellipsis);
            self._overflow_shader = None;
            self.mark_needs_layout();
        }
    }

    /// Returns the text scale factor.
    pub fn text_scale_factor(&self) -> f32 {
        self.text_painter.text_scale_factor()
    }

    /// Sets the text scale factor.
    pub fn set_text_scale_factor(&mut self, value: f32) {
        if (self.text_painter.text_scale_factor() - value).abs() > f32::EPSILON {
            self.text_painter.set_text_scale_factor(value);
            self._overflow_shader = None;
            self.mark_needs_layout();
        }
    }

    /// Returns the maximum number of lines.
    pub fn max_lines(&self) -> Option<u32> {
        self.text_painter.max_lines()
    }

    /// Sets the maximum number of lines.
    ///
    /// If the text exceeds this limit, it will be truncated according to `overflow`.
    pub fn set_max_lines(&mut self, value: Option<u32>) {
        if self.text_painter.max_lines() != value {
            self.text_painter.set_max_lines(value);
            self._overflow_shader = None;
            self.mark_needs_layout();
        }
    }

    /// Returns the strut style.
    pub fn strut_style(&self) -> Option<&StrutStyle> {
        self.text_painter.strut_style()
    }

    /// Sets the strut style for consistent line heights.
    pub fn set_strut_style(&mut self, value: Option<StrutStyle>) {
        self.text_painter.set_strut_style(value);
        self._overflow_shader = None;
        self.mark_needs_layout();
    }

    /// Returns the text width basis.
    pub fn text_width_basis(&self) -> TextWidthBasis {
        self.text_painter.text_width_basis()
    }

    /// Sets the text width basis.
    pub fn set_text_width_basis(&mut self, value: TextWidthBasis) {
        if self.text_painter.text_width_basis() != value {
            self.text_painter.set_text_width_basis(value);
            self._overflow_shader = None;
            self.mark_needs_layout();
        }
    }

    /// Returns the text height behavior.
    pub fn text_height_behavior(&self) -> Option<&TextHeightBehavior> {
        self.text_painter.text_height_behavior()
    }

    /// Sets the text height behavior.
    pub fn set_text_height_behavior(&mut self, value: Option<TextHeightBehavior>) {
        self.text_painter.set_text_height_behavior(value);
        self._overflow_shader = None;
        self.mark_needs_layout();
    }

    /// Returns the selection color.
    pub fn selection_color(&self) -> Option<Color> {
        self.selection_color
    }

    /// Sets the selection color.
    pub fn set_selection_color(&mut self, value: Option<Color>) {
        if self.selection_color != value {
            self.selection_color = value;
            self.mark_needs_paint();
        }
    }

    // ========================================================================
    // Layout Helpers
    // ========================================================================

    /// Adjusts max width based on soft_wrap and overflow settings.
    fn adjust_max_width(&self, max_width: f32) -> f32 {
        if self.soft_wrap || self.overflow == TextOverflow::Ellipsis {
            max_width
        } else {
            f32::INFINITY
        }
    }

    /// Measures text directly using cosmic-text, bypassing TextPainter delegation.
    ///
    /// Returns the computed size of the text.
    fn measure_text_size(&self, max_width: f32) -> Size {
        let Some(text) = self.text() else {
            return Size::ZERO;
        };

        // Get font size from style or use default
        let font_size = text
            .style()
            .and_then(|s| s.font_size.map(|f| f as f32))
            .unwrap_or(14.0);

        let scaled_font_size = font_size * self.text_scale_factor();
        let plain_text = text.to_plain_text();

        // Use max_width constraint if finite
        let max_width_opt = if max_width.is_finite() {
            Some(max_width)
        } else {
            None
        };

        // Direct measurement using cosmic-text
        let result = measure_text(
            &plain_text,
            text.style(),
            scaled_font_size,
            max_width_opt,
            None,
        );

        tracing::debug!(
            text = %plain_text,
            font_size = scaled_font_size,
            max_width = ?max_width_opt,
            result_width = result.width,
            result_height = result.height,
            line_count = result.line_count,
            "RenderParagraph::measure_text_size"
        );

        Size::new(result.width, result.height)
    }

    /// Lays out text with the given constraints.
    fn layout_text_with_constraints(&mut self, constraints: &BoxConstraints) {
        let adjusted_max_width = self.adjust_max_width(constraints.max_width);
        self.text_painter
            .layout(constraints.min_width, adjusted_max_width);
    }

    // ========================================================================
    // Cursor and Hit Testing
    // ========================================================================

    /// Returns the offset at which to paint the caret.
    ///
    /// Valid only after layout.
    pub fn get_offset_for_caret(
        &mut self,
        position: TextPosition,
        _caret_prototype: Rect,
    ) -> Offset {
        self.text_painter.get_offset_for_caret(position)
    }

    /// Returns the text position for a screen offset.
    ///
    /// Valid only after layout.
    pub fn get_position_for_offset(&self, offset: Offset) -> TextPosition {
        self.text_painter.get_position_for_offset(offset)
    }

    /// Returns bounding boxes for a text selection.
    ///
    /// Valid only after layout.
    pub fn get_boxes_for_selection(&self, selection: TextRange) -> Vec<TextBox> {
        self.text_painter
            .get_boxes_for_selection(selection.start, selection.end)
    }

    /// Returns the word boundary at the given position.
    ///
    /// Valid only after layout.
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        self.text_painter.get_word_boundary(position)
    }

    /// Returns metrics for each line in the laid out text.
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        self.text_painter.get_line_metrics()
    }

    /// Returns the preferred line height.
    ///
    /// This does not require layout to be complete.
    pub fn preferred_line_height(&self) -> f32 {
        // Estimate based on font size and scale factor
        let font_size = self
            .text()
            .and_then(|t| t.style())
            .and_then(|s| s.font_size)
            .unwrap_or(14.0) as f32;
        font_size * self.text_scale_factor() * 1.2
    }
}

// ============================================================================
// RenderObject Implementation
// ============================================================================

impl RenderObject for RenderParagraph {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        None // TODO: Implement when pipeline is connected
    }

    fn attach(&mut self, _owner: &PipelineOwner) {
        // No children to attach
    }

    fn detach(&mut self) {
        // No children to detach
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        // RenderParagraph is a leaf - no children
    }

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        // RenderParagraph is a leaf - no children
    }

    fn redepth_child(&mut self, _child: &mut dyn RenderObject) {
        // RenderParagraph is a leaf - no children
    }

    fn mark_parent_needs_layout(&mut self) {
        // TODO: Implement when parent tracking is added
    }

    fn schedule_initial_layout(&mut self) {
        // TODO: Implement when pipeline is connected
    }

    fn schedule_initial_paint(&mut self) {
        // TODO: Implement when pipeline is connected
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Delegate to RenderBox::paint implementation
        RenderBox::paint(self, context, offset);
    }

    fn perform_layout_impl(&mut self) {
        // Get cached constraints from base().cached_constraints()
        let constraints = self
            .base()
            .cached_constraints()
            .expect("perform_layout_impl called without cached constraints");

        // Delegate to RenderBox::perform_layout
        RenderBox::perform_layout(self, constraints);
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // RenderParagraph is a leaf - no children
    }

    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // RenderParagraph is a leaf - no children
    }


}

// ============================================================================
// RenderBox Implementation
// ============================================================================

impl RenderBox for RenderParagraph {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let adjusted_max_width = self.adjust_max_width(constraints.max_width);

        // Measure text directly using cosmic-text (not through TextPainter)
        let text_size = self.measure_text_size(adjusted_max_width);

        // Also layout TextPainter for painting (it needs layout() called before paint())
        self.text_painter
            .layout(constraints.min_width, adjusted_max_width);

        // Constrain to box constraints
        let size = constraints.constrain(text_size);
        self.size = size;

        tracing::debug!(
            text_size = ?text_size,
            constrained_size = ?size,
            constraints = ?constraints,
            "RenderParagraph::perform_layout"
        );

        // Determine if we need clipping
        let did_overflow_height = size.height < text_size.height;
        let did_overflow_width = size.width < text_size.width;
        let has_visual_overflow = did_overflow_width || did_overflow_height;

        self.needs_clipping = has_visual_overflow && self.overflow != TextOverflow::Visible;
        self._overflow_shader = None; // Fade shader not implemented yet

        self._last_constraints = Some(constraints);
        size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let Some(text) = self.text() else {
            return;
        };

        if self.needs_clipping {
            let bounds = Rect::from_ltwh(offset.dx, offset.dy, self.size.width, self.size.height);
            context.canvas().save();
            context.canvas().clip_rect(bounds);
        }

        // Paint text directly using canvas.draw_text()
        let plain_text = text.to_plain_text();
        let style = text.style().cloned().unwrap_or_default();

        // Get color from style or use black
        let color = style.color.unwrap_or(flui_types::styling::Color::BLACK);
        let paint = flui_types::painting::Paint::fill(color);

        context
            .canvas()
            .draw_text(&plain_text, offset, self.size, &style, &paint);

        tracing::debug!(
            text = %plain_text,
            offset = ?offset,
            size = ?self.size,
            "RenderParagraph::paint"
        );

        if self.needs_clipping {
            context.canvas().restore();
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Text always accepts hits for text selection
        true
    }

    fn hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        // TODO: Hit test inline children (WidgetSpans)
        false
    }

    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        // Text can be wrapped to any width
        0.0
    }

    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        // Measure text without width constraint to get natural width
        self.measure_text_size(f32::INFINITY).width
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        let adjusted_width = self.adjust_max_width(width);
        self.measure_text_size(adjusted_width).height
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        if !self.text_painter.did_layout() {
            return None;
        }
        Some(
            self.text_painter
                .compute_distance_to_actual_baseline(to_painting_baseline(baseline)),
        )
    }
}

// ============================================================================
// TextBaseline Conversion
// ============================================================================

/// Converts rendering TextBaseline to painting TextBaseline
fn to_painting_baseline(baseline: TextBaseline) -> flui_painting::TextBaseline {
    match baseline {
        TextBaseline::Alphabetic => flui_painting::TextBaseline::Alphabetic,
        TextBaseline::Ideographic => flui_painting::TextBaseline::Ideographic,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::typography::TextSpan;

    fn create_test_paragraph(text: &str) -> RenderParagraph {
        let span = InlineSpan::from(TextSpan::new(text));
        RenderParagraph::new(span, TextDirection::Ltr)
    }

    #[test]
    fn test_render_paragraph_new() {
        let paragraph = create_test_paragraph("Hello, World!");
        assert!(paragraph.text().is_some());
        assert_eq!(paragraph.text_direction(), Some(TextDirection::Ltr));
        assert!(paragraph.soft_wrap());
        assert_eq!(paragraph.overflow(), TextOverflow::Clip);
    }

    #[test]
    fn test_render_paragraph_set_text_align() {
        let mut paragraph = create_test_paragraph("Test");
        assert_eq!(paragraph.text_align(), TextAlign::Start);

        paragraph.set_text_align(TextAlign::Center);
        assert_eq!(paragraph.text_align(), TextAlign::Center);
    }

    #[test]
    fn test_render_paragraph_set_soft_wrap() {
        let mut paragraph = create_test_paragraph("Test");
        assert!(paragraph.soft_wrap());

        paragraph.set_soft_wrap(false);
        assert!(!paragraph.soft_wrap());
    }

    #[test]
    fn test_render_paragraph_set_overflow() {
        let mut paragraph = create_test_paragraph("Test");
        assert_eq!(paragraph.overflow(), TextOverflow::Clip);

        paragraph.set_overflow(TextOverflow::Ellipsis);
        assert_eq!(paragraph.overflow(), TextOverflow::Ellipsis);
    }

    #[test]
    fn test_render_paragraph_set_max_lines() {
        let mut paragraph = create_test_paragraph("Test");
        assert!(paragraph.max_lines().is_none());

        paragraph.set_max_lines(Some(3));
        assert_eq!(paragraph.max_lines(), Some(3));
    }

    #[test]
    fn test_render_paragraph_layout() {
        let mut paragraph = create_test_paragraph("Hello, World!");
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY);

        let size = paragraph.perform_layout(constraints);

        assert!(size.width > 0.0);
        assert!(size.height > 0.0);
        assert!(size.width <= 200.0);
    }

    #[test]
    fn test_render_paragraph_as_render_box() {
        let paragraph = create_test_paragraph("Test");
        // Should compile - RenderParagraph implements RenderBox
        let _: &dyn RenderBox = &paragraph;
    }

    #[test]
    fn test_render_paragraph_boxed() {
        let paragraph: Box<dyn RenderBox> = Box::new(create_test_paragraph("Test"));
        assert_eq!(paragraph.size(), Size::ZERO);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for RenderParagraph {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("textAlign", format!("{:?}", self.text_align()));
        properties.add("textDirection", format!("{:?}", self.text_direction()));
        properties.add("softWrap", self.soft_wrap);
        properties.add("overflow", format!("{:?}", self.overflow));
        if let Some(max_lines) = self.max_lines() {
            properties.add("maxLines", max_lines);
        }
    }
}

impl HitTestTarget for RenderParagraph {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}
