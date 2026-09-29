//! The font system's three doors: shaping never moves the database
//! generation, `register_font` moves it exactly once per call, and a laid-out
//! `TextPainter` re-lays-out once a face has been registered.
//!
//! Its own test target: these tests append to the process-wide font database,
//! which the `painting_it` binary's tests deliberately never do.

use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{TextPainter, shared_font_system};

const PROBE_SANS: &[u8] = include_bytes!("../assets/fonts/probe-sans-400.ttf");
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

#[test]
fn register_font_bumps_the_generation_once() {
    let fonts = shared_font_system();
    let before = fonts.generation();
    fonts
        .register_font(PROBE_SANS)
        .expect("FLUI Probe Sans loads");
    assert_eq!(fonts.generation(), before + 1);
    assert!(
        fonts.register_font(b"not a font").is_err(),
        "zero loadable faces is an error"
    );
    assert_eq!(
        fonts.generation(),
        before + 1,
        "a failed registration is not a change"
    );
}

#[test]
fn register_font_invalidates_a_laid_out_painter() {
    let fonts = shared_font_system();
    let mut painter = probe_painter("iiii wwww");
    painter.layout(0.0, f64::INFINITY);
    let before = painter.size();
    // Same constraints: without a registration this is the cached early
    // return, and the size cannot change.
    painter.layout(0.0, f64::INFINITY);
    assert_eq!(painter.size(), before);

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    painter.layout(0.0, f64::INFINITY);
    assert_ne!(
        painter.size(),
        before,
        "a face registered after layout must shape the same text again: in the \
         proportional fallback 'iiii' and 'wwww' differ, in the monospace probe they do not"
    );
}
