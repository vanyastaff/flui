//! Parley's paragraph metrics and painted runs on the bundled Roboto
//! (ADR-0092 §8 gate 8).
//!
//! `TextPainter` measures on Parley and paints the runs of the same layout,
//! while its caret queries read a cosmic-text layout until ADR-0092 §10
//! step 5. `parley_metrics_round_to_todays_baseline` lays a painter out with
//! Roboto named, with no family and with the monospace generic, regular and
//! bold, which both shapers must resolve to the bundled Roboto Regular
//! rather than a host face (painting mapping decision 16), and compares it
//! with the cosmic-text layout of the same text: the paragraph height, a
//! single line's width, and the device row the first baseline is painted on
//! (`ShapedRun::placed_glyphs`, `round(baseline × scale)`), which is where
//! cosmic-text placed it.
//!
//! `measured_lines_are_painted_lines` extends it past one line: wrapped
//! paragraphs, hard breaks, and style combinations the painter normalizes
//! measure the lines and height they paint, and hard breaks lay out the lines
//! Parley gives them.

#[path = "support/cases.rs"]
mod cases;

use flui_foundation::geometry::Offset;
use flui_painting::glyphs::FontRegistry;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, ShapedParagraph, TextBaseline, TextContext, TextLayout,
    TextPainter,
};

const SIZES: [f64; 5] = [13.0, 14.0, 16.0, 18.0, 32.0];
const HEIGHTS: [Option<f64>; 2] = [None, Some(1.5)];
const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];
const TEXT: &str = "Hamburgefonstiv 0123";
const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
/// Roboto by name, the default family, and a generic other than sans-serif.
const FAMILIES: [Option<&str>; 3] = [Some("Roboto"), None, Some("monospace")];
/// Regular, and a weight the bundled Roboto has no face for.
const WEIGHTS: [FontWeight; 2] = [FontWeight::W400, FontWeight::W700];

/// The paragraph `painter` records.
fn painted(painter: &TextPainter) -> std::sync::Arc<ShapedParagraph> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(std::sync::Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// The device row the first run's glyphs are placed on at `scale`, from the
/// origin.
fn first_baseline_row(paragraph: &ShapedParagraph, scale: f32) -> i32 {
    let mut fonts = FontRegistry::new();
    let run = paragraph.runs().next().expect("the text shapes a run");
    let key = fonts.prepare_run(&run).expect("a shaped face registers");
    run.placed_glyphs(key, (0.0, 0.0), scale)
        .next()
        .expect("the run has a glyph")
        .y
}

/// Every family, weight, size, line height and scale factor paints the first
/// baseline on the device row cosmic-text placed it on, with the paragraph
/// height it measured and a single line's width within a hundredth of a
/// pixel.
#[test]
fn parley_metrics_round_to_todays_baseline() {
    // With `bundled-fonts` the process font system already carries Roboto
    // and binds the generic families to it (painting mapping decision 16).
    // Registering it again adds a face but moves no generic binding, so the
    // default-family rows still depend on that one.
    flui_painting::shared_font_system()
        .register_font(ROBOTO)
        .expect("the bundled Roboto loads");
    let mut context = TextContext::new(&FontCollection::new());
    let mut failures = Vec::new();
    for (family, weight, size, height) in FAMILIES.into_iter().flat_map(|family| {
        WEIGHTS.into_iter().flat_map(move |weight| {
            SIZES.into_iter().flat_map(move |size| {
                HEIGHTS
                    .into_iter()
                    .map(move |height| (family, weight, size, height))
            })
        })
    }) {
        let style = TextStyle {
            font_family: family.map(str::to_owned),
            font_weight: Some(weight),
            font_size: Some(size),
            height,
            ..TextStyle::default()
        };
        let mut painter = TextPainter::new()
            .with_text(TextSpan::styled(TEXT, style.clone()))
            .with_text_direction(TextDirection::Ltr);
        painter.layout(&mut context, 0.0, f64::INFINITY);
        let paragraph = painted(&painter);
        let cosmic =
            TextLayout::new(TEXT, Some(&style), size, None, None, TextDirection::Ltr).metrics();
        let case = format!("{family:?} {weight:?} {size} px, height {height:?}");
        if (painter.height() - cosmic.height).abs() > 1e-3
            || (paragraph.size().height - cosmic.height).abs() > 1e-3
        {
            failures.push(format!(
                "{case}: height measured {} painted {} cosmic-text {}",
                painter.height(),
                paragraph.size().height,
                cosmic.height
            ));
        }
        if (painter.width() - cosmic.width).abs() > 0.01 {
            failures.push(format!(
                "{case}: width measured {} cosmic-text {}",
                painter.width(),
                cosmic.width
            ));
        }
        let alphabetic = painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic);
        for scale in SCALES {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "cosmic-text's own baseline rule: a rounded device row"
            )]
            let todays = (cosmic.alphabetic_baseline * f64::from(scale)).round() as i32;
            let row = first_baseline_row(&paragraph, scale);
            if row != todays {
                failures.push(format!(
                    "{case}, scale {scale}: baseline painted on row {row}, cosmic-text's row                      {todays} (measured baseline {alphabetic})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

/// Lays `painter` out at `max_width` and returns it with the paragraph it
/// paints, after asserting it measured that paragraph's height, and, for a
/// single line (`max_width` unbounded), its width.
fn assert_measures_what_it_paints(
    mut painter: TextPainter,
    max_width: f64,
) -> (TextPainter, std::sync::Arc<ShapedParagraph>) {
    let mut context = TextContext::new(&FontCollection::new());
    painter.layout(&mut context, 0.0, max_width);
    let painted = painted(&painter);
    assert!(
        (painter.height() - painted.size().height).abs() < 1e-3,
        "measured {} high, painted {} high ({} lines)",
        painter.height(),
        painted.size().height,
        painted.line_count()
    );
    if max_width.is_infinite() {
        assert!(
            (painter.width() - painted.size().width).abs() < 0.01,
            "measured {} wide, painted {} wide",
            painter.width(),
            painted.size().width
        );
    }
    (painter, painted)
}

/// Asserts `text` paints `lines` lines, each as tall as one line of the
/// default style, and measures what it paints.
fn assert_paints_lines(text: &str, lines: usize) {
    let (one, _) =
        assert_measures_what_it_paints(painter("A", TextStyle::default()), f64::INFINITY);
    let (measured, painted) =
        assert_measures_what_it_paints(painter(text, TextStyle::default()), f64::INFINITY);
    assert_eq!(
        painted.line_count(),
        lines,
        "{text:?} paints {} lines, not {lines}",
        painted.line_count()
    );
    #[expect(clippy::cast_precision_loss, reason = "a handful of lines")]
    let expected = one.height() * lines as f64;
    assert!(
        (measured.height() - expected).abs() < 1e-3,
        "{text:?} measures {} high, {lines} lines are {expected}",
        measured.height()
    );
}

/// A trailing newline ends a line and starts an empty one: Parley's reading,
/// which the owner chose (ADR-0092 §10 step 4).
fn a_trailing_newline_is_a_line() {
    assert_paints_lines("A\n", 2);
}

/// CR LF breaks the line once, as LF does: `"A\r\nB"` is two lines, and a
/// trailing CR LF adds one empty line, as a trailing LF does (painting
/// mapping decision 18). Parley alone breaks at the CR and again at the LF.
fn crlf_breaks() {
    assert_paints_lines("A\r\nB", 2);
    assert_paints_lines("A\r\n", 2);
}

/// U+2028 LINE SEPARATOR breaks the line.
fn a_line_separator_breaks() {
    assert_paints_lines("A\u{2028}B", 2);
}

/// U+2029 PARAGRAPH SEPARATOR breaks the line, trailing or not.
fn a_paragraph_separator_breaks() {
    assert_paints_lines("A\u{2029}B", 2);
    assert_paints_lines("A\u{2029}", 2);
}

/// U+0085 NEXT LINE does not break the line.
fn next_line_does_not_break() {
    assert_paints_lines("A\u{85}B", 1);
}

fn painter(text: &str, style: TextStyle) -> TextPainter {
    TextPainter::new()
        .with_text(TextSpan::styled(text, style))
        .with_text_direction(TextDirection::Ltr)
}

/// A word wider than the line breaks between its glyphs on both shapers.
fn an_overlong_word_breaks_between_glyphs() {
    assert_measures_what_it_paints(painter("supercalifragilistic", TextStyle::default()), 50.0);
}

/// Words, one of which is wider than the line.
fn words_break_where_paint_breaks_them() {
    assert_measures_what_it_paints(painter("one two three four", TextStyle::default()), 30.0);
}

/// A short word, then one that must break by glyph on a line of its own.
fn a_short_word_then_an_overlong_one() {
    assert_measures_what_it_paints(
        painter("a supercalifragilistic", TextStyle::default()),
        50.0,
    );
}

/// A hard break between two wrapped paragraphs.
fn a_hard_break_between_wrapped_lines() {
    assert_measures_what_it_paints(painter("one two\nthree four", TextStyle::default()), 30.0);
}

fn spaced() -> TextStyle {
    TextStyle {
        letter_spacing: Some(10.0),
        ..TextStyle::default()
    }
}

/// Letter spacing on a style that sets no size applies at the default size.
fn letter_spacing_without_a_size() {
    assert_measures_what_it_paints(painter("AAAA", spaced()), f64::INFINITY);
}

/// The same, under a text scale factor.
fn scaled_letter_spacing_without_a_size() {
    assert_measures_what_it_paints(
        painter("AAAA", spaced()).with_text_scale_factor(1.5),
        f64::INFINITY,
    );
}

/// A line height on a style that sets no size is a multiple of the default.
fn line_height_without_a_size() {
    let style = TextStyle {
        height: Some(2.0),
        ..TextStyle::default()
    };
    assert_measures_what_it_paints(painter("A", style), f64::INFINITY);
}

/// `max_lines` of zero keeps every line on both shapers.
fn zero_max_lines_keeps_every_line() {
    assert_measures_what_it_paints(
        painter("one two three four", TextStyle::default()).with_max_lines(Some(0)),
        30.0,
    );
}

/// Each row measures the height it paints, and a single line its width; the
/// hard-break rows also paint the number of lines they name.
#[test]
fn measured_lines_are_painted_lines() {
    cases::run_cases(
        "measured_lines_are_painted_lines",
        &[
            (
                "an_overlong_word_breaks_between_glyphs",
                an_overlong_word_breaks_between_glyphs,
            ),
            (
                "words_break_where_paint_breaks_them",
                words_break_where_paint_breaks_them,
            ),
            (
                "a_short_word_then_an_overlong_one",
                a_short_word_then_an_overlong_one,
            ),
            (
                "a_hard_break_between_wrapped_lines",
                a_hard_break_between_wrapped_lines,
            ),
            (
                "letter_spacing_without_a_size",
                letter_spacing_without_a_size,
            ),
            (
                "scaled_letter_spacing_without_a_size",
                scaled_letter_spacing_without_a_size,
            ),
            ("line_height_without_a_size", line_height_without_a_size),
            (
                "zero_max_lines_keeps_every_line",
                zero_max_lines_keeps_every_line,
            ),
            ("a_trailing_newline_is_a_line", a_trailing_newline_is_a_line),
            ("crlf_breaks", crlf_breaks),
            ("a_line_separator_breaks", a_line_separator_breaks),
            ("a_paragraph_separator_breaks", a_paragraph_separator_breaks),
            ("next_line_does_not_break", next_line_does_not_break),
        ],
    );
}
