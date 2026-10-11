//! `AnimationControllerBuilder` validates bounds like the constructors do.

use flui_animation::{RangeDefect, ValueRange};

/// `.bounds()` applies the constructors' rule: bounded means finite,
/// endpoints and span alike. A `lower >= upper` check alone accepts `NaN`
/// (`NaN >= upper` is always `false`) and any infinite pair with
/// `lower < upper`.
#[test]
fn bounds_rejects_non_finite_endpoints() {
    let cases = [
        (f64::NAN, 1.0, RangeDefect::NonFinite),
        (0.0, f64::NAN, RangeDefect::NonFinite),
        (f64::NEG_INFINITY, f64::INFINITY, RangeDefect::NonFinite),
        (f64::NEG_INFINITY, 5.0, RangeDefect::NonFinite),
        (5.0, f64::INFINITY, RangeDefect::NonFinite),
        (f64::NEG_INFINITY, f64::NEG_INFINITY, RangeDefect::NonFinite),
        (-f64::MAX, f64::MAX, RangeDefect::SpanOverflow),
        (2.0, 1.0, RangeDefect::Inverted),
        (1.0, 1.0, RangeDefect::Inverted),
    ];
    for (lower, upper, defect) in cases {
        let result: Result<ValueRange, RangeDefect> = ValueRange::new(lower, upper);
        assert_eq!(result, Err(defect), "bounds({lower}, {upper})");
    }
}
