// PORT-TARGET: flui-widgets::RichText, flui-widgets::TextField
//! `TextLayout` -- cosmic-text-backed shaped text buffer.
//!
//! Extracted from the 1,243-LOC
//! `text_layout.rs` god module. Holds the global `FONT_SYSTEM`
//! singleton (lazy `OnceLock<Mutex<FontSystem>>`; per-shape lock;
//! off the per-command hot path) plus the `TextLayout` struct that
//! wraps cosmic-text's `Buffer` with cursor / hit-test / line-metric
//! operations.

use std::sync::{Arc, OnceLock};

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
/// `crates/flui-painting/ARCHITECTURE.md ## Future Enhancements`.
//
// REMOVE_BY: 2026-09-22 — audit P-17 cadence marker. By this date the
// `catch_unwind` wrapper around cosmic-text's `set_text` /
// `shape_until_scroll` is the principled fix; if `cosmic-text` panics
// are still not a today-problem, push the date out, but do not let the
// "accept the corruption" footnote drift unverified.
static FONT_SYSTEM: OnceLock<Arc<Mutex<FontSystem>>> = OnceLock::new();

/// Gets or initializes the process-wide font system as a shared handle.
///
/// Held in an `Arc` (per ADR-0016) so the render engine's glyph pipeline
/// can shape against the *same* `FontSystem` this module measures with:
/// a font registered through
/// [`PaintingBinding::register_font`](crate::PaintingBinding::register_font)
/// becomes visible to both measurement and rendering, closing the historic
/// two-`FontSystem` gap where a registered face could measure but not paint.
fn font_system_arc() -> &'static Arc<Mutex<FontSystem>> {
    FONT_SYSTEM.get_or_init(|| {
        tracing::debug!("Initializing global FontSystem");
        Arc::new(Mutex::new(FontSystem::new()))
    })
}

/// Gets or initializes the global font system for in-crate shaping.
pub(super) fn font_system() -> &'static Mutex<FontSystem> {
    // Deref-coerces `&Arc<Mutex<_>>` → `&Mutex<_>` at the return site.
    font_system_arc()
}

/// Returns the shared font-system handle so another subsystem (e.g. the
/// render engine's glyph pipeline) can shape against the exact same faces
/// this module measures with. See ADR-0016.
pub(crate) fn shared_font_system() -> SharedFontSystem {
    SharedFontSystem(Arc::clone(font_system_arc()))
}

/// A cheaply-cloneable handle to the process-wide [`FontSystem`] the
/// framework shapes and measures text with.
///
/// cosmic-text's `FontSystem` needs `&mut` access to shape and owns a large
/// font database plus shaping caches, so it cannot be snapshotted or handed
/// out by value. This handle shares one instance behind a lock (per
/// ADR-0016) and mediates access through a scoped callback, so the lock type
/// never appears in a public signature (SP-6). `Clone` is an `Arc` bump —
/// clone it to give another subsystem access to the *same* faces, so a font
/// registered through
/// [`PaintingBinding::register_font`](crate::PaintingBinding::register_font)
/// is visible to both measurement and rendering.
#[derive(Clone)]
pub struct SharedFontSystem(Arc<Mutex<FontSystem>>);

impl SharedFontSystem {
    /// Runs `f` with exclusive access to the font system, holding the lock
    /// only for the duration of the call.
    ///
    /// Keep the closure short — it runs on the per-shape path (measurement,
    /// glyph rendering), not the per-command hot path.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut FontSystem) -> R) -> R {
        let mut font_system = self.0.lock();
        f(&mut font_system)
    }
}

impl std::fmt::Debug for SharedFontSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `FontSystem` itself is a large, non-Debug font database; the
        // handle's identity is all that is meaningful to print.
        f.debug_struct("SharedFontSystem").finish_non_exhaustive()
    }
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
    /// Source text (concatenation of all runs), kept for byte-based
    /// queries (e.g. `get_word_boundary`) that need to inspect
    /// characters around a caret without re-walking every glyph in
    /// every layout run.
    text: String,
    /// The styled runs the buffer was shaped from. Owned so the
    /// max-lines truncation can re-shape a SLICED prefix with the same
    /// per-run attributes — a rich layout truncates without losing the
    /// styling of the kept spans.
    runs: Vec<OwnedRun>,
    /// Font size used for layout.
    font_size: f32,
    /// Line height used for layout.
    line_height: f32,
    /// Text direction.
    direction: TextDirection,
    /// Whether `with_overflow` truncated the text to a max line count.
    truncated: bool,
}

/// One styled run feeding rich shaping (`Buffer::set_rich_text`).
#[derive(Clone)]
struct OwnedRun {
    text: String,
    attrs: cosmic_text::AttrsOwned,
}

impl TextLayout {
    /// Creates a new text layout with no line-count limit.
    pub fn new(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
    ) -> Self {
        Self::with_overflow(
            text,
            style,
            font_size,
            max_width,
            line_height,
            direction,
            None,
            None,
        )
    }

    /// Creates a text layout enforcing an optional maximum visual line
    /// count.
    ///
    /// When the shaped text exceeds `max_lines`, the buffer is RE-SHAPED
    /// on the truncated prefix so the layout's size, line metrics, and
    /// paint output all agree — lines beyond the limit do not exist,
    /// they are not merely skipped at paint. With an `ellipsis`, glyphs
    /// are dropped from the last kept line until the ellipsis fits the
    /// width constraint, then it is appended (Flutter
    /// `ParagraphStyle.maxLines` + `ellipsis` semantics); without one,
    /// the text is cut at the last kept line's end (clip semantics).
    ///
    /// Worst case the fit loop re-shapes once per dropped glyph on the
    /// last line — bounded by that line's glyph count; typical case is
    /// one extra shape.
    #[allow(clippy::too_many_arguments)] // mirrors the shaping input surface; callers are the two crate-internal wrappers
    pub fn with_overflow(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
        max_lines: Option<usize>,
        ellipsis: Option<&str>,
    ) -> Self {
        Self::from_spans(
            vec![(text.to_string(), style.cloned())],
            style,
            font_size,
            max_width,
            line_height,
            direction,
            max_lines,
            ellipsis,
        )
    }

    /// Creates a RICH text layout from styled spans.
    ///
    /// Each span carries its own (already inheritance-merged) style:
    /// per-span font selection, weight, style, font size, and letter
    /// spacing all reach the shaper — a bold or larger child span
    /// measures as bold or larger instead of being flattened to the
    /// root style.
    /// `default_style` and `font_size` describe the buffer-level
    /// defaults applied where a span has no style of its own.
    ///
    /// Truncation (`max_lines`/`ellipsis`) slices the SPANS, so a
    /// truncated rich layout keeps the styling of everything it kept;
    /// the ellipsis inherits the last kept span's attributes.
    #[allow(clippy::too_many_arguments)] // mirrors the shaping input surface
    pub fn from_spans(
        spans: Vec<(String, Option<TextStyle>)>,
        default_style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
        max_lines: Option<usize>,
        ellipsis: Option<&str>,
    ) -> Self {
        debug_assert!(
            font_size > 0.0 && font_size.is_finite(),
            "TextLayout font_size must be positive and finite, got {font_size}"
        );

        let line_height = line_height.unwrap_or(font_size * 1.2);
        let default_attrs = cosmic_text::AttrsOwned::new(&style_to_attrs(default_style));

        let runs: Vec<OwnedRun> = spans
            .into_iter()
            .map(|(text, style)| {
                let attrs = match &style {
                    Some(style) => {
                        let mut attrs = style_to_attrs(Some(style));
                        // Per-span font size/line height ride on the attrs
                        // (cosmic's per-span Metrics); spans without one
                        // inherit the buffer-level default.
                        #[allow(clippy::cast_possible_truncation)]
                        // f64 style sizes → f32 shaping space
                        if let Some(size) = style.font_size.map(|s| s as f32) {
                            #[allow(clippy::cast_possible_truncation)]
                            let span_line_height =
                                style.height.map_or(size * 1.2, |h| h as f32 * size);
                            attrs = attrs.metrics(Metrics::new(size, span_line_height));
                            // cosmic letter spacing is in EM; ours is in
                            // logical px.
                            #[allow(clippy::cast_possible_truncation)]
                            if let Some(spacing) = style.letter_spacing.map(|s| s as f32)
                                && size > 0.0
                            {
                                attrs = attrs.letter_spacing(spacing / size);
                            }
                        }
                        cosmic_text::AttrsOwned::new(&attrs)
                    }
                    None => default_attrs.clone(),
                };
                OwnedRun { text, attrs }
            })
            .collect();

        let mut font_system = font_system().lock();
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
        buffer.set_size(&mut font_system, max_width, None);

        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        let mut this = Self {
            buffer,
            text,
            runs,
            font_size,
            line_height,
            direction,
            truncated: false,
        };
        this.shape_runs(&mut font_system);

        if let Some(max_lines) = max_lines
            && max_lines > 0
        {
            this.enforce_max_lines(&mut font_system, max_lines, ellipsis, max_width);
        }

        this
    }

    /// (Re-)shapes the buffer from the current runs.
    fn shape_runs(&mut self, font_system: &mut FontSystem) {
        self.buffer.set_rich_text(
            font_system,
            self.runs
                .iter()
                .map(|run| (run.text.as_str(), run.attrs.as_attrs())),
            &cosmic_text::Attrs::new(),
            Shaping::Advanced,
            None,
        );
        self.buffer.shape_until_scroll(font_system, false);
    }

    /// Truncates the shaped buffer to `max_lines` visual lines,
    /// optionally appending `ellipsis` to the last kept line.
    ///
    /// Rich-aware: the cut slices the styled RUNS (a run boundary is a
    /// char boundary of the concatenated text by construction), so the
    /// kept prefix re-shapes with its original per-span attributes and
    /// the ellipsis inherits the last kept run's.
    fn enforce_max_lines(
        &mut self,
        font_system: &mut FontSystem,
        max_lines: usize,
        ellipsis: Option<&str>,
        max_width: Option<f32>,
    ) {
        // Visual-line end positions as byte offsets into the ORIGINAL
        // text. Glyph offsets are relative to their source line, so the
        // per-line base offsets are reconstructed from the same '\n'
        // split cosmic uses for buffer lines.
        let line_bases: Vec<usize> = {
            let mut bases = vec![0usize];
            for (i, b) in self.text.bytes().enumerate() {
                if b == b'\n' {
                    bases.push(i + 1);
                }
            }
            bases
        };
        let run_ends: Vec<usize> = self
            .buffer
            .layout_runs()
            .map(|run| {
                let base = line_bases.get(run.line_i).copied().unwrap_or(0);
                base + run.glyphs.last().map_or(0, |g| g.end)
            })
            .collect();
        if run_ends.len() <= max_lines {
            return;
        }
        self.truncated = true;

        // The original (untruncated) inputs survive the fit loop; the
        // layout's own fields are only committed on success.
        let full_runs = std::mem::take(&mut self.runs);
        let full_text = std::mem::take(&mut self.text);

        let mut cut = run_ends[max_lines - 1];
        loop {
            let mut candidate = Self::sliced_runs(&full_runs, cut);
            if let Some(ellipsis) = ellipsis
                && !ellipsis.is_empty()
            {
                let attrs = candidate.last().or(full_runs.first()).map_or_else(
                    || cosmic_text::AttrsOwned::new(&cosmic_text::Attrs::new()),
                    |run| run.attrs.clone(),
                );
                candidate.push(OwnedRun {
                    text: ellipsis.to_string(),
                    attrs,
                });
            }
            self.runs = candidate;
            self.shape_runs(font_system);

            let lines = self.buffer.layout_runs().count();
            let last_width = self
                .buffer
                .layout_runs()
                .last()
                .map_or(0.0, |run| run.line_w);
            let fits_width = max_width.is_none_or(|w| last_width <= w);
            let exhausted = cut == 0;
            if (lines <= max_lines && fits_width) || exhausted {
                // Commit: concatenated text mirrors the shaped runs.
                self.text = self.runs.iter().map(|run| run.text.as_str()).collect();
                return;
            }

            // Drop one more character (never splitting a codepoint) and
            // retry; an empty prefix terminates the loop with just the
            // ellipsis (or the empty string) shaped.
            let Some((prev, _)) = full_text[..cut].char_indices().next_back() else {
                self.text = self.runs.iter().map(|run| run.text.as_str()).collect();
                return;
            };
            cut = prev;
        }
    }

    /// The styled runs covering `text[..cut]`: full runs before the
    /// cut, the run containing it sliced at the (char-boundary) cut.
    fn sliced_runs(runs: &[OwnedRun], cut: usize) -> Vec<OwnedRun> {
        let mut out = Vec::new();
        let mut base = 0usize;
        for run in runs {
            let end = base + run.text.len();
            if cut <= base {
                break;
            }
            if cut >= end {
                out.push(run.clone());
            } else {
                out.push(OwnedRun {
                    text: run.text[..cut - base].to_string(),
                    attrs: run.attrs.clone(),
                });
                break;
            }
            base = end;
        }
        out
    }

    /// Returns the computed metrics for this layout.
    ///
    /// Baselines come from the shaper: cosmic-text's `LayoutRun::line_y`
    /// IS the alphabetic baseline of the line; the ideographic baseline
    /// is bounded by the first line's descent edge (cosmic exposes no
    /// per-font ideographic metric). The old `height * 0.8` / `× 1.125`
    /// approximations survive ONLY in the empty-text branch, where no
    /// shaped run exists to ask.
    pub fn metrics(&self) -> TextLayoutResult {
        let mut total_height = 0.0f32;
        let mut max_line_width = 0.0f32;
        let mut line_count = 0usize;
        let mut first_baseline = 0.0f32;
        let mut first_descent_edge = 0.0f32;

        for run in self.buffer.layout_runs() {
            line_count += 1;
            max_line_width = max_line_width.max(run.line_w);
            total_height = total_height.max(run.line_top + run.line_height);

            if line_count == 1 {
                first_baseline = run.line_y;
                first_descent_edge = run.line_top + run.line_height;
            }
        }

        if line_count == 0 {
            // Empty text: nothing was shaped, synthesize from the line
            // box. The only consumer is empty-string measurement.
            line_count = 1;
            total_height = self.line_height;
            first_baseline = self.line_height * 0.8;
            first_descent_edge = self.line_height;
        }

        TextLayoutResult {
            width: max_line_width,
            height: total_height,
            line_count,
            max_line_width,
            alphabetic_baseline: first_baseline,
            ideographic_baseline: first_descent_edge,
            truncated: self.truncated,
        }
    }

    /// Whether `with_overflow` truncated the text to its max line count.
    #[must_use]
    pub fn was_truncated(&self) -> bool {
        self.truncated
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
            // Shaper-derived: `line_y` is the baseline, so ascent/descent
            // are exact line-box distances, not font-size guesses.
            let ascent = run.line_y - run.line_top;
            let descent = (run.line_top + run.line_height) - run.line_y;

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
                run.line_y as f64,
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
