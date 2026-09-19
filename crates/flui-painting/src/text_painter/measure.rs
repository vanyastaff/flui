//! `TextPainter` layout and measurement: `layout`, the cached metrics it
//! produces, and the size / baseline / overflow queries over them.

use std::sync::Arc;

use flui_types::{
    geometry::{Offset, Pixels, Size},
    typography::{InlineSpan, TextAlign, TextDirection, TextStyle},
};

use super::{DEFAULT_FONT_SIZE, LayoutMetrics, TextBaseline, TextLayoutCache, TextPainter};
use crate::text_layout::TextLayout;

impl TextPainter {
    /// Computes the text layout within the given width constraints.
    ///
    /// # Panics
    ///
    /// Panics if `text` or `text_direction` is not set.
    #[expect(clippy::expect_used)] // Documented precondition: text and text_direction must be set
    pub fn layout(&mut self, min_width: f32, max_width: f32) {
        // NaN is forbidden, but `+INFINITY` is the documented "no max
        // width" sentinel — `compute_paint_offset` and the cosmic-text
        // path below detect `!is_finite()` and skip alignment shifts /
        // width clamping. Do not tighten this to `is_finite()`.
        assert!(
            !max_width.is_nan() && !min_width.is_nan(),
            "Width constraints must not be NaN"
        );

        let font_generation = crate::shared_font_system().generation();
        if let Some(cache) = &self.layout_cache
            && cache.font_generation == font_generation
            && (cache.min_width - min_width).abs() < f32::EPSILON
            && (cache.max_width - max_width).abs() < f32::EPSILON
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

        let (metrics, layout) =
            self.compute_layout_metrics(text, min_width, max_width, LineOverflow::Enforce);

        // Precompute intrinsic widths (Parley-inspired: shape-once, query-many).
        // min_intrinsic = width at max_width=0 (widest unbreakable run)
        // max_intrinsic = width at max_width=∞ (single-line width)
        // Both probes omit max_lines truncation — truncating a zero-width
        // layout can collapse visible text to an empty prefix (#1085) — but
        // still floor at the ellipsis width when truncation can leave only
        // the ellipsis.
        let (min_metrics, _) =
            self.compute_layout_metrics(text, 0.0, 0.0, LineOverflow::IgnoreForWidthIntrinsic);
        let (max_metrics, _) = self.compute_layout_metrics(
            text,
            0.0,
            f32::INFINITY,
            LineOverflow::IgnoreForWidthIntrinsic,
        );
        let ellipsis_floor = self.ellipsis_width_floor(text);
        let min_intrinsic_width = min_metrics.size.width.0.max(ellipsis_floor);
        let max_intrinsic_width = max_metrics.size.width.0.max(ellipsis_floor);

        self.layout_cache = Some(TextLayoutCache {
            font_generation,
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
    pub(super) fn span_colors(&self, text: &InlineSpan) -> Vec<Option<flui_types::Color>> {
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

    /// Computes layout metrics for the text using cosmic-text.
    ///
    /// [`LineOverflow::Enforce`] applies `max_lines` / ellipsis (committed
    /// layout, dry size, height probes). [`LineOverflow::IgnoreForWidthIntrinsic`]
    /// shapes without line-count truncation so a zero-width wrap cannot erase
    /// visible content under `max_lines` (#1085). Callers that need the
    /// ellipsis as a width floor apply [`Self::ellipsis_width_floor`] on top.
    fn compute_layout_metrics(
        &self,
        text: &InlineSpan,
        min_width: f32,
        max_width: f32,
        line_overflow: LineOverflow,
    ) -> (LayoutMetrics, TextLayout) {
        let font_size = text
            .style()
            .and_then(|s| s.font_size.map(|f| f as f32))
            .unwrap_or(DEFAULT_FONT_SIZE);

        let scaled_font_size = font_size * self.text_scale_factor;
        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);

        let max_width_opt = if max_width.is_finite() {
            Some(max_width)
        } else {
            None
        };

        // RICH shaping: the span tree flattens to per-run styles with
        // inheritance (`TextStyle::merge`), so a bold or larger child
        // span measures as bold or larger — `to_plain_text` used to
        // flatten everything onto the root style. The text scale factor
        // is baked into each run's font size here, where the effective
        // size is known.
        let spans = collect_styled_spans(text, self.text_scale_factor);
        // max_lines/ellipsis are ENFORCED by the shaper-level truncation:
        // size, line metrics, and painted glyphs all agree on the kept
        // lines (pre-fix the painter only *detected* the overflow and
        // painted every line anyway). Intrinsic width probes skip this path.
        let (max_lines, ellipsis) = match line_overflow {
            LineOverflow::Enforce => (self.max_lines.map(|n| n as usize), self.ellipsis.as_deref()),
            LineOverflow::IgnoreForWidthIntrinsic => (None, None),
        };
        let layout = TextLayout::from_spans(
            spans,
            text.style(),
            scaled_font_size,
            max_width_opt,
            None,
            direction,
            max_lines,
            ellipsis,
        );

        let layout_result = layout.metrics();

        let did_exceed_max_lines = layout_result.truncated;

        let width = layout_result.width.max(min_width);

        // Shaper-derived (descent edge of the first line); the old value
        // was `alphabetic × 1.125`, a constant with no font behind it.
        let ideographic_baseline = layout_result.ideographic_baseline;

        let paint_offset = self.compute_paint_offset(width, max_width);

        let metrics = LayoutMetrics {
            size: Size::new(Pixels(width), Pixels(layout_result.height)),
            alphabetic_baseline: layout_result.alphabetic_baseline,
            ideographic_baseline,
            did_exceed_max_lines,
            paint_offset,
        };

        (metrics, layout)
    }

    /// Shaped width of the configured ellipsis when `max_lines` can truncate.
    ///
    /// Truncating layouts may commit an ellipsis-only buffer once the text
    /// prefix is exhausted, so width intrinsics must not report a value
    /// narrower than that ellipsis even when line-count truncation is skipped
    /// for the main probe (#1085 follow-up).
    fn ellipsis_width_floor(&self, text: &InlineSpan) -> f32 {
        let Some(ellipsis) = self.ellipsis.as_deref().filter(|e| !e.is_empty()) else {
            return 0.0;
        };
        if self.max_lines.is_none_or(|n| n == 0) {
            return 0.0;
        }

        let font_size = text
            .style()
            .and_then(|s| s.font_size.map(|f| f as f32))
            .unwrap_or(DEFAULT_FONT_SIZE);
        let scaled_font_size = font_size * self.text_scale_factor;
        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);
        let layout = TextLayout::from_spans(
            vec![(ellipsis.to_string(), text.style().cloned())],
            text.style(),
            scaled_font_size,
            None,
            None,
            direction,
            None,
            None,
        );
        layout.metrics().width
    }

    /// Computes the paint offset based on text alignment.
    pub(super) fn compute_paint_offset(
        &self,
        content_width: f32,
        max_width: f32,
    ) -> Offset<Pixels> {
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

        Offset::new(Pixels(dx), Pixels(0.0))
    }

    // ===== Metrics =====

    /// Returns the computed size after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn size(&self) -> Size<Pixels> {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing size")
            .size
    }

    /// Returns the computed width after layout.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.size().width.0
    }

    /// Returns the computed height after layout.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.size().height.0
    }

    /// Returns the distance from the top to the alphabetic baseline.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
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
    /// Returns the precomputed value from the layout cache when available
    /// (O(1) after `layout()`). Falls back to a fresh cosmic-text layout
    /// when called before `layout()`.
    #[must_use]
    pub fn max_intrinsic_width(&self) -> f32 {
        if let Some(cache) = &self.layout_cache {
            return cache.max_intrinsic_width;
        }
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        let (metrics, _) = self.compute_layout_metrics(
            text,
            0.0,
            f32::INFINITY,
            LineOverflow::IgnoreForWidthIntrinsic,
        );
        metrics.size.width.0.max(self.ellipsis_width_floor(text))
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
    /// Returns the precomputed value from the layout cache when available
    /// (O(1) after `layout()`). Falls back to a fresh cosmic-text layout
    /// when called before `layout()`.
    #[must_use]
    pub fn min_intrinsic_width(&self) -> f32 {
        if let Some(cache) = &self.layout_cache {
            return cache.min_intrinsic_width;
        }
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        let (metrics, _) =
            self.compute_layout_metrics(text, 0.0, 0.0, LineOverflow::IgnoreForWidthIntrinsic);
        metrics.size.width.0.max(self.ellipsis_width_floor(text))
    }

    /// The height the text takes when laid out at `width` — both the min
    /// and max intrinsic height for a paragraph (Flutter
    /// `RenderParagraph._computeIntrinsicHeight`).
    #[must_use]
    pub fn intrinsic_height(&self, width: f32) -> f32 {
        let Some(text) = self.text.as_ref() else {
            return 0.0;
        };
        // Wrap at `width` (max) but pass `0` as the min: only the height is
        // wanted, and a non-zero min only inflates the width field via
        // `width.max(min_width)` — an infinite `width` probe would otherwise
        // make that field infinite.
        let (metrics, _) = self.compute_layout_metrics(text, 0.0, width, LineOverflow::Enforce);
        metrics.size.height.0
    }

    /// The size the text would take under the given width constraints,
    /// without committing to `layout_cache` — Flutter's
    /// `TextPainter`-backed dry layout. Returns `Size::ZERO` when no text
    /// is set.
    #[must_use]
    pub fn dry_size(&self, min_width: f32, max_width: f32) -> Size<Pixels> {
        let Some(text) = self.text.as_ref() else {
            return Size::ZERO;
        };
        let (metrics, _) =
            self.compute_layout_metrics(text, min_width, max_width, LineOverflow::Enforce);
        metrics.size
    }

    /// Where the first baseline of the given kind would sit after a dry
    /// layout under the width constraints, without touching
    /// `layout_cache`.
    #[must_use]
    pub fn dry_baseline(
        &self,
        min_width: f32,
        max_width: f32,
        baseline: TextBaseline,
    ) -> Option<f32> {
        let text = self.text.as_ref()?;
        let (metrics, _) =
            self.compute_layout_metrics(text, min_width, max_width, LineOverflow::Enforce);
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
    scale: f32,
) -> Vec<(String, Option<TextStyle>)> {
    fn walk(
        span: &flui_types::typography::TextSpan,
        inherited: Option<&TextStyle>,
        scale: f32,
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
            let mut effective = merged.clone();
            if let Some(style) = &mut effective {
                // Scale font_size to device pixels.
                if let Some(size) = style.font_size {
                    style.font_size = Some(size * f64::from(scale));
                }
                // Scale letter_spacing by the same DPR factor so that
                // `from_spans` can compute the EM ratio as
                // `spacing / font_size` using consistent (device-px) units.
                // Without this scaling, at DPR=2 a 2px spacing on a 16px
                // font yields 2/32=0.0625 EM instead of the correct 0.125 EM.
                if let Some(spacing) = style.letter_spacing {
                    style.letter_spacing = Some(spacing * f64::from(scale));
                }
            }
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
