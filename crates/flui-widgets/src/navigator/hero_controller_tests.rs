//! Unit tests of the `HeroController`'s private predicates. The mounted
//! measurement suite lives in `crates/flui-widgets/tests/hero_controller.rs`.

/// `_HeroFlightManifest.isValid` (`heroes.dart:530`):
/// `toHeroLocation.isFinite && (isDiverted || fromHeroLocation.isFinite)`.
///
/// Unit-tested directly, because no reachable FLUI configuration produces a non-finite
/// rect today — every rect comes from `box_size` and `transform_to`. Asserting it
/// end-to-end would assert nothing. See `is_valid_flight`'s docs.
///
/// Red-check: `to_rect.is_finite() || from_rect.is_finite()` in `is_valid_flight`.
#[test]
fn a_non_finite_rect_is_never_flown() {
    use super::hero_controller::is_valid_flight;
    use flui_geometry::Rect;
    use flui_types::geometry::px;

    let finite = Rect::from_ltwh(px(0.0), px(0.0), px(10.0), px(10.0));
    let infinite = Rect::from_ltwh(px(0.0), px(0.0), px(f32::INFINITY), px(10.0));
    let nan = Rect::from_ltwh(px(f32::NAN), px(0.0), px(10.0), px(10.0));

    assert!(is_valid_flight(finite, finite));
    assert!(!is_valid_flight(infinite, finite), "an infinite source");
    assert!(
        !is_valid_flight(finite, infinite),
        "an infinite destination"
    );
    assert!(!is_valid_flight(nan, finite), "a NaN origin");
}
