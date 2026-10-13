//! Controller commands, repeat sampling and delivery through public owning handles.

use flui_animation::{
    Animation, AnimationController, AnimationError, AnimationInput, AnimationStatus, Curve,
    DrivenController, Iterations, MotionClock, RangeDefect, RepeatMode, RepeatSpec, Simulation,
    ValueRange, Vsync, curves,
};
use flui_foundation::Listenable;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn controller(ms: u64) -> (DrivenController, AnimationController) {
    let owner =
        AnimationController::builder(Duration::from_millis(ms)).build_on(Some(&Vsync::new()));
    let handle = owner.controller().clone();
    (owner, handle)
}

fn forward_from_mid_scales_run_duration() {
    let (_owner, c) = controller(100);
    c.forward_from(Some(0.5)).unwrap();
    c.tick_at(Duration::from_millis(25));
    assert!(
        (c.value() - 0.75).abs() < 1e-3,
        "remaining half-range takes 50ms"
    );
    c.tick_at(Duration::from_millis(50));
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
}

fn small_distances_keep_their_authored_duration() {
    for (from, target) in [(0.0, 0.5e-6), (0.5e-6, 0.0)] {
        let (_owner, c) = controller(1000);
        c.set_value(from);
        let run = c.animate_to(target, Some(Duration::from_secs(1))).unwrap();
        assert_eq!(
            c.value(),
            from,
            "a distinct target does not settle at admission"
        );
        assert!(run.is_pending());
        c.tick_at(Duration::from_millis(250));
        assert!((c.value() - (from + (target - from) * 0.25)).abs() < 1e-15);
        assert!(
            run.is_pending(),
            "the authored duration still owns completion"
        );
        c.tick_at(Duration::from_secs(1));
        assert_eq!(c.value(), target);
        assert!(run.is_complete());
    }
}

fn small_remaining_distance_runs_until_its_scaled_deadline() {
    for reverse in [false, true] {
        let (_owner, c) = controller(1000);
        let (from, target, run) = if reverse {
            (0.5e-6, 0.0, c.reverse_from(Some(0.5e-6)).unwrap())
        } else {
            (
                1.0 - 0.5e-6,
                1.0,
                c.forward_from(Some(1.0 - 0.5e-6)).unwrap(),
            )
        };
        assert_eq!(c.value(), from, "remaining distance is nonzero");
        assert!(run.is_pending());
        c.tick_at(Duration::from_nanos(250));
        assert!((c.value() - (from + (target - from) * 0.5)).abs() < 1e-15);
        assert!(run.is_pending());
        c.tick_at(Duration::from_micros(1));
        assert_eq!(c.value(), target);
        assert!(run.is_complete());
    }
}

fn small_from_jumps_notify_their_exact_value() {
    for reverse in [false, true] {
        let (_owner, c) = controller(1000);
        c.set_value(if reverse { 1.0 } else { 0.0 });
        let values = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&values);
        let source = c.clone();
        let listener = c.add_listener(Rc::new(move || observed.borrow_mut().push(source.value())));
        let from = if reverse { 1.0 - 0.5e-6 } else { 0.5e-6 };
        let run = if reverse {
            c.reverse_from(Some(from))
        } else {
            c.forward_from(Some(from))
        }
        .unwrap();
        assert_eq!(
            *values.borrow(),
            [from],
            "every changed position is observable"
        );
        assert!(run.is_pending());
        c.remove_listener(listener);
    }
}

fn small_zero_duration_settlement_notifies_the_endpoint() {
    let (_owner, c) = controller(1000);
    let values = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&values);
    let source = c.clone();
    let listener = c.add_listener(Rc::new(move || observed.borrow_mut().push(source.value())));
    let run = c.animate_to(0.5e-6, Some(Duration::ZERO)).unwrap();
    assert!(run.is_complete());
    assert_eq!(
        *values.borrow(),
        [0.5e-6],
        "the endpoint change is delivered once"
    );
    c.remove_listener(listener);
}

fn nearby_interior_positions_keep_their_directional_phase() {
    for reverse in [false, true] {
        for position in [0.5e-6, 1.0 - 0.5e-6] {
            let (_owner, c) = controller(1000);
            c.set_value(0.5);
            let _run = if reverse { c.reverse() } else { c.forward() }.unwrap();
            c.set_value(position);
            assert_eq!(c.value(), position);
            assert_eq!(
                c.status(),
                if reverse {
                    AnimationStatus::Reverse
                } else {
                    AnimationStatus::Forward
                },
                "a representable interior position is not a bound"
            );
            assert!(!c.is_animating());
            c.stop().unwrap();
            assert_eq!(
                c.status(),
                if reverse {
                    AnimationStatus::ReverseSettled
                } else {
                    AnimationStatus::ForwardSettled
                },
                "an interior stop settles by its run direction"
            );
        }
    }
}

fn set_value_nan_is_canonicalized() {
    let (_owner, c) = controller(100);
    c.set_value(0.5);
    c.set_value(f64::NAN);
    assert_eq!(c.value(), 0.0, "NaN selects the lower bound");
    assert_eq!(c.status(), AnimationStatus::ReverseSettled);
}

fn bounded_ranges_refuse_infinity_and_unbounded_starts_at_zero() {
    assert_eq!(ValueRange::new(20.0, 10.0), Err(RangeDefect::Inverted));
    assert_eq!(
        ValueRange::new(f64::NEG_INFINITY, f64::INFINITY),
        Err(RangeDefect::NonFinite)
    );
    let owner = AnimationController::builder(Duration::from_millis(1))
        .unbounded()
        .build_on(Some(&Vsync::new()));
    let c = owner.controller();
    assert_eq!(c.value(), 0.0);
    let run = c.animate_to(1.0, None).unwrap();
    c.tick_at(Duration::from_micros(250));
    assert_eq!(
        c.value(),
        0.25,
        "motion starts at zero rather than an infinite bound"
    );
    assert!(run.is_pending());
}

fn disposed_controller_rejects_forward() {
    let (mut owner, c) = controller(100);
    owner.dispose();
    assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
}

fn bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound() {
    let (_owner, c) = controller(100);
    c.animate_to(f64::INFINITY, None).unwrap();
    c.tick_at(Duration::from_millis(100));
    assert_eq!(c.value(), 1.0);
    let (_owner, c) = controller(100);
    c.forward_from(Some(f64::INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        1.0,
        "clamped from equals the forward target synchronously"
    );
    let (_owner, c) = controller(100);
    c.reverse_from(Some(f64::NEG_INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        0.0,
        "clamped from equals the reverse target synchronously"
    );
}

fn fling_refuses_a_non_finite_velocity() {
    let (_owner, c) = controller(100);
    assert!(
        matches!(c.fling(f64::NAN), Err(AnimationError::NonFiniteInput {
        input: AnimationInput::Velocity, value,
    }) if value.is_nan())
    );
    assert!(!c.is_animating());
}

struct GoesNanMidRun {
    nan_at: f64,
}

impl Simulation for GoesNanMidRun {
    fn x(&self, time: f64) -> f64 {
        if time >= self.nan_at { f64::NAN } else { time }
    }
    fn dx(&self, _: f64) -> f64 {
        1.0
    }
    fn is_done(&self, _: f64) -> bool {
        false
    }
}

fn a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_millis(100))
        .unbounded()
        .build_on(Some(&vsync));
    let c = owner.controller();
    let mut future = c.animate_with(GoesNanMidRun { nan_at: 1.0 }).unwrap();
    let mut clock = MotionClock::new();
    vsync.tick_all(&clock.frame(Duration::ZERO));
    vsync.tick_all(&clock.frame(Duration::from_millis(500)));
    assert_eq!(c.value(), 0.5);
    assert!(vsync.has_running());
    vsync.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!(
        c.value(),
        0.5,
        "a non-finite sample ends at the last finite value"
    );
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
    assert!(!vsync.has_running());
    use std::future::Future;
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        std::pin::Pin::new(&mut future).poll(&mut context),
        std::task::Poll::Ready(Ok(()))
    );
}

fn status_callback_can_reenter_controller_without_deadlock() {
    let (_owner, c) = controller(100);
    let reentered = Arc::new(AtomicUsize::new(0));
    let source = c.clone();
    let observed = Arc::clone(&reentered);
    let _subscription = c.subscribe_status(Rc::new(move |status| {
        if status == AnimationStatus::ForwardSettled {
            let _value = source.value();
            source.reverse().unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
        }
    }));
    c.forward().unwrap();
    c.tick_at(Duration::from_millis(100));
    assert_eq!(reentered.load(Ordering::SeqCst), 1);
}

fn repeat_consumes_all_cycles_in_one_long_frame() {
    let (_owner, c) = controller(100);
    c.repeat(
        RepeatSpec::new(RepeatMode::Restart)
            .with_period(Duration::from_millis(10))
            .with_iterations(Iterations::Finite(4)),
    )
    .unwrap();
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.tick_at(Duration::from_millis(45));
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
}

/// A repeat that could never change value is refused: equal endpoints by
/// `ValueRange`, a range outside the bounds once clamped into them.
fn repeat_rejects_an_empty_range() {
    assert_eq!(ValueRange::new(0.5, 0.5), Err(RangeDefect::Inverted));
    let (_owner, c) = controller(100);
    let outside = ValueRange::new(2.0, 3.0).expect("ordered range");
    assert!(matches!(
        c.repeat(RepeatSpec::new(RepeatMode::Restart).with_range(outside)),
        Err(AnimationError::InvalidRepeatRange {
            min: 1.0,
            max: 1.0,
            defect: RangeDefect::Inverted,
            ..
        })
    ));
}

/// Zero iterations settle at the call, at the current value clamped into
/// the range: no iteration ran, so there is no end to land on, in either
/// mode and whatever the period.
fn zero_iterations_settle_at_the_call_without_landing() {
    for mode in [RepeatMode::Restart, RepeatMode::Alternate] {
        let (_owner, c) = controller(100);
        c.set_value(0.1);
        let generation = c.run_generation();
        let range = ValueRange::new(0.2, 0.8).expect("ordered range");
        let run = c
            .repeat(
                RepeatSpec::new(mode)
                    .with_range(range)
                    .with_period(Duration::from_secs(1))
                    .with_iterations(Iterations::Finite(0)),
            )
            .expect("zero iterations");
        assert!(run.is_complete(), "{mode:?}");
        assert_eq!(c.value(), 0.2, "{mode:?}");
        assert_eq!(c.status(), AnimationStatus::ForwardSettled, "{mode:?}");
        assert_eq!(c.run_generation(), generation, "{mode:?}: no run starts");
        c.tick_at(Duration::from_secs(5));
        assert_eq!(c.value(), 0.2, "{mode:?}");
    }
}

fn repeat_value_is_partition_invariant_across_a_skipped_cycle() {
    for mode in [RepeatMode::Restart, RepeatMode::Alternate] {
        let (_direct_owner, direct) = controller(1000);
        let (_partitioned_owner, partitioned) = controller(1000);
        for c in [&direct, &partitioned] {
            c.repeat(RepeatSpec::new(mode).with_period(Duration::from_secs(1)))
                .unwrap();
        }
        direct.tick_at(Duration::from_millis(1250));
        partitioned.tick_at(Duration::from_secs(1));
        partitioned.tick_at(Duration::from_millis(1250));
        let expected = match mode {
            RepeatMode::Alternate => 0.75,
            _ => 0.25,
        };
        assert!((direct.value() - expected).abs() < 1e-6, "{mode:?}");
        assert_eq!(direct.value(), partitioned.value());
        assert_eq!(direct.status(), partitioned.status());
    }
}

fn finite_bounce_holds_stale_samples_and_settles_on_its_final_leg() {
    let (_owner, c) = controller(100);
    c.repeat(RepeatSpec::new(RepeatMode::Alternate).with_iterations(Iterations::Finite(4)))
        .unwrap();
    for (millis, expected, tolerance) in [
        (25, 0.25, 1e-6),
        (50, 0.5, 1e-6),
        (99, 0.99, 1e-3),
        (100, 1.0, 1e-6),
        (60, 1.0, 1e-6),
    ] {
        c.tick_at(Duration::from_millis(millis));
        assert!((c.value() - expected).abs() < tolerance, "millis={millis}");
    }
    let (_owner, c) = controller(100);
    c.repeat(RepeatSpec::new(RepeatMode::Alternate).with_iterations(Iterations::Finite(4)))
        .unwrap();
    c.tick_at(Duration::from_millis(160));
    assert!((c.value() - 0.4).abs() < 1e-6);
    let (_owner, c) = controller(100);
    c.repeat(RepeatSpec::new(RepeatMode::Alternate).with_iterations(Iterations::Finite(4)))
        .unwrap();
    c.tick_at(Duration::from_millis(400));
    assert!((c.value() - 0.0).abs() < 1e-6);
    assert_eq!(c.status(), AnimationStatus::ReverseSettled);
}

fn maximum_elapsed_time_exhausts_a_finite_repeat() {
    let (_owner, c) = controller(100);
    c.repeat(
        RepeatSpec::new(RepeatMode::Restart)
            .with_period(Duration::from_millis(100))
            .with_iterations(Iterations::Finite(2)),
    )
    .unwrap();
    c.tick_at(Duration::MAX);
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
    assert!((c.value() - 1.0).abs() < 1e-6);
}

fn repeat_with_zero_period_settles_synchronously_at_the_call() {
    let (_owner, c) = controller(100);
    let generation = c.run_generation();
    let future = c
        .repeat(
            RepeatSpec::new(RepeatMode::Alternate)
                .with_period(Duration::ZERO)
                .with_iterations(Iterations::Finite(3)),
        )
        .unwrap();
    assert!(future.is_complete());
    assert!((c.value() - 1.0).abs() < 1e-6);
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
    assert_eq!(c.run_generation(), generation);
    c.tick_at(Duration::from_secs(1));
    assert!((c.value() - 1.0).abs() < 1e-6);
    let (_owner, c) = controller(100);
    let future = c
        .repeat(RepeatSpec::new(RepeatMode::Restart).with_period(Duration::ZERO))
        .unwrap();
    assert!(future.is_complete());
    assert!((c.value() - 1.0).abs() < 1e-6);
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
}

fn repeat_reverse_leg_velocity_is_negative() {
    let (_owner, c) = controller(100);
    c.repeat(RepeatSpec::new(RepeatMode::Alternate).with_period(Duration::from_secs(1)))
        .unwrap();
    c.tick_at(Duration::from_millis(1500));
    assert!((c.value() - 0.5).abs() < 1e-6);
    assert!((c.velocity() - (-1.0)).abs() < 1e-6);
}

fn animate_to_curved_eases_through_the_given_curve() {
    let (_owner, c) = controller(100);
    c.animate_to_curved(
        1.0,
        Some(Duration::from_millis(100)),
        Arc::new(curves::EASE_IN_QUINT),
    )
    .unwrap();
    c.tick_at(Duration::from_millis(50));
    let expected = curves::EASE_IN_QUINT.transform(0.5);
    assert!((c.value() - expected).abs() < 1e-3);
    c.tick_at(Duration::from_millis(100));
    assert_eq!(c.value(), 1.0);
    assert_eq!(c.status(), AnimationStatus::ForwardSettled);
}

fn a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable() {
    let (_owner, c) = controller(100);
    let first = c.forward().unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&order);
    let _subscription =
        c.subscribe_status(Rc::new(move |_| observed.lock().push("new_run_status")));
    let observed = Arc::clone(&order);
    first.when_complete_or_cancel(move |_| observed.lock().push("displaced_run_canceled"));
    c.animate_to(1.0, Some(Duration::ZERO)).unwrap();
    assert_eq!(
        order.lock().as_slice(),
        &["new_run_status", "displaced_run_canceled"]
    );
}

struct InstantSimulation {
    value: f64,
}

impl Simulation for InstantSimulation {
    fn x(&self, _: f64) -> f64 {
        self.value
    }
    fn dx(&self, _: f64) -> f64 {
        0.0
    }
    fn is_done(&self, _: f64) -> bool {
        true
    }
}

fn simulation_run_future_resolves_ok_when_the_simulation_finishes() {
    let (_owner, c) = controller(100);
    let future = c.animate_with(InstantSimulation { value: 0.5 }).unwrap();
    assert!(future.is_pending());
    c.tick_at(Duration::ZERO);
    assert!(future.is_complete());
}

fn stop_cancels_the_active_run() {
    let (_owner, c) = controller(100);
    let future = c.forward().unwrap();
    c.stop().unwrap();
    assert!(future.is_canceled());
}

fn completion_callback_can_read_and_write_its_controller() {
    let (_owner, c) = controller(100);
    let source = c.clone();
    let future = c.forward().unwrap();
    let observed = Arc::new(Mutex::new(None));
    let delivered = Arc::clone(&observed);
    future.when_complete_or_cancel(move |outcome| {
        assert_eq!(source.value(), 1.0);
        source.set_value(0.5);
        *delivered.lock() = Some(outcome);
    });
    c.tick_at(Duration::from_millis(100));
    assert_eq!(*observed.lock(), Some(Ok(())));
    assert_eq!(
        c.value(),
        0.5,
        "reentrant write survives the finishing frame"
    );
}

fn a_panicking_status_listener_leaves_the_finished_run_ok() {
    let (_owner, c) = controller(100);
    let future = c.forward().unwrap();
    let seen = Arc::new(Mutex::new(None));
    let observed = Arc::clone(&seen);
    future.when_complete_or_cancel(move |outcome| *observed.lock() = Some(outcome));
    let _subscription = c.subscribe_status(Rc::new(|status| {
        assert!(
            status != AnimationStatus::ForwardSettled,
            "status listener panics on completion"
        );
    }));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.tick_at(Duration::from_millis(100));
    }));
    assert!(result.is_err());
    assert_eq!(
        *seen.lock(),
        Some(Ok(())),
        "actual continuation delivery survives listener failure"
    );
    assert!(future.is_complete());
}

/// `set_value` mid-run ends the run while the status stays directional, so
/// `velocity()` reports no motion until a new run starts, on a bounded and
/// an unbounded controller alike.
fn velocity_is_zero_once_set_value_ends_the_run() {
    for bounded in [true, false] {
        let builder = AnimationController::builder(Duration::from_secs(1));
        let owner = if bounded {
            builder.build_on(Some(&Vsync::new()))
        } else {
            builder.unbounded().build_on(Some(&Vsync::new()))
        };
        let c = owner.controller();
        let target = if bounded { None } else { Some(1.0) };
        let _run = match target {
            None => c.forward(),
            Some(target) => c.animate_to(target, None),
        }
        .expect("run");
        c.tick_at(Duration::ZERO);
        c.tick_at(Duration::from_millis(500));
        assert!(c.velocity() > 0.0, "bounded={bounded}: the run moves");
        c.set_value(0.2);
        assert_eq!(c.status(), AnimationStatus::Forward, "bounded={bounded}");
        assert_eq!(c.velocity(), 0.0, "bounded={bounded}: no run, no motion");
        let _next = match target {
            None => c.forward(),
            Some(target) => c.animate_to(target, None),
        }
        .expect("next run");
        c.tick_at(Duration::from_millis(600));
        c.tick_at(Duration::from_millis(700));
        assert!(
            c.velocity() > 0.0,
            "bounded={bounded}: a new run moves again"
        );
    }
}

#[test]
fn controller_contract() {
    crate::run_table(&[
        (
            "velocity is zero once set_value ends the run",
            velocity_is_zero_once_set_value_ends_the_run,
        ),
        (
            "small distances keep authored duration",
            small_distances_keep_their_authored_duration,
        ),
        (
            "small remaining distance keeps scaled deadline",
            small_remaining_distance_runs_until_its_scaled_deadline,
        ),
        (
            "small from jumps notify",
            small_from_jumps_notify_their_exact_value,
        ),
        (
            "small zero duration settlement notifies",
            small_zero_duration_settlement_notifies_the_endpoint,
        ),
        (
            "nearby interior positions keep phase",
            nearby_interior_positions_keep_their_directional_phase,
        ),
        (
            "forward from mid scales run duration",
            forward_from_mid_scales_run_duration,
        ),
        (
            "set value nan is canonicalized",
            set_value_nan_is_canonicalized,
        ),
        (
            "finite bounds and unbounded origin",
            bounded_ranges_refuse_infinity_and_unbounded_starts_at_zero,
        ),
        (
            "disposed controller rejects forward",
            disposed_controller_rejects_forward,
        ),
        (
            "infinite target and from clamp to their bound",
            bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound,
        ),
        (
            "animate to curved uses the given curve",
            animate_to_curved_eases_through_the_given_curve,
        ),
        ("stop cancels the active run", stop_cancels_the_active_run),
        (
            "simulation future resolves on completion",
            simulation_run_future_resolves_ok_when_the_simulation_finishes,
        ),
    ]);
}

#[test]
fn repeat_contract() {
    crate::run_table(&[
        (
            "repeat consumes all cycles in one long frame",
            repeat_consumes_all_cycles_in_one_long_frame,
        ),
        (
            "repeat refuses an empty range",
            repeat_rejects_an_empty_range,
        ),
        (
            "zero iterations settle at the call",
            zero_iterations_settle_at_the_call_without_landing,
        ),
        (
            "repeat sample is partition invariant",
            repeat_value_is_partition_invariant_across_a_skipped_cycle,
        ),
        (
            "finite bounce holds stale time and exhausts",
            finite_bounce_holds_stale_samples_and_settles_on_its_final_leg,
        ),
        (
            "maximum time exhausts finite repeat",
            maximum_elapsed_time_exhausts_a_finite_repeat,
        ),
        (
            "zero period settles synchronously",
            repeat_with_zero_period_settles_synchronously_at_the_call,
        ),
        (
            "reverse leg velocity is negative",
            repeat_reverse_leg_velocity_is_negative,
        ),
    ]);
}

#[test]
fn controller_failure_contract() {
    crate::run_table(&[
        (
            "fling refuses non-finite velocity",
            fling_refuses_a_non_finite_velocity,
        ),
        (
            "non-finite simulation sample ends the run",
            a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value,
        ),
        (
            "listener failure preserves successful completion",
            a_panicking_status_listener_leaves_the_finished_run_ok,
        ),
    ]);
}

#[test]
fn reentrancy_and_run_ordering() {
    crate::run_table(&[
        (
            "status callback reenters controller",
            status_callback_can_reenter_controller_without_deadlock,
        ),
        (
            "zero duration displacement preserves delivery order",
            a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable,
        ),
        (
            "completion callback reads and writes its controller",
            completion_callback_can_read_and_write_its_controller,
        ),
    ]);
}
