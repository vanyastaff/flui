//! `TextPainter` painting and cursor queries over what
//! [`super::measure`]'s `layout()` cached: paint records its paragraph, the
//! cursor queries read a cosmic-text layout built on the first of them.

use std::sync::Arc;

use crate::typography::{LineMetrics, TextBox, TextPosition, TextRange};
use flui_foundation::geometry::Offset;

use super::{TextLayoutCache, TextPainter};
use crate::Canvas;
use crate::text_layout::TextLayout;

impl TextPainter {
    /// The cosmic-text layout the cursor queries read, shaped at the cached
    /// width on the first query and kept until the next `layout()`, which
    /// drops it when a face has been registered on the process font database
    /// since it was shaped. So a registration costs one reshape, at the first
    /// query after the next `layout()`, and never one per query.
    #[expect(clippy::expect_used)] // Documented precondition: layout() sets text
    fn caret_layout(&self, cache: &TextLayoutCache) -> Arc<TextLayout> {
        let (_, layout) = cache.caret_layout.get_or_init(|| {
            let text = self.text.as_ref().expect(
                "BUG: a TextPainter with a cached layout has text — set_text drops the cache",
            );
            // Read before shaping: a face registered while this shapes leaves
            // the stamp behind, and the next `layout()` shapes again.
            let generation = crate::shared_font_system().generation();
            (
                generation,
                Arc::new(self.cosmic_layout(text, cache.max_width)),
            )
        });
        Arc::clone(layout)
    }

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
    pub fn get_offset_for_caret(&self, position: TextPosition) -> Offset<f64> {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_offset_for_caret() — it reads the cached layout that layout() populates");

        let offset = self.caret_layout(cache).get_offset_for_caret(position);

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
    pub fn get_position_for_offset(&self, offset: Offset<f64>) -> TextPosition {
        let cache = self
            .layout_cache
            .as_ref()
            .expect("BUG: TextPainter::layout() must be called before get_position_for_offset() — it reads the cached layout that layout() populates");

        let adjusted = offset - cache.paint_offset;
        self.caret_layout(cache).get_position_for_offset(adjusted)
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

        self.caret_layout(cache).get_line_metrics()
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

        let mut boxes = self
            .caret_layout(cache)
            .get_boxes_for_range(TextRange::new(start, end));

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

        self.caret_layout(cache).get_word_boundary(position)
    }

    // ===== Painting =====

    /// Paints the text onto the canvas at the given offset.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[expect(clippy::expect_used)] // Documented precondition: layout() must be called first, text must be set
    pub fn paint(&self, canvas: &mut Canvas, offset: Offset<f64>) {
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
            .unwrap_or(crate::styling::Color::BLACK);
        // The very paragraph this painter measured: what the engine
        // rasterises is, by identity, what was laid out.
        canvas.draw_paragraph(&cache.paragraph, paint_offset, color);
    }
}
