//! A face registered on the process font system reaches a laid-out
//! `TextPainter`'s caret layout, and not its measurement or paint, until
//! registration moves to the collection.
//!
//! Its own test target: these tests append to the process-wide font database,
//! which the `painting_it` binary's tests deliberately never do.

use std::sync::Arc;

use flui_foundation::geometry::Offset;

use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, DrawOp, ShapedParagraph, TextPainter, shared_font_system};

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

/// Wide enough for the probe text on one line, and finite: the painter
/// keeps its cached layout only for a finite width it was laid out at.
const WIDTH: f64 = 400.0;

const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");

/// The same text styled with the probe family: before the face is registered
/// it resolves to sans-serif, after it to the monospace probe.
fn probe_painter(text: &str) -> TextPainter {
    // The probe ships a single face at weight 100; asking for it by weight is
    // what keeps cosmic-text on the family once it is present.
    let style = TextStyle {
        font_family: Some("FLUI Probe Mono".to_string()),
        font_weight: Some(FontWeight::W100),
        ..TextStyle::default()
    };
    TextPainter::new()
        .with_text(TextSpan::new(text).with_style(style))
        .with_text_direction(TextDirection::Ltr)
}

/// The paragraph `paint` records for `painter`.
fn painted_paragraph(painter: &TextPainter) -> Arc<ShapedParagraph> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// The width of the first line the caret queries read.
fn caret_line_width(painter: &TextPainter) -> f64 {
    painter
        .get_line_metrics()
        .first()
        .expect("a laid-out painter has a line")
        .width
}

/// Measurement and paint shape on Parley through the lent context and never
/// read the process font system, while carets still shape on it (painting
/// mapping decision 15): a face registered there re-shapes the caret layout
/// once, at the next `layout()`, and leaves the measured size and the painted
/// paragraph alone.
///
/// This pins a known gap, not the contract: once registration goes through the
/// collection (the rest of ADR-0092 §10 step 3b), a registered face must reach
/// measurement and paint as well, and this test is inverted rather than
/// deleted.
#[test]
fn a_face_registered_on_the_process_font_system_reaches_carets_not_measurement_or_paint() {
    let mut text_cx = text_cx();
    let mut painter = probe_painter("iiii wwww");
    painter.layout(&mut text_cx, 0.0, WIDTH);
    let measured = painter.size();
    let painted = painted_paragraph(&painter);
    let caret_width = caret_line_width(&painter);

    shared_font_system()
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    // Until the next `layout()` the cursor queries keep the caret layout
    // they have: shaping it again on every query would put a full cosmic-text
    // shape, under the process font lock, on each caret and selection read.
    for query in 0..2 {
        assert_eq!(
            caret_line_width(&painter),
            caret_width,
            "caret query {query} after the registration and before `layout()` reads the \
             caret layout it already had"
        );
    }
    painter.layout(&mut text_cx, 0.0, WIDTH);

    assert!(
        Arc::ptr_eq(&painted, &painted_paragraph(&painter)),
        "the process registration does not reach the painted paragraph"
    );
    assert!(
        (caret_line_width(&painter) - caret_width).abs() > 1.0,
        "the caret layout moves to the probe: in the proportional fallback 'iiii' and \
         'wwww' differ, in the monospace probe they do not ({caret_width} vs {})",
        caret_line_width(&painter)
    );
    assert_eq!(
        painter.size(),
        measured,
        "measurement is Parley's on the context's collection, which the process \
         registration does not reach"
    );
}
