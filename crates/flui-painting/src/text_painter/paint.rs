//! `TextPainter` painting and cursor queries, all over the layout that
//! [`super::measure`]'s `layout()` cached.

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
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_offset_for_caret(&self, position: TextPosition) -> Offset<Pixels> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_offset_for_caret() — it reads the cached layout that layout() populates");

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
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_position_for_offset() — it reads the cached layout that layout() populates");

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
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_line_metrics() — it reads the cached layout that layout() populates");

        cache.layout.get_line_metrics()
    }

    /// Returns bounding boxes for a text selection.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_boxes_for_selection(&self, start: usize, end: usize) -> Vec<TextBox> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_boxes_for_selection() — it reads the cached layout that layout() populates");

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
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_word_boundary() — it reads the cached layout that layout() populates");

        cache.layout.get_word_boundary(position)
    }

    // ===== Painting =====

    /// Paints the text onto the canvas at the given offset.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first, text must be set
    pub fn paint(&self, canvas: &mut Canvas, offset: Offset<Pixels>) {
        // Check `text` first: it is the *root-cause* precondition.
        // If both `text` and `layout_cache` are unset, "text must be
        // set" is the actionable message — the cache only exists
        // because `layout()` ran, and `layout()` requires `text`.
        let text = self
            .text
            .as_ref()
            .expect("BUG: TextPainter.text must be set before paint() — TextPainter::layout() requires text to be set, and paint() requires layout() to have run first");

        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before paint() — it reads the cached layout that layout() populates");

        let paint_offset = offset + cache.paint_offset;
        let color = text
            .style()
            .and_then(crate::text_layout::paint_color)
            .unwrap_or(flui_types::Color::BLACK);
        // The very layout this painter measured: what the engine rasterises
        // is, by identity, what was laid out.
        canvas.draw_paragraph(&cache.layout, paint_offset, color);
    }
}
