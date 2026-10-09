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

fn preferred_language_identity_preserves_variants_and_extensions() {
    use flui_platform_api::Locale;
    for (tag, language, region) in [
        ("ca-ES-valencia", "ca", Some("ES")),
        ("en-US-u-hc-h12", "en", Some("US")),
        ("sl-rozaj-biske", "sl", None),
    ] {
        let locale = Locale::from_language_tag(tag).expect("well-formed preferred language");
        assert_eq!(locale.language(), language, "{tag}");
        assert_eq!(
            locale.country(),
            region,
            "a variant must not become the region: {tag}"
        );
        assert_eq!(
            locale.to_language_tag(),
            tag,
            "preference identity must not truncate subtags"
        );
        assert_ne!(
            locale,
            Locale::new(language, region).expect("valid base locale"),
            "variant/extension identity must survive resource matching"
        );
    }
}

fn malformed_preferred_language_tags_are_refused() {
    for tag in [
        "",
        "en--US",
        "en-",
        "-US",
        "en-US-!",
        "english?",
        "en-1234-1234",
        "en-u-ca-gregory-u-hc-h12",
        "x-private-",
        "x-toolongpart",
    ] {
        assert!(
            flui_platform_api::Locale::from_language_tag(tag).is_none(),
            "invalid tag accepted: {tag}"
        );
    }
}

fn deprecated_subtags_canonicalize_everywhere_a_locale_is_built() {
    use flui_platform_api::Locale;
    for (old, current) in [("in", "id"), ("iw", "he"), ("ji", "yi")] {
        let authored = Locale::new(old, None::<&str>).expect("legacy language");
        let parsed: Locale = old.parse().expect("legacy language tag");
        let preferred = Locale::new(current, None::<&str>).expect("preferred language");
        let mut identities = std::collections::HashSet::new();
        identities.insert(authored);
        assert!(identities.contains(&preferred));
        assert_eq!(parsed, preferred);
        #[cfg(feature = "serde")]
        {
            let decoded: Locale = serde_json::from_value(serde_json::Value::String(old.to_owned()))
                .expect("legacy serialized tag");
            assert_eq!(decoded, preferred);
        }
    }
    for (old, current) in [
        ("BU", "MM"),
        ("DD", "DE"),
        ("FX", "FR"),
        ("TP", "TL"),
        ("YD", "YE"),
        ("ZR", "CD"),
    ] {
        assert_eq!(
            Locale::new("en", Some(old)),
            Locale::new("en", Some(current))
        );
    }
    for (language, country, script) in [
        ("en-US", None, None),
        ("en", Some("Latn"), None),
        ("en", None, Some("US")),
        ("", None, None),
    ] {
        assert!(Locale::with_script(language, country, script).is_err());
    }
    let full: Locale = "iw-Hebr-BU-u-ca-hebrew".parse().expect("full legacy tag");
    assert_eq!(full.to_language_tag(), "he-Hebr-MM-u-ca-hebrew");
    for (input, expected) in [
        ("EN_latn_us", "en-Latn-US"),
        ("x-PRIVATE", "x-private"),
        ("i-klingon", "i-klingon"),
    ] {
        assert_eq!(
            input.parse::<Locale>().expect("valid tag").to_string(),
            expected
        );
    }
    #[cfg(feature = "serde")]
    {
        let encoded = serde_json::to_string(&full).expect("serialize language identity");
        assert_eq!(encoded, "\"he-Hebr-MM-u-ca-hebrew\"");
        assert_eq!(
            serde_json::from_str::<Locale>(&encoded).expect("deserialize complete identity"),
            full
        );
        assert!(serde_json::from_str::<Locale>("\"en--US\"").is_err());
    }
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
                "deprecated_subtags_canonicalize_everywhere_a_locale_is_built",
                deprecated_subtags_canonicalize_everywhere_a_locale_is_built as fn(),
            ),
            (
                "preferred_language_identity_preserves_variants_and_extensions",
                preferred_language_identity_preserves_variants_and_extensions as fn(),
            ),
            (
                "malformed_preferred_language_tags_are_refused",
                malformed_preferred_language_tags_are_refused as fn(),
            ),
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
