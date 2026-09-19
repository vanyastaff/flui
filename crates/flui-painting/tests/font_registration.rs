//! The font system's three doors: shaping never moves the database
//! generation, `register_font` moves it exactly once per call, and a laid-out
//! `TextPainter` re-lays-out once a face has been registered.
//!
//! Its own test target: these tests append to the process-wide font database,
//! which the `painting_it` binary's tests deliberately never do.

use flui_painting::{TextPainter, shared_font_system};
use flui_types::typography::{FontWeight, TextDirection, TextSpan, TextStyle};

const PROBE_SANS: &[u8] = include_bytes!("../assets/fonts/probe-sans-400.ttf");
const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");

fn painter(text: &str) -> TextPainter {
    TextPainter::new()
        .with_text(TextSpan::new(text))
        .with_text_direction(TextDirection::Ltr)
}

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
fn shaping_never_bumps_the_generation() {
    let fonts = shared_font_system();
    let before = fonts.generation();
    for _ in 0..3 {
        painter("shape me").layout(0.0, f32::INFINITY);
        fonts.shape(|shaper| {
            let _ = shaper.resolve_font(None);
        });
    }
    assert_eq!(fonts.generation(), before);
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
    painter.layout(0.0, f32::INFINITY);
    let before = painter.size();
    // Same constraints: without a registration this is the cached early
    // return, and the size cannot change.
    painter.layout(0.0, f32::INFINITY);
    assert_eq!(painter.size(), before);

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    painter.layout(0.0, f32::INFINITY);
    assert_ne!(
        painter.size(),
        before,
        "a face registered after layout must shape the same text again: in the \
         proportional fallback 'iiii' and 'wwww' differ, in the monospace probe they do not"
    );
}

/// An `Icon` measured with no engine in the process shapes in the embedded
/// icon face, not in whatever fallback the host offers: U+E87D ("favorite"
/// in Material Icons) has a non-zero advance through `TextLayout` alone.
#[test]
fn icon_fonts_measure_before_any_engine_exists() {
    let style = TextStyle {
        font_family: Some("Material Icons".to_string()),
        ..TextStyle::default()
    };
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("\u{e87d}").with_style(style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(0.0, f32::INFINITY);
    let width = painter.size().width.0;
    assert!(
        width > 10.0,
        "the icon glyph must have a real advance, got {width}"
    );

    let fonts = shared_font_system();
    let resolved_family = fonts.shape(|shaper| {
        let style = TextStyle {
            font_family: Some("Material Icons".to_string()),
            ..TextStyle::default()
        };
        format!("{:?}", shaper.resolve_font(Some(&style)).family)
    });
    assert!(
        resolved_family.contains("Material Icons"),
        "the style must resolve to the embedded family, got {resolved_family}"
    );
}
