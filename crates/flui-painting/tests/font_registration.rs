//! The font system's doors: shaping never moves the database generation,
//! `register_font` moves it exactly once per call, a laid-out `TextPainter`
//! re-lays-out once a face has been registered, and a requested weight is
//! resolved to one the registered family can serve.
//!
//! Its own test target: these tests append to the process-wide font database,
//! which the `painting_it` binary's tests deliberately never do.

use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{TextPainter, shared_font_system};

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

#[path = "support/cases.rs"]
mod cases;

const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
/// The same monospaced family as `PROBE_MONO`, at weight 600.
const PROBE_MONO_600: &[u8] = include_bytes!("../assets/fonts/probe-mono-600.ttf");
/// One face with `usWeightClass` 400 and an `fvar` `wght` axis spanning
/// 100..900.
const PROBE_VARIABLE: &[u8] = include_bytes!("../assets/fonts/probe-variable-wght.ttf");

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
#[cfg_attr(
    feature = "parley-layout",
    ignore = "painting mapping decision 15: registration reaches the process font system, not the collection Parley measures on"
)]
fn register_font_invalidates_a_laid_out_painter() {
    let fonts = shared_font_system();
    let mut painter = probe_painter("iiii wwww");
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);
    let before = painter.size();
    // Same constraints: without a registration this is the cached early
    // return, and the size cannot change.
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);
    assert_eq!(painter.size(), before);

    fonts
        .register_font(PROBE_MONO)
        .expect("the probe face loads");
    painter.layout(&mut text_cx(), 0.0, f64::INFINITY);
    assert_ne!(
        painter.size(),
        before,
        "a face registered after layout must shape the same text again: in the \
         proportional fallback 'iiii' and 'wwww' differ, in the monospace probe they do not"
    );
}

/// The weight `family` is shaped with when a style asks for `requested`.
fn resolved_weight(family: &str, requested: FontWeight) -> Option<u16> {
    let style = TextStyle {
        font_family: Some(family.to_owned()),
        font_weight: Some(requested),
        ..TextStyle::default()
    };
    shared_font_system().shape(|shaper| shaper.resolve_font(Some(&style)).weight)
}

/// A weight the family carries is requested unchanged.
fn a_carried_weight_is_kept() {
    assert_eq!(
        resolved_weight("FLUI Probe Mono", FontWeight::W600),
        Some(600)
    );
}

/// A monospaced face is no licence to serve every weight: cosmic-text's
/// `is_mono` describes the request (a `Family::Monospace` lookup), not the
/// face, and its exact-weight filter drops a family whose faces all miss the
/// weight. Accepting W500 here would hand cosmic-text a weight that makes it
/// abandon the family (issue #929).
fn a_monospaced_family_is_snapped_like_any_other() {
    assert_ne!(
        resolved_weight("FLUI Probe Mono", FontWeight::W500),
        Some(500),
        "the family carries 100 and 600, not 500"
    );
}

/// CSS font matching resolves W500 downward before it looks above 500, so
/// the family's 100 wins over the nearer 600.
fn a_missing_weight_snaps_in_css_order_not_by_distance() {
    assert_eq!(
        resolved_weight("FLUI Probe Mono", FontWeight::W500),
        Some(100)
    );
}

/// A variable face serves any weight its `wght` axis covers, although its
/// default instance carries only 400; past the axis the request snaps to the
/// carried weight.
fn a_variable_axis_serves_the_weights_it_covers() {
    assert_eq!(
        resolved_weight("FLUI Probe Variable", FontWeight::W900),
        Some(900)
    );
    assert_eq!(
        resolved_weight("FLUI Probe Variable", FontWeight::W100),
        Some(100)
    );
}

#[test]
fn requested_weights_resolve_to_ones_the_family_serves() {
    let fonts = shared_font_system();
    for face in [PROBE_MONO, PROBE_MONO_600, PROBE_VARIABLE] {
        fonts.register_font(face).expect("the probe face loads");
    }
    cases::run_cases(
        "requested_weights_resolve_to_ones_the_family_serves",
        &[
            ("a_carried_weight_is_kept", a_carried_weight_is_kept),
            (
                "a_monospaced_family_is_snapped_like_any_other",
                a_monospaced_family_is_snapped_like_any_other,
            ),
            (
                "a_missing_weight_snaps_in_css_order_not_by_distance",
                a_missing_weight_snaps_in_css_order_not_by_distance,
            ),
            (
                "a_variable_axis_serves_the_weights_it_covers",
                a_variable_axis_serves_the_weights_it_covers,
            ),
        ],
    );
}
