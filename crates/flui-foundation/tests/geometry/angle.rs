//! `Angle`: numeric interpolation keeps whole turns; `nearest_equivalent` picks the
//! representative within half a turn of a reference.

use flui_foundation::geometry::{Angle, Lerp, QuarterTurns};

fn assert_degrees(actual: Angle, expected: f64, what: &str) {
    assert!(
        (actual.degrees() - expected).abs() < 1e-9,
        "{what}: {}°, expected {expected}°",
        actual.degrees()
    );
}

fn three_quarters_back_is_a_quarter_forward() {
    let shorter = Angle::from_degrees(270.0).nearest_equivalent(Angle::ZERO);
    assert_degrees(shorter, -90.0, "270° from 0°");
}

fn crossing_zero_continues_past_a_full_turn() {
    let shorter = Angle::from_degrees(10.0).nearest_equivalent(Angle::from_degrees(350.0));
    assert_degrees(shorter, 370.0, "10° from 350°");
}

fn an_exact_half_turn_resolves_toward_increasing_angle() {
    let half = Angle::from_turns(0.5);
    assert_eq!(half.nearest_equivalent(Angle::ZERO), half);
    let back = Angle::from_turns(-0.5).nearest_equivalent(Angle::ZERO);
    assert_eq!(back, half, "-½ turn from 0 resolves to +½");
}

fn whole_turns_are_dropped() {
    let shorter = Angle::from_degrees(750.0).nearest_equivalent(Angle::ZERO);
    assert_degrees(shorter, 30.0, "750° from 0°");
}

fn nan_stays_nan() {
    assert!(
        Angle::from_radians(f64::NAN)
            .nearest_equivalent(Angle::ZERO)
            .radians()
            .is_nan()
    );
    assert!(
        Angle::ZERO
            .nearest_equivalent(Angle::from_radians(f64::NAN))
            .radians()
            .is_nan()
    );
}

fn opposite_extreme_angles_stay_finite() {
    let reference = Angle::from_radians(-f64::MAX);
    let result = Angle::from_radians(f64::MAX).nearest_equivalent(reference);
    assert!(result.radians().is_finite(), "{result:?}");
    // Within half a turn of the reference, which is all f64 can resolve at MAX.
    assert_eq!(result.radians(), reference.radians());
}

fn numeric_interpolation_keeps_whole_turns() {
    // CSS Transforms 1 §10: rotate(45deg) -> rotate(1215deg) spins 3.25 turns.
    let begin = Angle::from_degrees(45.0);
    let end = Angle::from_degrees(1215.0);
    assert_degrees(begin.lerp_to(&end, 0.5), 630.0, "midpoint");
    assert_degrees(begin.lerp_to(&end, 1.5), 1800.0, "extrapolated");
}

fn quarter_turns_convert_exactly() {
    assert_eq!(Angle::from(QuarterTurns::Two), Angle::from_turns(0.5));
    assert_degrees(Angle::from(QuarterTurns::Three), 270.0, "three quarters");
}

#[test]
fn angle_nearest_equivalent_takes_the_shorter_arc() {
    crate::run_table(&[
        (
            "three quarters back is a quarter forward",
            three_quarters_back_is_a_quarter_forward,
        ),
        (
            "crossing zero continues past a full turn",
            crossing_zero_continues_past_a_full_turn,
        ),
        (
            "an exact half turn resolves toward increasing angle",
            an_exact_half_turn_resolves_toward_increasing_angle,
        ),
        ("whole turns are dropped", whole_turns_are_dropped),
        ("nan stays nan", nan_stays_nan),
        (
            "opposite extreme angles stay finite",
            opposite_extreme_angles_stay_finite,
        ),
        (
            "numeric interpolation keeps whole turns",
            numeric_interpolation_keeps_whole_turns,
        ),
        (
            "quarter turns convert exactly",
            quarter_turns_convert_exactly,
        ),
    ]);
}
