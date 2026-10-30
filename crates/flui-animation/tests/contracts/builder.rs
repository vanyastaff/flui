//! `AnimationControllerBuilder` validates bounds like the constructors do.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, RangeDefect, ValueRange};

struct WarningCount(Arc<AtomicUsize>);

impl tracing::Subscriber for WarningCount {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn non_finite_initial_value(input: f64, bounded: bool, expected: f64) {
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(WarningCount(Arc::clone(&warnings)), || {
        let builder = AnimationController::builder(Duration::from_secs(1)).initial_value(input);
        let controller = if bounded {
            builder.bounds(ValueRange::new(3.0, 7.0).unwrap()).build()
        } else {
            builder.unbounded().build()
        };
        assert_eq!(controller.value(), expected, "initial input {input}");
        assert_eq!(
            warnings.load(Ordering::Relaxed),
            1,
            "invalid initialization must be observable"
        );
        controller.set_value(f64::NAN).expect("no build is running");
        assert_eq!(
            warnings.load(Ordering::Relaxed),
            1,
            "initialization and direct writes share one warning latch"
        );
        let run = controller.animate_to(5.0, None).unwrap();
        controller.tick_at(Duration::from_secs(1));
        assert!(
            run.is_complete(),
            "a canonicalized initial value must allow subsequent motion"
        );
        assert_eq!(controller.value(), 5.0);
        assert_eq!(warnings.load(Ordering::Relaxed), 1);
    });
}

fn bounded_initial_nan() {
    non_finite_initial_value(f64::NAN, true, 3.0);
}
fn bounded_initial_positive_infinity() {
    non_finite_initial_value(f64::INFINITY, true, 7.0);
}
fn bounded_initial_negative_infinity() {
    non_finite_initial_value(f64::NEG_INFINITY, true, 3.0);
}
fn unbounded_initial_nan() {
    non_finite_initial_value(f64::NAN, false, 0.0);
}
fn unbounded_initial_positive_infinity() {
    non_finite_initial_value(f64::INFINITY, false, 0.0);
}
fn unbounded_initial_negative_infinity() {
    non_finite_initial_value(f64::NEG_INFINITY, false, 0.0);
}

fn finite_initial_values_drive_from_the_canonical_sample() {
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(WarningCount(Arc::clone(&warnings)), || {
        for (input, bounded, admitted) in [
            (2.0, true, 3.0),
            (4.0, true, 4.0),
            (8.0, true, 7.0),
            (4.0, false, 4.0),
        ] {
            let builder = AnimationController::builder(Duration::from_secs(1)).initial_value(input);
            let controller = if bounded {
                builder.bounds(ValueRange::new(3.0, 7.0).unwrap()).build()
            } else {
                builder.unbounded().build()
            };
            let run = controller
                .animate_to(5.0, Some(Duration::from_secs(1)))
                .unwrap();
            controller.tick_at(Duration::from_millis(500));
            assert_eq!(
                controller.value(),
                f64::midpoint(admitted, 5.0),
                "input={input}, bounded={bounded}"
            );
            assert!(run.is_pending());
            controller.tick_at(Duration::from_secs(1));
            assert_eq!(controller.value(), 5.0);
            assert!(run.is_complete());
        }
    });
    assert_eq!(
        warnings.load(Ordering::Relaxed),
        0,
        "finite initialization and motion do not warn"
    );
}

fn default_bounds_and_directional_durations_drive_both_legs() {
    let controller = AnimationController::builder(Duration::from_secs(1))
        .reverse_duration(Duration::from_secs(2))
        .build();
    let entrance = controller.forward().unwrap();
    controller.tick_at(Duration::from_millis(500));
    assert_eq!(controller.value(), 0.5);
    assert!(entrance.is_pending());
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(controller.value(), 1.0);
    assert!(entrance.is_complete());
    let exit = controller.reverse().unwrap();
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(controller.value(), 0.5);
    assert!(exit.is_pending());
    controller.tick_at(Duration::from_secs(2));
    assert_eq!(controller.value(), 0.0);
    assert!(exit.is_complete());
}

#[test]
fn controller_builder_contract() {
    crate::run_table(&[
        (
            "finite initial samples",
            finite_initial_values_drive_from_the_canonical_sample,
        ),
        (
            "default bounds and directional durations",
            default_bounds_and_directional_durations_drive_both_legs,
        ),
        ("bounded initial NaN", bounded_initial_nan),
        (
            "bounded initial positive infinity",
            bounded_initial_positive_infinity,
        ),
        (
            "bounded initial negative infinity",
            bounded_initial_negative_infinity,
        ),
        ("unbounded initial NaN", unbounded_initial_nan),
        (
            "unbounded initial positive infinity",
            unbounded_initial_positive_infinity,
        ),
        (
            "unbounded initial negative infinity",
            unbounded_initial_negative_infinity,
        ),
    ]);
}

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
