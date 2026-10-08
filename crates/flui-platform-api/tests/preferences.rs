use flui_platform_api::{InvalidPreference, MotionPreference, SystemPreferences};

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
    for invalid in [0.0, -0.0, -1.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
        assert_eq!(
            SystemPreferences::default().with_text_scale(invalid),
            Err(InvalidPreference::TextScale)
        );
    }
    for valid in [f64::from_bits(1), 0.5, 1.0, 2.0, f64::MAX] {
        assert!(SystemPreferences::default().with_text_scale(valid).is_ok());
    }
}

#[test]
fn preferences_contract() {
    crate::run_table(
        "preferences_contract",
        &[
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
