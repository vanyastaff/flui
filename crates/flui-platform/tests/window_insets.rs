//! `WindowInsets`: validation, and the keyboard's occlusion sampled along its
//! motion (ADR-0276 §1).

use std::time::Duration;

use flui_foundation::geometry::EdgeInsets;
use flui_platform::{
    EventTime, InsetCurve, InsetMotion, InvalidInsets, KeyboardInset, WindowInsets,
};

const SHOWN: EdgeInsets = EdgeInsets::new(0.0, 0.0, 300.0, 0.0);
const START: EventTime = EventTime::from_nanos(1_000_000_000);

fn at_ms(ms: u64) -> EventTime {
    START
        .checked_add(Duration::from_millis(ms))
        .expect("on the timeline")
}

fn showing(curve: InsetCurve) -> KeyboardInset {
    let motion = InsetMotion::try_new(EdgeInsets::ZERO, START, Duration::from_millis(250), curve)
        .expect("a valid motion");
    KeyboardInset::try_moving(SHOWN, motion).expect("a valid target")
}

fn a_linear_motion_is_sampled_along_its_duration() {
    let keyboard = showing(InsetCurve::Linear);
    let before = START.checked_add(Duration::ZERO).expect("on the timeline");
    assert_eq!(keyboard.at(EventTime::from_nanos(0)), EdgeInsets::ZERO);
    assert_eq!(keyboard.at(before), EdgeInsets::ZERO);
    assert_eq!(keyboard.at(at_ms(125)).bottom, 150.0);
    assert_eq!(keyboard.at(at_ms(250)), SHOWN);
    assert_eq!(keyboard.at(at_ms(10_000)), SHOWN);
    assert_eq!(keyboard.settles_at(), Some(at_ms(250)));
}

fn a_bezier_motion_follows_its_curve() {
    // CSS `ease`: well ahead of linear at a quarter of the time.
    let ease = InsetCurve::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    let keyboard = showing(ease);
    let quarter = keyboard.at(at_ms(62)).bottom / SHOWN.bottom;
    assert!((0.38..0.42).contains(&quarter), "ease at 0.25: {quarter}");
    let samples: Vec<f64> = (0..=250)
        .step_by(10)
        .map(|ms| keyboard.at(at_ms(ms)).bottom)
        .collect();
    assert!(
        samples.windows(2).all(|pair| pair[0] <= pair[1]),
        "monotonic: {samples:?}"
    );
}

fn the_system_keyboard_curve_is_its_bezier() {
    for t in [0.0, 0.1, 0.33, 0.5, 0.9, 1.0] {
        assert_eq!(
            InsetCurve::SystemKeyboard.progress(t),
            InsetCurve::SYSTEM_KEYBOARD_BEZIER.progress(t)
        );
    }
    assert_eq!(InsetCurve::SystemKeyboard.progress(1.0), 1.0);
}

fn a_keyboard_at_rest_is_its_target() {
    let keyboard = KeyboardInset::try_at_rest(SHOWN).expect("a valid target");
    assert_eq!(keyboard.at(START), SHOWN);
    assert_eq!(keyboard.settles_at(), None);
    assert_eq!(KeyboardInset::HIDDEN.at(START), EdgeInsets::ZERO);
}

fn invalid_insets_are_refused() {
    let nan = EdgeInsets::new(f64::NAN, 0.0, 0.0, 0.0);
    let negative = EdgeInsets::new(0.0, 0.0, -1.0, 0.0);
    assert_eq!(
        KeyboardInset::try_at_rest(nan),
        Err(InvalidInsets::NonFinite)
    );
    assert_eq!(
        KeyboardInset::try_at_rest(negative),
        Err(InvalidInsets::Negative)
    );
    assert_eq!(
        WindowInsets::ZERO.try_with_safe_area(EdgeInsets::all(f64::INFINITY)),
        Err(InvalidInsets::NonFinite)
    );
    assert_eq!(
        WindowInsets::ZERO.try_with_system_gestures(negative),
        Err(InvalidInsets::Negative)
    );
    assert_eq!(
        InsetMotion::try_new(EdgeInsets::ZERO, START, Duration::ZERO, InsetCurve::Linear),
        Err(InvalidInsets::ZeroDuration)
    );
    let outside = InsetCurve::CubicBezier {
        x1: 1.5,
        y1: 0.0,
        x2: 0.5,
        y2: 1.0,
    };
    assert_eq!(
        InsetMotion::try_new(EdgeInsets::ZERO, START, Duration::from_millis(1), outside),
        Err(InvalidInsets::InvalidCurve)
    );
}

#[test]
fn window_insets_table() {
    super::run_table(
        "window_insets",
        &[
            (
                "a_linear_motion_is_sampled_along_its_duration",
                a_linear_motion_is_sampled_along_its_duration as fn(),
            ),
            (
                "a_bezier_motion_follows_its_curve",
                a_bezier_motion_follows_its_curve,
            ),
            (
                "the_system_keyboard_curve_is_its_bezier",
                the_system_keyboard_curve_is_its_bezier,
            ),
            (
                "a_keyboard_at_rest_is_its_target",
                a_keyboard_at_rest_is_its_target,
            ),
            ("invalid_insets_are_refused", invalid_insets_are_refused),
        ],
    );
}
