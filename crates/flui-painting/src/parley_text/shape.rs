//! Shaping a paragraph on a UI runtime's [`TextContext`] (ADR-0092 §1).
//!
//! One ranged builder per paragraph: the defaults first, then each span's
//! style over its byte range. The result answers the paragraph's metrics,
//! and turns into the [`ShapedParagraph`] the display list carries, so the
//! paragraph paints from the layout that measured it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use super::caret::PlacedLine;
use crate::display_list::paragraph::RunData;
use crate::display_list::{FontBlob, FontFace, ShapedGlyph, ShapedParagraph};
use crate::glyphs::{Synthesis, fake_bold_width};
use crate::styling::Color;
use crate::typography::{FontStyle, TextAlign, TextDirection, TextStyle};
use flui_foundation::geometry::{Rect, Size};
use parley::fontique::Collection;
use parley::layout::PositionedLayoutItem;
use parley::setting::{FontFeature, FontVariation, Tag};
use parley::style::{
    FontFamily, FontFamilyName, FontFeatures, FontStyle as ParleyFontStyle, FontVariations,
    FontWeight, GenericFamily, LineHeight, OverflowWrap, StyleProperty,
};
use parley::{Alignment, AlignmentOptions, FontData, Layout};

use crate::text_layout::font_resolve::{Family, resolve_family_name};
use crate::text_layout::{TextContext, TextLayoutResult, paint_color};

/// What one paragraph is shaped from.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphSpec<'a> {
    /// The styled spans, in order; their texts concatenate to the paragraph.
    /// Each span's style is already merged over its ancestors'.
    pub spans: &'a [(String, Option<TextStyle>)],
    /// The style applied where a span sets nothing of its own.
    pub default_style: Option<&'a TextStyle>,
    /// The paragraph's font size, in logical pixels.
    pub font_size: f32,
    /// Signed adjustment after style inheritance; zero preserves authored
    /// weights and axes. Adjusted requested weights are bounded to 1..1000.
    pub font_weight_adjustment: i32,
    /// The width lines break at; `None` breaks only at hard breaks.
    pub max_width: Option<f32>,
    /// Minimum allocated width, independent of the line-breaking cap.
    pub min_width: f32,
    /// Per-line alignment inside the allocated paragraph box.
    pub text_align: TextAlign,
    /// The line height in logical pixels; `None` is `1.2 ×` each run's own
    /// font size, so a larger span grows its line box.
    pub line_height: Option<f32>,
    /// Resolves Start/End alignment. It does not override Parley's inferred
    /// bidi base direction (mapping decision 12).
    pub direction: TextDirection,
    /// The lines the paragraph keeps; `None` or `Some(0)` keeps every line.
    /// Lines past it are dropped, and the metrics report `truncated`.
    pub max_lines: Option<usize>,
    /// What ends the last kept line when lines are dropped; `None` or empty
    /// cuts the text at that line's end. The ellipsis takes the style of the
    /// last span it follows, and text is dropped from the end of the line
    /// until the line and the ellipsis fit `max_width`.
    pub ellipsis: Option<&'a str>,
}

/// A shaped, line-broken paragraph.
///
/// Measurement, paint and the caret queries (`caret`, `position_at`,
/// `boxes`, `line_metrics`, `word_boundary`) all read this one layout.
pub struct ParagraphLayout {
    pub(super) layout: Layout<SpanBrush>,
    /// The text the layout holds, an appended ellipsis included.
    pub(super) text: String,
    spans: Vec<SpanInfo>,
    pub(super) line_height: f32,
    max_lines: Option<usize>,
    alignment_width: f32,
    content_widths: (f64, f64),
    /// Whether an ellipsis replaced dropped lines; the layout itself then
    /// holds only the kept ones.
    pub(super) ellipsized: bool,
    /// Where the kept text ends: before an appended ellipsis, or at the end
    /// of the last kept line (its hard break left out) when lines are
    /// dropped. Caret and hit queries never reach past it.
    pub(super) kept_text: usize,
    /// The kept lines' clusters where they are painted, for the caret
    /// queries; built on the first one.
    pub(super) placed: OnceLock<Vec<PlacedLine>>,
}

// A painter keeps its layout in its cache, and a render object holding the
// painter moves between threads.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ParagraphLayout>();
};

/// What a span was shaped with, for [`ShapedParagraph::describe_spans`].
#[derive(Clone, Debug)]
struct SpanInfo {
    len: usize,
    family: String,
    weight: f32,
    italic: bool,
    size: Option<f32>,
    color: Option<Color>,
}

impl ParagraphLayout {
    /// How many lines the paragraph keeps.
    pub(super) fn kept(&self) -> usize {
        self.max_lines
            .map_or(self.layout.len(), |max| max.min(self.layout.len()))
    }

    /// The paragraph's metrics, read from its laid-out lines.
    ///
    /// Empty text lays out one empty line in the paragraph's font, so it
    /// reports the line box and baseline a line of text in that font has
    /// (flui-painting `ARCHITECTURE.md`, mapping decision 15). A layout with
    /// no line at all reports one line box of the paragraph's line height
    /// with the baseline at `0.8 ×` that height.
    #[must_use]
    pub fn metrics(&self) -> TextLayoutResult {
        let Some(first) = self.layout.get(0) else {
            return TextLayoutResult {
                width: f64::from(self.alignment_width),
                height: f64::from(self.line_height),
                line_count: 1,
                max_line_width: 0.0,
                alphabetic_baseline: f64::from(self.line_height * 0.8),
                ideographic_baseline: f64::from(self.line_height),
                truncated: false,
            };
        };
        let line = first.metrics();
        let kept = self.kept();
        let (width, height) = self.kept_extent();
        TextLayoutResult {
            width: f64::from(self.alignment_width),
            height: f64::from(height),
            line_count: kept.max(1),
            max_line_width: f64::from(width),
            alphabetic_baseline: f64::from(line.baseline),
            ideographic_baseline: f64::from(line.block_max_coord),
            truncated: self.ellipsized || kept < self.layout.len(),
        }
    }

    /// The width and height of the kept lines.
    pub(super) fn kept_extent(&self) -> (f32, f32) {
        let kept = self.kept();
        if kept < self.layout.len() {
            // The layout's own width and height cover every line; a
            // truncated paragraph measures only the lines it keeps, with
            // Parley's rule (trailing whitespace excluded from the width).
            self.layout
                .lines()
                .take(kept)
                .fold((0.0_f32, 0.0_f32), |(width, height), line| {
                    let metrics = line.metrics();
                    let extent = metrics.inline_min_coord + Self::visible_line_width(&line);
                    (width.max(extent), height + metrics.line_height)
                })
        } else {
            let width = self
                .layout
                .lines()
                .map(|line| Self::visible_line_width(&line))
                .fold(0.0_f32, f32::max);
            (width, self.layout.height())
        }
    }

    pub(super) fn visible_line_width(line: &parley::layout::Line<'_, SpanBrush>) -> f32 {
        let advance: f32 = line
            .runs()
            .map(|run| run.clusters().map(|cluster| cluster.advance()).sum::<f32>())
            .sum();
        advance - line.metrics().trailing_whitespace
    }

    /// The paragraph's narrowest and widest widths: `(min, max)`, where min
    /// takes every soft break (the widest unbreakable run) and max takes
    /// none (the single-line width). Independent of the width it was broken
    /// at and of `max_lines`.
    #[must_use]
    pub fn content_widths(&self) -> (f64, f64) {
        if self.text.is_empty() {
            return (0.0, 0.0);
        }
        self.content_widths
    }

    /// The paragraph as the display list carries it: its kept lines' glyph
    /// runs, each naming its face by blob, with the metrics' size.
    ///
    /// `root` is the colour the paragraph command paints with; a run whose
    /// span colour equals it carries none of its own, so a root-only recolour
    /// repaints without reshaping.
    ///
    /// Lines use Parley's alignment in the allocated paragraph box. These
    /// same offsets and cluster advances are used by the caret queries.
    #[must_use]
    pub fn to_shaped(&self, root: Option<Color>) -> ShapedParagraph {
        let metrics = self.metrics();
        let mut faces: Vec<FontFace> = Vec::new();
        let mut face_index: HashMap<(u64, u32), u32> = HashMap::new();
        let mut em_bounds: Vec<Option<[f32; 4]>> = Vec::new();
        let mut runs = Vec::new();
        let mut glyphs = Vec::new();
        let mut coords = Vec::new();
        let mut baselines = Vec::new();
        let mut ink = InkBounds::default();
        for line in self.layout.lines().take(self.kept()) {
            let line_metrics = line.metrics();
            baselines.push(line_metrics.baseline);
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let font = run.font();
                let face = *face_index
                    .entry((font.data.id(), font.index))
                    .or_insert_with(|| {
                        em_bounds.push(head_bounds(font));
                        faces.push(face_of(font));
                        u32::try_from(faces.len() - 1).unwrap_or(u32::MAX)
                    });
                let synthesis = synthesis_of(&run.synthesis());
                let font_size = run.font_size();
                let coords_start = coords.len();
                coords.extend_from_slice(run.normalized_coords());
                let glyph_start = glyphs.len();
                let baseline = glyph_run.baseline();
                for glyph in glyph_run.positioned_glyphs() {
                    let Ok(id) = u16::try_from(glyph.id) else {
                        warn_glyph_id(glyph.id);
                        continue;
                    };
                    let x = glyph.x;
                    let dy = glyph.y - baseline;
                    ink.add(
                        em_bounds.get(face as usize).copied().flatten(),
                        x,
                        glyph.y,
                        font_size,
                        synthesis,
                    );
                    glyphs.push(ShapedGlyph { id, x, dy });
                }
                runs.push(RunData {
                    face,
                    font_size,
                    coords: range32(coords_start, coords.len()),
                    synthesis,
                    color: glyph_run
                        .style()
                        .brush
                        .0
                        .filter(|color| Some(*color) != root),
                    baseline,
                    glyphs: range32(glyph_start, glyphs.len()),
                });
            }
        }
        let spans = self.spans.iter().map(|span| describe(span, root)).collect();
        ShapedParagraph {
            text: self.text.as_str().into(),
            size: Size::new(metrics.width, metrics.height),
            baselines: baselines.into(),
            ink: ink.finish(),
            faces: faces.into(),
            runs: runs.into(),
            glyphs: glyphs.into(),
            coords: coords.into(),
            spans,
        }
    }
}

impl fmt::Debug for ParagraphLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParagraphLayout")
            .field("text", &self.text)
            .field("lines", &self.layout.len())
            .finish_non_exhaustive()
    }
}

/// Confines Parley's public line-width escape hatch to allocation alignment.
/// The shaped clusters are reused; this repeats line breaking, not shaping.
fn align_allocated_lines(layout: &mut Layout<SpanBrush>, spec: &ParagraphSpec<'_>, width: f32) {
    let cap = spec.max_width.unwrap_or(f32::MAX);
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(cap);
    breaker.state_mut().set_line_max_advance(cap);
    while breaker.break_next().is_some() {
        breaker.set_prior_line_width(width);
    }
    breaker.finish();
    let alignment = match spec.text_align.resolve(spec.direction) {
        TextAlign::Left | TextAlign::Start => Alignment::Left,
        TextAlign::Right | TextAlign::End => Alignment::Right,
        TextAlign::Center => Alignment::Center,
        TextAlign::Justify => Alignment::Justify,
    };
    layout.align(alignment, AlignmentOptions::default());
}

/// `start..end` as `u32`s; a paragraph holds fewer than 2^32 glyphs.
fn range32(start: usize, end: usize) -> std::ops::Range<u32> {
    let narrow = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    narrow(start)..narrow(end)
}

/// Parley's face as a blob FLUI owns: the same bytes under the same id. The
/// shared handle keeps fontique's source-cache entry alive, so the id does
/// not change while a paragraph or a registry holds it.
fn face_of(font: &FontData) -> FontFace {
    let (bytes, id) = font.data.clone().into_raw_parts();
    FontFace::new(FontBlob::new(id, bytes), font.index)
}

/// fontique's synthesis suggestion as a raster key's: the skew rounded to
/// whole degrees.
fn synthesis_of(synthesis: &parley::fontique::Synthesis) -> Synthesis {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a skew angle in degrees, clamped to the i8 range first"
    )]
    let skew_degrees = synthesis
        .skew()
        .map_or(0, |degrees| degrees.round().clamp(-127.0, 127.0) as i8);
    Synthesis {
        embolden: synthesis.embolden(),
        skew_degrees,
    }
}

/// A glyph id past `u16` has no raster key; the glyph is skipped. OpenType
/// glyph ids are 16-bit, so no face Parley shapes with produces one.
fn warn_glyph_id(id: u32) {
    tracing::warn!(id, "a glyph id past u16 is not painted");
}

/// The face's `head` bounds in em units: `[x_min, y_min, x_max, y_max]`,
/// y up. `None` when the face has no `head` table or no units per em.
fn head_bounds(font: &FontData) -> Option<[f32; 4]> {
    let index = usize::try_from(font.index).ok()?;
    let face = swash::FontRef::from_index(font.data.data(), index)?;
    let head = face.table(swash::tag_from_bytes(b"head"))?;
    let read_u16 = |at: usize| Some(u16::from_be_bytes([*head.get(at)?, *head.get(at + 1)?]));
    let read_i16 = |at: usize| read_u16(at).map(|v| f32::from(v.cast_signed()));
    let units = f32::from(read_u16(18)?);
    if units <= 0.0 {
        return None;
    }
    Some([
        read_i16(36)? / units,
        read_i16(38)? / units,
        read_i16(40)? / units,
        read_i16(42)? / units,
    ])
}

/// The tangent of the steepest skew a glyph's bitmap may carry, per degree:
/// `tan(θ)` for the run's skew.
fn skew_slant(skew_degrees: i8) -> f32 {
    f32::from(skew_degrees.unsigned_abs()).to_radians().tan()
}

/// What a glyph's bitmap may add around its outline's bounds: the snap of
/// its origin to a pixel, hinting, and the coverage of partly covered edge
/// pixels, in logical pixels.
const GLYPH_INK_MARGIN: f32 = 2.0;

/// The union of every glyph's ink box, relative to the paragraph's origin.
#[derive(Default)]
struct InkBounds {
    rect: Option<Rect<f64>>,
    /// A glyph's face had no bounds to take its ink from.
    unbounded: bool,
}

impl InkBounds {
    /// Adds a glyph of a face with `em` bounds, its pen at `x` on a baseline
    /// at `y`, at `size` px.
    ///
    /// A glyph's outline lies within its face's `head` bounds (the union of
    /// every glyph in the face), scaled to its size and placed at its pen
    /// position. An oblique grows it by the skew over the taller of ascent
    /// and descent, a synthetic bold by the rasterizer's outline growth on
    /// each side at any scale (at most `size / 48`), and every glyph by
    /// [`GLYPH_INK_MARGIN`] plus a twentieth of an em for a variable face's
    /// instance reaching past its default one's bounds.
    fn add(&mut self, em: Option<[f32; 4]>, x: f32, y: f32, size: f32, synthesis: Synthesis) {
        let Some([x_min, y_min, x_max, y_max]) = em else {
            self.unbounded = true;
            return;
        };
        let slant = skew_slant(synthesis.skew_degrees) * y_max.abs().max(y_min.abs()) * size;
        // `fake_bold_width` over two is at most `size / 48` per side at any
        // device scale: its ratio peaks at 1/24 for small sizes.
        let bold = if synthesis.embolden {
            (fake_bold_width(size) / 2.0).max(size / 48.0)
        } else {
            0.0
        };
        let margin = GLYPH_INK_MARGIN + 0.05 * size + bold;
        let rect = Rect::from_ltrb(
            f64::from(x + x_min * size - slant - margin),
            f64::from(y - y_max * size - margin),
            f64::from(x + x_max * size + slant + margin),
            f64::from(y - y_min * size + margin),
        );
        self.rect = Some(self.rect.map_or(rect, |ink| ink.union(&rect)));
    }

    /// The box, `None` when a face gave no bounds; a paragraph with no glyph
    /// inks nothing.
    fn finish(self) -> Option<Rect<f64>> {
        if self.unbounded {
            return None;
        }
        Some(self.rect.unwrap_or(Rect::from_ltrb(0.0, 0.0, 0.0, 0.0)))
    }
}

/// One span's line in [`ShapedParagraph::describe_spans`]: the text a
/// snapshot prints.
fn describe(span: &SpanInfo, root: Option<Color>) -> String {
    let mut parts = vec![
        format!("len={}", span.len),
        format!("family={}", span.family),
        format!("weight={}", span.weight),
        format!("style={}", if span.italic { "Italic" } else { "Normal" }),
    ];
    if let Some(size) = span.size {
        parts.push(format!("size={size:.2}"));
    }
    if let Some(color) = span.color.filter(|color| Some(*color) != root) {
        parts.push(format!(
            "color=#{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a
        ));
    }
    parts.join(" ")
}

/// The colour a span paints with, carried through shaping per glyph run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SpanBrush(Option<Color>);

/// A layout and what it was shaped from.
struct Shaped {
    layout: Layout<SpanBrush>,
    text: String,
    spans: Vec<SpanInfo>,
}

/// The characters Parley ends a line at.
pub(super) fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{2028}' | '\u{2029}'
    )
}

impl TextContext {
    /// Shapes and line-breaks `paragraph` on this context's fonts.
    pub fn shape(&mut self, paragraph: &ParagraphSpec<'_>) -> ParagraphLayout {
        debug_assert!(
            paragraph.font_size > 0.0 && paragraph.font_size.is_finite(),
            "ParagraphSpec font_size must be positive and finite, got {}",
            paragraph.font_size
        );
        let max_lines = paragraph.max_lines.filter(|&lines| lines > 0);
        let mut shaped = self.shape_spans(paragraph, paragraph.spans);
        let mut ellipsized = false;
        let mut kept_text = shaped.text.len();
        if let (Some(max_lines), Some(ellipsis)) =
            (max_lines, paragraph.ellipsis.filter(|e| !e.is_empty()))
            && shaped.layout.len() > max_lines
        {
            shaped = self.ellipsize(paragraph, &shaped, max_lines, ellipsis);
            ellipsized = true;
            kept_text = shaped.text.len() - ellipsis.len();
        } else if let Some(max_lines) = max_lines
            && let Some(last) = shaped.layout.get(max_lines - 1)
            && shaped.layout.len() > max_lines
        {
            kept_text = last.text_range().end;
            while let Some(c) = shaped.text[..kept_text].chars().next_back()
                && is_hard_break(c)
            {
                kept_text -= c.len_utf8();
            }
        }
        let kept = max_lines.map_or(shaped.layout.len(), |max| max.min(shaped.layout.len()));
        let content_width = shaped
            .layout
            .lines()
            .take(kept)
            .fold(0.0_f32, |width, line| {
                let metrics = line.metrics();
                width.max(metrics.inline_min_coord + metrics.advance - metrics.trailing_whitespace)
            });
        let alignment_width = paragraph.min_width.max(content_width);
        let widths = shaped.layout.calculate_content_widths();
        let content_widths = (f64::from(widths.min), f64::from(widths.max));
        align_allocated_lines(&mut shaped.layout, paragraph, alignment_width);
        ParagraphLayout {
            layout: shaped.layout,
            text: shaped.text,
            spans: shaped.spans,
            line_height: paragraph.line_height.unwrap_or(paragraph.font_size * 1.2),
            max_lines,
            alignment_width,
            content_widths,
            ellipsized,
            kept_text,
            placed: OnceLock::new(),
        }
    }

    /// `shaped` cut after its `max_lines`-th line with `ellipsis` appended,
    /// dropping text from the end of that line until the line with the
    /// ellipsis fits the width and no line is added.
    ///
    /// The first cut is a guess from the line's cluster advances; each check
    /// that fails drops one more character, so the worst case shapes once
    /// per character of the last kept line.
    fn ellipsize(
        &mut self,
        paragraph: &ParagraphSpec<'_>,
        shaped: &Shaped,
        max_lines: usize,
        ellipsis: &str,
    ) -> Shaped {
        let full = &shaped.text;
        let Some(line) = shaped.layout.get(max_lines - 1) else {
            return self.shape_spans(paragraph, paragraph.spans);
        };
        let line_start = line.text_range().start;
        let mut cut = line.text_range().end;
        if let Some(max_width) = paragraph.max_width {
            let ellipsis_width = {
                let style = sliced_spans(paragraph.spans, cut)
                    .last()
                    .and_then(|(_, style)| style.clone());
                let spans = vec![(ellipsis.to_owned(), style)];
                self.shape_spans(paragraph, &spans).layout.width()
            };
            // Clusters in logical order: the advance of a logical prefix is
            // its width whatever its visual order.
            let mut clusters: Vec<(std::ops::Range<usize>, f32)> = line
                .runs()
                .flat_map(|run| {
                    run.clusters()
                        .map(|cluster| (cluster.text_range(), cluster.advance()))
                        .collect::<Vec<_>>()
                })
                .collect();
            clusters.sort_by_key(|(range, _)| range.start);
            let mut width = 0.0;
            let mut fit = line_start;
            for (range, advance) in clusters {
                width += advance;
                if width + ellipsis_width > max_width {
                    break;
                }
                fit = range.end;
            }
            cut = fit.clamp(line_start, cut);
        }
        // The break that ended the line stays out, or the ellipsis would
        // start a line of its own.
        while let Some(c) = full[..cut].chars().next_back()
            && is_hard_break(c)
        {
            cut -= c.len_utf8();
        }
        loop {
            let mut spans = sliced_spans(paragraph.spans, cut);
            let style = spans
                .last()
                .or(paragraph.spans.first())
                .and_then(|(_, style)| style.clone());
            spans.push((ellipsis.to_owned(), style));
            let candidate = self.shape_spans(paragraph, &spans);
            let fits = candidate.layout.len() <= max_lines
                && paragraph.max_width.is_none_or(|max_width| {
                    candidate.layout.lines().last().is_none_or(|line| {
                        let metrics = line.metrics();
                        metrics.advance - metrics.trailing_whitespace <= max_width + 1e-3
                    })
                });
            let Some((previous, _)) = full[..cut].char_indices().next_back() else {
                // Only the ellipsis is left: it is kept even if it does not
                // fit, as nothing narrower can stand for the dropped text.
                return candidate;
            };
            if fits {
                return candidate;
            }
            cut = previous;
        }
    }

    /// Shapes `spans` with `paragraph`'s other settings.
    fn shape_spans(
        &mut self,
        paragraph: &ParagraphSpec<'_>,
        spans: &[(String, Option<TextStyle>)],
    ) -> Shaped {
        let text: String = spans.iter().map(|(text, _)| text.as_str()).collect();
        let breaks = one_break_per_crlf(&text);

        // Families are resolved against the collection before the builder
        // borrows it (`resolve_family_name`).
        let collection = &mut self.font_cx.collection;
        let (default_family, default_family_name) = family(collection, None);
        let default_properties = paragraph
            .default_style
            .map(|style| properties(collection, style, None, paragraph.font_weight_adjustment))
            .unwrap_or_default();
        let default_info = span_info(
            0,
            default_family_name,
            paragraph.default_style,
            None,
            paragraph.font_weight_adjustment,
        );
        let mut start = 0;
        let mut infos = Vec::with_capacity(spans.len());
        let span_properties: Vec<_> = spans
            .iter()
            .map(|(span, style)| {
                let range = start..start + span.len();
                start = range.end;
                let properties = style
                    .as_ref()
                    .map(|style| {
                        properties(
                            collection,
                            style,
                            paragraph.default_style,
                            paragraph.font_weight_adjustment,
                        )
                    })
                    .unwrap_or_default();
                infos.push(match style {
                    Some(style) => {
                        let (_, name) = family(collection, Some(style));
                        span_info(
                            span.len(),
                            name,
                            Some(style),
                            paragraph.default_style,
                            paragraph.font_weight_adjustment,
                        )
                    }
                    None => SpanInfo {
                        len: span.len(),
                        ..default_info.clone()
                    },
                });
                (range, properties)
            })
            .collect();

        // Unquantized: the layout is in logical pixels, and Parley's
        // quantization would round ascent, descent and the leading halves to
        // whole logical pixels, which at a device scale other than 1 is not
        // the device grid. Metrics stay exact, and a baseline reaches the
        // device grid once, when it is placed (`(baseline * scale).round()`);
        // `tests/parley_metrics_oracle.rs` pins the agreement.
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, &breaks, 1.0, false);
        builder.push_default(default_family);
        builder.push_default(StyleProperty::FontSize(paragraph.font_size));
        if let Some(weight) = adjusted_weight(
            paragraph.default_style,
            None,
            paragraph.font_weight_adjustment,
        ) {
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        }
        // A word wider than the line breaks between its glyphs instead of
        // overflowing the line. `BreakWord` rather than `Anywhere`: the
        // min-content width stays the widest word (flui-painting
        // `ARCHITECTURE.md`, mapping decision 15).
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::BreakWord));
        // No explicit height is 1.2 em of each run's own size, so a larger
        // span grows its line box; an explicit height is one absolute line
        // box for the paragraph.
        builder.push_default(StyleProperty::LineHeight(match paragraph.line_height {
            Some(height) => LineHeight::Absolute(height),
            None => LineHeight::FontSizeRelative(1.2),
        }));
        for property in default_properties {
            builder.push_default(property);
        }
        for (range, properties) in span_properties {
            for property in properties {
                builder.push(property, range.clone());
            }
        }
        let mut layout = builder.build(&breaks);
        layout.break_all_lines(paragraph.max_width);
        Shaped {
            layout,
            text,
            spans: infos,
        }
    }
}

/// `text` as Parley is handed it: a CR directly before an LF becomes a
/// space, so CR LF breaks the line once, as a lone LF does, where Parley
/// breaks at the CR and again at the LF. The space is the CR's one byte, so
/// every byte offset into the layout (clusters, span lengths, the ellipsis
/// cut) still indexes `text`; as trailing whitespace it adds nothing to the
/// line's width.
fn one_break_per_crlf(text: &str) -> Cow<'_, str> {
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", " \n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// The spans covering `text[..cut]`: whole spans before the cut, the span
/// holding it sliced there. `cut` is a char boundary of the concatenation.
fn sliced_spans(
    spans: &[(String, Option<TextStyle>)],
    cut: usize,
) -> Vec<(String, Option<TextStyle>)> {
    let mut out = Vec::new();
    let mut base = 0;
    for (text, style) in spans {
        if cut <= base {
            break;
        }
        let end = base + text.len();
        if cut >= end {
            out.push((text.clone(), style.clone()));
        } else {
            out.push((text[..cut - base].to_owned(), style.clone()));
            break;
        }
        base = end;
    }
    out
}

/// What a span of `len` bytes styled `style` in `family` was shaped with.
fn span_info(
    len: usize,
    family: String,
    style: Option<&TextStyle>,
    default: Option<&TextStyle>,
    adjustment: i32,
) -> SpanInfo {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a style's f64 size narrows to the f32 it is shaped at"
    )]
    SpanInfo {
        len,
        family,
        weight: adjusted_weight(style, default, adjustment).unwrap_or_else(|| {
            style
                .and_then(|style| style.font_weight)
                .map_or(400.0, |weight| f32::from(weight.value()))
        }),
        italic: style
            .and_then(|style| style.font_style)
            .is_some_and(|font_style| font_style == FontStyle::Italic),
        size: style
            .and_then(|style| style.font_size)
            .map(|size| size as f32),
        color: style.and_then(paint_color),
    }
}

/// The one family a style is shaped with, and its name as a snapshot prints
/// it: FLUI's family rule (`resolve_family_name`) over the families
/// `collection` holds, so an absent name never reaches Parley's fallback
/// walk (flui-painting `ARCHITECTURE.md`, mapping decision 8). Nothing follows it in the list: past that family,
/// Parley walks the collection's fallback families, which a collection fed
/// from the host takes from FLUI's lists for it
/// (`FontCollection::with_host_fonts`).
fn family(
    collection: &mut Collection,
    style: Option<&TextStyle>,
) -> (StyleProperty<'static, SpanBrush>, String) {
    let family = resolve_family_name(style, |name| holds_exactly(collection, name));
    let described = format!("{family:?}");
    let name = match family {
        Family::Name(name) => FontFamilyName::Named(Cow::Owned(name.to_owned())),
        Family::Serif => FontFamilyName::Generic(GenericFamily::Serif),
        Family::SansSerif => FontFamilyName::Generic(GenericFamily::SansSerif),
        Family::Cursive => FontFamilyName::Generic(GenericFamily::Cursive),
        Family::Fantasy => FontFamilyName::Generic(GenericFamily::Fantasy),
        Family::Monospace => FontFamilyName::Generic(GenericFamily::Monospace),
    };
    (
        StyleProperty::FontFamily(FontFamily::Single(name)),
        described,
    )
}

/// Whether `collection` holds a family spelled exactly `name`.
///
/// fontique looks family names up without regard to case; the rule asks for
/// the exact spelling, as fontdb matches names, so a
/// style naming `"segoe ui"` degrades to the sans-serif generic rather than
/// shaping in Segoe UI on one host and in the generic's family on another.
pub(crate) fn holds_exactly(collection: &mut Collection, name: &str) -> bool {
    collection
        .family_id(name)
        .and_then(|id| collection.family_name(id))
        .is_some_and(|held| held == name)
}

/// One resolved request for both family matching and the variable weight axis.
/// Zero deliberately keeps the existing authored matching/axis behavior.
#[expect(
    clippy::cast_possible_truncation,
    reason = "valid axes use shaping f32; the adjusted result is bounded to 1..1000"
)]
fn adjusted_weight(
    style: Option<&TextStyle>,
    default: Option<&TextStyle>,
    adjustment: i32,
) -> Option<f32> {
    if adjustment == 0 {
        return None;
    }
    let variations = style
        .filter(|style| {
            style.font_variations.iter().any(|variation| {
                Tag::parse(&variation.axis).is_some() && (variation.value as f32).is_finite()
            })
        })
        .or(default);
    let axis = variations.and_then(|style| {
        style.font_variations.iter().rev().find_map(|variation| {
            let value = variation.value as f32;
            (variation.axis == "wght" && value.is_finite()).then_some(f64::from(value))
        })
    });
    let authored = axis.unwrap_or_else(|| {
        style
            .and_then(|style| style.font_weight)
            .or_else(|| default.and_then(|style| style.font_weight))
            .map_or(400.0, |weight| f64::from(weight.value()))
    });
    Some((authored + f64::from(adjustment)).clamp(1.0, 1000.0) as f32)
}

/// The Parley properties `style` sets, with a resolved weight adjustment.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64 style values narrow to Parley's f32 layout space"
)]
fn properties(
    collection: &mut Collection,
    style: &TextStyle,
    default: Option<&TextStyle>,
    adjustment: i32,
) -> Vec<StyleProperty<'static, SpanBrush>> {
    let mut properties = Vec::new();
    let adjusted = adjusted_weight(Some(style), default, adjustment);
    if style.font_family.is_some() {
        properties.push(family(collection, Some(style)).0);
    }
    if let Some(weight) = adjusted {
        properties.push(StyleProperty::FontWeight(FontWeight::new(weight)));
    } else if let Some(weight) = style.font_weight {
        properties.push(StyleProperty::FontWeight(FontWeight::new(f32::from(
            weight.value(),
        ))));
    }
    if let Some(font_style) = style.font_style {
        properties.push(StyleProperty::FontStyle(match font_style {
            FontStyle::Normal => ParleyFontStyle::Normal,
            FontStyle::Italic => ParleyFontStyle::Italic,
        }));
    }
    if let Some(size) = style.font_size {
        properties.push(StyleProperty::FontSize(size as f32));
    }
    if let Some(spacing) = style.letter_spacing {
        properties.push(StyleProperty::LetterSpacing(spacing as f32));
    }
    if let Some(spacing) = style.word_spacing {
        properties.push(StyleProperty::WordSpacing(spacing as f32));
    }
    let features: Vec<_> = style
        .font_features
        .iter()
        .filter_map(|feature| {
            Some(FontFeature::new(
                Tag::parse(&feature.feature)?,
                u16::try_from(feature.value).ok()?,
            ))
        })
        .collect();
    if !features.is_empty() {
        properties.push(StyleProperty::FontFeatures(FontFeatures::List(Cow::Owned(
            features,
        ))));
    }
    let variations: Vec<_> = style
        .font_variations
        .iter()
        .filter_map(|variation| {
            let value = variation.value as f32;
            let value = if variation.axis == "wght" && value.is_finite() {
                adjusted.unwrap_or(value)
            } else {
                value
            };
            value
                .is_finite()
                .then_some(FontVariation::new(Tag::parse(&variation.axis)?, value))
        })
        .collect();
    if !variations.is_empty() {
        properties.push(StyleProperty::FontVariations(FontVariations::List(
            Cow::Owned(variations),
        )));
    }
    if let Some(height) = style.height {
        properties.push(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            height as f32,
        )));
    }
    if let Some(color) = paint_color(style) {
        properties.push(StyleProperty::Brush(SpanBrush(Some(color))));
    }
    properties
}

#[cfg(test)]
mod tests {
    use crate::typography::{TextDirection, TextStyle};

    use super::{ParagraphLayout, ParagraphSpec};
    use crate::text_layout::{FontCollection, TextContext};

    const WIDTH: f32 = 400.0;

    fn shaped(text: &str, direction: TextDirection) -> ParagraphLayout {
        let spans: Vec<(String, Option<TextStyle>)> = vec![(text.to_owned(), None)];
        TextContext::new(&FontCollection::new()).shape(&ParagraphSpec {
            font_weight_adjustment: 0,
            spans: &spans,
            default_style: None,
            font_size: 16.0,
            max_width: Some(WIDTH),
            min_width: WIDTH,
            text_align: crate::typography::TextAlign::Start,
            line_height: None,
            direction,
            max_lines: None,
            ellipsis: None,
        })
    }

    fn first_line_offset(paragraph: &ParagraphLayout) -> f32 {
        paragraph
            .layout
            .get(0)
            .expect("BUG: non-empty text lays out at least one line")
            .metrics()
            .offset
    }

    /// `Rtl` pushes a line to the right edge and nothing more: Parley 0.11
    /// resolves the base direction from the first strong character, so a
    /// Latin-first paragraph stays LTR under `Rtl` and a Hebrew-first one is
    /// RTL under `Ltr`. SkParagraph would order the first as an RTL
    /// paragraph; this pins the divergence until the base direction can be
    /// set (ADR-0092 §10 step 5).
    #[test]
    fn rtl_aligns_lines_right_without_setting_the_base_direction() {
        let ltr = shaped("abc 123 !", TextDirection::Ltr);
        let rtl = shaped("abc 123 !", TextDirection::Rtl);

        assert!(
            first_line_offset(&ltr).abs() < f32::EPSILON,
            "an Ltr line starts at the left edge, got {}",
            first_line_offset(&ltr)
        );
        assert!(
            first_line_offset(&rtl) > 0.0,
            "an Rtl line is pushed toward the right edge, got {}",
            first_line_offset(&rtl)
        );
        assert!(
            !rtl.layout.is_rtl(),
            "Latin-first text keeps an LTR base direction under Rtl"
        );
        assert!(
            shaped("\u{05E9}\u{05DC}\u{05D5}\u{05DD} abc", TextDirection::Ltr)
                .layout
                .is_rtl(),
            "Hebrew-first text takes an RTL base direction under Ltr"
        );
    }

    /// On a collection holding only the bundled faces, a glyph the style's
    /// family lacks measures in Roboto, the collection's last fallback, not as
    /// `.notdef`: Cyrillic in Material Icons measures exactly as Cyrillic in
    /// Roboto.
    #[test]
    fn a_glyph_the_named_family_lacks_measures_in_roboto_on_the_bundled_collection() {
        let fonts = FontCollection::new();
        let width = |family: &str| {
            let style = TextStyle {
                font_family: Some(family.to_owned()),
                ..TextStyle::default()
            };
            let spans: Vec<(String, Option<TextStyle>)> = vec![("Привет".to_owned(), Some(style))];
            TextContext::new(&fonts)
                .shape(&ParagraphSpec {
                    font_weight_adjustment: 0,
                    spans: &spans,
                    default_style: None,
                    font_size: 16.0,
                    max_width: None,
                    min_width: 0.0,
                    text_align: crate::typography::TextAlign::Start,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                    ellipsis: None,
                })
                .metrics()
                .width
        };
        let roboto = width("Roboto");
        let icons = width("Material Icons");
        assert!(
            (icons - roboto).abs() < 1e-3,
            "Cyrillic in Material Icons measures {icons}, in Roboto {roboto}"
        );
    }

    fn wrapped(max_lines: Option<usize>) -> ParagraphLayout {
        let spans: Vec<(String, Option<TextStyle>)> = vec![(
            "one two three four five six seven eight nine ten".to_owned(),
            None,
        )];
        TextContext::new(&FontCollection::new()).shape(&ParagraphSpec {
            font_weight_adjustment: 0,
            spans: &spans,
            default_style: None,
            font_size: 16.0,
            max_width: Some(60.0),
            min_width: 0.0,
            text_align: crate::typography::TextAlign::Start,
            line_height: Some(20.0),
            direction: TextDirection::Ltr,
            max_lines,
            ellipsis: None,
        })
    }

    /// `max_lines` stops the metrics at the kept lines: the height covers
    /// only them and `truncated` says lines were dropped. Without it the
    /// same paragraph reports every line.
    #[test]
    fn max_lines_truncates_parley_metrics() {
        let full = wrapped(None).metrics();
        assert!(
            full.line_count > 2,
            "the probe must wrap past two lines, got {}",
            full.line_count
        );
        assert!(!full.truncated);

        let kept = wrapped(Some(2)).metrics();
        assert_eq!(kept.line_count, 2);
        assert!(kept.truncated, "dropping lines reports truncation");
        assert!(
            (kept.height - 40.0).abs() < 1e-3,
            "two 20 px lines, got {}",
            kept.height
        );
        assert!(kept.height < full.height);
        assert!(kept.width <= 60.0 + 1e-3);

        let exact = wrapped(Some(full.line_count)).metrics();
        assert!(!exact.truncated, "keeping every line is no truncation");
        assert!((exact.height - full.height).abs() < 1e-3);
    }

    /// The content widths are the widest word and the single-line width,
    /// whatever width the paragraph was broken at.
    #[test]
    fn content_widths_bound_every_break_width() {
        let paragraph = wrapped(None);
        let (min, max) = paragraph.content_widths();
        assert!(min > 0.0 && min < max, "min {min}, max {max}");
        let spans: Vec<(String, Option<TextStyle>)> = vec![("seven".to_owned(), None)];
        let seven = TextContext::new(&FontCollection::new())
            .shape(&ParagraphSpec {
                font_weight_adjustment: 0,
                spans: &spans,
                default_style: None,
                font_size: 16.0,
                max_width: None,
                min_width: 0.0,
                text_align: crate::typography::TextAlign::Start,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .metrics()
            .width;
        assert!(
            min >= seven - 1e-3,
            "min {min} covers the word 'seven' ({seven})"
        );
    }
}
