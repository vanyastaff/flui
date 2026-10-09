use flui_platform_api::{InvalidPreference, MotionPreference, SystemPreferences};

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
