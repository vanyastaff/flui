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
use crate::display_list::paragraph::{RunData, SpanDescription};
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
use crate::text_layout::{TextContext, TextLayoutError, TextLayoutResult, paint_color};

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
    /// The kept lines by run and grapheme, for `TextPainter::line_layouts`;
    /// built on the first query.
    pub(super) line_layouts: OnceLock<Vec<crate::typography::TextLineLayout>>,
    /// The colour each paint run paints with, by the run's index in its
    /// glyph runs' [`SpanBrush`]: a colour is not shaped, so the layout keeps
    /// it beside the runs and [`recolor`](Self::recolor) replaces it.
    run_colors: Box<[Option<Color>]>,
    /// How many spans the paragraph was shaped from: the length a
    /// recolour's colours must have.
    source_spans: usize,
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
    family: std::sync::Arc<str>,
    weight: f32,
    italic: bool,
    size: Option<f32>,
    /// The paragraph span this one stands for: itself, or for an ellipsis
    /// the span whose style it takes.
    source: usize,
    /// The paint run it paints in; `None` for an empty span, which paints
    /// nothing.
    paint_run: Option<usize>,
}

impl SpanInfo {
    /// What [`ShapedParagraph::describe_spans`] prints for this span, with
    /// `color` where its run paints in another colour than the paragraph
    /// command's.
    fn description(&self, color: Option<Color>) -> SpanDescription {
        SpanDescription {
            len: self.len,
            family: std::sync::Arc::clone(&self.family),
            weight: self.weight,
            italic: self.italic,
            size: self.size,
            color,
        }
    }
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

    /// [`Self::content_widths`] of the text as its spans give it, or `None`
    /// when the paragraph was ellipsized: its shaped text is then the kept
    /// prefix and the ellipsis, not the spans.
    ///
    /// Shaping does not depend on the width the lines break at nor on
    /// `max_lines`, so a paragraph that was not ellipsized answers what a
    /// separate unconstrained shaping of the same spans would.
    #[must_use]
    pub(crate) fn authored_content_widths(&self) -> Option<(f64, f64)> {
        (!self.ellipsized).then(|| self.content_widths())
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
        self.shaped_with(root, &self.run_colors, None)
    }

    /// The glyphs of the paragraph spans `casts` names, as the shadow they
    /// cast draws them (ADR-0309 §3): every run colour cleared. A glyph
    /// belongs to the span its cluster starts in, so spans that share a
    /// Parley run (they differ only in paint) still cast apart. `None` when
    /// no kept glyph is cast.
    pub(crate) fn silhouette(&self, casts: &dyn Fn(usize) -> bool) -> Option<ShapedParagraph> {
        let paragraph = self.shaped_with(None, &[], Some(casts));
        (!paragraph.glyphs.is_empty()).then_some(paragraph)
    }

    /// The span backgrounds over the kept lines, the ellipsis included: one
    /// rectangle per stretch of adjacent clusters on one line whose
    /// paragraph span `fill` gives the same colour, as tall as the line box
    /// (ADR-0309 §2). A hard break paints nothing.
    pub(crate) fn backgrounds(
        &self,
        fill: &dyn Fn(usize) -> Option<Color>,
    ) -> Vec<(Rect<f64>, Color)> {
        let sources = SpanSourceMap::of(&self.spans);
        let mut out: Vec<(Rect<f64>, Color)> = Vec::new();
        for line in self.layout.lines().take(self.kept()) {
            let metrics = line.metrics();
            let (top, bottom) = (metrics.block_min_coord, metrics.block_max_coord);
            let mut x = metrics.offset + metrics.inline_min_coord;
            let mut open: Option<(f32, f32, Color)> = None;
            let mut close = |open: &mut Option<(f32, f32, Color)>| {
                if let Some((left, right, color)) = open.take() {
                    out.push((
                        Rect::from_ltrb(
                            f64::from(left),
                            f64::from(top),
                            f64::from(right),
                            f64::from(bottom),
                        ),
                        color,
                    ));
                }
            };
            for run in line.runs() {
                for cluster in run.visual_clusters() {
                    let (left, right) = (x, x + cluster.advance());
                    x = right;
                    let color = (!cluster.is_hard_line_break())
                        .then(|| sources.at(cluster.text_range().start))
                        .flatten()
                        .and_then(fill);
                    match (&mut open, color) {
                        (Some((_, open_right, open_color)), Some(color))
                            if *open_color == color && (*open_right - left).abs() < 1e-3 =>
                        {
                            *open_right = right;
                        }
                        (_, Some(color)) => {
                            close(&mut open);
                            open = Some((left, right, color));
                        }
                        (_, None) => close(&mut open),
                    }
                }
            }
            close(&mut open);
        }
        out
    }

    /// The stretches of the kept lines a decoration is drawn along
    /// (ADR-0310 §3): adjacent clusters on one line that `decorate` gives
    /// the same value, joined into one stretch, with the line's trailing
    /// whitespace and hard breaks left out. A stretch takes the decoration
    /// metrics of its largest run, so a fallback face inside decorated text
    /// does not step the line.
    pub(crate) fn decorated_stretches<V: Copy + PartialEq>(
        &self,
        decorate: &dyn Fn(ClusterSite) -> Option<V>,
    ) -> Vec<DecoratedStretch<V>> {
        let sources = SpanSourceMap::of(&self.spans);
        let mut out: Vec<DecoratedStretch<V>> = Vec::new();
        for line in self.layout.lines().take(self.kept()) {
            let metrics = line.metrics();
            let baseline = f64::from(metrics.baseline);
            let trailing = self.trailing_whitespace_start(line.text_range());
            let mut x = metrics.offset + metrics.inline_min_coord;
            let mut open: Option<(DecoratedStretch<V>, f32)> = None;
            for run in line.runs() {
                let size = run.font_size();
                let run_metrics = DecorationMetrics::of(run.metrics(), size);
                for cluster in run.visual_clusters() {
                    let (left, right) = (x, x + cluster.advance());
                    x = right;
                    let start = cluster.text_range().start;
                    let value = (start < trailing)
                        .then(|| sources.info_at(start))
                        .flatten()
                        .and_then(|info| {
                            let span = self.spans.get(info)?;
                            decorate(ClusterSite {
                                span: span.source,
                                text_byte: (start < self.kept_text).then_some(start),
                                glyph: span
                                    .paint_run
                                    .and_then(|run| self.run_colors.get(run).copied().flatten()),
                            })
                        });
                    match (&mut open, value) {
                        (Some((stretch, largest)), Some(value))
                            if stretch.value == value
                                && (stretch.right - f64::from(left)).abs() < 1e-3 =>
                        {
                            stretch.right = f64::from(right);
                            if size > *largest {
                                *largest = size;
                                stretch.metrics = run_metrics;
                            }
                        }
                        (_, Some(value)) => {
                            out.extend(open.take().map(|(stretch, _)| stretch));
                            open = Some((
                                DecoratedStretch {
                                    left: f64::from(left),
                                    right: f64::from(right),
                                    baseline,
                                    metrics: run_metrics,
                                    value,
                                },
                                size,
                            ));
                        }
                        (_, None) => out.extend(open.take().map(|(stretch, _)| stretch)),
                    }
                }
            }
            out.extend(open.take().map(|(stretch, _)| stretch));
        }
        out
    }

    /// Where the whitespace a line ends with starts, in the shaped text: a
    /// decoration is not drawn under the spaces and break a line hangs past
    /// its end. No-break spaces are not trailing whitespace.
    fn trailing_whitespace_start(&self, line: std::ops::Range<usize>) -> usize {
        let Some(text) = self.text.get(line.clone()) else {
            return line.end;
        };
        let kept = text.trim_end_matches(|c: char| {
            c.is_whitespace() && !matches!(c, '\u{00A0}' | '\u{2007}' | '\u{202F}')
        });
        line.start + kept.len()
    }

    /// The paint runs' colours when the paragraph's spans, in the order it
    /// was shaped from, paint `colors`, or `None` when they do not fit this
    /// layout: a different number of spans, or two spans that share a paint
    /// run given different colours, which only a new layout can split.
    fn recolored_runs(&self, colors: &[Option<Color>]) -> Option<Box<[Option<Color>]>> {
        if colors.len() != self.source_spans {
            return None;
        }
        let mut runs: Vec<Option<Option<Color>>> = vec![None; self.run_colors.len()];
        for span in &self.spans {
            let Some(run) = span.paint_run else {
                continue;
            };
            let color = *colors.get(span.source)?;
            let slot = runs.get_mut(run)?;
            match slot {
                Some(set) if *set != color => return None,
                Some(_) => {}
                None => *slot = Some(color),
            }
        }
        runs.into_iter()
            .collect::<Option<Vec<Option<Color>>>>()
            .map(Vec::into_boxed_slice)
    }

    /// Gives the paragraph's spans the colours `colors` (one per span it
    /// was shaped from, as [`span_paint_color`] resolves them) without
    /// shaping, and returns the paragraph painted with them over `root`.
    ///
    /// `None`, and the layout unchanged, when the colours do not fit its
    /// paint runs ([`recolored_runs`](Self::recolored_runs)).
    pub(crate) fn recolor(
        &mut self,
        root: Option<Color>,
        colors: &[Option<Color>],
    ) -> Option<ShapedParagraph> {
        let runs = self.recolored_runs(colors)?;
        let paragraph = self.shaped_with(root, &runs, None);
        self.run_colors = runs;
        Some(paragraph)
    }

    /// [`recolor`](Self::recolor) without building the paragraph, for a
    /// recolour the paragraph already painted keeps (one that changes only
    /// spans painted in the root's colour, with the root). `false`, and the
    /// layout unchanged, when the colours do not fit its paint runs.
    pub(crate) fn recolor_runs(&mut self, colors: &[Option<Color>]) -> bool {
        let Some(runs) = self.recolored_runs(colors) else {
            return false;
        };
        self.run_colors = runs;
        true
    }

    /// The paragraph with its paint runs coloured `runs`; with `casts`, only
    /// the glyphs whose cluster starts in a paragraph span it names.
    fn shaped_with(
        &self,
        root: Option<Color>,
        runs: &[Option<Color>],
        casts: Option<&dyn Fn(usize) -> bool>,
    ) -> ShapedParagraph {
        let run_color = |run: usize| {
            runs.get(run)
                .copied()
                .flatten()
                .filter(|color| Some(*color) != root)
        };
        let sources = casts.map(|_| SpanSourceMap::of(&self.spans));
        let metrics = self.metrics();
        let mut faces: Vec<FontFace> = Vec::new();
        let mut face_index: HashMap<(u64, u32), u32> = HashMap::new();
        let mut em_bounds: Vec<Option<[f32; 4]>> = Vec::new();
        let mut runs = Vec::new();
        let mut glyphs = Vec::new();
        let mut coords = Vec::new();
        let mut baselines = Vec::new();
        let mut ink = InkBounds::default();
        // For `casts`: the line item the glyph runs come from, the byte its
        // clusters start at for each of its glyphs in glyph order, and how
        // many of those the item's earlier glyph runs took.
        let mut item: Option<(usize, std::ops::Range<usize>)> = None;
        let mut starts: Vec<usize> = Vec::new();
        let mut taken = 0_usize;
        for (line_index, line) in self.layout.lines().take(self.kept()).enumerate() {
            let line_metrics = line.metrics();
            baselines.push(line_metrics.baseline);
            for line_item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = line_item else {
                    continue;
                };
                let run = glyph_run.run();
                let first = if casts.is_some() {
                    let key = (line_index, run.text_range());
                    if item.as_ref() != Some(&key) {
                        starts.clear();
                        starts.extend(run.visual_clusters().flat_map(|cluster| {
                            let start = cluster.text_range().start;
                            cluster.glyphs().map(move |_| start)
                        }));
                        taken = 0;
                        item = Some(key);
                    }
                    let first = taken;
                    taken += glyph_run.glyphs().count();
                    first
                } else {
                    0
                };
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
                for (index, glyph) in glyph_run.positioned_glyphs().enumerate() {
                    if let (Some(casts), Some(sources)) = (casts, sources.as_ref()) {
                        let cast = starts
                            .get(first + index)
                            .and_then(|&start| sources.at(start))
                            .is_some_and(casts);
                        if !cast {
                            continue;
                        }
                    }
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
                if casts.is_some() && glyphs.len() == glyph_start {
                    // Nothing of this run casts the shadow.
                    coords.truncate(coords_start);
                    continue;
                }
                runs.push(RunData {
                    face,
                    font_size,
                    coords: range32(coords_start, coords.len()),
                    synthesis,
                    color: run_color(glyph_run.style().brush.0),
                    baseline,
                    glyphs: range32(glyph_start, glyphs.len()),
                });
            }
        }
        let spans = self
            .spans
            .iter()
            .map(|span| span.description(span.paint_run.and_then(run_color)))
            .collect::<Box<[SpanDescription]>>();
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

/// The paragraph span a byte of the shaped text belongs to: the shaped
/// spans' ends, in order, and the paragraph span each stands for (an
/// ellipsis stands for the span whose style it takes).
struct SpanSourceMap {
    ends: Vec<usize>,
    sources: Vec<usize>,
}

impl SpanSourceMap {
    fn of(spans: &[SpanInfo]) -> Self {
        let mut end = 0_usize;
        let ends = spans
            .iter()
            .map(|span| {
                end = end.saturating_add(span.len);
                end
            })
            .collect::<Vec<usize>>();
        let sources = spans.iter().map(|span| span.source).collect::<Vec<usize>>();
        Self { ends, sources }
    }

    /// The paragraph span byte `byte` of the shaped text belongs to.
    fn at(&self, byte: usize) -> Option<usize> {
        self.sources.get(self.info_at(byte)?).copied()
    }

    /// The shaped span byte `byte` of the shaped text belongs to, by its
    /// index in the layout's spans.
    fn info_at(&self, byte: usize) -> Option<usize> {
        let index = self.ends.partition_point(|&end| end <= byte);
        (index < self.ends.len()).then_some(index)
    }
}

/// A cluster a decoration may be drawn along, as
/// [`ParagraphLayout::decorated_stretches`] asks about it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterSite {
    /// The paragraph span the cluster belongs to; an ellipsis belongs to the
    /// span whose style it takes.
    pub(crate) span: usize,
    /// The cluster's first byte in the paragraph's text; `None` for the
    /// ellipsis, which is not in it.
    pub(crate) text_byte: Option<usize>,
    /// The colour the cluster's glyphs paint in; `None` paints the
    /// paragraph's root colour.
    pub(crate) glyph: Option<Color>,
}

/// One stretch of one line a decoration is drawn along, in the paragraph's
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DecoratedStretch<V> {
    pub(crate) left: f64,
    pub(crate) right: f64,
    /// The line's baseline.
    pub(crate) baseline: f64,
    pub(crate) metrics: DecorationMetrics,
    pub(crate) value: V,
}

/// Where a decoration line lies, from the baseline, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinePlacement {
    /// The line's top edge.
    pub(crate) top: f64,
    pub(crate) thickness: f64,
}

/// Where a run's font puts its decoration lines (ADR-0310 §2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DecorationMetrics {
    pub(crate) underline: LinePlacement,
    /// At the ascent, as thick as the underline.
    pub(crate) overline: LinePlacement,
    pub(crate) line_through: LinePlacement,
}

impl DecorationMetrics {
    /// The decoration lines of a run of `size` whose font reports `metrics`.
    ///
    /// The font's values are trusted only within an em of the baseline and
    /// with a thickness up to a quarter em: a font without them makes
    /// Parley substitute values in font units, not pixels. Outside that, an
    /// underline is an eighteenth of an em thick, that far below the
    /// baseline, and a line-through is centred half an x-height above it.
    pub(crate) fn of(metrics: &parley::layout::RunMetrics, size: f32) -> Self {
        let em = f64::from(size).max(0.0);
        let fallback = em / 18.0;
        let thickness = |size: f32| {
            let size = f64::from(size);
            (size.is_finite() && size > 0.0 && size <= em / 4.0).then_some(size)
        };
        // The font's offsets are y up, to the line's top edge.
        let top = |offset: f32, range: std::ops::RangeInclusive<f64>| {
            let offset = f64::from(offset);
            (offset.is_finite() && range.contains(&offset)).then_some(-offset)
        };
        let placed = |top: Option<f64>, thickness: Option<f64>| {
            Some(LinePlacement {
                top: top?,
                thickness: thickness?,
            })
        };
        let underline = placed(
            top(metrics.underline_offset, -em..=0.0),
            thickness(metrics.underline_size),
        )
        .unwrap_or(LinePlacement {
            top: fallback,
            thickness: fallback,
        });
        let line_through = placed(
            top(metrics.strikethrough_offset, 0.0..=em),
            thickness(metrics.strikethrough_size),
        )
        .unwrap_or_else(|| {
            let x_height = metrics
                .x_height
                .map(f64::from)
                .filter(|height| height.is_finite() && (0.0..=em).contains(height))
                .unwrap_or(em * 0.5);
            LinePlacement {
                top: -(x_height / 2.0) - underline.thickness / 2.0,
                thickness: underline.thickness,
            }
        });
        let ascent = f64::from(metrics.ascent);
        let ascent = if ascent.is_finite() && (0.0..=em * 2.0).contains(&ascent) {
            ascent
        } else {
            em * 0.8
        };
        Self {
            underline,
            overline: LinePlacement {
                top: -ascent,
                thickness: underline.thickness,
            },
            line_through,
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
        // Logical ink geometry is f64. Widen before arithmetic: finite
        // backend coordinates can overflow when adding an aligned pen and
        // the face's overhang in f32.
        let [x_min, y_min, x_max, y_max] = [x_min, y_min, x_max, y_max].map(f64::from);
        let logical_size = f64::from(size);
        let slant = f64::from(skew_slant(synthesis.skew_degrees))
            * y_max.abs().max(y_min.abs())
            * logical_size;
        // `fake_bold_width` over two is at most `size / 48` per side at any
        // device scale: its ratio peaks at 1/24 for small sizes.
        let bold = if synthesis.embolden {
            (f64::from(fake_bold_width(size)) / 2.0).max(logical_size / 48.0)
        } else {
            0.0
        };
        let margin = f64::from(GLYPH_INK_MARGIN) + 0.05 * logical_size + bold;
        let x = f64::from(x);
        let y = f64::from(y);
        let rect = Rect::from_ltrb(
            x + x_min * logical_size - slant - margin,
            y - y_max * logical_size - margin,
            x + x_max * logical_size + slant + margin,
            y - y_min * logical_size + margin,
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

/// The paint run a glyph run belongs to: an index into its layout's run
/// colours, not the colour itself, so recolouring a span changes no style
/// Parley shaped with. Parley splits shaping items only on font properties,
/// never on the brush, so the index splits glyph runs where colours differ
/// and shapes nothing differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpanBrush(usize);

/// The colour a span styled `style` paints with in a paragraph whose
/// default style is `default`: its own, or the default's where it sets
/// none. The one rule shaping and a recolour both use.
pub(crate) fn span_paint_color(
    style: Option<&TextStyle>,
    default: Option<&TextStyle>,
) -> Option<Color> {
    style
        .and_then(paint_color)
        .or_else(|| default.and_then(paint_color))
}

/// Which paragraph span each span handed to `shape_spans` stands for.
#[derive(Clone, Copy, Debug)]
enum SpanSources {
    /// The paragraph's own spans, in order.
    Own,
    /// The first `prefix` spans are the paragraph's own, cut at the
    /// ellipsis; the ellipsis after them takes the style of span
    /// `ellipsis`.
    Ellipsized { prefix: usize, ellipsis: usize },
}

impl SpanSources {
    fn source(self, index: usize) -> usize {
        match self {
            Self::Own => index,
            Self::Ellipsized { prefix, ellipsis } => {
                if index < prefix {
                    index
                } else {
                    ellipsis
                }
            }
        }
    }
}

/// A layout and what it was shaped from.
struct Shaped {
    layout: Layout<SpanBrush>,
    text: String,
    spans: Vec<SpanInfo>,
    run_colors: Vec<Option<Color>>,
}

/// The characters Parley ends a line at.
pub(super) fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{2028}' | '\u{2029}'
    )
}

fn validate_request(paragraph: &ParagraphSpec<'_>) -> Result<(), TextLayoutError> {
    crate::text_layout::error::font_size(f64::from(paragraph.font_size))?;
    crate::text_layout::error::width(f64::from(paragraph.min_width))?;
    if let Some(width) = paragraph.max_width {
        crate::text_layout::error::width(f64::from(width))?;
    }
    let default_height = paragraph.line_height.unwrap_or(paragraph.font_size * 1.2);
    if !default_height.is_finite() || default_height < 0.0 {
        return Err(TextLayoutError::InvalidLineHeight {
            height: f64::from(default_height),
        });
    }
    let default_size = paragraph
        .default_style
        .and_then(|style| style.font_size)
        .unwrap_or(f64::from(paragraph.font_size));
    for style in paragraph.default_style.into_iter().chain(
        paragraph
            .spans
            .iter()
            .filter_map(|(_, style)| style.as_ref()),
    ) {
        let size = crate::text_layout::error::font_size(style.font_size.unwrap_or(default_size))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "check the backend representation"
        )]
        if let Some(height) = style.height {
            let relative = height as f32;
            let resolved = relative * size;
            if relative < 0.0 || !relative.is_finite() || !resolved.is_finite() {
                return Err(TextLayoutError::InvalidLineHeight { height });
            }
        } else if paragraph.line_height.is_none() && !(size * 1.2).is_finite() {
            return Err(TextLayoutError::InvalidLineHeight {
                height: f64::from(size * 1.2),
            });
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "check the backend representation"
        )]
        for spacing in [style.letter_spacing, style.word_spacing]
            .into_iter()
            .flatten()
        {
            if !(spacing as f32).is_finite() {
                return Err(TextLayoutError::InvalidSpacing { spacing });
            }
        }
    }
    Ok(())
}

fn validate_layout(paragraph: &ParagraphLayout) -> Result<(), TextLayoutError> {
    let metrics = paragraph.metrics();
    let values = [
        metrics.width,
        metrics.height,
        metrics.max_line_width,
        metrics.alphabetic_baseline,
        metrics.ideographic_baseline,
        paragraph.content_widths.0,
        paragraph.content_widths.1,
    ];
    if values.into_iter().any(|value| !value.is_finite()) {
        return Err(TextLayoutError::NonFiniteGeometry);
    }
    for line in paragraph.layout.lines() {
        let metrics = line.metrics();
        if [
            metrics.ascent,
            metrics.descent,
            metrics.leading,
            metrics.line_height,
            metrics.baseline,
            metrics.offset,
            metrics.advance,
            metrics.trailing_whitespace,
            metrics.inline_min_coord,
            metrics.inline_max_coord,
            metrics.block_min_coord,
            metrics.block_max_coord,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(TextLayoutError::NonFiniteGeometry);
        }
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item
                && (!run.baseline().is_finite()
                    || !run.advance().is_finite()
                    || run.positioned_glyphs().any(|glyph| {
                        !glyph.x.is_finite() || !glyph.y.is_finite() || !glyph.advance.is_finite()
                    }))
            {
                return Err(TextLayoutError::NonFiniteGeometry);
            }
        }
    }
    Ok(())
}

impl TextContext {
    /// Shapes and line-breaks `paragraph` on this context's fonts.
    pub fn shape(
        &mut self,
        paragraph: &ParagraphSpec<'_>,
    ) -> Result<ParagraphLayout, TextLayoutError> {
        validate_request(paragraph)?;
        let max_lines = paragraph.max_lines.filter(|&lines| lines > 0);
        let mut shaped = self.shape_spans(paragraph, paragraph.spans, SpanSources::Own)?;
        let mut ellipsized = false;
        let mut kept_text = shaped.text.len();
        if let (Some(max_lines), Some(ellipsis)) =
            (max_lines, paragraph.ellipsis.filter(|e| !e.is_empty()))
            && shaped.layout.len() > max_lines
        {
            shaped = self.ellipsize(paragraph, &shaped, max_lines, ellipsis)?;
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
        let layout = ParagraphLayout {
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
            line_layouts: OnceLock::new(),
            run_colors: shaped.run_colors.into_boxed_slice(),
            source_spans: paragraph.spans.len(),
        };
        validate_layout(&layout)?;
        Ok(layout)
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
    ) -> Result<Shaped, TextLayoutError> {
        let full = &shaped.text;
        let Some(line) = shaped.layout.get(max_lines - 1) else {
            return self.shape_spans(paragraph, paragraph.spans, SpanSources::Own);
        };
        let line_start = line.text_range().start;
        let mut cut = line.text_range().end;
        if let Some(max_width) = paragraph.max_width {
            let ellipsis_width = {
                let style = sliced_spans(paragraph.spans, cut)
                    .last()
                    .and_then(|(_, style)| style.clone());
                let spans = vec![(ellipsis.to_owned(), style)];
                self.shape_spans(paragraph, &spans, SpanSources::Own)?
                    .layout
                    .width()
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
            // The sliced spans keep the paragraph's indices; the ellipsis
            // takes the style, and so the source, of the last of them.
            let sources = SpanSources::Ellipsized {
                prefix: spans.len(),
                ellipsis: spans.len().saturating_sub(1),
            };
            let style = spans
                .last()
                .or(paragraph.spans.first())
                .and_then(|(_, style)| style.clone());
            spans.push((ellipsis.to_owned(), style));
            let candidate = self.shape_spans(paragraph, &spans, sources)?;
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
                return Ok(candidate);
            };
            if fits {
                return Ok(candidate);
            }
            cut = previous;
        }
    }

    /// Shapes `spans` with `paragraph`'s other settings; `sources` names the
    /// paragraph span each of them stands for.
    ///
    /// Adjacent non-empty spans that paint one colour share a paint run, and
    /// each run's index is the brush over its spans, so the run colours are
    /// the layout's to replace without shaping. An empty span paints nothing
    /// and opens no run.
    fn shape_spans(
        &mut self,
        paragraph: &ParagraphSpec<'_>,
        spans: &[(String, Option<TextStyle>)],
        sources: SpanSources,
    ) -> Result<Shaped, TextLayoutError> {
        let text: String = spans.iter().map(|(text, _)| text.as_str()).collect();
        let breaks = one_break_per_crlf(&text);

        // Families are resolved against the collection before the builder
        // borrows it (`resolve_family_name`).
        let collection = &mut self.font_cx.collection;
        let (default_family, default_family_name) = family(collection, None);
        let default_properties = paragraph.default_style.map_or_default(|style| {
            properties(collection, style, None, paragraph.font_weight_adjustment)
        });
        let default_info = span_info(
            0,
            default_family_name,
            paragraph.default_style,
            None,
            paragraph.font_weight_adjustment,
        );
        let mut start = 0;
        let mut infos = Vec::with_capacity(spans.len());
        let mut run_colors: Vec<Option<Color>> = Vec::new();
        let span_properties: Vec<_> = spans
            .iter()
            .enumerate()
            .map(|(index, (span, style))| {
                let range = start..start + span.len();
                start = range.end;
                let mut properties = style.as_ref().map_or_default(|style| {
                    properties(
                        collection,
                        style,
                        paragraph.default_style,
                        paragraph.font_weight_adjustment,
                    )
                });
                let paint_run = (!span.is_empty()).then(|| {
                    let color = span_paint_color(style.as_ref(), paragraph.default_style);
                    if run_colors.last() != Some(&color) {
                        run_colors.push(color);
                    }
                    run_colors.len() - 1
                });
                // Run 0 is the default brush, which every span starts with.
                if let Some(run) = paint_run.filter(|&run| run > 0) {
                    properties.push(StyleProperty::Brush(SpanBrush(run)));
                }
                let info = match style {
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
                };
                infos.push(SpanInfo {
                    source: sources.source(index),
                    paint_run,
                    ..info
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
        // Reject overflow before the line breaker consumes derived advances.
        let widths = layout.calculate_content_widths();
        if !widths.min.is_finite() || !widths.max.is_finite() {
            return Err(TextLayoutError::NonFiniteGeometry);
        }
        layout.break_all_lines(paragraph.max_width);
        Ok(Shaped {
            layout,
            text,
            spans: infos,
            run_colors,
        })
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

/// What a span of `len` bytes styled `style` in `family` was shaped with;
/// its source and paint run are the caller's to set.
fn span_info(
    len: usize,
    family: std::sync::Arc<str>,
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
        source: 0,
        paint_run: None,
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
) -> (StyleProperty<'static, SpanBrush>, std::sync::Arc<str>) {
    let family = resolve_family_name(style, |name| holds_exactly(collection, name));
    let described: std::sync::Arc<str> = format!("{family:?}").into();
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
        TextContext::new(&FontCollection::new())
            .shape(&ParagraphSpec {
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
            .expect("valid fixture shapes")
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
                .expect("valid fixture shapes")
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
        TextContext::new(&FontCollection::new())
            .shape(&ParagraphSpec {
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
            .expect("valid fixture shapes")
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
            .expect("valid fixture shapes")
            .metrics()
            .width;
        assert!(
            min >= seven - 1e-3,
            "min {min} covers the word 'seven' ({seven})"
        );
    }

    /// A font's own decoration metrics are used where they are in range; the
    /// values Parley substitutes for a font without them, in font units, and
    /// non-finite ones fall back to an eighteenth of an em.
    #[test]
    fn decoration_metrics_trust_the_font_only_in_range() {
        use parley::layout::RunMetrics;

        use super::{DecorationMetrics, LinePlacement};

        let size = 72.0;
        let em_18 = 4.0;
        let font = RunMetrics {
            ascent: 66.0,
            x_height: Some(38.0),
            underline_offset: -7.0,
            underline_size: 3.0,
            strikethrough_offset: 20.0,
            strikethrough_size: 5.0,
            ..RunMetrics::default()
        };
        assert_eq!(
            DecorationMetrics::of(&font, size),
            DecorationMetrics {
                underline: LinePlacement {
                    top: 7.0,
                    thickness: 3.0
                },
                overline: LinePlacement {
                    top: -66.0,
                    thickness: 3.0
                },
                line_through: LinePlacement {
                    top: -20.0,
                    thickness: 5.0
                },
            },
            "a font's metrics in range are its own"
        );

        // 2048 units per em over 18, unscaled: what a font without the
        // metrics reports.
        let units = 2048.0 / 18.0;
        let substituted = RunMetrics {
            underline_offset: units,
            underline_size: units,
            strikethrough_offset: units,
            strikethrough_size: units,
            ..font
        };
        let not_finite = RunMetrics {
            ascent: f32::NAN,
            x_height: Some(f32::INFINITY),
            underline_offset: f32::NAN,
            underline_size: f32::INFINITY,
            strikethrough_offset: f32::NEG_INFINITY,
            strikethrough_size: f32::NAN,
            ..font
        };
        for (fixture, metrics, ascent, x_height) in [
            ("substituted", substituted, 66.0, 38.0),
            ("not finite", not_finite, 72.0 * 0.8, 72.0 * 0.5),
        ] {
            assert_eq!(
                DecorationMetrics::of(&metrics, size),
                DecorationMetrics {
                    underline: LinePlacement {
                        top: em_18,
                        thickness: em_18
                    },
                    overline: LinePlacement {
                        top: -ascent,
                        thickness: em_18
                    },
                    line_through: LinePlacement {
                        top: -(x_height / 2.0) - em_18 / 2.0,
                        thickness: em_18
                    },
                },
                "{fixture} metrics fall back"
            );
        }
    }
}
