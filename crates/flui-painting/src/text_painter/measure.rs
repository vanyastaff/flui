//! `TextPainter` layout and measurement: `layout`, the cached metrics it
//! produces, and the size / baseline / overflow queries over them.

use std::sync::Arc;

use crate::typography::{InlineSpan, TextDirection, TextStyle};
use flui_foundation::geometry::Size;

use super::{LayoutMetrics, TextBaseline, TextLayoutCache, TextPainter};
use crate::text_layout::{FontsKey, TextContext, TextLayoutError, TextLayoutResult};

impl TextPainter {
    /// What a cached layout was taken against: `text_cx`'s collection and
    /// its generation, which shaped it.
    pub(super) fn font_key(text_cx: &TextContext) -> FontsKey {
        text_cx.fonts().key()
    }

    /// Computes the text layout within the given width constraints,
    /// measuring through `text_cx`.
    ///
    /// A cached layout is kept while the constraints and `text_cx`'s
    /// collection and generation are unchanged; a context over another
    /// collection, or a face registered since, shapes again.
    ///
    /// A failed layout discards the current cache; it cannot publish geometry
    /// from a previous request as the result of this one.
    pub fn layout(
        &mut self,
        text_cx: &mut TextContext,
        min_width: f64,
        max_width: f64,
    ) -> Result<(), super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed())
            .layout(self, min_width, max_width)
    }

    pub(super) fn commit_layout(
        &mut self,
        text_cx: &mut TextContext,
        resolved: ResolvedText,
        min_width: f64,
        max_width: f64,
        shape_widths: (f32, Option<f32>),
        sizing: super::TextSizing,
    ) -> Result<(), TextLayoutError> {
        let fonts = Self::font_key(text_cx);

        // One layout measures and paints: the paragraph the display list
        // carries is built from the layout the metrics are read from.
        let layout = self.parley_paragraph(
            text_cx,
            &resolved,
            shape_widths.0,
            shape_widths.1,
            LineOverflow::Enforce,
        )?;
        let result = layout.metrics();
        let metrics = Self::metrics_from(&result, min_width);
        let root = self
            .text
            .as_ref()
            .and_then(InlineSpan::style)
            .and_then(crate::text_layout::paint_color);
        let paragraph = Arc::new(layout.to_shaped(root));

        // Precompute intrinsic widths (shape once, query many).
        let (min_intrinsic_width, max_intrinsic_width) =
            self.intrinsic_widths(text_cx, &resolved)?;

        self.layout_cache = Some(TextLayoutCache {
            fonts,
            sizing,
            answers: resolved.answers,
            min_width,
            max_width,
            size: metrics.size,
            alphabetic_baseline: metrics.alphabetic_baseline,
            ideographic_baseline: metrics.ideographic_baseline,
            did_exceed_max_lines: metrics.did_exceed_max_lines,
            paragraph,
            layout,
            min_intrinsic_width,
            max_intrinsic_width,
        });
        Ok(())
    }

    /// The colour each shaped run carries, relative to the root. Baked into
    /// the layout at shape time, so `set_text` treats a change to one as a
    /// layout change.
    pub(super) fn span_colors(text: &InlineSpan) -> Vec<Option<crate::styling::Color>> {
        let root = text.style().and_then(crate::text_layout::paint_color);
        collect_authored_spans(text)
            .iter()
            .map(|(_, style)| {
                style
                    .as_ref()
                    .and_then(crate::text_layout::paint_color)
                    .filter(|color| Some(*color) != root)
            })
            .collect()
    }

    /// Shapes resolved typography on Parley through `text_cx`. Every span
    /// carries its merged style; the root's style, scaled as a span's is, is
    /// the paragraph default, so a paragraph with no run (empty text)
    /// measures the line its style would, family and line height included.
    ///
    /// [`LineOverflow::Enforce`] applies `max_lines` and the ellipsis
    /// (committed layout, dry size, height probes), so a dry probe measures
    /// what `layout` paints. [`LineOverflow::IgnoreForWidthIntrinsic`]
    /// shapes without line-count truncation so a zero-width wrap cannot erase
    /// visible content under `max_lines` (#1085). Callers that need the
    /// ellipsis as a width floor apply [`Self::ellipsis_width_floor`] on top.
    pub(super) fn parley_paragraph(
        &self,
        text_cx: &mut TextContext,
        text: &ResolvedText,
        min_width: f32,
        max_width: Option<f32>,
        line_overflow: LineOverflow,
    ) -> Result<crate::parley_text::ParagraphLayout, TextLayoutError> {
        let (max_lines, ellipsis) = match line_overflow {
            LineOverflow::Enforce => (self.max_lines.map(|n| n as usize), self.ellipsis.as_deref()),
            LineOverflow::IgnoreForWidthIntrinsic => (None, None),
        };
        text_cx.shape(&crate::parley_text::ParagraphSpec {
            font_weight_adjustment: self.font_weight_adjustment,
            spans: &text.spans,
            default_style: text.default_style.as_ref(),
            font_size: text.font_size,
            max_width,
            min_width,
            text_align: self.text_align,
            line_height: None,
            direction: self.text_direction.unwrap_or(TextDirection::Ltr),
            max_lines,
            ellipsis,
        })
    }

    /// The box metrics a shaped result gives under the width constraints.
    pub(super) fn metrics_from(result: &TextLayoutResult, min_width: f64) -> LayoutMetrics {
        let width = result.width.max(min_width);
        LayoutMetrics {
            size: Size::new(width, result.height),
            alphabetic_baseline: result.alphabetic_baseline,
            // Shaper-derived (descent edge of the first line).
            ideographic_baseline: result.ideographic_baseline,
            did_exceed_max_lines: result.truncated,
        }
    }

    /// `(min, max)` intrinsic widths: the widest unbreakable run and the
    /// single-line width, both without `max_lines` truncation (#1085) and
    /// both floored at the ellipsis width when truncation can leave only the
    /// ellipsis.
    pub(super) fn intrinsic_widths(
        &self,
        text_cx: &mut TextContext,
        text: &ResolvedText,
    ) -> Result<(f64, f64), TextLayoutError> {
        let floor = self.ellipsis_width_floor(text_cx, text)?;
        let (min, max) = self
            .parley_paragraph(
                text_cx,
                text,
                0.0,
                None,
                LineOverflow::IgnoreForWidthIntrinsic,
            )?
            .content_widths();
        Ok((min.max(floor), max.max(floor)))
    }

    /// Shaped width of the configured ellipsis when `max_lines` can truncate.
    ///
    /// Truncating layouts may commit an ellipsis-only buffer once the text
    /// prefix is exhausted, so width intrinsics must not report a value
    /// narrower than that ellipsis even when line-count truncation is skipped
    /// for the main probe (#1085 follow-up).
    fn ellipsis_width_floor(
        &self,
        text_cx: &mut TextContext,
        text: &ResolvedText,
    ) -> Result<f64, TextLayoutError> {
        let Some(ellipsis) = self.ellipsis.as_deref().filter(|e| !e.is_empty()) else {
            return Ok(0.0);
        };
        if self.max_lines.is_none() {
            return Ok(0.0);
        }

        // The one paragraph truncation keeps whatever the width is the
        // ellipsis alone, and `ellipsize` styles it as the first run: shaped
        // the same way, over the root as the paragraph default, the floor is
        // that paragraph's width, however the runs' styles differ from the
        // root's.
        let first = text.spans.first().and_then(|(_, style)| style.clone());
        let spans = vec![(ellipsis.to_string(), first)];
        Ok(text_cx
            .shape(&crate::parley_text::ParagraphSpec {
                font_weight_adjustment: self.font_weight_adjustment,
                spans: &spans,
                default_style: text.default_style.as_ref(),
                font_size: text.font_size,
                max_width: None,
                min_width: 0.0,
                text_align: crate::typography::TextAlign::Start,
                line_height: None,
                direction: self.text_direction.unwrap_or(TextDirection::Ltr),
                max_lines: None,
                ellipsis: None,
            })?
            .metrics()
            .width)
    }

    // ===== Metrics =====

    /// Returns the computed size after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn size(&self) -> Size<f64> {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing size")
            .size
    }

    /// Returns the computed width after layout.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.size().width
    }

    /// Returns the computed height after layout.
    #[must_use]
    pub fn height(&self) -> f64 {
        self.size().height
    }

    /// Returns the distance from the top to the alphabetic baseline.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> f64 {
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
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn did_exceed_max_lines(&self) -> bool {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing did_exceed_max_lines")
            .did_exceed_max_lines
    }

    // ===== Intrinsic dimensions =====
    //
    // Transient measurements that do NOT touch `layout_cache`, so a
    // parent may probe intrinsics without disturbing the painter's
    // committed layout. Width intrinsics reshape without max_lines
    // truncation, then floor at the ellipsis width when truncation can
    // leave only the ellipsis; height / dry probes keep full overflow.
    // Returns 0 when no text is set.

    /// The width the text wants with no line wrapping — its single-line
    /// width.
    ///
    /// Skips `max_lines` truncation so the probe measures shaped content
    /// (#1085). When an ellipsis is configured with `max_lines`, the result
    /// is floored at the ellipsis width so intrinsic sizing cannot under-
    /// allocate a truncating layout.
    ///
    /// Returns the precomputed value from the layout cache when it was
    /// measured against `text_cx`'s fonts (O(1) after `layout()`).
    /// Otherwise measures through `text_cx`.
    pub fn max_intrinsic_width(
        &self,
        text_cx: &mut TextContext,
    ) -> Result<f64, super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed()).max_intrinsic_width(self)
    }

    /// The narrowest width the text can take without overflowing — the
    /// width of its widest unbreakable run, found by wrapping at every
    /// opportunity.
    ///
    /// Skips `max_lines` truncation so a zero-width wrap probe cannot
    /// erase visible text (#1085). When an ellipsis is configured with
    /// `max_lines`, the result is floored at the ellipsis width — truncating
    /// layouts may commit an ellipsis-only buffer once the prefix is
    /// exhausted.
    ///
    /// Returns the precomputed value from the layout cache when it was
    /// measured against `text_cx`'s fonts (O(1) after `layout()`).
    /// Otherwise measures through `text_cx`.
    pub fn min_intrinsic_width(
        &self,
        text_cx: &mut TextContext,
    ) -> Result<f64, super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed()).min_intrinsic_width(self)
    }

    /// The height the text takes when laid out at `width` — both the min
    /// and max intrinsic height for a paragraph.
    pub fn intrinsic_height(
        &self,
        text_cx: &mut TextContext,
        width: f64,
    ) -> Result<f64, super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed())
            .intrinsic_height(self, width)
    }

    /// The size the text would take under the given width constraints,
    /// without committing to `layout_cache` (a dry layout). Returns `Size::ZERO` when no text
    /// is set.
    pub fn dry_size(
        &self,
        text_cx: &mut TextContext,
        min_width: f64,
        max_width: f64,
    ) -> Result<Size<f64>, super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed())
            .dry_size(self, min_width, max_width)
    }

    /// Where the first baseline of the given kind would sit after a dry
    /// layout under the width constraints, without touching
    /// `layout_cache`.
    pub fn dry_baseline(
        &self,
        text_cx: &mut TextContext,
        min_width: f64,
        max_width: f64,
        baseline: TextBaseline,
    ) -> Result<Option<f64>, super::TextMeasurementError> {
        super::TextMeasurement::new(text_cx, &super::TextSizing::fixed())
            .dry_baseline(self, min_width, max_width, baseline)
    }
}

/// Owned typography shared by a measurement's committed and transient shapes.
/// It never replaces the painter's authored span tree or crosses measurements.
pub(super) struct ResolvedText {
    pub(super) spans: Vec<(String, Option<TextStyle>)>,
    pub(super) default_style: Option<TextStyle>,
    pub(super) font_size: f32,
    pub(super) answers: Vec<super::TextResolvedSize>,
}

pub(super) fn shaping_widths(
    min_width: f64,
    max_width: f64,
) -> Result<(f32, Option<f32>), TextLayoutError> {
    let min_width = crate::text_layout::error::width(min_width)?;
    let max_width = if max_width == f64::INFINITY {
        None
    } else {
        Some(crate::text_layout::error::width(max_width)?)
    };
    Ok((min_width, max_width))
}

/// Whether line-count / ellipsis truncation runs during a metrics probe.
///
/// Width intrinsics deliberately skip truncation (#1085); layout and dry
/// probes keep it so paint and measurement agree.
#[derive(Clone, Copy, Debug)]
pub(super) enum LineOverflow {
    Enforce,
    IgnoreForWidthIntrinsic,
}

/// Flattens an [`InlineSpan`] tree into per-run `(text, merged style)`
/// pairs in document order, applying style INHERITANCE: each child's
/// style merges over its ancestors' (`TextStyle::merge`), so a bold
/// child of a sized parent shapes bold at the parent's size.
///
/// **Placeholder spans** are emitted as `\u{FFFC}` (Unicode Object
/// Replacement Character) with the inherited style. The shaper gives
/// it a glyph; the caller tracks placeholder positions separately for
/// widget rendering.
///
/// Average and worst case O(total spans + text bytes): one pre-order
/// walk.
/// Merge inheritance before resolving size. The authored sizes remain available
/// for an exact answer lookup; shaping never reconstructs them from scaled runs.
pub(super) fn collect_authored_spans(span: &InlineSpan) -> Vec<(String, Option<TextStyle>)> {
    fn walk(
        span: &crate::typography::TextSpan,
        inherited: Option<&TextStyle>,
        out: &mut Vec<(String, Option<TextStyle>)>,
    ) {
        let merged: Option<TextStyle> = match (inherited, span.style.as_ref()) {
            (Some(parent), Some(own)) => Some(parent.merge(own)),
            (Some(parent), None) => Some(parent.clone()),
            (None, Some(own)) => Some(own.clone()),
            (None, None) => None,
        };
        if let Some(text) = &span.text
            && !text.is_empty()
        {
            out.push((text.clone(), merged.clone()));
        }
        for child in &span.children {
            walk(child, merged.as_ref(), out);
        }
    }

    let mut out = Vec::new();
    match span {
        InlineSpan::Text(root) => walk(root, None, &mut out),
        InlineSpan::Placeholder(_placeholder) => {
            // Emit a Unicode Object Replacement Character (\u{FFFC})
            // as a placeholder. The shaper gives it a glyph; we track
            // its position separately for widget rendering.
            out.push(("\u{FFFC}".to_string(), None));
        }
    }
    out
}
