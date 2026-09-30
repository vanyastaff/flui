//! The font system's three doors: shaping never moves the database
//! generation, `register_font` moves it exactly once per call, and a laid-out
//! `TextPainter` re-lays-out once a face has been registered.
//!
//! Its own test target: these tests append to the process-wide font database,
//! which the `painting_it` binary's tests deliberately never do.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_painting::text_layout::TextLayout;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, DrawOp, TextPainter, shared_font_system};

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

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

/// The layout `paint` records for `painter`.
fn painted_layout(painter: &TextPainter) -> Arc<TextLayout> {
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let list = canvas.finish();
    list.iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { layout, .. } => Some(Arc::clone(layout)),
            _ => None,
        })
        .expect("a laid-out painter records a paragraph")
}

/// Measurement shapes on Parley through the lent context and never reads the
/// process font system, while paint still shapes on it (painting mapping
/// decision 15): a face registered there re-shapes the painted layout and
/// leaves the measured size alone. With cosmic-text measurement the size would
/// follow the painted layout and change with it.
///
/// This pins a known gap, not the contract: once registration goes through the
/// collection (the rest of ADR-0092 §10 step 3b), a registered face must reach
/// measurement as well, and this test is inverted rather than deleted.
#[test]
fn a_face_registered_on_the_process_font_system_reaches_paint_not_measurement() {
    let mut text_cx = text_cx();
    let mut painter = probe_painter("iiii wwww");
    painter.layout(&mut text_cx, 0.0, f64::INFINITY);
    let measured = painter.size();
    let painted = painted_layout(&painter);

    shared_font_system()
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    painter.layout(&mut text_cx, 0.0, f64::INFINITY);
    let repainted = painted_layout(&painter);

    assert!(
        !Arc::ptr_eq(&painted, &repainted),
        "a face registered on the process font system shapes the painted layout again"
    );
    assert!(
        (repainted.metrics().width - painted.metrics().width).abs() > 1.0,
        "the painted layout moves to the probe: in the proportional fallback 'iiii' and \
         'wwww' differ, in the monospace probe they do not ({} vs {})",
        painted.metrics().width,
        repainted.metrics().width
    );
    assert_eq!(
        painter.size(),
        measured,
        "measurement is Parley's on the context's collection, which the process \
         registration does not reach"
    );
}
