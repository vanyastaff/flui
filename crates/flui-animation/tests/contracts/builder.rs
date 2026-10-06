//! `AnimationControllerBuilder` validates bounds like the constructors do.

use std::time::Duration;

use flui_animation::{AnimationControllerBuilder, AnimationError};
use flui_scheduler::UpdateScheduler;

/// `.bounds()` applies the constructors' rule: bounded means finite,
/// endpoints and span alike. A `lower >= upper` check alone accepts `NaN`
/// (`NaN >= upper` is always `false`) and any infinite pair with
/// `lower < upper`.
#[test]
fn bounds_rejects_non_finite_endpoints() {
    let scheduler = UpdateScheduler::new();
    let cases: &[(f64, f64)] = &[
        (f64::NAN, 1.0),
        (0.0, f64::NAN),
        (f64::NEG_INFINITY, f64::INFINITY),
        (f64::NEG_INFINITY, 5.0),
        (5.0, f64::INFINITY),
        (f64::NEG_INFINITY, f64::NEG_INFINITY),
        (-f64::MAX, f64::MAX),
    ];
    for &(lower, upper) in cases {
        let result = AnimationControllerBuilder::new(Duration::from_millis(100), &scheduler)
            .bounds(lower, upper);
        assert!(
            matches!(result, Err(AnimationError::InvalidBounds(_))),
            "bounds({lower}, {upper}) must be rejected"
        );
    }
}
