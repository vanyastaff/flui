//! Carets, selection boxes, hit-testing, line metrics and word boundaries on
//! the [`ParagraphLayout`] that measured and painted (flui-painting
//! `ARCHITECTURE.md`, mapping decision 15).
//!
//! Every answer is in the painted box's coordinates: each cluster edge is
//! moved by the same per-line shift paint moves glyphs by
//! ([`ParagraphLayout::line_shift`]), so a caret sits on the glyph it
//! follows. Only the kept text is reachable: an offset past it, in dropped
//! lines or in an appended ellipsis, answers its end.

use std::ops::Range;

use flui_foundation::geometry::{Offset, Rect};
use parley::layout::BreakReason;

use super::ParagraphLayout;
use super::shape::is_hard_break as is_break;
use crate::text_boundaries::{grapheme_bounds, word_at};
use crate::typography::{
    LineMetrics, TextAffinity, TextBox, TextDirection, TextPosition, TextRange,
};

/// One cluster where it is painted.
#[derive(Clone, Debug)]
pub(super) struct Placed {
    range: Range<usize>,
    left: f32,
    right: f32,
    rtl: bool,
    hard_break: bool,
}

impl Placed {
    /// The x of the edge a caret before the cluster sits on.
    fn leading(&self) -> f32 {
        if self.rtl { self.right } else { self.left }
    }

    /// The x of the edge a caret after the cluster sits on.
    fn trailing(&self) -> f32 {
        if self.rtl { self.left } else { self.right }
    }

    /// The x at byte `offset` inside the cluster, in proportion to the bytes
    /// before it.
    #[expect(
        clippy::cast_precision_loss,
        reason = "byte counts within one cluster are tiny"
    )]
    fn x_at(&self, offset: usize) -> f32 {
        let len = self.range.len().max(1) as f32;
        let progress = offset.saturating_sub(self.range.start) as f32 / len;
        let width = self.right - self.left;
        if self.rtl {
            self.right - width * progress
        } else {
            self.left + width * progress
        }
    }
}

/// One kept line where it is painted.
#[derive(Debug)]
pub(super) struct PlacedLine {
    top: f32,
    bottom: f32,
    /// Where a caret on the line with no cluster sits.
    start: f32,
    /// The line's text, before its hard break.
    text_start: usize,
    /// Every kept cluster, in visual order.
    clusters: Vec<Placed>,
}

impl ParagraphLayout {
    /// The kept lines with their clusters, in painted coordinates, built on
    /// the first query and kept with the layout.
    fn placed_lines(&self) -> &[PlacedLine] {
        self.placed.get_or_init(|| self.place_lines())
    }

    /// Places the kept lines' clusters where paint puts their glyphs.
    fn place_lines(&self) -> Vec<PlacedLine> {
        let (box_width, _) = self.kept_extent();
        self.layout
            .lines()
            .take(self.kept())
            .map(|line| {
                let metrics = line.metrics();
                let start = self.line_start(metrics, box_width) + metrics.inline_min_coord;
                let mut x = start;
                let mut clusters = Vec::new();
                for run in line.runs() {
                    let rtl = run.is_rtl();
                    for cluster in run.visual_clusters() {
                        let left = x;
                        x += cluster.advance();
                        let range = cluster.text_range();
                        if range.start < self.kept_text {
                            clusters.push(Placed {
                                range,
                                left,
                                right: x,
                                rtl,
                                hard_break: cluster.is_hard_line_break(),
                            });
                        }
                    }
                }
                PlacedLine {
                    top: metrics.block_min_coord,
                    bottom: metrics.block_max_coord,
                    start,
                    text_start: line.text_range().start.min(self.kept_text),
                    clusters,
                }
            })
            .collect()
    }

    /// `offset` clamped to the kept text and snapped down to a char boundary.
    fn clamp_offset(&self, offset: usize) -> usize {
        self.text.floor_char_boundary(offset.min(self.kept_text))
    }

    /// The caret before the text at `offset`: its x and the index of its line.
    ///
    /// Inside a cluster that spans several scalars the caret is a proportional
    /// slice of it. Where the clusters before and after `offset` are painted
    /// apart (a soft wrap, a bidi run boundary), `Downstream` takes the edge
    /// of the one after and `Upstream` the edge of the one before; after a hard
    /// break the caret starts the next line whatever the affinity.
    fn caret_at(&self, lines: &[PlacedLine], position: TextPosition) -> (f32, usize) {
        let offset = self.clamp_offset(position.offset);
        let mut before: Option<(usize, &Placed)> = None;
        let mut after: Option<(usize, &Placed)> = None;
        for (index, line) in lines.iter().enumerate() {
            for cluster in &line.clusters {
                if cluster.range.start < offset && offset < cluster.range.end {
                    return (cluster.x_at(offset), index);
                }
                if cluster.range.end == offset {
                    before = Some((index, cluster));
                }
                if cluster.range.start == offset {
                    after = Some((index, cluster));
                }
            }
        }
        match (before, after) {
            (Some((line, cluster)), after) if cluster.hard_break => after.map_or_else(
                || {
                    let next = (line + 1).min(lines.len().saturating_sub(1));
                    (lines.get(next).map_or(0.0, |line| line.start), next)
                },
                |(line, cluster)| (cluster.leading(), line),
            ),
            (Some((line, cluster)), _) if position.affinity == TextAffinity::Upstream => {
                (cluster.trailing(), line)
            }
            (_, Some((line, cluster))) => (cluster.leading(), line),
            (Some((line, cluster)), None) => (cluster.trailing(), line),
            (None, None) => {
                let line = lines
                    .iter()
                    .rposition(|line| line.text_start <= offset)
                    .unwrap_or(0);
                (lines.get(line).map_or(0.0, |line| line.start), line)
            }
        }
    }

    /// The top-left of the caret before the text at `position`, in the
    /// painted box. Per scalar: an offset inside a grapheme gets its own
    /// caret, a proportional slice of the cluster.
    pub(crate) fn caret(&self, position: TextPosition) -> Offset<f64> {
        let lines = self.placed_lines();
        let (x, line) = self.caret_at(lines, position);
        let top = lines.get(line).map_or(0.0, |line| line.top);
        Offset::new(f64::from(x), f64::from(top))
    }

    /// The text position nearest `point`, in the painted box, snapped to a
    /// grapheme boundary.
    ///
    /// Above the first line reads as the first line and below the last as
    /// the last. The cluster under the point answers the edge the point is
    /// nearer; a hit past a hard break's cluster answers its start, since the
    /// position after it is on the next line. An offset inside a grapheme
    /// goes to whichever of the grapheme's edges has its caret nearer the
    /// point on the same line.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a point in layout space narrows to Parley's f32"
    )]
    pub(crate) fn position_at(&self, point: Offset<f64>) -> TextPosition {
        let lines = self.placed_lines();
        let (x, y) = (point.dx as f32, point.dy as f32);
        let Some(index) = lines
            .iter()
            .position(|line| y < line.bottom)
            .or(lines.len().checked_sub(1))
        else {
            return TextPosition::downstream(0);
        };
        let line = &lines[index];
        let Some(cluster) = line
            .clusters
            .iter()
            .find(|cluster| x < cluster.right)
            .or(line.clusters.last())
        else {
            return TextPosition::downstream(line.text_start);
        };
        let left_half = x <= f32::midpoint(cluster.left, cluster.right);
        let (offset, affinity) = if cluster.hard_break || left_half != cluster.rtl {
            (cluster.range.start, TextAffinity::Downstream)
        } else {
            (cluster.range.end, TextAffinity::Upstream)
        };
        let offset = self.clamp_offset(offset);
        let (start, end) = grapheme_bounds(&self.text[..self.kept_text], offset);
        if start == end {
            return TextPosition::new(offset, affinity);
        }
        let candidate = |offset: usize, affinity: TextAffinity| {
            let position = TextPosition::new(offset, affinity);
            let (caret, on) = self.caret_at(lines, position);
            (on == index).then_some(((caret - x).abs(), position))
        };
        match (
            candidate(start, TextAffinity::Downstream),
            candidate(end, TextAffinity::Upstream),
        ) {
            (Some((to_start, at_start)), Some((to_end, at_end))) => {
                if to_end < to_start {
                    at_end
                } else {
                    at_start
                }
            }
            (None, Some((_, at_end))) => at_end,
            (Some((_, at_start)), None) => at_start,
            (None, None) => TextPosition::downstream(start),
        }
    }

    /// The boxes of the text in `range`: one per stretch of adjacent
    /// clusters of one direction on one line, in visual order, as tall as the
    /// line box. A hard break paints nothing and gets no box.
    pub(crate) fn boxes(&self, range: TextRange) -> Vec<TextBox> {
        let end = self.clamp_offset(range.end);
        let start = self.clamp_offset(range.start.min(end));
        if start >= end {
            return Vec::new();
        }
        let mut boxes = Vec::new();
        for line in self.placed_lines() {
            let mut open: Option<(f32, f32, bool)> = None;
            let mut close = |open: &mut Option<(f32, f32, bool)>| {
                if let Some((left, right, rtl)) = open.take() {
                    boxes.push(TextBox::new(
                        Rect::from_ltrb(
                            f64::from(left),
                            f64::from(line.top),
                            f64::from(right),
                            f64::from(line.bottom),
                        ),
                        if rtl {
                            TextDirection::Rtl
                        } else {
                            TextDirection::Ltr
                        },
                    ));
                }
            };
            for cluster in &line.clusters {
                let selected = cluster.range.start < end && cluster.range.end > start;
                if !selected || cluster.hard_break {
                    close(&mut open);
                    continue;
                }
                let (a, b) = (
                    cluster.x_at(start.max(cluster.range.start)),
                    cluster.x_at(end.min(cluster.range.end)),
                );
                let (left, right) = (a.min(b), a.max(b));
                match &mut open {
                    Some((_, open_right, rtl))
                        if *rtl == cluster.rtl && (*open_right - left).abs() < 1e-3 =>
                    {
                        *open_right = right;
                    }
                    _ => {
                        close(&mut open);
                        open = Some((left, right, cluster.rtl));
                    }
                }
            }
            close(&mut open);
        }
        boxes
    }

    /// One entry per kept line, in the painted box.
    ///
    /// `hard_break` is true on a line that ends at an explicit break or at
    /// the end of the paragraph, as Flutter's `LineMetrics.hardBreak`; the
    /// last kept line of truncated text ends at neither. `width` leaves out
    /// trailing whitespace and `left` is where the line's visible text
    /// starts; `end_index` leaves out the line's hard break,
    /// which `end_including_newline` keeps. A layout with no line reports one
    /// line box of the paragraph's line height.
    pub(crate) fn line_metrics(&self) -> Vec<LineMetrics> {
        let (box_width, _) = self.kept_extent();
        let mut metrics: Vec<LineMetrics> = self
            .layout
            .lines()
            .take(self.kept())
            .enumerate()
            .map(|(number, line)| {
                let m = line.metrics();
                let hang = if self.layout.is_rtl() {
                    m.trailing_whitespace
                } else {
                    0.0
                };
                let range = line.text_range();
                let with_newline = range.end.min(self.kept_text);
                let start = range.start.min(with_newline);
                let text = &self.text[start..with_newline];
                let without_newline = text.trim_end_matches(is_break);
                let end_index = start + without_newline.len();
                let visible_end = start + without_newline.trim_end().len();
                let ends_paragraph = !self.ellipsized && range.end >= self.text.len();
                LineMetrics::new(
                    line.break_reason() == BreakReason::Explicit || ends_paragraph,
                    f64::from(m.baseline - m.block_min_coord),
                    f64::from(m.block_max_coord - m.baseline),
                    f64::from(m.baseline - m.block_min_coord),
                    f64::from(m.block_max_coord - m.block_min_coord),
                    f64::from(m.advance - m.trailing_whitespace),
                    f64::from(self.line_start(m, box_width) + m.inline_min_coord + hang),
                    f64::from(m.baseline),
                    number,
                    start,
                    end_index,
                    visible_end,
                    with_newline,
                )
            })
            .collect();
        if metrics.is_empty() {
            let height = f64::from(self.line_height);
            metrics.push(LineMetrics::new(
                true,
                height * 0.8,
                height * 0.2,
                height * 0.8,
                height,
                0.0,
                0.0,
                height * 0.8,
                0,
                0,
                0,
                0,
                0,
            ));
        }
        metrics
    }

    /// The word at `position` in the kept text (see
    /// [`word_at`] for the tie-break).
    pub(crate) fn word_boundary(&self, position: TextPosition) -> TextRange {
        let (start, end) = word_at(&self.text[..self.kept_text], position.offset);
        TextRange::new(start, end)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::parley_text::ParagraphSpec;
    use crate::text_layout::{FontCollection, TextContext};
    use crate::typography::{TextDirection, TextStyle};

    use crate::text_boundaries::word_segments;

    /// Word boundaries come from ICU4X's segmenter over the text rather
    /// than from the layout's cluster flags, because the layout holds a
    /// space where the text has the CR of a CR LF. Away from CR the two are
    /// the same segmenter and agree: every segment start is a cluster Parley
    /// marks as a boundary, and no other cluster is. Fails if the segmenter
    /// FLUI builds differs from the one Parley marks clusters with (another
    /// constructor, dictionary data on one side only).
    #[test]
    fn word_boundaries_agree_with_the_layouts_clusters() {
        let corpus = [
            "Hello, world! don't stop at 3.14 or foo_bar.",
            "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, \u{43c}\u{438}\u{440}! \
             \u{41a}\u{430}\u{43a} \u{434}\u{435}\u{43b}\u{430}?",
            "\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}",
        ];
        let fonts = FontCollection::new();
        let mut context = TextContext::new(&fonts);
        for text in corpus {
            let spans: Vec<(String, Option<TextStyle>)> = vec![(text.to_owned(), None)];
            let paragraph = context.shape(&ParagraphSpec {
                spans: &spans,
                default_style: None,
                font_size: 16.0,
                max_width: None,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            });
            let mut marked = BTreeSet::new();
            for line in paragraph.layout.lines() {
                for run in line.runs() {
                    for cluster in run.clusters() {
                        if cluster.is_word_boundary() {
                            marked.insert(cluster.text_range().start);
                        }
                    }
                }
            }
            let segmented: BTreeSet<usize> =
                word_segments(text).map(|segment| segment.start).collect();
            assert_eq!(segmented, marked, "{text:?}");
        }
    }
}
