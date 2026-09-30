//! A requested weight resolves to one the registered family can serve.
//!
//! Its own test target: it registers the probe families on the process-wide
//! font database, and `font_registration` measures the probe family before
//! registering it, so the two cannot share a process.

use flui_painting::shared_font_system;
use flui_painting::typography::{FontWeight, TextStyle};

#[path = "support/cases.rs"]
mod cases;

/// `FLUI Probe Mono` at weight 100.
const PROBE_MONO: &[u8] = include_bytes!("../assets/fonts/probe-mono-100.ttf");
/// The same monospaced family at weight 600.
const PROBE_MONO_600: &[u8] = include_bytes!("../assets/fonts/probe-mono-600.ttf");
/// One face with `usWeightClass` 400 and an `fvar` `wght` axis spanning
/// 100..900.
const PROBE_VARIABLE: &[u8] = include_bytes!("../assets/fonts/probe-variable-wght.ttf");

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
/// default instance carries only 400.
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
