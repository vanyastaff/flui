//! Public admission, numerical recovery and timestamp contracts for smoothing.

use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::processing::{OneEuroFilter, OneEuroFilter2D};
use web_time::Instant;

fn finite_extreme_samples_do_not_publish_nan() {
    let mut filter = OneEuroFilter::default();
    assert_eq!(filter.filter(f64::MAX, 1.0), f64::MAX);
    let result = filter.filter(-f64::MAX, 1.0);
    assert!(result.is_finite(), "finite inputs published {result}");
}

fn regressed_timestamp_preserves_the_last_accepted_anchor() {
    let start = Instant::now();
    let mut actual = OneEuroFilter2D::default();
    let mut control = actual;
    for filter in [&mut actual, &mut control] {
        filter.filter(start, Offset::ZERO);
        filter.filter(start + Duration::from_secs(2), Offset::new(10.0, 20.0));
    }
    actual.filter(start + Duration::from_secs(1), Offset::new(90.0, 80.0));
    let next = start + Duration::from_secs(3);
    let position = Offset::new(30.0, 40.0);
    assert_eq!(actual.filter(next, position), control.filter(next, position));
}

#[test]
fn one_euro_admission_and_recovery() {
    let cases: &[(&str, fn())] = &[
        ("finite extremes", finite_extreme_samples_do_not_publish_nan),
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
