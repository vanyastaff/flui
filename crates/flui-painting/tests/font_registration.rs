//! A face registered on the app's collection reaches a laid-out
//! `TextPainter`'s measurement, paint and carets together.
//!
//! The collection is fed from a scan of the host's fonts, as the app's is.

use std::sync::Arc;

use flui_foundation::geometry::Offset;

use flui_painting::glyphs::FaceKey;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{
    Canvas, DrawOp, FontCollection, HostFonts, ShapedParagraph, TextContext, TextPainter,
};

/// Wide enough for the probe text on one line, and finite: the painter
/// keeps its cached layout only for a finite width it was laid out at.
const WIDTH: f64 = 400.0;

const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");

/// The same text styled with the probe family: before the face is registered
/// it resolves to sans-serif, after it to the monospace probe.
fn probe_painter(text: &str) -> TextPainter {
    // The probe ships a single face at weight 100.
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
/// measurement, paint and carets together at the next `layout()`, which
/// shapes the one layout all three read. Before that `layout()` the painter
/// keeps what it had, carets included.
///
/// Fails if caret queries read another layout than the one that measured
/// and painted (the carets stay on the fallback), or if a registration misses
/// the collection (measurement and paint stay on the fallback).
#[test]
fn a_face_registered_on_the_collection_reaches_measurement_paint_and_carets() {
    let fonts = FontCollection::with_host_fonts(&HostFonts::scan());
    let mut text_cx = TextContext::new(&fonts);
    let mut painter = probe_painter("iiii wwww");
    painter.layout(&mut text_cx, 0.0, WIDTH);
    let measured = painter.size();
    let painted = painted_paragraph(&painter);
    let caret_width = caret_line_width(&painter);

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    // Until the next `layout()` the cursor queries read the layout the
    // painter measured and painted: a query never shapes.
    for query in 0..2 {
        assert_eq!(
            caret_line_width(&painter),
            caret_width,
            "caret query {query} after the registration and before `layout()` reads the \
             layout it already had"
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
        "the carets move to the probe ({caret_width} vs {})",
        caret_line_width(&painter)
    );
}

const PROBE_SANS: &[u8] = include_bytes!("../assets/fonts/probe-sans-400.ttf");
/// "FLUI Probe Mono" at weight 600: a second face of the probe family.
const PROBE_MONO_600: &[u8] = include_bytes!("../assets/fonts/probe-mono-600.ttf");

/// The faces `fonts` paints `AAAA` in, styled `FLUI Probe Mono` at `weight`.
fn probe_faces(fonts: &FontCollection, weight: FontWeight) -> Vec<FaceKey> {
    let style = TextStyle {
        font_family: Some("FLUI Probe Mono".to_string()),
        font_weight: Some(weight),
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("AAAA").with_style(style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(&mut TextContext::new(fonts), 0.0, WIDTH);
    painted_faces(&painted_paragraph(&painter))
}

/// A host feed adds the host's faces and then raises the collection's
/// generation exactly once, however many sources it added, so each
/// pipeline lays its text out once more when the feed lands. Before it
/// runs the collection holds the bundled faces alone. Fails if the feed
/// announces each file (the generation rises twice here) or nothing.
fn a_host_feed_raises_the_generation_once() {
    use flui_painting::testing::{collection_holds, feed_with_host, host_fed, host_fonts_from};

    let (fonts, feed) = FontCollection::with_host_feed();
    let before = fonts.generation();
    assert!(!host_fed(&fonts));
    assert!(!collection_holds(&fonts, "FLUI Probe Mono"));

    feed_with_host(feed, host_fonts_from(&[PROBE_MONO, PROBE_SANS])).run();

    assert!(collection_holds(&fonts, "FLUI Probe Mono"));
    assert!(collection_holds(&fonts, "FLUI Probe Sans"));
    assert!(host_fed(&fonts));
    assert_eq!(
        fonts.generation(),
        before + 1,
        "two sources fed, one announcement"
    );
}

/// A family the collection holds when the feed starts (here registered by
/// the app) is not fed again: the host's weight-600 face of it stays out,
/// so a weight-600 style keeps painting in the registered face. Fails if
/// the feed adds a host copy to a family the app already has.
fn a_family_held_before_the_feed_is_not_fed_again() {
    use flui_painting::testing::{feed_with_host, host_fonts_from};

    let (fonts, feed) = FontCollection::with_host_feed();
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    let registered = probe_faces(&fonts, FontWeight::W600);

    feed_with_host(feed, host_fonts_from(&[PROBE_MONO_600])).run();

    assert_eq!(
        probe_faces(&fonts, FontWeight::W600),
        registered,
        "the family keeps only the face the app registered"
    );
}

/// A feed that changes nothing leaves the generation alone, so no
/// pipeline lays its text out again for it: a host with no fonts (as on
/// wasm32), and a host whose only family the collection already holds.
/// Fails if the feed announces every landing whatever it added.
fn a_feed_that_adds_nothing_leaves_the_generation_alone() {
    use flui_painting::testing::{feed_with_host, host_fed, host_fonts_from};

    let (fonts, feed) = FontCollection::with_host_feed();
    let before = fonts.generation();
    feed_with_host(feed, host_fonts_from(&[])).run();
    assert!(host_fed(&fonts), "the feed ran");
    assert_eq!(fonts.generation(), before, "an empty host adds nothing");

    let (fonts, feed) = FontCollection::with_host_feed();
    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    let registered = fonts.generation();
    feed_with_host(feed, host_fonts_from(&[PROBE_MONO_600])).run();
    assert_eq!(
        fonts.generation(),
        registered,
        "a host holding only a family the app registered adds nothing"
    );
}

#[test]
fn host_feed_contract() {
    crate::cases::run_cases(
        "host_feed_contract",
        &[
            (
                "a_host_feed_raises_the_generation_once",
                a_host_feed_raises_the_generation_once,
            ),
            (
                "a_family_held_before_the_feed_is_not_fed_again",
                a_family_held_before_the_feed_is_not_fed_again,
            ),
            (
                "a_feed_that_adds_nothing_leaves_the_generation_alone",
                a_feed_that_adds_nothing_leaves_the_generation_alone,
            ),
        ],
    );
}
