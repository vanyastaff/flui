// PORT-TARGET: flui-widgets::RichText, flui-widgets::TextField
//! `TextLayout` -- cosmic-text-backed shaped text buffer.
//!
//! Mythos chain U6 extracted these from the 1,243-LOC
//! `text_layout.rs` god module. Holds the global `FONT_SYSTEM`
//! singleton (lazy `OnceLock<Mutex<FontSystem>>`; per-shape lock;
//! off the per-command hot path) plus the `TextLayout` struct that
//! wraps cosmic-text's `Buffer` with cursor / hit-test / line-metric
//! operations.

use std::sync::OnceLock;

use cosmic_text::{Buffer, Cursor, FontSystem, Metrics, Shaping};
use flui_types::{
    geometry::{Offset, Pixels, Rect},
    typography::{
        LineMetrics, TextAffinity, TextBox, TextDirection, TextPosition, TextRange, TextStyle,
    },
};
use parking_lot::Mutex;

use super::{LineInfo, TextLayoutResult, measure::style_to_attrs};

/// Global font system instance.
///
/// cosmic-text requires a `FontSystem` for font discovery and shaping.
/// We use a global instance with interior mutability for convenience.
///
/// # Poisoning caveat
///
/// We deliberately use `parking_lot::Mutex`, which does *not* poison
/// the lock when a panic occurs while it is held. If `cosmic-text`
/// panics mid-`set_text` or mid-`shape_until_scroll` (e.g. an internal
/// invariant trips during font fallback), the surviving `FontSystem`
/// is conceptually corrupt but no subsequent caller will observe a
/// `PoisonError`. We accept that today because (a) cosmic-text panics
/// are rare in practice, and (b) `std::sync::Mutex`'s poisoning would
/// force every call site to `match` the lock result. A `catch_unwind`
/// wrapper around `set_text` / `shape_until_scroll` is the
/// principled fix and is tracked in
/// `crates/flui-painting/ARCHITECTURE.md ## Outstanding refactors`.
static FONT_SYSTEM: OnceLock<Mutex<FontSystem>> = OnceLock::new();

/// Gets or initializes the global font system.
pub(super) fn font_system() -> &'static Mutex<FontSystem> {
    FONT_SYSTEM.get_or_init(|| {
        tracing::debug!("Initializing global FontSystem");
        Mutex::new(FontSystem::new())
    })
}

/// A laid out text buffer with cursor and hit testing support.
///
/// Wraps a cosmic-text `Buffer` and provides methods for:
/// - Cursor positioning (offset → screen position).
/// - Hit testing (screen position → text offset).
/// - Line metrics.
/// - Selection boxes.
pub struct TextLayout {
    /// The underlying cosmic-text buffer.
    buffer: Buffer,
    /// Source text, kept for byte-based queries (e.g.
    /// `get_word_boundary`) that need to inspect characters around a
    /// caret without re-walking every glyph in every layout run.
    text: String,
    /// Font size used for layout.
    font_size: f32,
    /// Line height used for layout.
    line_height: f32,
    /// Text direction.
    direction: TextDirection,
}

impl TextLayout {
    /// Creates a new text layout.
    pub fn new(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
    ) -> Self {
        debug_assert!(
            font_size > 0.0 && font_size.is_finite(),
            "TextLayout::new font_size must be positive and finite, got {font_size}"
        );

        let mut font_system = font_system().lock();

        let line_height = line_height.unwrap_or(font_size * 1.2);
        let metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(&mut font_system, metrics);
        buffer.set_size(&mut font_system, max_width, None);

        let attrs = style_to_attrs(style);
        buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut font_system, false);

        Self {
            buffer,
            text: text.to_string(),
            font_size,
            line_height,
            direction,
        }
    }

    /// Returns the computed metrics for this layout.
    pub fn metrics(&self) -> TextLayoutResult {
        let mut total_height = 0.0f32;
        let mut max_line_width = 0.0f32;
        let mut line_count = 0usize;
        let mut first_baseline = 0.0f32;

        for run in self.buffer.layout_runs() {
            line_count += 1;
            max_line_width = max_line_width.max(run.line_w);
            total_height = total_height.max(run.line_top + run.line_height);

            if line_count == 1 {
                first_baseline = run.line_top + run.line_height * 0.8;
            }
        }

        if line_count == 0 {
            line_count = 1;
            total_height = self.line_height;
            first_baseline = self.line_height * 0.8;
        }

        TextLayoutResult {
            width: max_line_width,
            height: total_height,
            line_count,
            max_line_width,
            alphabetic_baseline: first_baseline,
        }
    }

    /// Returns the screen offset for a caret at the given text
    /// position.
    pub fn get_offset_for_caret(&mut self, position: TextPosition) -> Offset<Pixels> {
        let mut font_system = font_system().lock();

        let cursor = Cursor::new(0, position.offset);

        if let Some(layout_cursor) = self.buffer.layout_cursor(&mut font_system, cursor) {
            let mut x = 0.0f32;
            let mut y = 0.0f32;

            for run in self.buffer.layout_runs() {
                if run.line_i == layout_cursor.line {
                    y = run.line_top;

                    for glyph in run.glyphs {
                        if glyph.start <= position.offset && position.offset <= glyph.end {
                            let glyph_progress = if glyph.end > glyph.start {
                                (position.offset - glyph.start) as f32
                                    / (glyph.end - glyph.start) as f32
                            } else {
                                0.0
                            };
                            x = glyph.x + glyph.w * glyph_progress;
                            break;
                        }
                        x = glyph.x + glyph.w;
                    }
                    break;
                }
            }

            Offset::new(Pixels(x), Pixels(y))
        } else {
            Offset::ZERO
        }
    }

    /// Returns the text position for a screen offset.
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let x = offset.dx.0;
        let y = offset.dy.0;

        let mut target_line: Option<usize> = None;
        let mut line_top = 0.0f32;

        for run in self.buffer.layout_runs() {
            if y >= run.line_top && y < run.line_top + run.line_height {
                target_line = Some(run.line_i);
                line_top = run.line_top;
                break;
            }
        }

        let target_line = if let Some(l) = target_line {
            l
        } else {
            let mut last_line = 0;
            for run in self.buffer.layout_runs() {
                last_line = run.line_i;
            }
            if y >= line_top {
                last_line
            } else {
                return TextPosition::upstream(0);
            }
        };

        for run in self.buffer.layout_runs() {
            if run.line_i == target_line {
                let mut last_offset = run.glyphs.first().map_or(0, |g| g.start);

                for glyph in run.glyphs {
                    let glyph_center = glyph.x + glyph.w / 2.0;

                    if x < glyph_center {
                        return TextPosition::new(glyph.start, TextAffinity::Downstream);
                    }

                    last_offset = glyph.end;
                }

                return TextPosition::new(last_offset, TextAffinity::Upstream);
            }
        }

        TextPosition::upstream(0)
    }

    /// Returns line metrics for all lines in the layout.
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let mut metrics = Vec::new();

        for (line_number, run) in self.buffer.layout_runs().enumerate() {
            let ascent = self.font_size * 0.8;
            let descent = self.font_size * 0.2;

            let start_index = run.glyphs.first().map_or(0, |g| g.start);
            let end_index = run.glyphs.last().map_or(start_index, |g| g.end);

            metrics.push(LineMetrics::new(
                true,
                ascent as f64,
                descent as f64,
                ascent as f64,
                run.line_height as f64,
                run.line_w as f64,
                0.0,
                run.line_top as f64 + ascent as f64,
                line_number,
                start_index,
                end_index,
                end_index,
                end_index,
            ));
        }

        if metrics.is_empty() {
            metrics.push(LineMetrics::new(
                true,
                (self.font_size * 0.8) as f64,
                (self.font_size * 0.2) as f64,
                (self.font_size * 0.8) as f64,
                self.line_height as f64,
                0.0,
                0.0,
                (self.font_size * 0.8) as f64,
                0,
                0,
                0,
                0,
                0,
            ));
        }

        metrics
    }

    /// Returns extended line information including RTL status.
    pub fn get_line_info(&self) -> Vec<LineInfo> {
        let mut info = Vec::new();

        for run in self.buffer.layout_runs() {
            let start_index = run.glyphs.first().map_or(0, |g| g.start);
            let end_index = run.glyphs.last().map_or(start_index, |g| g.end);

            info.push(LineInfo {
                line_number: run.line_i,
                is_rtl: run.rtl,
                width: run.line_w,
                height: run.line_height,
                top: run.line_top,
                start_index,
                end_index,
            });
        }

        if info.is_empty() {
            info.push(LineInfo {
                line_number: 0,
                is_rtl: self.direction.is_rtl(),
                width: 0.0,
                height: self.line_height,
                top: 0.0,
                start_index: 0,
                end_index: 0,
            });
        }

        info
    }

    /// Returns true if any line in the layout is RTL.
    pub fn has_rtl_content(&self) -> bool {
        self.buffer.layout_runs().any(|run| run.rtl)
    }

    /// Returns true if the layout contains bidirectional text.
    pub fn is_bidirectional(&self) -> bool {
        let mut has_ltr = false;
        let mut has_rtl = false;

        for run in self.buffer.layout_runs() {
            if run.rtl {
                has_rtl = true;
            } else {
                has_ltr = true;
            }

            if has_ltr && has_rtl {
                return true;
            }
        }

        false
    }

    /// Returns bounding boxes for the given text range.
    #[allow(clippy::needless_pass_by_value)]
    pub fn get_boxes_for_range(&self, range: TextRange) -> Vec<TextBox> {
        let mut boxes = Vec::new();

        for run in self.buffer.layout_runs() {
            let mut line_start_x: Option<f32> = None;
            let mut line_end_x = 0.0f32;

            for glyph in run.glyphs {
                if glyph.end > range.start && glyph.start < range.end {
                    if line_start_x.is_none() {
                        let start_offset = if glyph.start < range.start {
                            let progress = (range.start - glyph.start) as f32
                                / (glyph.end - glyph.start) as f32;
                            glyph.x + glyph.w * progress
                        } else {
                            glyph.x
                        };
                        line_start_x = Some(start_offset);
                    }

                    line_end_x = if glyph.end > range.end {
                        let progress =
                            (range.end - glyph.start) as f32 / (glyph.end - glyph.start) as f32;
                        glyph.x + glyph.w * progress
                    } else {
                        glyph.x + glyph.w
                    };
                }
            }

            if let Some(start_x) = line_start_x {
                let rect = Rect::from_ltrb(
                    Pixels(start_x),
                    Pixels(run.line_top),
                    Pixels(line_end_x),
                    Pixels(run.line_top + run.line_height),
                );
                boxes.push(TextBox::new(rect, self.direction));
            }
        }

        boxes
    }

    /// Returns the word boundary at the given text position.
    ///
    /// The implementation expands left and right from `position.offset`
    /// (a byte offset into `self.text`, matching cosmic-text's
    /// `glyph.start` convention used elsewhere in this module) over
    /// runs of non-whitespace characters. Multi-byte UTF-8 codepoints
    /// are stepped via `str::char_indices` so we never split inside a
    /// codepoint.
    ///
    /// Semantics are deliberately "non-whitespace run", not full
    /// UAX #29 word segmentation; full segmentation would need the
    /// `unicode-segmentation` crate, which is filed as an
    /// `Outstanding refactor`. The previous implementation was
    /// O(n²) in the glyph count *and* incorrect for the common ASCII
    /// case (every byte index was a glyph start, so every call
    /// returned the entire line).
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        let text = self.text.as_str();
        let total = text.len();
        let mut offset = position.offset.min(total);

        // Snap to the nearest preceding char boundary so we never
        // dereference inside a multi-byte codepoint.
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }

        let bytes = text.as_bytes();

        // Walk left across non-whitespace bytes. We can scan by byte
        // because every char-boundary byte of an ASCII whitespace
        // character is itself ASCII (0..0x80), and continuation bytes
        // (0x80..0xC0) are never whitespace. The result is always a
        // valid char boundary because the moment we hit a whitespace
        // byte we stop one byte *after* it (or at 0).
        let mut start = offset;
        while start > 0 {
            let prev = bytes[start - 1];
            if prev.is_ascii_whitespace() {
                break;
            }
            // Step over continuation bytes to the next char start.
            start -= 1;
            while start > 0 && !text.is_char_boundary(start) {
                start -= 1;
            }
        }

        // Walk right across non-whitespace.
        let mut end = offset;
        while end < total {
            let cur = bytes[end];
            if cur.is_ascii_whitespace() {
                break;
            }
            end += 1;
            while end < total && !text.is_char_boundary(end) {
                end += 1;
            }
        }

        TextRange::new(start, end)
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextLayout")
            .field("font_size", &self.font_size)
            .field("line_height", &self.line_height)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}
