//! Carets, selection boxes, hit-testing and word boundaries read the layout
//! that measured and painted (ADR-0092 §10 step 5; flui-painting
//! `ARCHITECTURE.md`, mapping decision 15).
//!
//! Every row lays a `TextPainter` out on a collection holding only the
//! bundled faces and asks its public queries.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_painting::typography::{TextAffinity, TextDirection, TextPosition, TextSpan, TextStyle};
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
/// painter's alignment offset left out.
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
