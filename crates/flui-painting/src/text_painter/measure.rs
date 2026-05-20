//! `TextPainter` layout + measurement: `layout`,
//! `compute_layout_metrics`, `compute_paint_offset`, `size`, `width`,
//! `height`, `compute_distance_to_actual_baseline`,
//! `did_exceed_max_lines`.
//!
//! Mythos chain U7 extracted these from the 990-LOC `text_painter.rs`
//! god module.

use flui_types::{
    geometry::{Offset, Pixels, Size},
    typography::{InlineSpan, TextAlign, TextDirection},
};

use super::{DEFAULT_FONT_SIZE, LayoutMetrics, TextBaseline, TextLayoutCache, TextPainter};
use crate::text_layout::TextLayout;

impl TextPainter {
    /// Computes the text layout within the given width constraints.
    ///
    /// # Panics
    ///
    /// Panics if `text` or `text_direction` is not set.
    #[allow(clippy::expect_used)] // Documented precondition: text and text_direction must be set
    pub fn layout(&mut self, min_width: f32, max_width: f32) {
        assert!(
            !max_width.is_nan() && !min_width.is_nan(),
            "Width constraints must not be NaN"
        );

        if let Some(cache) = &self.layout_cache
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
        let font_size = text
            .style()
            .and_then(|s| {
                s.font_size.map(|f| {
                    #[allow(clippy::cast_possible_truncation)]
                    let size = f as f32;
                    size
                })
            })
            .unwrap_or(DEFAULT_FONT_SIZE);

        let scaled_font_size = font_size * self.text_scale_factor;
        let direction = self.text_direction.unwrap_or(TextDirection::Ltr);

        let max_width_opt = if max_width.is_finite() {
            Some(max_width)
        } else {
            None
        };

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

        #[allow(clippy::cast_possible_truncation)]
        let line_count = layout_result.line_count as u32;
        let did_exceed_max_lines = self.max_lines.is_some_and(|max| line_count > max);

        let width = layout_result.width.max(min_width);

        let ideographic_baseline = layout_result.alphabetic_baseline * 1.125;

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

        Offset::new(Pixels(dx), Pixels(0.0))
    }

    // ===== Metrics =====

    /// Returns the computed size after layout.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](Self::layout) has not been called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
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
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
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
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn did_exceed_max_lines(&self) -> bool {
        self.layout_cache
            .as_ref()
            .expect("layout() must be called before accessing did_exceed_max_lines")
            .did_exceed_max_lines
    }
}
