//! The haptic pattern builder and its unit values (ADR-0210 §2).

use std::time::Duration;

use flui_platform::haptics::{
    HapticEvent, HapticPattern, HapticPatternError, Intensity, Sharpness, UnitRangeError,
};

const fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn built(builder: flui_platform::haptics::HapticPatternBuilder) -> Result<(), HapticPatternError> {
    builder.build().map(drop)
}

fn unordered_events() {
    let result = built(
        HapticPattern::builder()
            .transient(ms(30), 1.0, 1.0)
            .transient(ms(10), 1.0, 1.0),
    );
    assert_eq!(result, Err(HapticPatternError::Unordered { index: 1 }));
    // Two events at the same time are ordered.
    assert_eq!(
        built(
            HapticPattern::builder()
                .transient(ms(10), 1.0, 1.0)
                .transient(ms(10), 0.5, 0.5)
        ),
        Ok(())
    );
}

fn unordered_control_points() {
    let not_increasing = built(
        HapticPattern::builder()
            .transient(ms(0), 1.0, 1.0)
            .continuous(ms(5), [(ms(20), 0.5, 0.5), (ms(20), 0.0, 0.5)]),
    );
    assert_eq!(
        not_increasing,
        Err(HapticPatternError::Unordered { index: 1 })
    );
    let zero_offset =
        built(HapticPattern::builder().continuous(ms(0), [(Duration::ZERO, 0.0, 0.5)]));
    assert_eq!(zero_offset, Err(HapticPatternError::Unordered { index: 0 }));
}

fn non_finite_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            built(HapticPattern::builder().transient(ms(0), value, 0.5)),
            Err(HapticPatternError::NonFinite),
            "intensity {value}"
        );
        assert_eq!(
            built(HapticPattern::builder().continuous(ms(0), [(ms(5), 0.0, value)])),
            Err(HapticPatternError::NonFinite),
            "sharpness {value}"
        );
    }
}

fn out_of_range_values() {
    for value in [-f32::MIN_POSITIVE, 1.0 + f32::EPSILON, -1.0, 2.0] {
        assert_eq!(
            built(HapticPattern::builder().transient(ms(0), 0.5, value)),
            Err(HapticPatternError::OutOfRange),
            "sharpness {value}"
        );
    }
}

fn open_envelopes() {
    assert_eq!(
        built(HapticPattern::builder().continuous(ms(0), [(ms(50), 0.1, 0.5)])),
        Err(HapticPatternError::EnvelopeNotClosed)
    );
    assert_eq!(
        built(HapticPattern::builder().continuous(ms(0), [])),
        Err(HapticPatternError::EnvelopeNotClosed)
    );
}

fn too_long_patterns() {
    let max = HapticPattern::MAX_LENGTH;
    assert_eq!(
        built(HapticPattern::builder().transient(max, 1.0, 1.0)),
        Ok(()),
        "a pattern ending exactly at the cap is admitted"
    );
    assert_eq!(
        built(HapticPattern::builder().transient(max + Duration::from_nanos(1), 1.0, 1.0)),
        Err(HapticPatternError::TooLong { max })
    );
    assert_eq!(
        built(
            HapticPattern::builder().continuous(max.saturating_sub(ms(10)), [(ms(20), 0.0, 0.0)])
        ),
        Err(HapticPatternError::TooLong { max }),
        "a continuous event's envelope counts"
    );
    assert_eq!(
        built(HapticPattern::builder().continuous(Duration::MAX, [(ms(1), 0.0, 0.0)])),
        Err(HapticPatternError::TooLong { max }),
        "an end past Duration::MAX is refused, not wrapped"
    );
}

fn the_first_error_wins() {
    let result = built(
        HapticPattern::builder()
            .transient(ms(0), f32::NAN, 0.5)
            .transient(ms(0), 2.0, 0.5),
    );
    assert_eq!(result, Err(HapticPatternError::NonFinite));
}

fn unit_value_boundaries() {
    for (value, admitted) in [
        (0.0, true),
        (-0.0, true),
        (1.0, true),
        (f32::MIN_POSITIVE, true),
        (1.0 - f32::EPSILON, true),
        (-f32::MIN_POSITIVE, false),
        (1.0 + f32::EPSILON, false),
        (f32::NAN, false),
        (f32::INFINITY, false),
    ] {
        assert_eq!(
            Intensity::new(value).is_some(),
            admitted,
            "intensity {value}"
        );
        assert_eq!(
            Sharpness::new(value).is_some(),
            admitted,
            "sharpness {value}"
        );
        let converted: Result<Intensity, UnitRangeError> = value.try_into();
        assert_eq!(converted.is_ok(), admitted, "try_from {value}");
    }
    assert!(
        Intensity::new(-0.0)
            .expect("admitted")
            .get()
            .is_sign_positive(),
        "negative zero is admitted as zero"
    );
    assert_eq!(Intensity::new(0.0), Some(Intensity::MIN));
    assert_eq!(Sharpness::new(1.0), Some(Sharpness::MAX));
}

fn a_valid_pattern_keeps_its_events() {
    let pattern = HapticPattern::builder()
        .transient(ms(0), 1.0, 0.25)
        .continuous(ms(10), [(ms(40), 0.5, 0.5), (ms(90), 0.0, 0.5)])
        .build()
        .expect("valid");
    assert_eq!(pattern.duration(), ms(100));
    assert!(matches!(
        pattern.events(),
        [
            HapticEvent::Transient { at, .. },
            HapticEvent::Continuous { curve, .. },
        ] if *at == ms(0) && curve.points().len() == 2
    ));
    let copy = HapticPattern::builder()
        .event(pattern.events()[0].clone())
        .event(pattern.events()[1].clone())
        .build()
        .expect("valid copy");
    assert_eq!(copy, pattern);
}

#[test]
fn haptic_pattern_builder_rejects_invalid_patterns() {
    super::run_table(
        "haptic pattern builder",
        &[
            ("unordered_events", unordered_events),
            ("unordered_control_points", unordered_control_points),
            ("non_finite_values", non_finite_values),
            ("out_of_range_values", out_of_range_values),
            ("open_envelopes", open_envelopes),
            ("too_long_patterns", too_long_patterns),
            ("the_first_error_wins", the_first_error_wins),
            ("unit_value_boundaries", unit_value_boundaries),
            (
                "a_valid_pattern_keeps_its_events",
                a_valid_pattern_keeps_its_events,
            ),
        ],
    );
}
