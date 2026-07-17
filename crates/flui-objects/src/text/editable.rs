//! RenderEditable - single-line editable text visual core.
//!
//! This is the render-object half of Flutter's `RenderEditable`: it owns text
//! layout, paints the collapsed caret, participates in hit testing, and reports
//! text metrics. It deliberately does not own keyboard input, focus, IME
//! composition state, or the editing buffer; those stay in
//! `flui-widgets::EditableText` and `TextEditingController` (which does track
//! an IME composing region — see its "IME composition" doc section), matching
//! Flutter's widget/render split.
//!
//! Scope of this first slice:
//! - single-line text layout, caret margin, caret paint;
//! - dry layout, intrinsics, baseline, and hit-test-self;
//! - collapsed caret only.
//!
//! Deferred: selection painting, a hidden caret while composing rendered as
//! a *separate* render-object state (the owning widget instead suppresses
//! this object's ordinary `show_caret` — see `flui_widgets::EditableText`'s
//! doc), scroll offset, multiline viewport behavior, and obscured text.
//!
//! **Composing-region underline** (ADR-0033): [`RenderEditable::composing_range`]
//! paints one thin rect per selection box under the composing text — a
//! declared **1px-at-baseline+1 approximation**, not real font underline
//! metrics (`TextStyle` has no `decoration` field to merge, unlike Flutter's
//! `TextStyle(decoration: TextDecoration.underline)`) — do not call this
//! parity. [`RenderEditable::rect_for_composing_range`] exposes the same
//! geometry as a bounding rect for the IME cursor-area loop (ADR-0032), with
//! Flutter's own caret-rect fallback order (`_updateComposingRectIfNeeded`,
//! `editable_text.dart`, tag `3.44.0`). Single-line only: box-to-byte-range
//! mapping (`get_boxes_for_range`) compares a global byte range against
//! per-run glyph indices, which is only correct while this object is
//! constrained to one line (`max_lines(1)`) — revisit when multiline lands.

use std::ops::Range;

use flui_foundation::Diagnosticable;
use flui_painting::{Invalidation, Paint, TextBaseline as PainterBaseline, TextPainter};
use flui_tree::Leaf;
use flui_types::{
    Color, Offset, Point, Rect, Size,
    geometry::px,
    typography::{InlineSpan, TextAlign, TextDirection, TextPosition},
};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

const DEFAULT_CARET_WIDTH: f32 = 1.0;
const DEFAULT_CARET_HEIGHT: f32 = 18.0;
const CARET_GAP: f32 = 1.0;

/// Thickness of the composing-region underline, in logical pixels.
///
/// Declared divergence (ADR-0033): a flat 1px bar, not a font's actual
/// underline metrics — `TextStyle` has no `decoration` field to source real
/// metrics from.
const COMPOSING_UNDERLINE_THICKNESS: f32 = 1.0;

/// Gap between the alphabetic baseline and the top of the composing-region
/// underline, in logical pixels. Same value as [`CARET_GAP`] by coincidence,
/// not by any shared meaning — kept as a separate constant so the two can
/// diverge later without one silently dragging the other along.
const COMPOSING_UNDERLINE_GAP: f32 = 1.0;

/// Render object that lays out editable text and paints a collapsed caret.
#[derive(Debug)]
pub struct RenderEditable {
    painter: TextPainter,
    plain_text: String,
    caret_byte_offset: usize,
    show_caret: bool,
    caret_width: f32,
    caret_height: f32,
    caret_color: Color,
    force_line: bool,
    caret_offset: Offset,
    /// The in-progress IME composition's byte range into [`Self::plain_text`],
    /// if any — paints an underline (ADR-0033), never a selection highlight.
    /// Always char-boundary-clamped against the current text, mirroring
    /// [`Self::caret_byte_offset`]'s own clamping — see
    /// [`Self::clamp_composing_range`].
    composing_range: Option<Range<usize>>,
}

impl RenderEditable {
    /// Creates editable text laid out in `direction`.
    ///
    /// Defaults follow Flutter's `RenderEditable` constructor where possible:
    /// `force_line = true` and `cursorWidth = 1.0`. The public
    /// `EditableText` widget may choose a different cursor width to preserve
    /// its own widget-level default.
    #[must_use]
    pub fn new(text: impl Into<InlineSpan>, direction: TextDirection) -> Self {
        let text = text.into();
        let plain_text = text.to_plain_text();
        Self {
            painter: TextPainter::new()
                .with_text(text)
                .with_text_direction(direction)
                .with_max_lines(Some(1)),
            plain_text,
            caret_byte_offset: 0,
            show_caret: false,
            caret_width: DEFAULT_CARET_WIDTH,
            caret_height: DEFAULT_CARET_HEIGHT,
            caret_color: Color::BLACK,
            force_line: true,
            caret_offset: Offset::ZERO,
            composing_range: None,
        }
    }

    /// Sets the text alignment (builder form).
    #[must_use]
    pub fn with_text_align(mut self, align: TextAlign) -> Self {
        self.painter.set_text_align(align);
        self
    }

    /// Sets the accessibility text scale factor (builder form).
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.painter.set_text_scale_factor(factor);
        self
    }

    /// Sets the collapsed caret byte offset into the plain text (builder form).
    #[must_use]
    pub fn with_caret_byte_offset(mut self, offset: usize) -> Self {
        self.caret_byte_offset = self.safe_caret_offset(offset);
        self
    }

    /// Sets whether the collapsed caret is painted (builder form).
    #[must_use]
    pub fn with_show_caret(mut self, show: bool) -> Self {
        self.show_caret = show;
        self
    }

    /// Sets the caret width in logical pixels (builder form).
    #[must_use]
    pub fn with_caret_width(mut self, width: f32) -> Self {
        self.caret_width = non_negative_finite(width, DEFAULT_CARET_WIDTH);
        self
    }

    /// Sets the caret height in logical pixels (builder form).
    #[must_use]
    pub fn with_caret_height(mut self, height: f32) -> Self {
        self.caret_height = non_negative_finite(height, DEFAULT_CARET_HEIGHT);
        self
    }

    /// Sets the caret fill color (builder form).
    #[must_use]
    pub fn with_caret_color(mut self, color: Color) -> Self {
        self.caret_color = color;
        self
    }

    /// Sets the composing-region byte range (builder form) — `None` when no
    /// IME composition is active. Clamped to valid UTF-8 boundaries against
    /// the current text, the same way [`Self::with_caret_byte_offset`] clamps
    /// the caret.
    #[must_use]
    pub fn with_composing_range(mut self, range: Option<Range<usize>>) -> Self {
        self.composing_range = range.map(|r| self.clamp_composing_range(r));
        self
    }

    /// Disables `force_line` sizing (builder form).
    ///
    /// With `force_line = true`, finite incoming max width becomes this box's
    /// width, matching Flutter's single-line editable default.
    #[must_use]
    pub fn without_force_line(mut self) -> Self {
        self.force_line = false;
        self
    }

    /// Replaces the text span and returns the invalidation level.
    pub fn set_text(&mut self, text: impl Into<InlineSpan>) -> Invalidation {
        let text = text.into();
        self.plain_text = text.to_plain_text();
        self.caret_byte_offset = self.safe_caret_offset(self.caret_byte_offset);
        // Defense in depth, mirroring the caret re-clamp just above: a
        // composing range that outlived a text replacement degrades to an
        // in-bounds (if wrong) slice instead of a `get_boxes_for_selection`
        // out-of-range read.
        self.composing_range = self
            .composing_range
            .take()
            .map(|range| self.clamp_composing_range(range));
        self.painter.set_text(Some(text))
    }

    /// Replaces the composing-region range and returns the invalidation
    /// level — always paint-only: the composing region changes what gets an
    /// underline, never glyph shaping.
    pub fn set_composing_range(&mut self, range: Option<Range<usize>>) -> Invalidation {
        let clamped = range.map(|r| self.clamp_composing_range(r));
        if clamped == self.composing_range {
            Invalidation::None
        } else {
            self.composing_range = clamped;
            Invalidation::Paint
        }
    }

    /// The composing region's bounding rect in this object's local painted
    /// coordinate space — one rect per selection box, folded via
    /// [`Rect::union`]. `None` whenever there is nothing meaningful to
    /// report: no active composing range, an empty range, no layout yet, or
    /// zero boxes — **never [`Rect::ZERO`]**, which would read to a caller
    /// as "the composing region is at the origin" instead of "there is no
    /// composing region." `flui_widgets::EditableText`'s IME cursor-area
    /// loop (ADR-0032) prefers this over [`Self::caret_local_rect`] and
    /// falls back to it on `None` — Flutter's own
    /// `_updateComposingRectIfNeeded` order.
    #[must_use]
    pub fn rect_for_composing_range(&self) -> Option<Rect> {
        let range = self.composing_range.clone()?;
        if range.is_empty() || !self.painter.has_layout() {
            return None;
        }
        self.painter
            .get_boxes_for_selection(range.start, range.end)
            .into_iter()
            .map(|text_box| text_box.rect)
            .reduce(|acc, rect| acc.union(&rect))
    }

    /// The plain text used for caret byte offsets.
    #[must_use]
    pub fn plain_text(&self) -> &str {
        &self.plain_text
    }

    /// The current caret byte offset, clamped to a valid UTF-8 boundary.
    #[must_use]
    pub fn caret_byte_offset(&self) -> usize {
        self.caret_byte_offset
    }

    /// The committed caret offset from the last layout pass.
    #[must_use]
    pub fn caret_offset(&self) -> Offset {
        self.caret_offset
    }

    /// Whether the collapsed caret is currently painted. Visibility for the
    /// *geometry* is a separate question — see
    /// [`caret_local_rect`](Self::caret_local_rect)'s doc.
    #[must_use]
    pub fn show_caret(&self) -> bool {
        self.show_caret
    }

    /// The collapsed caret's rect in this object's local painted coordinate
    /// space: origin at [`caret_offset`](Self::caret_offset), sized
    /// `caret_width` × `caret_height`.
    ///
    /// **Visibility-independent**: this returns geometry regardless of
    /// `show_caret` — composition is exactly when the platform IME
    /// candidate window should track the caret (see `flui_widgets::
    /// EditableText`'s IME cursor-area tracking loop), which is also when
    /// `show_caret` may be `false` — this object still has no rendering
    /// state of its own for "hidden because composing"; the owning widget
    /// achieves it by passing `show_caret = false` through the ordinary
    /// flag, driven by `TextEditingController::caret_hidden_by_ime`
    /// (ADR-0033). A caller that also wants the *painted* caret must check
    /// `show_caret` itself; `paint` does exactly that around its own use of
    /// this same rect.
    ///
    /// Local painted coordinates: relative to this render object's own
    /// origin, pre-transform. When internal scrolling lands (see the module
    /// doc's "Deferred" list), this accessor stays viewport-relative — it is
    /// not a full-text-content-space caret position.
    #[must_use]
    pub fn caret_local_rect(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(self.caret_offset.dx, self.caret_offset.dy),
            Size::new(px(self.caret_width), px(self.caret_height)),
        )
    }

    /// Read access to the underlying painter for selection geometry work.
    #[must_use]
    pub fn painter(&self) -> &TextPainter {
        &self.painter
    }

    fn caret_margin(&self) -> f32 {
        CARET_GAP + self.caret_width
    }

    fn text_width_constraints(&self, constraints: &BoxConstraints) -> (f32, f32) {
        let available_max_width = (constraints.max_width.get() - self.caret_margin()).max(0.0);
        let available_min_width = if available_max_width.is_finite() {
            constraints.min_width.get().min(available_max_width)
        } else {
            constraints.min_width.get()
        };

        let min_width = if self.force_line && available_max_width.is_finite() {
            available_max_width
        } else {
            available_min_width
        };

        // This first slice is single-line: matching Flutter's non-multiline
        // `_adjustConstraints`, the text itself lays out with unbounded max
        // width and may overflow the box until scrolling lands.
        (min_width, f32::INFINITY)
    }

    fn size_for_text(&self, constraints: &BoxConstraints, text_size: Size) -> Size {
        let natural_width = text_size.width.get() + self.caret_margin();
        let width = if self.force_line && constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            px(natural_width)
        };
        let height = px(text_size.height.get().max(self.caret_height));
        constraints.constrain(Size::new(width, height))
    }

    fn intrinsic_text_width(&self, width: f32) -> f32 {
        if width.is_finite() {
            (width - self.caret_margin()).max(0.0)
        } else {
            width
        }
    }

    fn safe_caret_offset(&self, offset: usize) -> usize {
        if offset >= self.plain_text.len() {
            return self.plain_text.len();
        }
        if self.plain_text.is_char_boundary(offset) {
            return offset;
        }
        self.plain_text
            .char_indices()
            .map(|(idx, _)| idx)
            .chain(std::iter::once(self.plain_text.len()))
            .find(|idx| *idx >= offset)
            .unwrap_or(self.plain_text.len())
    }

    /// Clamp a composing-region range to `plain_text`'s current bounds and
    /// char boundaries, the same way [`Self::safe_caret_offset`] clamps a
    /// single index. A range whose start outlived the text (`start > end`
    /// after clamping) collapses to a zero-width range at the clamped
    /// start — matching an empty composing region rather than reordering
    /// the bounds.
    fn clamp_composing_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.safe_caret_offset(range.start);
        let end = self.safe_caret_offset(range.end);
        if start > end {
            start..start
        } else {
            start..end
        }
    }

    /// The color the composing-region underline must paint — identical to
    /// the color the glyphs themselves resolve to, by construction (ADR-0033's
    /// color-resolution contract): `foreground` takes precedence over
    /// `color`, falling back to black, exactly matching
    /// `flui-engine`'s `render_text_span` resolution for the root span
    /// style. Reads the span this object already owns through `painter()`
    /// rather than requiring a separately plumbed color, since nothing else
    /// resolves it earlier in the pipeline that this object could observe.
    fn resolved_glyph_color(&self) -> Color {
        self.painter
            .text()
            .and_then(InlineSpan::style)
            .and_then(|style| style.foreground.or(style.color))
            .unwrap_or(Color::BLACK)
    }

    /// The underline rect for one composing-region selection box: 1px thick
    /// ([`COMPOSING_UNDERLINE_THICKNESS`]), positioned
    /// [`COMPOSING_UNDERLINE_GAP`] below the alphabetic baseline, clamped to
    /// stay inside `box_rect`'s vertical span. Declared divergence from real
    /// font underline metrics — see the module doc's ADR-0033 note.
    fn underline_rect_for_box(&self, box_rect: Rect) -> Rect {
        let baseline = self
            .compute_distance_to_actual_baseline(TextBaseline::Alphabetic)
            .unwrap_or_else(|| box_rect.height().get());
        let top = box_rect.top().get();
        let max_top = (box_rect.bottom().get() - COMPOSING_UNDERLINE_THICKNESS).max(top);
        let y = (baseline + COMPOSING_UNDERLINE_GAP).clamp(top, max_top);
        Rect::from_ltrb(
            box_rect.left(),
            px(y),
            box_rect.right(),
            px(y + COMPOSING_UNDERLINE_THICKNESS),
        )
    }
}

impl Diagnosticable for RenderEditable {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add("text", self.plain_text.clone());
        properties.add_enum("text_align", self.painter.text_align());
        properties.add(
            "text_direction",
            self.painter
                .text_direction()
                .map_or_else(|| "unset".to_string(), |d| format!("{d:?}")),
        );
        properties.add("caret_byte_offset", self.caret_byte_offset);
        properties.add_flag("show_caret", self.show_caret, "show caret");
        properties.add("caret_width", self.caret_width);
        properties.add("caret_height", self.caret_height);
        properties.add("caret_color", format!("{:?}", self.caret_color));
        properties.add_flag("force_line", self.force_line, "force line");
        properties.add(
            "composing_range",
            self.composing_range
                .as_ref()
                .map_or_else(|| "none".to_string(), |r| format!("{}..{}", r.start, r.end)),
        );
    }
}

impl RenderBox for RenderEditable {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let (min_width, max_width) = self.text_width_constraints(&constraints);
        self.painter.layout(min_width, max_width);
        let size = self.size_for_text(&constraints, self.painter.size());
        let caret_position =
            TextPosition::downstream(self.safe_caret_offset(self.caret_byte_offset));
        self.caret_offset = self.painter.get_offset_for_caret(caret_position);
        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let (min_width, max_width) = self.text_width_constraints(&constraints);
        let text_size = self.painter.dry_size(min_width, max_width);
        self.size_for_text(&constraints, text_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        _ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        let (min_width, max_width) = self.text_width_constraints(&constraints);
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        self.painter
            .dry_baseline(min_width, max_width, painter_baseline)
    }

    fn compute_min_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.min_intrinsic_width() + self.caret_margin()
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.max_intrinsic_width() + self.caret_margin()
    }

    fn compute_min_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter
            .intrinsic_height(self.intrinsic_text_width(width))
            .max(self.caret_height)
    }

    fn compute_max_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter
            .intrinsic_height(self.intrinsic_text_width(width))
            .max(self.caret_height)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        self.painter.has_layout().then(|| {
            self.painter
                .compute_distance_to_actual_baseline(painter_baseline)
        })
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_own_size()
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        if !self.painter.has_layout() {
            return;
        }

        self.painter.paint(ctx.canvas(), Offset::ZERO);

        if let Some(range) = self.composing_range.clone()
            && !range.is_empty()
        {
            let boxes = self.painter.get_boxes_for_selection(range.start, range.end);
            if !boxes.is_empty() {
                let underline_paint = Paint::fill(self.resolved_glyph_color());
                for text_box in &boxes {
                    ctx.canvas()
                        .draw_rect(self.underline_rect_for_box(text_box.rect), &underline_paint);
                }
            }
        }

        if self.show_caret && self.caret_width > 0.0 && self.caret_height > 0.0 {
            ctx.canvas()
                .draw_rect(self.caret_local_rect(), &Paint::fill(self.caret_color));
        }
    }
}

fn non_negative_finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::context::intrinsics_test_support::leaf_dry_layout;
    use flui_types::typography::TextSpan;

    #[test]
    fn dry_layout_force_line_uses_finite_max_width() {
        let editable = RenderEditable::new(TextSpan::new("abc"), TextDirection::Ltr);
        let constraints = BoxConstraints::loose(Size::new(px(120.0), px(80.0)));
        let size = leaf_dry_layout(|ctx| editable.compute_dry_layout(constraints, ctx));

        assert_eq!(size.width, px(120.0));
        assert!(size.height.get() >= DEFAULT_CARET_HEIGHT);
    }

    #[test]
    fn caret_offset_is_clamped_to_utf8_boundary() {
        let editable =
            RenderEditable::new(TextSpan::new("a€b"), TextDirection::Ltr).with_caret_byte_offset(2);

        assert_eq!(editable.caret_byte_offset(), 4);
    }

    #[test]
    fn set_composing_range_dedupes_unchanged_and_invalidates_paint_on_change() {
        let mut editable = RenderEditable::new(TextSpan::new("abcdef"), TextDirection::Ltr);

        assert_eq!(
            editable.set_composing_range(Some(1..3)),
            Invalidation::Paint
        );
        assert_eq!(editable.set_composing_range(Some(1..3)), Invalidation::None);
        assert_eq!(
            editable.set_composing_range(Some(1..4)),
            Invalidation::Paint
        );
        assert_eq!(editable.set_composing_range(None), Invalidation::Paint);
        assert_eq!(editable.set_composing_range(None), Invalidation::None);
    }
}
