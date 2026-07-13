//! `TextPainter` painting + cursor methods: `paint`,
//! `get_offset_for_caret`, `get_position_for_offset`,
//! `get_line_metrics`, `get_boxes_for_selection`, `get_word_boundary`.
//!
//! Extracted from the 990-LOC `text_painter.rs`
//! god module. All methods here depend on the cached layout
//! (`TextLayoutCache`) populated by [`super::measure`]'s `layout()`.

use flui_types::{
    geometry::{Offset, Pixels},
    typography::{LineMetrics, TextBox, TextPosition, TextRange},
};

use super::TextPainter;
use crate::Canvas;

impl TextPainter {
    // ===== Cursor and Selection =====

    /// Returns the screen offset for a caret at the given text
    /// position.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_offset_for_caret(&mut self, position: TextPosition) -> Offset<Pixels> {
        let cache = self
            .layout_cache
            .as_mut()
            .expect("layout() must be called before get_offset_for_caret()");

        let offset = cache.layout.get_offset_for_caret(position);

        offset + cache.paint_offset
    }

    /// Returns the text position for a screen offset.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_position_for_offset()");

        let adjusted = offset - cache.paint_offset;
        cache.layout.get_position_for_offset(adjusted)
    }

    /// Returns metrics for each line in the laid out text.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_line_metrics()");

        cache.layout.get_line_metrics()
    }

    /// Returns bounding boxes for a text selection.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_boxes_for_selection(&self, start: usize, end: usize) -> Vec<TextBox> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before get_boxes_for_selection()");

        let mut boxes = cache.layout.get_boxes_for_range(TextRange::new(start, end));

        for text_box in &mut boxes {
            text_box.rect = text_box.rect.translate_offset(cache.paint_offset);
        }

        boxes
    }

    /// Returns the word boundary at the given text position.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first
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
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[allow(clippy::expect_used)] // Documented precondition: layout() must be called first, text must be set
    pub fn paint(&self, canvas: &mut Canvas, offset: Offset<Pixels>) {
        // Check `text` first: it is the *root-cause* precondition.
        // If both `text` and `layout_cache` are unset, "text must be
        // set" is the actionable message — the cache only exists
        // because `layout()` ran, and `layout()` requires `text`.
        let text = self
            .text
            .as_ref()
            .expect("TextPainter.text must be set before paint");

        let cache = self
            .layout_cache
            .as_ref()
            .expect("layout() must be called before paint()");

        let paint_offset = offset + cache.paint_offset;

        // Pass wrap-width to the GPU text renderer so glyphon respects
        // the same line-breaking constraints as the cosmic-text layout cache.
        // None = unbounded (no wrapping); Some(w) = wrap at w pixels.
        let wrap_width = if cache.max_width.is_finite() && cache.max_width > 0.0 {
            Some(cache.max_width)
        } else {
            None
        };

        canvas.draw_text_span(
            text,
            paint_offset,
            self.text_scale_factor as f64,
            wrap_width,
        );
    }
}
