use flui_foundation::geometry::{DevicePixelRatio, Size};
use flui_platform::{InvalidPreference, MotionPreference, SystemPreferences};

fn native_mouse_geometry_preserves_full_area_and_half_extents() {
    use flui_platform::{GestureGeometry, NativeMouseGeometry};

    let ratio = DevicePixelRatio::try_new(2.0).expect("valid scale");
    let observed = NativeMouseGeometry::try_new(Size::new(12, 8), Size::new(6, 2), ratio)
        .expect("native geometry");
    let projected = GestureGeometry::from_native_mouse(&observed).expect("projection");
    assert_eq!(
        projected.mouse_double_click_area(),
        Some(Size::new(6.0, 4.0))
    );
    assert_eq!(projected.mouse_drag_tolerance(), Some(Size::new(3.0, 1.0)));
    assert_eq!(projected.pixel_ratio(), ratio);

    let zero = NativeMouseGeometry::try_new(Size::new(0, 8), Size::new(6, 0), ratio)
        .expect("zero axes are observations");
    let projected = GestureGeometry::from_native_mouse(&zero).expect("zero projection");
    assert_eq!(
        projected.mouse_double_click_area(),
        Some(Size::new(0.0, 4.0))
    );
    assert_eq!(projected.mouse_drag_tolerance(), Some(Size::new(3.0, 0.0)));
    assert!(NativeMouseGeometry::try_new(Size::new(-1, 8), Size::new(6, 2), ratio).is_err());

    let tiny = DevicePixelRatio::try_new(f64::from_bits(1)).expect("positive finite ratio");
    let extreme = NativeMouseGeometry::try_new(Size::new(1, 1), Size::new(1, 1), tiny)
        .expect("finite native observation");
    assert_eq!(
        GestureGeometry::from_native_mouse(&extreme),
        Err(InvalidPreference::GestureArea)
    );
}

fn native_gesture_timings_remain_independent() {
    use flui_platform::GesturePreferences;
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
    use flui_platform::{GestureGeometry, NativeTouchGeometry};
    for density in [1.0, 2.0, 3.0] {
        let native = NativeTouchGeometry::try_new(
            12,
            30,
            60,
            9000,
            DevicePixelRatio::try_new(density).expect("density"),
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
    let tiny = DevicePixelRatio::try_new(f64::from_bits(1)).expect("finite positive ratio");
    let native = NativeTouchGeometry::try_new(1, 1, 1, 2, tiny).expect("native observation");
    assert_eq!(
        GestureGeometry::from_native_touch(&native),
        Err(InvalidPreference::Distance)
    );
    assert!(NativeTouchGeometry::try_new(-1, 0, 1, 2, DevicePixelRatio::ONE).is_err());
    assert!(NativeTouchGeometry::try_new(0, 0, 2, 1, DevicePixelRatio::ONE).is_err());
}

fn gesture_geometry_refuses_invalid_distances_and_speeds() {
    use flui_platform::{Distance, FlingSpeeds, GestureGeometry};

    for value in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(Distance::try_new(value), Err(InvalidPreference::Distance));
        assert_eq!(
            FlingSpeeds::try_new(value, 10.0),
            Err(InvalidPreference::Speed)
        );
        assert_eq!(
            FlingSpeeds::try_new(1.0, value),
            Err(InvalidPreference::Speed)
        );
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
    assert!(Distance::try_new(0.0).is_ok());
    assert_eq!(
        FlingSpeeds::try_new(0.0, 1.0),
        Err(InvalidPreference::Speed)
    );
    assert_eq!(
        FlingSpeeds::try_new(10.0, 1.0),
        Err(InvalidPreference::FlingRange)
    );
    let speeds = FlingSpeeds::try_new(50.0, 8000.0).expect("valid speeds");
    assert_eq!((speeds.min(), speeds.max()), (50.0, 8000.0));
}

fn preferred_language_identity_preserves_variants_and_extensions() {
    use flui_platform::Locale;
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
            Locale::try_new(language, region).expect("valid base locale"),
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
            flui_platform::Locale::from_language_tag(tag).is_none(),
            "invalid tag accepted: {tag}"
        );
    }
}

fn deprecated_subtags_canonicalize_everywhere_a_locale_is_built() {
    use flui_platform::Locale;
    for (old, current) in [("in", "id"), ("iw", "he"), ("ji", "yi")] {
        let authored = Locale::try_new(old, None::<&str>).expect("legacy language");
        let parsed: Locale = old.parse().expect("legacy language tag");
        let preferred = Locale::try_new(current, None::<&str>).expect("preferred language");
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
            Locale::try_new("en", Some(old)),
            Locale::try_new("en", Some(current))
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

fn wheel_observations_keep_lines_pages_and_characters_apart() {
    use flui_platform::{WheelPreferences, WheelStep};

    let unknown = WheelPreferences::default();
    assert_eq!(unknown.vertical(), None, "an unread axis stays unknown");
    assert_eq!(unknown.horizontal_characters(), None);
    let disabled = WheelPreferences::default()
        .with_vertical(WheelStep::Lines(0))
        .with_horizontal_characters(0);
    assert_eq!(
        disabled.vertical(),
        Some(WheelStep::Lines(0)),
        "zero lines is the system disabling the axis, not an unknown value"
    );
    assert_eq!(disabled.horizontal_characters(), Some(0));
    let paged = WheelPreferences::default()
        .with_vertical(WheelStep::Page)
        .with_horizontal_characters(3);
    assert_eq!(paged.vertical(), Some(WheelStep::Page));
    assert_eq!(
        paged.horizontal_characters(),
        Some(3),
        "a page setting on one axis leaves the other axis's characters"
    );
    assert_ne!(
        WheelPreferences::default().with_vertical(WheelStep::Lines(3)),
        WheelPreferences::default().with_horizontal_characters(3),
        "three lines and three characters are different observations"
    );
    assert_eq!(
        SystemPreferences::default()
            .with_wheel(paged.clone())
            .wheel(),
        &paged,
        "a snapshot carries the wheel observation unchanged"
    );
}

/// Regional formats join a locale as `u` extension keywords (ADR-0278): in
/// key order, inside the extensions in singleton order and before private
/// use, with a key the locale already carries kept as it is. Fails if a key
/// overrides the app's own, lands out of canonical order, or a tag that
/// cannot carry an extension is changed.
fn regional_formats_join_the_locale_a_formatter_takes() {
    use flui_platform::{
        HourCycle, Locale, MeasurementSystem, NumberingSystem, RegionOverride, RegionalFormats,
        Weekday,
    };

    let all = RegionalFormats::default()
        .with_first_weekday(Weekday::Monday)
        .with_hour_cycle(HourCycle::H23)
        .with_measurement(MeasurementSystem::UsCustomary)
        .with_numbering(NumberingSystem::try_new("ARAB").expect("valid numbering"))
        .with_region(RegionOverride::try_new("gb").expect("valid region"));
    let hours = RegionalFormats::default().with_hour_cycle(HourCycle::H23);
    for (tag, formats, expected) in [
        (
            "en-US",
            &all,
            "en-US-u-fw-mon-hc-h23-ms-ussystem-nu-arab-rg-gbzzzz",
        ),
        ("en-US", &RegionalFormats::default(), "en-US"),
        (
            "en-US-u-hc-h12",
            &all,
            "en-US-u-fw-mon-hc-h12-ms-ussystem-nu-arab-rg-gbzzzz",
        ),
        ("ja-JP-u-ca-japanese", &hours, "ja-JP-u-ca-japanese-hc-h23"),
        ("en-a-bbb-x-priv", &hours, "en-a-bbb-u-hc-h23-x-priv"),
        ("de-t-en-z-zzz", &hours, "de-t-en-u-hc-h23-z-zzz"),
        (
            "en-u-attr-co-phonebk",
            &hours,
            "en-u-attr-co-phonebk-hc-h23",
        ),
        ("x-private", &hours, "x-private"),
        ("i-klingon", &hours, "i-klingon"),
    ] {
        let locale: Locale = tag.parse().expect("valid tag");
        assert_eq!(
            formats.apply_to(&locale).to_language_tag(),
            expected
                .parse::<Locale>()
                .expect("valid expectation")
                .to_language_tag(),
            "{tag}"
        );
    }
}

/// Each regional value names itself by its Unicode key's value and is read
/// back from it in any case; a value outside the key's vocabulary is
/// refused. Fails if a variant's name does not read back, or a malformed
/// numbering system or region enters a snapshot.
fn regional_values_read_back_their_unicode_names() {
    use flui_platform::{
        HourCycle, InvalidRegionalFormat, MeasurementSystem, NumberingSystem, RegionOverride,
        Weekday,
    };

    for day in Weekday::ALL {
        assert_eq!(Weekday::from_unicode_value(day.unicode_value()), Some(day));
        let upper = day.unicode_value().to_ascii_uppercase();
        assert_eq!(Weekday::from_unicode_value(&upper), Some(day));
    }
    for cycle in HourCycle::ALL {
        assert_eq!(
            HourCycle::from_unicode_value(cycle.unicode_value()),
            Some(cycle)
        );
    }
    assert!(HourCycle::H23.is_24_hour() && HourCycle::H24.is_24_hour());
    assert!(!HourCycle::H11.is_24_hour() && !HourCycle::H12.is_24_hour());
    for system in MeasurementSystem::ALL {
        assert_eq!(
            MeasurementSystem::from_unicode_value(system.unicode_value()),
            Some(system)
        );
    }
    assert_eq!(Weekday::from_unicode_value("monday"), None);
    assert_eq!(HourCycle::from_unicode_value("h13"), None);
    assert_eq!(MeasurementSystem::from_unicode_value("imperial"), None);

    for bad in ["ar", "latin-ext", "abcdefghi", "la_n"] {
        assert_eq!(
            NumberingSystem::try_new(bad),
            Err(InvalidRegionalFormat::NumberingSystem),
            "{bad}"
        );
    }
    assert_eq!(
        NumberingSystem::try_new("HANIDEC").map(|numbering| numbering.unicode_value().to_owned()),
        Ok("hanidec".to_owned())
    );
    for bad in ["G", "GBR", "4l9", "1234", "g1"] {
        assert_eq!(
            RegionOverride::try_new(bad),
            Err(InvalidRegionalFormat::Region),
            "{bad}"
        );
    }
    let latin_america = RegionOverride::try_new("419").expect("valid region");
    assert_eq!(latin_america.code(), "419");
    assert_eq!(latin_america.unicode_value(), "419zzzz");
    assert_eq!(
        RegionOverride::from_unicode_value("gbzzzz").map(|region| region.code().to_owned()),
        Some("GB".to_owned())
    );
    assert_eq!(
        RegionOverride::from_unicode_value("usca").map(|region| region.code().to_owned()),
        Some("US".to_owned())
    );
    assert_eq!(RegionOverride::from_unicode_value("gb"), None);
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
            (
                "wheel_observations_keep_lines_pages_and_characters_apart",
                wheel_observations_keep_lines_pages_and_characters_apart as fn(),
            ),
            (
                "regional_formats_join_the_locale_a_formatter_takes",
                regional_formats_join_the_locale_a_formatter_takes as fn(),
            ),
            (
                "regional_values_read_back_their_unicode_names",
                regional_values_read_back_their_unicode_names as fn(),
            ),
        ],
    );
}
