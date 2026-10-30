//! Public admission, numerical recovery and timestamp contracts for smoothing.

use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::gesture::{
    OneEuroFilter, OneEuroFilter2D, OneEuroFilterError, OneEuroTuning,
};
use web_time::Instant;

fn finite_extreme_samples_do_not_publish_nan() {
    let mut filter = OneEuroFilter::default();
    assert_eq!(filter.filter(f64::MAX, 1.0), Ok(f64::MAX));
    assert_eq!(
        filter.filter(-f64::MAX, 1.0),
        Err(OneEuroFilterError::ArithmeticOverflow)
    );
    assert_eq!(filter.filter(f64::MAX, 1.0), Ok(f64::MAX));
}

fn regressed_timestamp_preserves_the_last_accepted_anchor() {
    let start = Instant::now();
    let mut actual = OneEuroFilter2D::default();
    let mut control = actual;
    for filter in [&mut actual, &mut control] {
        filter.filter(start, Offset::ZERO).expect("first sample");
        filter
            .filter(start + Duration::from_secs(2), Offset::new(10.0, 20.0))
            .expect("second sample");
    }
    let held = actual
        .filter(start + Duration::from_secs(2), Offset::new(90.0, 80.0))
        .expect("duplicate holds value");
    assert_eq!(
        actual.filter(start + Duration::from_secs(1), Offset::new(90.0, 80.0)),
        Ok(held)
    );
    let next = start + Duration::from_secs(3);
    let position = Offset::new(30.0, 40.0);
    assert_eq!(
        actual.filter(next, position),
        control.filter(next, position)
    );
}

fn tuning_admission() {
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            OneEuroTuning::try_new(invalid, 0.0, 1.0),
            Err(OneEuroFilterError::InvalidMinCutoff)
        );
        assert_eq!(
            OneEuroTuning::try_new(1.0, 0.0, invalid),
            Err(OneEuroFilterError::InvalidDerivativeCutoff)
        );
    }
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            OneEuroTuning::try_new(1.0, invalid, 1.0),
            Err(OneEuroFilterError::InvalidBeta)
        );
    }
    for cutoff in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        assert!(OneEuroTuning::try_new(cutoff, 0.0, cutoff).is_ok());
        assert!(OneEuroTuning::try_new(cutoff, f64::MAX, cutoff).is_ok());
    }
}

fn nonfinite_samples_and_invalid_intervals_preserve_history() {
    let mut actual = OneEuroFilter::default();
    assert_eq!(
        actual.filter(f64::NAN, 0.0),
        Err(OneEuroFilterError::NonFiniteSample)
    );
    assert_eq!(
        actual.filter(0.0, f64::NAN),
        Err(OneEuroFilterError::InvalidInterval)
    );
    assert_eq!(actual.filter(0.0, 0.0), Ok(0.0));
    actual.filter(10.0, 1.0).expect("accepted motion");
    let mut control = actual;
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            actual.filter(invalid, 1.0),
            Err(OneEuroFilterError::NonFiniteSample)
        );
    }
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            actual.filter(20.0, invalid),
            Err(OneEuroFilterError::InvalidInterval)
        );
    }
    assert_eq!(actual.filter(20.0, 1.0), control.filter(20.0, 1.0));
}

fn duplicate_scalar_sample_keeps_the_derivative() {
    let mut actual = OneEuroFilter::default();
    actual.filter(0.0, 0.0).expect("first sample");
    let previous = actual.filter(10.0, 1.0).expect("accepted motion");
    let mut control = actual;
    assert_eq!(actual.filter(99.0, 0.0), Ok(previous));
    assert_eq!(actual.filter(20.0, 1.0), control.filter(20.0, 1.0));
}

fn overflow_refusal_allows_a_healthy_next_sample() {
    let mut actual = OneEuroFilter::default();
    actual.filter(10.0, 0.0).expect("first sample");
    actual.filter(20.0, 1.0).expect("accepted motion");
    let mut control = actual;
    assert_eq!(
        actual.filter(f64::MAX, f64::from_bits(1)),
        Err(OneEuroFilterError::ArithmeticOverflow)
    );
    assert_eq!(actual.filter(30.0, 1.0), control.filter(30.0, 1.0));

    let mut adaptive =
        OneEuroFilter::new(OneEuroTuning::try_new(1.0, f64::MAX, 1.0).expect("valid tuning"));
    adaptive.filter(0.0, 0.0).expect("first sample");
    let mut adaptive_control = adaptive;
    assert_eq!(
        adaptive.filter(2.0, 1.0),
        Err(OneEuroFilterError::ArithmeticOverflow)
    );
    assert_eq!(adaptive.filter(1.0, 2.0), adaptive_control.filter(1.0, 2.0));
}

fn extreme_cutoff_and_interval_keep_meaningful_smoothing() {
    let mut crossing = OneEuroFilter::default();
    crossing.filter(f64::MAX, 0.0).expect("finite first sample");
    assert_eq!(crossing.filter(-f64::MAX, f64::MAX), Ok(-f64::MAX));

    let mut high =
        OneEuroFilter::new(OneEuroTuning::try_new(f64::MAX, 0.0, f64::MAX).expect("finite cutoff"));
    high.filter(0.0, 0.0).expect("first sample");
    assert_eq!(high.filter(1.0, 1.0), Ok(1.0));

    let mut low = OneEuroFilter::new(
        OneEuroTuning::try_new(f64::from_bits(1), 0.0, 1.0).expect("positive cutoff"),
    );
    low.filter(0.0, 0.0).expect("first sample");
    let output = low.filter(1.0, f64::MAX).expect("finite long interval");
    assert!(
        output > 1e-15 && output < 1e-13,
        "tiny cutoff lost its finite contribution: {output}"
    );

    low.reset();
    low.filter(0.0, 0.0).expect("new first sample");
    assert_eq!(low.filter(1.0, 0.25), Ok(f64::from_bits(2)));
}

fn fixed_cutoff_matches_a_known_scalar_step() {
    let mut filter =
        OneEuroFilter::new(OneEuroTuning::try_new(1.0, 0.0, 1.0).expect("fixed cutoff"));
    filter.filter(0.0, 0.0).expect("first sample");
    let output = filter.filter(10.0, 1.0).expect("finite step");
    assert!(
        (output - 8.626_974_383_015_87).abs() < 1e-9,
        "unexpected scalar step: {output}"
    );
}

fn constant_extremes_and_opposite_signs_keep_their_meaning() {
    let cutoff = 3.0 / (7.0 * std::f64::consts::TAU);
    for value in [f64::MAX, -f64::MAX] {
        let mut filter =
            OneEuroFilter::new(OneEuroTuning::try_new(cutoff, 0.0, 1.0).expect("fixed cutoff"));
        assert_eq!(filter.filter(value, 0.0), Ok(value));
        assert_eq!(
            filter.filter(value, 1.0),
            Ok(value),
            "constant signal drifted"
        );
        assert_eq!(
            filter.filter(value, 1.0),
            Ok(value),
            "constant signal gained speed"
        );
    }

    // Half smoothing at a two-second interval. Opposite-sign positions have
    // a representable slope and must cancel without overflowing their difference.
    let mut crossing = OneEuroFilter::new(
        OneEuroTuning::try_new(1.0 / (2.0 * std::f64::consts::TAU), 0.0, 1.0)
            .expect("half smoothing"),
    );
    crossing.filter(f64::MAX, 0.0).expect("first sample");
    assert_eq!(crossing.filter(-f64::MAX, 2.0), Ok(0.0));

    // A positive cutoff/interval product below representable alpha holds
    // the previous endpoint, while the overflowing positive limit picks next.
    let mut held = OneEuroFilter::new(
        OneEuroTuning::try_new(f64::from_bits(1), 0.0, 1.0).expect("tiny cutoff"),
    );
    held.filter(0.0, 0.0).expect("first sample");
    assert_eq!(held.filter(1.0, 0.0625), Ok(0.0));
    let mut direct =
        OneEuroFilter::new(OneEuroTuning::try_new(f64::MAX, 0.0, 1.0).expect("large cutoff"));
    direct.filter(0.0, 0.0).expect("first sample");
    assert_eq!(direct.filter(42.0, 1.0), Ok(42.0));
}

fn rejected_axis_does_not_commit_another_axis_or_clock() {
    let start = Instant::now();
    let mut actual = OneEuroFilter2D::default();
    assert_eq!(
        actual.filter(start, Offset::new(42.0, f64::NAN)),
        Err(OneEuroFilterError::NonFiniteSample)
    );
    assert_eq!(
        actual.filter(start, Offset::new(0.0, -f64::MAX)),
        Ok(Offset::new(0.0, -f64::MAX))
    );
    let mut control = actual;
    assert_eq!(
        actual.filter(start + Duration::from_secs(1), Offset::new(10.0, f64::MAX)),
        Err(OneEuroFilterError::ArithmeticOverflow)
    );
    let next = start + Duration::from_secs(2);
    let position = Offset::new(20.0, -f64::MAX);
    assert_eq!(
        actual.filter(next, position),
        control.filter(next, position)
    );
}

fn reset_admits_a_new_stroke_and_timeline() {
    let mut scalar = OneEuroFilter::default();
    scalar.filter(0.0, 0.0).expect("first sample");
    scalar.filter(10.0, 1.0).expect("accepted motion");
    scalar.reset();
    assert_eq!(scalar.filter(42.0, 0.0), Ok(42.0));

    let start = Instant::now();
    let mut position = OneEuroFilter2D::default();
    position
        .filter(start + Duration::from_secs(2), Offset::ZERO)
        .expect("first sample");
    position.reset();
    assert_eq!(
        position.filter(start, Offset::new(42.0, 84.0)),
        Ok(Offset::new(42.0, 84.0))
    );
}

#[test]
fn one_euro_admission_and_recovery() {
    let cases: &[(&str, fn())] = &[
        ("tuning admission", tuning_admission),
        ("finite extremes", finite_extreme_samples_do_not_publish_nan),
        (
            "invalid input recovery",
            nonfinite_samples_and_invalid_intervals_preserve_history,
        ),
        (
            "duplicate scalar",
            duplicate_scalar_sample_keeps_the_derivative,
        ),
        (
            "overflow recovery",
            overflow_refusal_allows_a_healthy_next_sample,
        ),
        (
            "extreme smoothing",
            extreme_cutoff_and_interval_keep_meaningful_smoothing,
        ),
        ("known step", fixed_cutoff_matches_a_known_scalar_step),
        (
            "stable extremes",
            constant_extremes_and_opposite_signs_keep_their_meaning,
        ),
        (
            "atomic axes",
            rejected_axis_does_not_commit_another_axis_or_clock,
        ),
        ("reset", reset_admits_a_new_stroke_and_timeline),
        (
            "regressed timestamp",
            regressed_timestamp_preserves_the_last_accepted_anchor,
        ),
    ];
    let mut failures = Vec::new();
    for (name, case) in cases {
        if std::panic::catch_unwind(*case).is_err() {
            failures.push(*name);
        }
    }
    assert!(failures.is_empty(), "failed rows: {failures:?}");
}
