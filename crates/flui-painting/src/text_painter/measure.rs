//! `TextPainter` layout and measurement: `layout`, the cached metrics it
//! produces, and the size / baseline / overflow queries over them.

use std::sync::Arc;

use crate::typography::{InlineSpan, TextAlign, TextDirection, TextStyle};
use flui_foundation::geometry::{Offset, Size};

use super::{DEFAULT_FONT_SIZE, LayoutMetrics, TextBaseline, TextLayoutCache, TextPainter};
use crate::text_layout::{FontsKey, TextContext, TextLayout, TextLayoutResult};

impl TextPainter {
    /// What a cached layout was taken against: the process font database's
    /// generation, which the painted layout was shaped on, and `text_cx`'s
    /// collection and its generation, which measured it.
    fn font_key(text_cx: &TextContext) -> (u64, FontsKey) {
        (
            crate::shared_font_system().generation(),
            text_cx.fonts().key(),
        )
    }

    /// The cached layout, when it was measured against the fonts `text_cx`
    /// measures with now.
    fn cache_for(&self, text_cx: &TextContext) -> Option<&TextLayoutCache> {
        let (font_generation, fonts) = Self::font_key(text_cx);
        self.layout_cache
            .as_ref()
            .filter(|cache| cache.font_generation == font_generation && cache.fonts.matches(&fonts))
    }

    /// Computes the text layout within the given width constraints,
    /// measuring through `text_cx`.
    ///
    /// A cached layout is kept while the constraints, the process font
    /// database and `text_cx`'s collection and generation are unchanged; a
    /// context over another collection, or a face registered since, measures
    /// again.
    ///
    /// # Panics
    ///
    /// Panics if `text` or `text_direction` is not set.
    #[expect(clippy::expect_used)] // Documented precondition: text and text_direction must be set
    pub fn layout(&mut self, text_cx: &mut TextContext, min_width: f64, max_width: f64) {
        // NaN is forbidden, but `+INFINITY` is the documented "no max
        // width" sentinel — `compute_paint_offset` and the cosmic-text
        // path below detect `!is_finite()` and skip alignment shifts /
        // width clamping. Do not tighten this to `is_finite()`.
        assert!(
            !max_width.is_nan() && !min_width.is_nan(),
            "Width constraints must not be NaN"
        );
        text_cx.note_lent();

        let (font_generation, fonts) = Self::font_key(text_cx);
        if let Some(cache) = self.cache_for(text_cx)
            && (cache.min_width - min_width).abs() < f64::EPSILON
            && (cache.max_width - max_width).abs() < f64::EPSILON
        {
            return;
        }

        let text = self
            .text
            .as_ref()
            .expect("TextPainter.text must be set before layout");
        let _text_direction = self
            .text_direction
            .expect("TextPainter.text_direction must be set before layout");

        // The painted layout: glyphs come from it until ADR-0092 §10 step 4b,
        // carets and selection until step 5. It measures nothing.
        let layout = self.cosmic_layout(text, max_width);
        let result = self
            .parley_paragraph(text_cx, text, max_width, LineOverflow::Enforce)
            .metrics();
        let metrics = self.metrics_from(&result, min_width, max_width);

        // Precompute intrinsic widths (shape once, query many).
        let (min_intrinsic_width, max_intrinsic_width) = self.intrinsic_widths(text_cx, text);

        self.layout_cache = Some(TextLayoutCache {
            font_generation,
            fonts,
            min_width,
            max_width,
            size: metrics.size,
            alphabetic_baseline: metrics.alphabetic_baseline,
            ideographic_baseline: metrics.ideographic_baseline,
            did_exceed_max_lines: metrics.did_exceed_max_lines,
            paint_offset: metrics.paint_offset,
            layout: Arc::new(layout),
            min_intrinsic_width,
            max_intrinsic_width,
        });
    }

    /// The colour each shaped run carries, relative to the root. Baked into
    /// the layout at shape time, so `set_text` treats a change to one as a
    /// layout change.
    pub(super) fn span_colors(&self, text: &InlineSpan) -> Vec<Option<crate::styling::Color>> {
        let root = text.style().and_then(crate::text_layout::paint_color);
        collect_styled_spans(text, self.text_scale_factor)
            .iter()
            .map(|(_, style)| {
                style
                    .as_ref()
                    .and_then(crate::text_layout::paint_color)
                    .filter(|color| Some(*color) != root)
            })
            .collect()
    }

    /// The paragraph's font size with the text scale factor applied.
    fn scaled_font_size(&self, text: &InlineSpan) -> f64 {
        text.style()
            .and_then(|s| s.font_size)
            .unwrap_or(DEFAULT_FONT_SIZE)
            * self.text_scale_factor
    }

    /// Shapes `text` on cosmic-text at `max_width` for paint, with
    /// `max_lines` and the ellipsis applied so the painted glyphs keep the
    /// lines the measurement kept.
    fn cosmic_layout(&self, text: &InlineSpan, max_width: f64) -> TextLayout {
        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);
        // RICH shaping: the span tree flattens to per-run styles with
        // inheritance (`TextStyle::merge`), so a bold or larger child
        // span measures as bold or larger. The text scale factor is baked
        // into each run's font size here, where the effective size is known.
        let spans = collect_styled_spans(text, self.text_scale_factor);
        TextLayout::from_spans(
            spans,
            text.style(),
            self.scaled_font_size(text),
            max_width.is_finite().then_some(max_width),
            None,
            direction,
            self.max_lines.map(|n| n as usize),
            self.ellipsis.as_deref(),
        )
    }

    /// Shapes `text` on Parley through `text_cx` at `max_width`, with the
    /// same span flattening and scale as the painted layout. Every span
    /// carries its merged style, so no paragraph default style is passed: a
    /// default would lay its unscaled size under the scaled spans.
    ///
    /// [`LineOverflow::Enforce`] applies `max_lines` (committed layout, dry
    /// size, height probes). [`LineOverflow::IgnoreForWidthIntrinsic`]
    /// shapes without line-count truncation so a zero-width wrap cannot erase
    /// visible content under `max_lines` (#1085). Callers that need the
    /// ellipsis as a width floor apply [`Self::ellipsis_width_floor`] on top.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "f64 layout values narrow to Parley's f32 layout space"
    )]
    fn parley_paragraph(
        &self,
        text_cx: &mut TextContext,
        text: &InlineSpan,
        max_width: f64,
        line_overflow: LineOverflow,
    ) -> crate::parley_text::ParagraphLayout {
        let spans = collect_styled_spans(text, self.text_scale_factor);
        let max_lines = match line_overflow {
            LineOverflow::Enforce => self.max_lines.map(|n| n as usize),
            LineOverflow::IgnoreForWidthIntrinsic => None,
        };
        text_cx.shape(&crate::parley_text::ParagraphSpec {
            spans: &spans,
            default_style: None,
            font_size: self.scaled_font_size(text) as f32,
            max_width: max_width.is_finite().then_some(max_width as f32),
            line_height: None,
            direction: self.text_direction.unwrap_or(TextDirection::Ltr),
            max_lines,
        })
    }

    /// Parley's metrics for `text` broken at `max_width`.
    fn measure(
        &self,
        text_cx: &mut TextContext,
        text: &InlineSpan,
        max_width: f64,
        line_overflow: LineOverflow,
    ) -> TextLayoutResult {
        self.parley_paragraph(text_cx, text, max_width, line_overflow)
            .metrics()
    }

    /// The box metrics a shaped result gives under the width constraints.
    fn metrics_from(
        &self,
        result: &TextLayoutResult,
        min_width: f64,
        max_width: f64,
    ) -> LayoutMetrics {
        let width = result.width.max(min_width);
        LayoutMetrics {
            size: Size::new(width, result.height),
            alphabetic_baseline: result.alphabetic_baseline,
            // Shaper-derived (descent edge of the first line).
            ideographic_baseline: result.ideographic_baseline,
            did_exceed_max_lines: result.truncated,
            paint_offset: self.compute_paint_offset(width, max_width),
        }
    }

    /// `(min, max)` intrinsic widths: the widest unbreakable run and the
    /// single-line width, both without `max_lines` truncation (#1085) and
    /// both floored at the ellipsis width when truncation can leave only the
    /// ellipsis.
    fn intrinsic_widths(&self, text_cx: &mut TextContext, text: &InlineSpan) -> (f64, f64) {
        let floor = self.ellipsis_width_floor(text_cx, text);
        let (min, max) = self
            .parley_paragraph(
                text_cx,
                text,
                f64::INFINITY,
                LineOverflow::IgnoreForWidthIntrinsic,
            )
            .content_widths();
        (min.max(floor), max.max(floor))
    }

    /// Shaped width of the configured ellipsis when `max_lines` can truncate.
    ///
    /// Truncating layouts may commit an ellipsis-only buffer once the text
    /// prefix is exhausted, so width intrinsics must not report a value
    /// narrower than that ellipsis even when line-count truncation is skipped
    /// for the main probe (#1085 follow-up).
    fn ellipsis_width_floor(&self, text_cx: &mut TextContext, text: &InlineSpan) -> f64 {
        let Some(ellipsis) = self.ellipsis.as_deref().filter(|e| !e.is_empty()) else {
            return 0.0;
        };
        if self.max_lines.is_none_or(|n| n == 0) {
            return 0.0;
        }

        #[expect(
            clippy::cast_possible_truncation,
            reason = "f64 layout values narrow to Parley's f32 layout space"
        )]
        let font_size = self.scaled_font_size(text) as f32;
        let spans = vec![(
            ellipsis.to_string(),
            text.style()
                .map(|style| scaled_style(style, self.text_scale_factor)),
        )];
        text_cx
            .shape(&crate::parley_text::ParagraphSpec {
                spans: &spans,
                default_style: None,
                font_size,
                max_width: None,
                line_height: None,
                direction: self.text_direction.unwrap_or(TextDirection::Ltr),
                max_lines: None,
            })
            .metrics()
            .width
    }

    /// Computes the paint offset based on text alignment.
    pub(super) fn compute_paint_offset(&self, content_width: f64, max_width: f64) -> Offset<f64> {
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
    /// width (Flutter `RenderParagraph.computeMaxIntrinsicWidth`).
    ///
    /// Skips `max_lines` truncation so the probe measures shaped content
    /// (#1085). When an ellipsis is configured with `max_lines`, the result
    /// is floored at the ellipsis width so intrinsic sizing cannot under-
    /// allocate a truncating layout.
    ///
    /// Returns the precomputed value from the layout cache when it was
    /// measured against `text_cx`'s fonts (O(1) after `layout()`).
    /// Otherwise measures through `text_cx`.
    #[must_use]
    pub fn max_intrinsic_width(&self, text_cx: &mut TextContext) -> f64 {
        text_cx.note_lent();
        if let Some(cache) = self.cache_for(text_cx) {
            return cache.max_intrinsic_width;
        }
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        self.intrinsic_widths(text_cx, text).1
    }

    /// The narrowest width the text can take without overflowing — the
    /// width of its widest unbreakable run, found by wrapping at every
    /// opportunity (Flutter `RenderParagraph.computeMinIntrinsicWidth`).
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
    #[must_use]
    pub fn min_intrinsic_width(&self, text_cx: &mut TextContext) -> f64 {
        text_cx.note_lent();
        if let Some(cache) = self.cache_for(text_cx) {
            return cache.min_intrinsic_width;
        }
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        self.intrinsic_widths(text_cx, text).0
    }

    /// The height the text takes when laid out at `width` — both the min
    /// and max intrinsic height for a paragraph (Flutter
    /// `RenderParagraph._computeIntrinsicHeight`).
    #[must_use]
    pub fn intrinsic_height(&self, text_cx: &mut TextContext, width: f64) -> f64 {
        text_cx.note_lent();
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        self.measure(text_cx, text, width, LineOverflow::Enforce)
            .height
    }

    /// The size the text would take under the given width constraints,
    /// without committing to `layout_cache` — Flutter's
    /// `TextPainter`-backed dry layout. Returns `Size::ZERO` when no text
    /// is set.
    #[must_use]
    pub fn dry_size(&self, text_cx: &mut TextContext, min_width: f64, max_width: f64) -> Size<f64> {
        text_cx.note_lent();
        let Some(text) = self.text.as_ref() else {
            return Size::ZERO;
        };
        let result = self.measure(text_cx, text, max_width, LineOverflow::Enforce);
        self.metrics_from(&result, min_width, max_width).size
    }

    /// Where the first baseline of the given kind would sit after a dry
    /// layout under the width constraints, without touching
    /// `layout_cache`.
    #[must_use]
    pub fn dry_baseline(
        &self,
        text_cx: &mut TextContext,
        min_width: f64,
        max_width: f64,
        baseline: TextBaseline,
    ) -> Option<f64> {
        text_cx.note_lent();
        let text = self.text.as_ref()?;
        let result = self.measure(text_cx, text, max_width, LineOverflow::Enforce);
        let metrics = self.metrics_from(&result, min_width, max_width);
        Some(match baseline {
            TextBaseline::Alphabetic => metrics.alphabetic_baseline,
            TextBaseline::Ideographic => metrics.ideographic_baseline,
        })
    }
}

/// Whether line-count / ellipsis truncation runs during a metrics probe.
///
/// Width intrinsics deliberately skip truncation (#1085); layout and dry
/// probes keep it so paint and measurement agree.
#[derive(Clone, Copy, Debug)]
enum LineOverflow {
    Enforce,
    IgnoreForWidthIntrinsic,
}

/// Flattens an [`InlineSpan`] tree into per-run `(text, merged style)`
/// pairs in document order, applying style INHERITANCE: each child's
/// style merges over its ancestors' (`TextStyle::merge`), so a bold
/// child of a sized parent shapes bold at the parent's size.
///
/// The text scale factor is baked into every effective font size here
/// — the shaper sees final pixel sizes.
///
/// **Placeholder spans** are emitted as `\u{FFFC}` (Unicode Object
/// Replacement Character) with the inherited style. The shaper gives
/// it a glyph; the caller tracks placeholder positions separately for
/// widget rendering.
///
/// Average and worst case O(total spans + text bytes): one pre-order
/// walk.
pub(crate) fn collect_styled_spans(
    span: &InlineSpan,
    scale: f64,
) -> Vec<(String, Option<TextStyle>)> {
    fn walk(
        span: &crate::typography::TextSpan,
        inherited: Option<&TextStyle>,
        scale: f64,
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
            let effective = merged.as_ref().map(|style| scaled_style(style, scale));
            out.push((text.clone(), effective));
        }
        for child in &span.children {
            walk(child, merged.as_ref(), scale, out);
        }
    }

    let mut out = Vec::new();
    match span {
        InlineSpan::Text(root) => walk(root, None, scale, &mut out),
        InlineSpan::Placeholder(_placeholder) => {
            // Emit a Unicode Object Replacement Character (\u{FFFC})
            // as a placeholder. The shaper gives it a glyph; we track
            // its position separately for widget rendering.
            out.push(("\u{FFFC}".to_string(), None));
        }
    }
    out
}

/// `style` with its font size and letter spacing multiplied by `scale`.
///
/// Letter spacing scales with the size so that `from_spans` computes the EM
/// ratio as `spacing / font_size` in consistent units: without it, at a
/// scale of 2 a 2 px spacing on a 16 px font yields 2/32 = 0.0625 EM instead
/// of 0.125 EM.
fn scaled_style(style: &TextStyle, scale: f64) -> TextStyle {
    let mut style = style.clone();
    if let Some(size) = style.font_size {
        style.font_size = Some(size * scale);
    }
    if let Some(spacing) = style.letter_spacing {
        style.letter_spacing = Some(spacing * scale);
    }
    style
}
