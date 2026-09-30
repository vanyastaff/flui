//! Parley's paragraph metrics against the cosmic-text layout the painter
//! records, on the bundled Roboto (ADR-0092 §8 gate 8, the measurement side).
//!
//! `TextPainter` measures on Parley and paints a cosmic-text layout until
//! ADR-0092 §10 step 4b, so measured and painted text agree only while both
//! shape the same face to the same metrics. Each case lays a painter out on
//! one context, then reads the `Paragraph` it paints, with Roboto named, with
//! no family and with the monospace generic, regular and bold, which both
//! sides must resolve to the bundled Roboto Regular rather than a host face
//! (painting mapping decision 16). The
//! comparison is the one the painter makes observable: a baseline placed on
//! the device grid as `(line_y * scale).round()`
//! (`TextLayout::placed_glyphs`), the paragraph height, and a single line's
//! width.
//!
//! `measured_lines_are_painted_lines` extends it past one line: wrapped
//! paragraphs, and style combinations the two shapers read differently
//! unless the painter normalizes them, measure the height they paint.

#[path = "support/cases.rs"]
mod cases;

use flui_foundation::geometry::Offset;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, TextBaseline, TextContext, TextLayoutResult, TextPainter,
};

const SIZES: [f64; 5] = [13.0, 14.0, 16.0, 18.0, 32.0];
const HEIGHTS: [Option<f64>; 2] = [None, Some(1.5)];
const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
const TEXT: &str = "Hamburgefonstiv 0123";
const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
/// Roboto by name, the default family, and a generic other than sans-serif.
const FAMILIES: [Option<&str>; 3] = [Some("Roboto"), None, Some("monospace")];
/// Regular, and a weight the bundled Roboto has no face for.
const WEIGHTS: [FontWeight; 2] = [FontWeight::W400, FontWeight::W700];

struct Measured {
    width: f64,
    height: f64,
    alphabetic: f64,
}

/// The metrics of the layout `painter` records.
fn painted(painter: &TextPainter) -> TextLayoutResult {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { layout, .. } => Some(layout.metrics()),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// What the painter measured, and the metrics of the layout it paints.
fn measure_and_paint(
    context: &mut TextContext,
    family: Option<&str>,
    weight: FontWeight,
    size: f64,
    height: Option<f64>,
) -> (Measured, Measured) {
    let style = TextStyle {
        font_family: family.map(str::to_owned),
        font_weight: Some(weight),
        font_size: Some(size),
        height,
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled(TEXT, style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(context, 0.0, f64::INFINITY);
    let measured = Measured {
        width: painter.width(),
        height: painter.height(),
        alphabetic: painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
    };

    let painted = painted(&painter);
    let painted = Measured {
        width: painted.width,
        height: painted.height,
        alphabetic: painted.alphabetic_baseline,
    };
    (measured, painted)
}

/// Every family, weight, size, line height and scale factor places the first baseline
/// on the same device row in the measurement and the painted layout, with
/// equal paragraph height and a single line's width within a hundredth of a
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
        let (parley, cosmic) = measure_and_paint(&mut context, family, weight, size, height);
        let case = format!("{family:?} {weight:?} {size} px, height {height:?}");
        if (parley.height - cosmic.height).abs() > 1e-3 {
            failures.push(format!(
                "{case}: height measured {} painted {}",
                parley.height, cosmic.height
            ));
        }
        if (parley.width - cosmic.width).abs() > 0.01 {
            failures.push(format!(
                "{case}: width measured {} painted {}",
                parley.width, cosmic.width
            ));
        }
        for scale in SCALES {
            let device = |baseline: f64| (baseline * scale).round();
            if device(parley.alphabetic) != device(cosmic.alphabetic) {
                failures.push(format!(
                    "{case}, scale {scale}: baseline measured {} painted {}",
                    parley.alphabetic, cosmic.alphabetic
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Lays `painter` out at `max_width` and asserts it measured the height of
/// the layout it paints, and, for a single line (`max_width` unbounded), its
/// width. A wrapped line's width is not compared: Parley leaves trailing
/// whitespace out of it and the painted layout keeps it (painting mapping
/// decision 15).
fn assert_measures_what_it_paints(mut painter: TextPainter, max_width: f64) {
    let mut context = TextContext::new(&FontCollection::new());
    painter.layout(&mut context, 0.0, max_width);
    let painted = painted(&painter);
    assert!(
        (painter.height() - painted.height).abs() < 1e-3,
        "measured {} high, painted {} high ({} lines)",
        painter.height(),
        painted.height,
        painted.line_count
    );
    if max_width.is_infinite() {
        assert!(
            (painter.width() - painted.width).abs() < 0.01,
            "measured {} wide, painted {} wide",
            painter.width(),
            painted.width
        );
    }
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

/// Each row measures the height it paints, and a single line its width.
/// Hard breaks other than an interior `\n` are not rows: the two shapers
/// break at different characters and treat a trailing break differently
/// (painting mapping decision 15).
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
        ],
    );
}
