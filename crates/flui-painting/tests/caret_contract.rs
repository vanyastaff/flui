//! Carets, selection boxes, hit-testing and word boundaries read the layout
//! that measured and painted (ADR-0092 §10 step 5; flui-painting
//! `ARCHITECTURE.md`, mapping decision 15).
//!
//! Every row lays a `TextPainter` out on a collection holding only the
//! bundled faces and asks its public queries.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_painting::typography::{
    TextAffinity, TextAlign, TextDirection, TextPosition, TextSpan, TextStyle,
};
use flui_painting::{Canvas, DrawOp, FontCollection, ShapedParagraph, TextContext, TextPainter};

const SIZE: f64 = 32.0;
const EPS: f64 = 1e-3;

fn style() -> TextStyle {
    TextStyle {
        font_size: Some(SIZE),
        ..TextStyle::default()
    }
}

fn painter(text: &str, direction: TextDirection) -> TextPainter {
    TextPainter::new()
        .with_text(TextSpan::styled(text, style()))
        .with_text_direction(direction)
}

fn laid_out(mut painter: TextPainter, max_width: f64) -> TextPainter {
    painter.layout(
        &mut TextContext::new(&FontCollection::new()),
        0.0,
        max_width,
    );
    painter
}

fn ltr(text: &str) -> TextPainter {
    laid_out(painter(text, TextDirection::Ltr), f64::INFINITY)
}

fn caret(painter: &TextPainter, offset: usize) -> Offset<f64> {
    painter.get_offset_for_caret(TextPosition::downstream(offset))
}

fn hit(painter: &TextPainter, x: f64, y: f64) -> usize {
    painter.get_position_for_offset(Offset::new(x, y)).offset
}

/// The top of line `line`, from the line metrics.
fn line_top(painter: &TextPainter, line: usize) -> f64 {
    let metrics = &painter.get_line_metrics()[line];
    metrics.baseline - metrics.ascent
}

/// The middle of line `line`.
fn line_middle(painter: &TextPainter, line: usize) -> f64 {
    let metrics = &painter.get_line_metrics()[line];
    metrics.baseline - metrics.ascent + metrics.height / 2.0
}

/// The paragraph `painter` records, and where it records it.
fn painted(painter: &TextPainter) -> (Arc<ShapedParagraph>, Offset<f64>) {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph {
                paragraph, offset, ..
            } => Some((Arc::clone(paragraph), *offset)),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// Every hit across `from..to` on `y`, sampled every half pixel.
fn hits_across(painter: &TextPainter, from: f64, to: f64, y: f64) -> Vec<usize> {
    let mut x = from;
    let mut hits = Vec::new();
    while x < to {
        hits.push(hit(painter, x, y));
        x += 0.5;
    }
    hits
}

/// Carets advance along a line of Latin text.
pub(crate) fn caret_position() {
    let painter = ltr("Hello");
    let (start, mid, end) = (caret(&painter, 0), caret(&painter, 2), caret(&painter, 5));
    assert!(start.dx.abs() < EPS, "the first caret is at the line start");
    assert!(
        mid.dx > start.dx && end.dx > mid.dx,
        "{start:?} {mid:?} {end:?}"
    );
    assert!(
        (end.dx - painter.width()).abs() < EPS,
        "the last caret ends the line"
    );
}

/// A multi-space run is one segment: an offset inside it selects the whole
/// run, and an offset at either edge selects the adjacent word. Pinned
/// against a two-space run specifically, since a three-space run could not
/// distinguish "the whole run" from "a two-space sub-range".
pub(crate) fn two_space_run_word_boundary() {
    let painter = ltr("foo  bar");
    let word_at = |offset: usize| {
        let range = painter.get_word_boundary(TextPosition::new(offset, TextAffinity::Downstream));
        (range.start, range.end)
    };
    assert_eq!(word_at(3), (0, 3), "\"foo\"");
    assert_eq!(
        word_at(4),
        (3, 5),
        "inside the gap: the whole two-space run"
    );
    assert_eq!(word_at(5), (5, 8), "\"bar\"");
}

pub(crate) fn byte_offsets_snap_backward_and_clamp_at_the_text_end() {
    for text in ["é a", "中 a", "😀 a"] {
        let painter = ltr(text);
        let scalar_end = text.chars().next().expect("a first scalar").len_utf8();
        for offset in 1..scalar_end {
            assert_eq!(
                caret(&painter, offset),
                caret(&painter, 0),
                "{text:?} {offset}"
            );
            let word = painter.get_word_boundary(TextPosition::downstream(offset));
            assert_eq!((word.start, word.end), (0, scalar_end), "{text:?} {offset}");
        }
        for offset in [text.len() + 1, usize::MAX] {
            assert_eq!(
                caret(&painter, offset),
                caret(&painter, text.len()),
                "{text:?} {offset}"
            );
            assert_eq!(
                painter.get_word_boundary(TextPosition::downstream(offset)),
                painter.get_word_boundary(TextPosition::downstream(text.len())),
                "{text:?} {offset}"
            );
        }
    }
}

/// `e` and a combining acute are one grapheme: a hit anywhere over it
/// answers its start or its end, never the offset between the two scalars.
/// The caret at that offset still lies strictly between the two, so an input
/// method asking for a scalar's rect gets a proportional slice.
pub(crate) fn a_combining_mark_is_one_hit_target() {
    let painter = ltr("e\u{301}x");
    let (start, inner, end) = (caret(&painter, 0), caret(&painter, 1), caret(&painter, 3));
    assert!(
        start.dx < inner.dx && inner.dx < end.dx,
        "the caret between the scalars sits inside the grapheme: {start:?} {inner:?} {end:?}"
    );
    let hits = hits_across(&painter, start.dx + 0.25, end.dx, line_middle(&painter, 0));
    assert_ne!(hits, [] as [usize; 0]);
    assert!(
        hits.iter().all(|&offset| offset == 0 || offset == 3),
        "hits over the grapheme: {hits:?}"
    );
}

/// A ZWJ family sequence is one grapheme: hits over it answer its start or
/// its end.
pub(crate) fn a_zwj_family_is_one_hit_target() {
    let text = "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}b";
    let end_of_family = text.len() - 1;
    let painter = ltr(text);
    let (start, end) = (caret(&painter, 1), caret(&painter, end_of_family));
    assert!(
        end.dx > start.dx + 1.0,
        "the family has a width: {start:?} {end:?}"
    );
    let hits = hits_across(&painter, start.dx + 0.25, end.dx, line_middle(&painter, 0));
    assert!(
        hits.iter()
            .all(|&offset| offset == 1 || offset == end_of_family),
        "hits over the family: {hits:?}"
    );
}

/// A Hebrew paragraph runs right to left: the caret at its start is at the
/// right edge, the caret at its end at the left edge, carets fall monotonically
/// in between, and a hit in the middle of each letter answers one of that
/// letter's two edges.
pub(crate) fn rtl_paragraph_carets_run_right_to_left() {
    let text = "\u{05E9}\u{05DC}\u{05D5}\u{05DD}";
    let painter = laid_out(painter(text, TextDirection::Rtl), f64::INFINITY);
    let width = painter.width();
    assert!(width > 1.0);
    assert!(
        (caret(&painter, 0).dx - width).abs() < EPS,
        "the start is at the right edge: {:?}, width {width}",
        caret(&painter, 0)
    );
    assert!(
        caret(&painter, text.len()).dx.abs() < EPS,
        "the end is at the left edge: {:?}",
        caret(&painter, text.len())
    );
    let y = line_middle(&painter, 0);
    for (offset, letter) in text.char_indices() {
        let next = offset + letter.len_utf8();
        let (right, left) = (caret(&painter, offset).dx, caret(&painter, next).dx);
        assert!(right > left, "letter at {offset}: {right} > {left}");
        let got = hit(&painter, f64::midpoint(left, right), y);
        assert!(
            got == offset || got == next,
            "a hit in the letter at {offset} answers {got}"
        );
    }
}

/// In `"abc אבג def"` the Hebrew range's boxes are right-to-left and the
/// Latin ranges' left-to-right, and the boxes of the whole text tile the line
/// without overlapping.
pub(crate) fn mixed_bidi_boxes_carry_their_run_direction() {
    let text = "abc \u{05D0}\u{05D1}\u{05D2} def";
    let hebrew = 4..10;
    let painter = ltr(text);
    let directions = |start: usize, end: usize| -> Vec<TextDirection> {
        painter
            .get_boxes_for_selection(start, end)
            .iter()
            .map(|text_box| text_box.direction)
            .collect()
    };
    let rtl = directions(hebrew.start, hebrew.end);
    assert!(
        !rtl.is_empty() && rtl.iter().all(|d| *d == TextDirection::Rtl),
        "{rtl:?}"
    );
    for (start, end) in [(0, 3), (11, 14)] {
        let got = directions(start, end);
        assert!(
            !got.is_empty() && got.iter().all(|d| *d == TextDirection::Ltr),
            "{start}..{end}: {got:?}"
        );
    }
    let mut boxes: Vec<_> = painter
        .get_boxes_for_selection(0, text.len())
        .into_iter()
        .map(|text_box| text_box.rect)
        .collect();
    boxes.sort_by(|a, b| a.left().total_cmp(&b.left()));
    for pair in boxes.windows(2) {
        assert!(pair[0].right() <= pair[1].left() + EPS, "overlap: {pair:?}");
    }
    let left = boxes.first().map(flui_foundation::geometry::Rect::left);
    let right = boxes.last().map(flui_foundation::geometry::Rect::right);
    let covered: f64 = boxes
        .iter()
        .map(flui_foundation::geometry::Rect::width)
        .sum();
    assert!(left.is_some_and(|left| left.abs() < EPS), "{boxes:?}");
    assert!(
        right.is_some_and(|right| (right - painter.width()).abs() < EPS),
        "{boxes:?}"
    );
    assert!(
        (covered - painter.width()).abs() < EPS,
        "the boxes tile the line: {boxes:?}"
    );
}

/// After a trailing newline the caret is on the empty line it starts.
pub(crate) fn a_trailing_newline_puts_the_caret_on_the_empty_line() {
    let painter = ltr("A\n");
    assert_eq!(painter.get_line_metrics().len(), 2);
    let after = caret(&painter, 2);
    assert!(after.dx.abs() < EPS, "at the line start: {after:?}");
    assert!(
        (after.dy - line_top(&painter, 1)).abs() < EPS && after.dy > 0.0,
        "on the second line: {after:?}, its top {}",
        line_top(&painter, 1)
    );
}

/// CR LF is one break: the caret after it starts the next line, a hit never
/// answers the offset between CR and LF, and a selection over both lines has
/// a box on each.
pub(crate) fn crlf_is_one_break_for_carets() {
    let painter = ltr("A\r\nB");
    assert_eq!(painter.get_line_metrics().len(), 2);
    let after = caret(&painter, 3);
    assert!(after.dx.abs() < EPS, "at the line start: {after:?}");
    assert!((after.dy - line_top(&painter, 1)).abs() < EPS, "{after:?}");
    for line in 0..2 {
        let hits = hits_across(
            &painter,
            -5.0,
            painter.width() + 5.0,
            line_middle(&painter, line),
        );
        assert!(!hits.contains(&2), "line {line}: {hits:?}");
    }
    let mut tops: Vec<f64> = painter
        .get_boxes_for_selection(0, 4)
        .iter()
        .map(|text_box| text_box.rect.top())
        .collect();
    tops.dedup_by(|a, b| (*a - *b).abs() < EPS);
    assert_eq!(tops.len(), 2, "a box on each line: {tops:?}");
}

/// A selection on the second line of `"ab\ncd"` is one box on that line.
pub(crate) fn multi_line_selection_boxes_follow_their_line() {
    let painter = ltr("ab\ncd");
    let boxes = painter.get_boxes_for_selection(3, 5);
    assert_eq!(boxes.len(), 1, "{boxes:?}");
    let rect = boxes[0].rect;
    assert!((rect.top() - line_top(&painter, 1)).abs() < EPS, "{rect:?}");
    assert!(rect.left().abs() < EPS && rect.width() > 1.0, "{rect:?}");
    assert!(
        (rect.right() - caret(&painter, 5).dx).abs() < EPS,
        "the box ends at the caret after `d`: {rect:?}"
    );
}

/// Carets sit on the painted glyphs when lines align to the right inside a
/// box narrower than the width they broke at: the caret at a line's first
/// character is where its first glyph is painted, and the caret at the short
/// line's end is the box's right edge.
pub(crate) fn carets_sit_on_the_painted_glyphs() {
    let text = "Hello world\nHi";
    let painter = laid_out(painter(text, TextDirection::Rtl), 300.0);
    let (paragraph, offset) = painted(&painter);
    assert!(
        paragraph.size().width < 250.0,
        "the box is narrower than the break width"
    );
    let runs: Vec<_> = paragraph.runs().collect();
    let first_glyph = |run: usize| offset.dx + f64::from(runs[run].glyphs()[0].x);
    let last = runs.len() - 1;
    for (caret_at, glyph_x) in [(0, first_glyph(0)), (12, first_glyph(last))] {
        let got = caret(&painter, caret_at).dx;
        assert!(
            (got - glyph_x).abs() < EPS,
            "the caret at {caret_at} is at {got}, its glyph is painted at {glyph_x}"
        );
    }
    let end = caret(&painter, text.len()).dx;
    let right = offset.dx + paragraph.size().width;
    assert!(
        (end - right).abs() < EPS,
        "the short line ends at {end}, the box at {right}"
    );
}

/// At a soft wrap, downstream affinity puts the caret at the next line's
/// start and upstream affinity at the previous line's end.
pub(crate) fn a_soft_wrap_caret_follows_its_affinity() {
    let text = "aaaa bbbb";
    let one_line = ltr(text).width();
    let painter = laid_out(painter(text, TextDirection::Ltr), one_line * 0.7);
    assert_eq!(painter.get_line_metrics().len(), 2, "the probe wraps");
    let downstream = painter.get_offset_for_caret(TextPosition::downstream(5));
    let upstream = painter.get_offset_for_caret(TextPosition::upstream(5));
    assert!(downstream.dx.abs() < EPS, "{downstream:?}");
    assert!(
        (downstream.dy - line_top(&painter, 1)).abs() < EPS,
        "{downstream:?}"
    );
    assert!(upstream.dy.abs() < EPS && upstream.dx > 1.0, "{upstream:?}");
}

/// Truncated text keeps its carets and hits in the kept line: an offset in
/// the dropped text answers the caret at the kept text's end, before the
/// ellipsis.
pub(crate) fn truncated_carets_stay_in_kept_lines() {
    let text = "one two three four five six seven";
    let painter = laid_out(
        painter(text, TextDirection::Ltr)
            .with_max_lines(Some(1))
            .with_ellipsis(Some("\u{2026}".to_owned())),
        200.0,
    );
    assert!(painter.did_exceed_max_lines());
    let clamped = caret(&painter, text.len());
    assert!(clamped.dy.abs() < EPS, "{clamped:?}");
    assert!(
        clamped.dx < painter.width() - 1.0,
        "before the ellipsis: {clamped:?}"
    );
    for offset in (text.len() - 10)..text.len() {
        let got = caret(&painter, offset);
        assert!(
            got.dy.abs() < EPS && got.dx <= clamped.dx + EPS,
            "{offset}: {got:?}"
        );
    }
    let far = hit(&painter, painter.width() + 50.0, line_middle(&painter, 0));
    assert!(
        (caret(&painter, far).dx - clamped.dx).abs() < EPS,
        "a hit past the line answers the kept end, got {far}"
    );
}

/// Truncation without an ellipsis keeps carets and word boundaries in the
/// kept line: an offset in the dropped line answers the caret at the kept
/// line's end, and the word there is one the kept line holds.
pub(crate) fn truncated_text_without_an_ellipsis_stays_in_its_kept_line() {
    let text = "one two\nthree";
    let painter = laid_out(
        painter(text, TextDirection::Ltr).with_max_lines(Some(1)),
        f64::INFINITY,
    );
    assert!(painter.did_exceed_max_lines());
    let kept_end = caret(&painter, 7);
    assert!(kept_end.dx > 1.0, "{kept_end:?}");
    for offset in 8..=text.len() {
        let got = caret(&painter, offset);
        assert!(
            (got.dx - kept_end.dx).abs() < EPS && (got.dy - kept_end.dy).abs() < EPS,
            "{offset}: {got:?}, the kept end is {kept_end:?}"
        );
    }
    let word = painter.get_word_boundary(TextPosition::downstream(10));
    assert!(word.end <= 7, "a dropped word was selected: {word:?}");
}

/// Line metrics index each line's own text and say where it is painted:
/// every line ends at a hard break, the last at the paragraph's end; the
/// trailing space before a newline is in `end_index` but not in
/// `end_excluding_whitespace`; and a short line aligned right starts where
/// its glyphs are painted. Line metrics are in the paragraph's own box, the
/// allocated paragraph box.
pub(crate) fn line_metrics_index_each_line() {
    let ltr_lines = ltr("ab \ncd");
    let got: Vec<_> = ltr_lines
        .get_line_metrics()
        .iter()
        .map(|line| {
            (
                line.hard_break,
                line.start_index,
                line.end_index,
                line.end_excluding_whitespace,
                line.end_including_newline,
            )
        })
        .collect();
    assert_eq!(got, vec![(true, 0, 3, 2, 4), (true, 4, 6, 6, 6)]);
    for line in ltr_lines.get_line_metrics() {
        assert!(line.left.abs() < EPS, "{line:?}");
    }

    let rtl = laid_out(painter("abcd\nx", TextDirection::Rtl), 300.0);
    let lines = rtl.get_line_metrics();
    let short = &lines[1];
    assert!(short.left > 1.0, "{short:?}");
    assert!(
        (short.left + short.width - rtl.width()).abs() < EPS,
        "the short line ends at the box's right edge: {short:?}"
    );
    let (_, offset) = painted(&rtl);
    let first = caret(&rtl, 5).dx - offset.dx;
    assert!(
        (short.left - first).abs() < EPS,
        "the line starts at its first caret, {first} in the box: {short:?}"
    );
}

/// "FLUI Probe Arabic" (`tools/decoy-face/generate.py`): alef 300, seen 400,
/// lam 500 and meem 450 units wide, and lam + alef a 650-unit ligature
/// (`rlig`, glyph 6), 1000 units to the em.
const PROBE_ARABIC: &[u8] = include_bytes!("../assets/fonts/probe-arabic-ligature.ttf");

/// A lam-alef ligature is one glyph and one caret stop per scalar (mapping
/// decision 19). `الاسم` in the probe face shapes four glyphs, the ligature
/// among them, and measures their advances; the caret between lam and alef
/// sits midway across the ligature, and a hit in each quarter of it answers
/// the nearest of its three stops. Fails if the face stops ligating (five
/// glyphs, a wider paragraph), or if carets collapse the ligature's
/// components into one stop (the middle caret lands on an edge).
pub(crate) fn a_lam_alef_ligature_is_one_glyph_and_two_caret_stops() {
    let fonts = FontCollection::new();
    fonts
        .register_font(PROBE_ARABIC)
        .expect("the probe face loads");
    // Alef, lam, alef, seen, meem: bytes 0, 2, 4, 6 and 8.
    let text = "\u{0627}\u{0644}\u{0627}\u{0633}\u{0645}";
    let style = TextStyle {
        font_family: Some("FLUI Probe Arabic".to_owned()),
        font_size: Some(SIZE),
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(text, style))
        .with_text_direction(TextDirection::Rtl);
    painter.layout(&mut TextContext::new(&fonts), 0.0, f64::INFINITY);

    let (paragraph, _) = painted(&painter);
    let glyphs: Vec<u16> = paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.id))
        .collect();
    assert_eq!(glyphs.len(), 4, "alef, lam-alef, seen, meem: {glyphs:?}");
    assert!(
        glyphs.contains(&6),
        "the ligature glyph is painted: {glyphs:?}"
    );
    let units = f64::from(300 + 650 + 400 + 450);
    assert!(
        (painter.width() - units / 1000.0 * SIZE).abs() < EPS,
        "the paragraph measures the four advances: {}",
        painter.width()
    );

    let (right, middle, left) = (
        caret(&painter, 2).dx,
        caret(&painter, 4).dx,
        caret(&painter, 6).dx,
    );
    assert!(
        (right - left - 0.65 * SIZE).abs() < EPS,
        "the ligature's outer carets span its advance: {left}..{right}"
    );
    assert!(
        left < middle && middle < right && (middle - f64::midpoint(left, right)).abs() < EPS,
        "the caret between lam and alef is midway across the ligature: \
         {left} < {middle} < {right}"
    );

    let (width, y) = (right - left, line_middle(&painter, 0));
    let hits = [
        hit(&painter, right - width / 8.0, y),
        hit(&painter, right - width * 3.0 / 8.0, y),
        hit(&painter, left + width * 3.0 / 8.0, y),
        hit(&painter, left + width / 8.0, y),
    ];
    assert_eq!(
        hits,
        [2, 4, 4, 6],
        "each quarter of the ligature answers its nearest stop"
    );
}

fn aligned_painter(
    text: &str,
    align: TextAlign,
    direction: TextDirection,
    min: f64,
    max: f64,
) -> TextPainter {
    let mut painter = painter(text, direction).with_text_align(align);
    painter.layout(&mut TextContext::new(&FontCollection::new()), min, max);
    painter
}

fn compare_aligned_lines(left: &TextPainter, aligned: &TextPainter, factor: f64) {
    let lines = left.get_line_metrics();
    let (left_paragraph, left_offset) = painted(left);
    let (paragraph, offset) = painted(aligned);
    assert_eq!(paragraph.line_count(), lines.len());
    let left_runs: Vec<_> = left_paragraph.runs().collect();
    let runs: Vec<_> = paragraph.runs().collect();
    assert_eq!(runs.len(), left_runs.len());
    for (index, line) in lines.iter().enumerate() {
        let shift = (aligned.width() - line.width) * factor;
        let actual = caret(aligned, line.start_index);
        let original = caret(left, line.start_index);
        assert!(
            (actual.dx - original.dx - shift).abs() < EPS,
            "line {index}: {actual:?}, shift {shift}"
        );
        assert_eq!(
            hit(aligned, actual.dx, line_middle(aligned, index)),
            line.start_index
        );
        let boxes =
            aligned.get_boxes_for_selection(line.start_index, line.end_excluding_whitespace);
        assert!(!boxes.is_empty());
        assert!((boxes[0].rect.left() - actual.dx).abs() < EPS);
        for (glyph, old) in runs[index].glyphs().iter().zip(left_runs[index].glyphs()) {
            let delta = f64::from(glyph.x - old.x) + offset.dx - left_offset.dx;
            assert!(
                (delta - shift).abs() < EPS,
                "line {index} painted shift {delta}, expected {shift}"
            );
        }
    }
}

fn unequal_lines(align: TextAlign, direction: TextDirection, min: f64, max: f64, factor: f64) {
    let text = "WWWW\ni";
    let left = aligned_painter(text, TextAlign::Left, direction, min, max);
    let aligned = aligned_painter(text, align, direction, min, max);
    assert_eq!(aligned.get_line_metrics().len(), 2);
    assert!((aligned.width() - left.width()).abs() < EPS);
    compare_aligned_lines(&left, &aligned, factor);
}

pub(crate) fn centered_lines_use_the_tight_allocated_box() {
    unequal_lines(TextAlign::Center, TextDirection::Ltr, 200.0, 200.0, 0.5);
}

pub(crate) fn right_aligned_lines_use_the_tight_allocated_box() {
    unequal_lines(TextAlign::Right, TextDirection::Ltr, 200.0, 200.0, 1.0);
}

pub(crate) fn loose_centered_lines_stay_inside_the_measured_box() {
    unequal_lines(TextAlign::Center, TextDirection::Ltr, 0.0, 200.0, 0.5);
}

pub(crate) fn unbounded_centered_lines_align_without_wrapping() {
    unequal_lines(
        TextAlign::Center,
        TextDirection::Ltr,
        0.0,
        f64::INFINITY,
        0.5,
    );
}

pub(crate) fn rtl_start_aligns_each_line_right() {
    unequal_lines(TextAlign::Start, TextDirection::Rtl, 200.0, 200.0, 1.0);
}

pub(crate) fn rtl_end_aligns_each_line_left() {
    unequal_lines(TextAlign::End, TextDirection::Rtl, 200.0, 200.0, 0.0);
    let aligned = aligned_painter("WWWW\ni", TextAlign::End, TextDirection::Rtl, 200.0, 200.0);
    for line in aligned.get_line_metrics() {
        assert!(caret(&aligned, line.start_index).dx.abs() < EPS);
    }
}

fn native_rtl_alignment(align: TextAlign, factor: f64) {
    let text = "אבגד\nא";
    let left = aligned_painter(text, TextAlign::Left, TextDirection::Ltr, 200.0, 200.0);
    let aligned = aligned_painter(text, align, TextDirection::Rtl, 200.0, 200.0);
    let lines = left.get_line_metrics();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].width < lines[0].width);
    let (old, old_offset) = painted(&left);
    let (paragraph, offset) = painted(&aligned);
    let old_runs: Vec<_> = old.runs().collect();
    let runs: Vec<_> = paragraph.runs().collect();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs.len(), old_runs.len());
    for (index, line) in lines.iter().enumerate() {
        let shift = (200.0 - line.width) * factor;
        for position in [line.start_index, line.end_excluding_whitespace] {
            assert!((caret(&aligned, position).dx - caret(&left, position).dx - shift).abs() < EPS);
        }
        let original =
            left.get_boxes_for_selection(line.start_index, line.end_excluding_whitespace);
        let placed =
            aligned.get_boxes_for_selection(line.start_index, line.end_excluding_whitespace);
        assert!(!original.is_empty());
        assert_eq!(placed.len(), original.len());
        for (a, b) in placed.iter().zip(&original) {
            assert!((a.rect.left() - b.rect.left() - shift).abs() < EPS);
            assert!((a.rect.right() - b.rect.right() - shift).abs() < EPS);
        }
        let inside = original[0].rect.center().x;
        assert_eq!(
            hit(&aligned, inside + shift, line_middle(&aligned, index)),
            hit(&left, inside, line_middle(&left, index))
        );
        assert_ne!(runs[index].glyphs(), []);
        assert_eq!(runs[index].glyphs().len(), old_runs[index].glyphs().len());
        for (a, b) in runs[index].glyphs().iter().zip(old_runs[index].glyphs()) {
            assert!((f64::from(a.x - b.x) + offset.dx - old_offset.dx - shift).abs() < EPS);
        }
    }
}

pub(crate) fn native_rtl_lines_center_in_the_allocated_box() {
    native_rtl_alignment(TextAlign::Center, 0.5);
}

pub(crate) fn native_rtl_lines_align_to_the_right_edge() {
    native_rtl_alignment(TextAlign::Right, 1.0);
}

pub(crate) fn a_last_kept_soft_line_retains_native_justification() {
    let text = "one two three four five six seven eight nine";
    let left = aligned_painter(text, TextAlign::Left, TextDirection::Ltr, 200.0, 200.0);
    let mut justified = painter(text, TextDirection::Ltr)
        .with_text_align(TextAlign::Justify)
        .with_max_lines(Some(1));
    justified.layout(&mut TextContext::new(&FontCollection::new()), 200.0, 200.0);
    let original = left.get_line_metrics();
    assert!(original.len() > 1);
    assert!(!original[0].hard_break);
    assert!(justified.did_exceed_max_lines());
    let kept = justified.get_line_metrics();
    assert_eq!(kept.len(), 1);
    assert!((kept[0].width - 200.0).abs() < EPS);
    assert!(kept[0].width > original[0].width + 1.0);
    let edge =
        justified.get_offset_for_caret(TextPosition::upstream(kept[0].end_excluding_whitespace));
    assert!((edge.dx - 200.0).abs() < EPS);
    let boxes =
        justified.get_boxes_for_selection(kept[0].start_index, kept[0].end_excluding_whitespace);
    assert!((boxes.last().expect("kept selection").rect.right() - edge.dx).abs() < EPS);
    let (paragraph, _) = painted(&justified);
    let (old, _) = painted(&left);
    let last = paragraph
        .runs()
        .next()
        .expect("kept line")
        .glyphs()
        .last()
        .expect("kept glyph")
        .x;
    let old_last = old
        .runs()
        .next()
        .expect("original line")
        .glyphs()
        .last()
        .expect("original glyph")
        .x;
    assert!(last - old_last > 1.0);
}

pub(crate) fn trailing_whitespace_does_not_shift_visible_alignment() {
    let left = aligned_painter(
        "WWWW \ni",
        TextAlign::Left,
        TextDirection::Ltr,
        200.0,
        200.0,
    );
    let centered = aligned_painter(
        "WWWW \ni",
        TextAlign::Center,
        TextDirection::Ltr,
        200.0,
        200.0,
    );
    compare_aligned_lines(&left, &centered, 0.5);
}

pub(crate) fn justification_expands_soft_lines_but_not_the_final_line() {
    let text = "one two three four five six seven eight nine";
    let mut context = TextContext::new(&FontCollection::new());
    let mut left = painter(text, TextDirection::Ltr).with_text_align(TextAlign::Left);
    left.layout(&mut context, 200.0, 200.0);
    let mut justified = painter(text, TextDirection::Ltr).with_text_align(TextAlign::Justify);
    justified.layout(&mut context, 200.0, 200.0);
    let lines = justified.get_line_metrics();
    let original = left.get_line_metrics();
    assert!(lines.len() > 1);
    assert!(!lines[0].hard_break);
    assert!(
        (lines[0].width - 200.0).abs() < EPS,
        "justified visible width {}",
        lines[0].width
    );
    assert!(lines[0].width > original[0].width + 1.0);
    let last = lines.last().expect("final line");
    assert!((last.width - original.last().expect("original final line").width).abs() < EPS);
    let edge =
        justified.get_offset_for_caret(TextPosition::upstream(lines[0].end_excluding_whitespace));
    assert!(
        (edge.dx - 200.0).abs() < EPS,
        "justified caret edge {edge:?}"
    );
    let boxes =
        justified.get_boxes_for_selection(lines[0].start_index, lines[0].end_excluding_whitespace);
    assert!((boxes.last().expect("first line selection").rect.right() - edge.dx).abs() < EPS);
    let (paragraph, offset) = painted(&justified);
    let (old, old_offset) = painted(&left);
    let last_glyph = paragraph
        .runs()
        .next()
        .expect("first painted line")
        .glyphs()
        .last()
        .expect("last glyph")
        .x;
    let old_glyph = old
        .runs()
        .next()
        .expect("original first line")
        .glyphs()
        .last()
        .expect("original last glyph")
        .x;
    assert!(f64::from(last_glyph - old_glyph) + offset.dx - old_offset.dx > 1.0);
    assert!(
        (left.min_intrinsic_width(&mut context) - justified.min_intrinsic_width(&mut context))
            .abs()
            < EPS
    );
}

pub(crate) fn justification_leaves_hard_break_lines_unstretched() {
    let text = "one two\nthree four";
    let left = aligned_painter(text, TextAlign::Left, TextDirection::Ltr, 200.0, 200.0);
    let justified = aligned_painter(text, TextAlign::Justify, TextDirection::Ltr, 200.0, 200.0);
    compare_aligned_lines(&left, &justified, 0.0);
    for (a, b) in left
        .get_line_metrics()
        .iter()
        .zip(justified.get_line_metrics())
    {
        assert!((a.width - b.width).abs() < EPS);
    }
}

pub(crate) fn alignment_change_replaces_cached_positions() {
    let mut context = TextContext::new(&FontCollection::new());
    let mut changed = painter("WWWW\ni", TextDirection::Ltr).with_text_align(TextAlign::Left);
    changed.layout(&mut context, 200.0, 200.0);
    let left = aligned_painter("WWWW\ni", TextAlign::Left, TextDirection::Ltr, 200.0, 200.0);
    changed.set_text_align(TextAlign::Center);
    changed.layout(&mut context, 200.0, 200.0);
    compare_aligned_lines(&left, &changed, 0.5);
    let before = painted(&changed).0;
    changed.set_text_align(TextAlign::Center);
    changed.layout(&mut context, 200.0, 200.0);
    assert!(
        Arc::ptr_eq(&before, &painted(&changed).0),
        "unchanged alignment retains paint records"
    );
    changed.set_text_align(TextAlign::Right);
    changed.layout(&mut context, 200.0, 200.0);
    compare_aligned_lines(&left, &changed, 1.0);
}

pub(crate) fn ellipsized_lines_align_only_the_kept_text() {
    let text = "WWWW\ni\na much longer dropped line";
    let mut context = TextContext::new(&FontCollection::new());
    let build = |align| {
        painter(text, TextDirection::Ltr)
            .with_text_align(align)
            .with_max_lines(Some(2))
            .with_ellipsis(Some("…".to_string()))
    };
    let mut left = build(TextAlign::Left);
    left.layout(&mut context, 200.0, 200.0);
    let mut centered = build(TextAlign::Center);
    centered.layout(&mut context, 200.0, 200.0);
    assert!(centered.did_exceed_max_lines());
    assert_eq!(painted(&centered).0.line_count(), 2);
    assert!(painted(&centered).0.text().ends_with('…'));
    compare_aligned_lines(&left, &centered, 0.5);
    let end = centered.get_offset_for_caret(TextPosition::upstream(text.len()));
    assert!(end.dy < centered.height());
}

pub(crate) fn unbounded_breaking_uses_the_minimum_allocated_width() {
    unequal_lines(
        TextAlign::Right,
        TextDirection::Ltr,
        200.0,
        f64::INFINITY,
        1.0,
    );
}
