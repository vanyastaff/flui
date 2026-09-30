//! A face registered on the app's collection reaches a laid-out
//! `TextPainter`'s measurement, paint and caret layout together.
//!
//! Its own test target: it appends to the process-wide font database, which
//! the `painting_it` binary's tests deliberately never do.

use std::sync::Arc;

use flui_foundation::geometry::Offset;

use flui_painting::glyphs::FaceKey;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, ShapedParagraph, TextContext, TextPainter, shared_font_system,
};

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

/// The face every run of the painted paragraph names.
fn painted_faces(paragraph: &ShapedParagraph) -> Vec<FaceKey> {
    paragraph.runs().map(|run| run.face().key()).collect()
}

/// A face registered through the one door, the app's collection, reaches
/// measurement, paint and carets together at the next `layout()`: the
/// collection measures and paints with it, and the process font system it was
/// fed from lays carets out with it. Before that `layout()` the painter keeps
/// what it had, carets included.
///
/// Fails if the collection keeps no caret side (the carets stay on the
/// fallback), or if a registration reaches the process font system alone
/// (measurement and paint stay on the fallback).
#[test]
fn a_face_registered_on_the_collection_reaches_measurement_paint_and_carets() {
    let fonts = FontCollection::with_host_faces(&shared_font_system());
    let mut text_cx = TextContext::new(&fonts);
    let mut painter = probe_painter("iiii wwww");
    painter.layout(&mut text_cx, 0.0, WIDTH);
    let measured = painter.size();
    let painted = painted_paragraph(&painter);
    let caret_width = caret_line_width(&painter);

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    // Until the next `layout()` the cursor queries keep the caret layout
    // they have: shaping it again on every query would put a full cosmic-text
    // shape, under the process font lock, on each caret and selection read.
    for query in 0..2 {
        assert_eq!(
            caret_line_width(&painter),
            caret_width,
            "caret query {query} after the registration and before `layout()` reads the              caret layout it already had"
        );
    }
    painter.layout(&mut text_cx, 0.0, WIDTH);

    // In the proportional fallback 'iiii' and 'wwww' differ in width, in the
    // monospace probe they do not, so each side moves by more than rounding.
    assert!(
        (painter.size().width - measured.width).abs() > 1.0,
        "measurement moves to the probe ({} vs {})",
        measured.width,
        painter.size().width
    );
    let repainted = painted_paragraph(&painter);
    assert_ne!(
        painted_faces(&repainted),
        painted_faces(&painted),
        "paint draws the probe face"
    );
    assert_eq!(
        repainted.size(),
        painter.size(),
        "paint draws the layout that measured"
    );
    assert!(
        (caret_line_width(&painter) - caret_width).abs() > 1.0,
        "the caret layout moves to the probe ({caret_width} vs {})",
        caret_line_width(&painter)
    );
}
