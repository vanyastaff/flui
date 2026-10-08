use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_platform_api::{InvalidPreference, MotionPreference, SystemPreferences};

fn native_mouse_geometry_preserves_full_area_and_half_extents() {
    use flui_platform_api::{GestureGeometry, NativeMouseGeometry};

    let ratio = DevicePixelRatio::new(2.0).expect("valid scale");
    let observed = NativeMouseGeometry::new(Size::new(12, 8), Size::new(6, 2), ratio)
        .expect("native geometry");
    let projected = GestureGeometry::from_native_mouse(&observed).expect("projection");
    assert_eq!(
        projected.mouse_double_click_area(),
        Some(Size::new(6.0, 4.0))
    );
    assert_eq!(projected.mouse_drag_tolerance(), Some(Size::new(3.0, 1.0)));
    assert_eq!(projected.pixel_ratio(), ratio);

    let zero = NativeMouseGeometry::new(Size::new(0, 8), Size::new(6, 0), ratio)
        .expect("zero axes are observations");
    let projected = GestureGeometry::from_native_mouse(&zero).expect("zero projection");
    assert_eq!(
        projected.mouse_double_click_area(),
        Some(Size::new(0.0, 4.0))
    );
    assert_eq!(projected.mouse_drag_tolerance(), Some(Size::new(3.0, 0.0)));
    assert!(NativeMouseGeometry::new(Size::new(-1, 8), Size::new(6, 2), ratio).is_err());

    let tiny = DevicePixelRatio::new(f64::from_bits(1)).expect("positive finite ratio");
    let extreme = NativeMouseGeometry::new(Size::new(1, 1), Size::new(1, 1), tiny)
        .expect("finite native observation");
    assert_eq!(
        GestureGeometry::from_native_mouse(&extreme),
        Err(InvalidPreference::GestureArea)
    );
}

fn native_gesture_timings_remain_independent() {
    use flui_platform_api::GesturePreferences;
    use std::time::Duration;

    let observed = GesturePreferences::default()
        .with_double_click_interval(Duration::from_millis(900))
        .with_double_tap_interval(Duration::from_millis(300));
    assert_eq!(
        observed.double_click_interval(),
        Some(Duration::from_millis(900))
    );
    assert_eq!(
        observed.double_tap_interval(),
        Some(Duration::from_millis(300))
    );
    assert!(
        GesturePreferences::default()
            .double_tap_interval()
            .is_none()
    );
}

fn native_touch_geometry_projects_each_context_and_refuses_overflow() {
    use flui_platform_api::{GestureGeometry, NativeTouchGeometry};
    for density in [1.0, 2.0, 3.0] {
        let native = NativeTouchGeometry::new(
            12,
            30,
            60,
            9000,
            DevicePixelRatio::new(density).expect("density"),
        )
        .expect("native touch geometry");
        let projected = GestureGeometry::from_native_touch(&native).expect("projection");
        assert_eq!(
            projected.touch_slop().expect("touch slop").get(),
            12.0 / density
        );
        assert_eq!(
            projected
                .touch_double_tap_slop()
                .expect("double tap slop")
                .get(),
            30.0 / density
        );
        let speeds = projected.fling_speeds().expect("fling speeds");
        assert_eq!(
            (speeds.min(), speeds.max()),
            (60.0 / density, 9000.0 / density)
        );
        assert!(projected.mouse_double_click_area().is_none());
    }
    let tiny = DevicePixelRatio::new(f64::from_bits(1)).expect("finite positive ratio");
    let native = NativeTouchGeometry::new(1, 1, 1, 2, tiny).expect("native observation");
    assert_eq!(
        GestureGeometry::from_native_touch(&native),
        Err(InvalidPreference::Distance)
    );
    assert!(NativeTouchGeometry::new(-1, 0, 1, 2, DevicePixelRatio::ONE).is_err());
    assert!(NativeTouchGeometry::new(0, 0, 2, 1, DevicePixelRatio::ONE).is_err());
}

fn gesture_geometry_refuses_invalid_distances_and_speeds() {
    use flui_platform_api::{Distance, FlingSpeeds, GestureGeometry};

    for value in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(Distance::new(value), Err(InvalidPreference::Distance));
        assert_eq!(FlingSpeeds::new(value, 10.0), Err(InvalidPreference::Speed));
        assert_eq!(FlingSpeeds::new(1.0, value), Err(InvalidPreference::Speed));
        assert!(
            GestureGeometry::new(DevicePixelRatio::ONE)
                .with_mouse_double_click_area(Size::new(value, 1.0))
                .is_err()
        );
        assert!(
            GestureGeometry::new(DevicePixelRatio::ONE)
                .with_mouse_drag_tolerance(Size::new(1.0, value))
                .is_err()
        );
    }
    assert!(Distance::new(0.0).is_ok());
    assert_eq!(FlingSpeeds::new(0.0, 1.0), Err(InvalidPreference::Speed));
    assert_eq!(
        FlingSpeeds::new(10.0, 1.0),
        Err(InvalidPreference::FlingRange)
    );
    let speeds = FlingSpeeds::new(50.0, 8000.0).expect("valid speeds");
    assert_eq!((speeds.min(), speeds.max()), (50.0, 8000.0));
}

fn motion_duration_observations_preserve_their_meaning() {
    for zero in [0.0, -0.0] {
        assert_eq!(
            MotionPreference::from_duration_scale(zero),
            Ok(MotionPreference::Reduce)
        );
    }
    assert_eq!(
        MotionPreference::from_duration_scale(1.0),
        Ok(MotionPreference::NoPreference)
    );
    for scaled in [f64::from_bits(1), 0.5, 2.0, f64::MAX] {
        assert!(matches!(
            MotionPreference::from_duration_scale(scaled),
            Ok(MotionPreference::Scaled(_))
        ));
    }
    for invalid in [-1.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
        assert_eq!(
            MotionPreference::from_duration_scale(invalid),
            Err(InvalidPreference::DurationScale)
        );
    }
}

fn invalid_text_observations_cannot_enter_a_snapshot() {
    for invalid in [
        0.0,
        -0.0,
        -1.0,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NAN,
        f64::from_bits(1),
        (1.0_f64 / 64.0).next_down(),
        64.0_f64.next_up(),
        f64::MAX,
    ] {
        assert_eq!(
            SystemPreferences::default().with_text_scale(invalid),
            Err(InvalidPreference::TextScale)
        );
    }
    for valid in [1.0 / 64.0, 0.5, 1.0, 2.0, 64.0] {
        assert!(SystemPreferences::default().with_text_scale(valid).is_ok());
    }
}

#[test]
fn preferences_contract() {
    crate::run_table(
        "preferences_contract",
        &[
            (
                "native_mouse_geometry_preserves_full_area_and_half_extents",
                native_mouse_geometry_preserves_full_area_and_half_extents as fn(),
            ),
            (
                "native_gesture_timings_remain_independent",
                native_gesture_timings_remain_independent as fn(),
            ),
            (
                "native_touch_geometry_projects_each_context_and_refuses_overflow",
                native_touch_geometry_projects_each_context_and_refuses_overflow as fn(),
            ),
            (
                "gesture_geometry_refuses_invalid_distances_and_speeds",
                gesture_geometry_refuses_invalid_distances_and_speeds as fn(),
            ),
            (
                "motion_duration_observations_preserve_their_meaning",
                motion_duration_observations_preserve_their_meaning as fn(),
            ),
            (
                "invalid_text_observations_cannot_enter_a_snapshot",
                invalid_text_observations_cannot_enter_a_snapshot as fn(),
            ),
        ],
    );
}
