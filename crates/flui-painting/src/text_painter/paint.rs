//! `TextPainter` painting and cursor queries over what
//! [`super::measure`]'s `layout()` cached: paint records its paragraph, and
//! the cursor queries read the layout that measured it.

use crate::typography::{LineMetrics, TextBox, TextPosition, TextRange};
use flui_foundation::geometry::Offset;

use super::{TextLayoutCache, TextPainter};
use crate::Canvas;

impl TextPainter {
    /// The cached layout a cursor query reads.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been called.
    #[expect(clippy::panic, reason = "documented precondition: layout() runs first")]
    fn laid_out(&self, query: &str) -> &TextLayoutCache {
        self.layout_cache.as_ref().unwrap_or_else(|| {
            panic!(
                "BUG: TextPainter::layout() must be called before {query}() — it reads the cached \
                 layout that layout() populates"
            )
        })
    }

    // ===== Cursor and Selection =====

    /// Returns the offset of the caret before the text at `position`: the
    /// top-left of a caret as tall as its line.
    ///
    /// Per scalar: an offset inside a grapheme (between `e` and a combining
    /// mark) gets its own caret, a proportional slice of the grapheme, so an
    /// input method asking for one scalar's rect gets one. Where the text
    /// before and after `position` is painted apart (a soft wrap, a bidi run
    /// boundary), [`TextAffinity::Downstream`](crate::typography::TextAffinity)
    /// takes the side after it and `Upstream` the side before; after a hard
    /// break the caret starts the next line. An offset past the kept text
    /// (dropped lines, an ellipsis) answers the kept text's end.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    pub fn get_offset_for_caret(&self, position: TextPosition) -> Offset<f64> {
        let cache = self.laid_out("get_offset_for_caret");
        cache.layout.caret(position) + cache.paint_offset
    }

    /// Returns the text position for a screen offset: the grapheme boundary
    /// nearest the point on the line under it.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    pub fn get_position_for_offset(&self, offset: Offset<f64>) -> TextPosition {
        let cache = self.laid_out("get_position_for_offset");
        cache.layout.position_at(offset - cache.paint_offset)
    }

    /// Returns metrics for each kept line of the laid out text.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        self.laid_out("get_line_metrics").layout.line_metrics()
    }

    /// Returns the boxes of the text between `start` and `end`: one per
    /// stretch of one direction on one line, in visual order, each carrying
    /// its run's direction and as tall as its line.
    ///
    /// # Panics
    ///
    /// Panics if [`layout`](super::TextPainter::layout) has not been
    /// called.
    #[must_use]
    pub fn get_boxes_for_selection(&self, start: usize, end: usize) -> Vec<TextBox> {
        let cache = self.laid_out("get_boxes_for_selection");
        let mut boxes = cache.layout.boxes(TextRange::new(start, end));
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
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        self.laid_out("get_word_boundary")
            .layout
            .word_boundary(position)
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
