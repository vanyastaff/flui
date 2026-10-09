//! Test module for `AnimationController` (split out of `controller.rs` via
//! `#[path = "controller_tests.rs"] mod tests;` -- kept as a sibling file
//! because this crate's test module is itself larger than most production
//! files in the workspace; splitting it stops it from dominating
//! `controller.rs`'s own line count and lets an editor/reviewer open
//! "the tests" and "the implementation" as two separate, right-sized files.

use super::*;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn controller(ms: u64) -> AnimationController {
    AnimationController::builder(Duration::from_millis(ms)).build()
}

// Counter injection is private because reaching this boundary through the
// public API would require u64::MAX admissions or samples. All observations
// below use the controller and its actual run outcome.
fn exhausted_run_identities_refuse_without_displacing_the_last_run() {
    for (name, start) in [
        (
            "forward",
            (|c: &AnimationController| c.forward_from(Some(0.1)))
                as fn(&AnimationController) -> Result<AnimationRunFuture, AnimationError>,
        ),
        ("reverse", |c: &AnimationController| {
            c.reverse_from(Some(0.9))
        }),
        ("target", |c: &AnimationController| c.animate_to(0.9, None)),
        ("repeat", |c: &AnimationController| c.repeat(false)),
        ("fling", |c: &AnimationController| c.fling(1.0)),
        ("simulation", |c: &AnimationController| {
            c.animate_with(Box::new(InstantSimulation { value: 0.9 }))
        }),
    ] {
        let c = controller(100);
        c.inner.borrow_mut().run_generation = u64::MAX - 1;
        let last = c.forward().expect("last identity is available");
        c.tick_at(Duration::from_millis(25));
        let held = c.value();
        assert!(
            matches!(start(&c), Err(AnimationError::IdentityExhausted)),
            "{name}: terminal identity refuses a new run"
        );
        assert_eq!(c.value(), held, "{name}: refusal commits no input value");
        assert_eq!(c.run_generation(), u64::MAX);
        assert!(
            last.is_pending(),
            "{name}: refusal does not cancel accepted work"
        );
        c.tick_at(Duration::from_millis(100));
        assert!(last.is_complete());
        c.reset()
            .expect("reset still works after run identity exhaustion");
        assert!(
            matches!(start(&c), Err(AnimationError::IdentityExhausted)),
            "{name}: refusal is permanent"
        );
    }
}

fn exhausted_sample_identities_cancel_without_reissuing_a_stale_sample() {
    let c = controller(100);
    let run = c.forward().expect("run");
    c.inner.borrow_mut().sample_epoch = u64::MAX - 1;
    c.tick_at(Duration::from_millis(25));
    assert_eq!(c.value(), 0.25);
    c.tick_at(Duration::from_millis(50));
    assert_eq!(
        c.value(),
        0.25,
        "no sample identity can wrap to an older one"
    );
    assert!(
        run.is_canceled(),
        "accepted work receives a terminal outcome"
    );
    assert!(!c.is_animating());
    assert!(
        matches!(c.forward(), Err(AnimationError::IdentityExhausted)),
        "new runs cannot reuse exhausted sample identities"
    );
    c.reset().expect("healthy state edits remain available");
    assert!(
        matches!(c.forward(), Err(AnimationError::IdentityExhausted)),
        "refusal survives reset"
    );
}

// ---- remaining-fraction duration scaling ----

fn forward_from_mid_scales_run_duration() {
    let c = controller(100);
    c.forward_from(Some(0.5)).unwrap();
    // Half the range remains -> 50ms run. 25ms in = halfway -> 0.75.
    c.tick_at(std::time::Duration::from_secs_f64(0.025));
    assert!(
        (c.value() - 0.75).abs() < 1e-3,
        "constant velocity from 0.5: got {}",
        c.value()
    );
    c.tick_at(std::time::Duration::from_secs_f64(0.05));
    assert_eq!(c.status(), AnimationStatus::Completed);
    crate::test_cases::dispose_controller(&c);
}

fn set_value_nan_is_canonicalized() {
    let c = controller(100);
    c.set_value(0.5);
    c.set_value(f64::NAN);
    assert_eq!(
        c.value(),
        0.0,
        "NaN must canonicalize to the lower bound, not poison the value"
    );
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    crate::test_cases::dispose_controller(&c);
}

/// `without_ticker_bounds` is a BOUNDED constructor: bounded means
/// finite (#1183). This test used to assert the OPPOSITE — that a
/// wide-open `(NEG_INFINITY, INFINITY)` pair was ACCEPTED, landing
/// `value() == NEG_INFINITY` — that assertion is deliberately inverted
/// here, not preserved: unboundedness is now a constructor fact
/// ([`AnimationController::unbounded_without_ticker`]), not a bound
/// value, so the same wide-open pair this test used to accept must now
/// be rejected, and the unbounded shape starts at `0.0`, never `-inf`.
fn without_ticker_bounds_rejects_wide_open_ones() {
    let rejected = crate::ValueRange::new(20.0, 10.0).map(|bounds| {
        AnimationController::builder(Duration::from_millis(1))
            .bounds(bounds)
            .build()
    });
    assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

    let wide_open = crate::ValueRange::new(f64::NEG_INFINITY, f64::INFINITY).map(|bounds| {
        AnimationController::builder(Duration::from_millis(1))
            .bounds(bounds)
            .build()
    });
    assert!(
        matches!(wide_open, Err(AnimationError::InvalidBounds(_))),
        "a wide-open pair is unboundedness spelled as bounds -- reject it; \
         use unbounded_without_ticker instead"
    );

    let c = AnimationController::builder(Duration::from_millis(1))
        .unbounded()
        .build();
    assert_eq!(
        c.value(),
        0.0,
        "unbounded_without_ticker starts at 0.0, never -inf"
    );
    crate::test_cases::dispose_controller(&c);
}

fn disposed_controller_rejects_forward() {
    let c = controller(100);
    crate::test_cases::dispose_controller(&c);
    assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
}

// ---- #1183: unboundedness is a constructor fact ----------------------

/// `+-inf` still CLAMPS on a bounded controller (the "go to
/// the end" idiom) -- only a bound-less direction refuses.
fn bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound() {
    // `target` clamps to the finite upper bound and the run proceeds
    // normally from there (clamping the target does not mean an
    // instant settle: `value` still starts at the entry value and
    // reaches `1.0` only once the run completes).
    let c = controller(100);
    c.animate_to(f64::INFINITY, None).unwrap();
    c.tick_at(std::time::Duration::from_secs_f64(0.1));
    assert_eq!(
        c.value(),
        1.0,
        "animate_to(+inf) clamps to the finite upper bound"
    );
    crate::test_cases::dispose_controller(&c);

    // `from` clamping to the upper bound makes `forward_from`'s own
    // target (also `upper_bound`) already reached -- this settles
    // SYNCHRONOUSLY (zero distance), unlike the `animate_to` case above.
    let c = controller(100);
    c.forward_from(Some(f64::INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        1.0,
        "forward_from(Some(+inf)) clamps to the finite upper bound"
    );
    crate::test_cases::dispose_controller(&c);

    // Mirror case: `reverse_from`'s `from` clamps to the LOWER bound,
    // which is also `reverse`'s own target -- settles SYNCHRONOUSLY at
    // the lower bound, the reverse-direction twin of the case above.
    let c = controller(100);
    c.reverse_from(Some(f64::NEG_INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        0.0,
        "reverse_from(Some(-inf)) clamps to the finite lower bound"
    );
    crate::test_cases::dispose_controller(&c);
}

// ---- #1183: fling_with's ordering and non-finite refusals ---------------

/// `fling_with` refuses a non-finite `velocity` before ANY mutation:
/// unguarded, `NaN < 0.0` is `false`, so a NaN velocity took the
/// Forward branch and built a spring whose `SpringSimulation` never
/// reaches `is_done` for a NaN target.
fn fling_refuses_a_non_finite_velocity() {
    let c = controller(100);
    let r = c.fling(f64::NAN);
    assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
    assert!(!c.is_animating());
}

// ---- #1183: no path reads NaN -- simulation and tick endpoints ---------

/// A scratch [`Simulation`] whose `x()` is finite at `t = 0` (so
/// `drive_simulation` accepts it) but returns NaN once mid-run.
struct GoesNanMidRun {
    nan_at: f64,
}

impl Simulation for GoesNanMidRun {
    fn x(&self, time: f64) -> f64 {
        if time >= self.nan_at { f64::NAN } else { time }
    }
    fn dx(&self, _time: f64) -> f64 {
        1.0
    }
    fn is_done(&self, _time: f64) -> bool {
        false
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}

/// A mid-run non-finite sample ENDS the run at the last finite value --
/// settled status by direction, the future resolves `Ok`, the ticker
/// stops, and the run no longer holds `Vsync` open (checked via a real
/// registration's `has_running()`). NOT "value unchanged, run
/// continues": that would leave `active_run` installed forever.
fn a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value() {
    let vsync = crate::vsync::Vsync::new();
    let owner = AnimationController::builder(Duration::from_millis(100))
        .unbounded()
        .build_on(Some(&vsync));
    let c = owner.controller();

    let mut future = c.animate_with(GoesNanMidRun { nan_at: 1.0 }).unwrap();
    // `Vsync` anchors a run's `t = 0` on the FIRST `tick_all` it sees
    // after the run starts (registration alone reads no clock) -- so
    // the first call establishes the anchor at elapsed 0, exactly like
    // a real frame loop's first pumped frame after `animate_with`.
    vsync.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)));
    vsync.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)));
    assert_eq!(
        c.value(),
        0.5,
        "precondition: the run is progressing normally"
    );
    assert!(vsync.has_running());

    vsync.tick_all(&crate::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0))); // sim.x(1.0) is NaN
    assert_eq!(
        c.value(),
        0.5,
        "the run must end AT THE LAST FINITE VALUE, not read the NaN sample through"
    );
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!(
        !vsync.has_running(),
        "the run must not hold the frame loop open forever"
    );

    use std::future::Future;
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    assert_eq!(
        std::pin::Pin::new(&mut future).poll(&mut cx),
        std::task::Poll::Ready(Ok(())),
        "the future must resolve Ok -- the run ended on its own terms, not by cancellation"
    );
    crate::test_cases::dispose_controller(c);
}

// ---- B1c: status listener may re-enter the controller without deadlock ----

fn status_callback_can_reenter_controller_without_deadlock() {
    let c = controller(100);
    let reentered = Arc::new(AtomicUsize::new(0));
    let c2 = c.clone();
    let r2 = Arc::clone(&reentered);
    c.subscribe_status(std::rc::Rc::new(move |status| {
        if status == AnimationStatus::Completed {
            // Re-enter: read + mutate the controller from within the status
            // callback. Under the old notify-under-lock code this deadlocked.
            let _ = c2.value();
            let _ = c2.reverse();
            r2.fetch_add(1, Ordering::SeqCst);
        }
    }))
    .detach();
    c.forward().unwrap();
    c.tick_at(std::time::Duration::from_secs_f64(0.10)); // complete -> fires Completed -> callback re-enters
    assert_eq!(reentered.load(Ordering::SeqCst), 1);
    crate::test_cases::dispose_controller(&c);
}

// ---- repeat with a finite count stops + completes ----

fn repeat_consumes_all_cycles_in_one_long_frame() {
    let c = controller(100);
    // count = 4, period = 10ms. A single 45ms frame (a dropped-frame
    // catch-up) spans 4 whole cycles, so the repeat must already be
    // exhausted — not still Forward as the old one-cycle-per-tick path left
    // it after the first boundary.
    c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(4))
        .unwrap();
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.tick_at(std::time::Duration::from_secs_f64(0.045)); // 4.5 cycles elapsed in one frame
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "all four cycles retired in one long frame -> exhausted"
    );
    crate::test_cases::dispose_controller(&c);
}

/// The `min == max` equality case. A degenerate `min == max` range is
/// rejected; the mapping entry gives the rationale.
fn repeat_with_rejects_equal_min_and_max() {
    let c = controller(100);
    let r = c.repeat_with(Some(0.5), Some(0.5), false, None, None);
    assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    crate::test_cases::dispose_controller(&c);
}

// ---- repeat sampling is a pure function of elapsed time (#1078) ----

/// The issue's own reproduction: a repeating run's value/status at any
/// `tick_at(t)` depends only on the elapsed time since the run started,
/// never on how many intervening ticks partitioned the way there —
/// `tick_at(1.25)` must equal `tick_at(1.0); tick_at(1.25)`, for both
/// restart and bounce.
fn repeat_value_is_partition_invariant_across_a_skipped_cycle() {
    // Restart mode: skip cycle 0's boundary tick entirely.
    let direct = AnimationController::builder(Duration::from_secs(1)).build();
    direct
        .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    direct.tick_at(std::time::Duration::from_secs_f64(1.25));
    let partitioned = AnimationController::builder(Duration::from_secs(1)).build();
    partitioned
        .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    partitioned.tick_at(std::time::Duration::from_secs_f64(1.0));
    partitioned.tick_at(std::time::Duration::from_secs_f64(1.25));
    assert!(
        (direct.value() - 0.25).abs() < 1e-6,
        "value={}",
        direct.value()
    );
    assert_eq!(direct.value(), partitioned.value());
    assert_eq!(direct.status(), partitioned.status());
    crate::test_cases::dispose_controller(&direct);
    crate::test_cases::dispose_controller(&partitioned);

    // Bounce mode: same elapsed time, opposite leg (cycle index 1 is
    // the reverse leg).
    let direct = AnimationController::builder(Duration::from_secs(1)).build();
    direct
        .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    direct.tick_at(std::time::Duration::from_secs_f64(1.25));
    let partitioned = AnimationController::builder(Duration::from_secs(1)).build();
    partitioned
        .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    partitioned.tick_at(std::time::Duration::from_secs_f64(1.0));
    partitioned.tick_at(std::time::Duration::from_secs_f64(1.25));
    assert!(
        (direct.value() - 0.75).abs() < 1e-6,
        "value={}",
        direct.value()
    );
    assert_eq!(direct.value(), partitioned.value());
    assert_eq!(direct.status(), partitioned.status());
    crate::test_cases::dispose_controller(&direct);
    crate::test_cases::dispose_controller(&partitioned);
}

/// A finite-count bouncing repeat, ticked with ABSOLUTE frame timestamps:
/// A stale absolute timestamp holds the displayed sample. A later monotonic
/// timestamp resumes the repeat; finite counts still exhaust at the endpoint.
fn repeat_bounce_finite_count_and_absolute_time_rewind() {
    let c = controller(100);
    c.repeat_with(None, None, true, None, Some(4)).unwrap();
    c.tick_at(std::time::Duration::from_secs_f64(0.025));
    assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
    c.tick_at(std::time::Duration::from_secs_f64(0.05));
    assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
    c.tick_at(std::time::Duration::from_secs_f64(0.099));
    assert!((c.value() - 0.99).abs() < 1e-3, "value={}", c.value());
    c.tick_at(std::time::Duration::from_secs_f64(0.10));
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    // A stale sample cannot rewind the run.
    c.tick_at(std::time::Duration::from_secs_f64(0.06));
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    crate::test_cases::dispose_controller(&c);

    // The non-rewound interpretation, for contrast: elapsed 160ms lands
    // on the reverse leg's 0.4, not the forward leg's 0.6.
    let c2 = controller(100);
    c2.repeat_with(None, None, true, None, Some(4)).unwrap();
    c2.tick_at(std::time::Duration::from_secs_f64(0.16));
    assert!((c2.value() - 0.4).abs() < 1e-6, "value={}", c2.value());
    crate::test_cases::dispose_controller(&c2);

    // Exhaustion at exactly 400ms lands on the 4th (odd-indexed)
    // cycle's reverse-leg endpoint.
    let c3 = controller(100);
    c3.repeat_with(None, None, true, None, Some(4)).unwrap();
    c3.tick_at(std::time::Duration::from_secs_f64(0.4));
    assert!((c3.value() - 0.0).abs() < 1e-6, "value={}", c3.value());
    assert_eq!(c3.status(), AnimationStatus::Dismissed);
    crate::test_cases::dispose_controller(&c3);
}

/// `Duration::try_from_secs_f64` returning `Err` (an out-of-range
/// `cycle` after converting `Duration::MAX`) must saturate to a very large elapsed
/// time, never to zero — a zero-rewind would let a pathological input
/// never exhaust a finite repeat while `forward()` on the same input
/// completes normally. Red-check: `.map_or(0, |d| d.as_nanos())`
/// instead of `.unwrap_or(Duration::MAX)` — this repeat never exhausts.
fn repeat_tick_at_infinity_exhausts_a_finite_count_instead_of_rewinding() {
    let c = controller(100);
    c.repeat_with(None, None, false, Some(Duration::from_millis(100)), Some(2))
        .unwrap();
    c.tick_at(Duration::MAX);
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    crate::test_cases::dispose_controller(&c);
}

/// A zero effective period settles SYNCHRONOUSLY at the call — Android's
/// rule ("0 duration animator, ignore the repeat count and skip to the
/// end"); Compose rejects it. A later `tick_at`
/// changes nothing (no run was ever installed), and `run_generation` is
/// untouched.
fn repeat_with_zero_period_settles_synchronously_at_the_call() {
    // Finite count: lands on the count-th cycle's end (count=3, bounce,
    // from 0: cycle index 2 is even -> Forward -> max).
    let c = controller(100);
    let generation_before = c.run_generation();
    let future = c
        .repeat_with(None, None, true, Some(Duration::ZERO), Some(3))
        .unwrap();
    assert!(future.is_complete());
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert_eq!(c.run_generation(), generation_before);
    c.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert!(
        (c.value() - 1.0).abs() < 1e-6,
        "a later tick must change nothing"
    );
    crate::test_cases::dispose_controller(&c);

    // Infinite count: lands on cycle 0's end (Android's skip-to-end) —
    // a documented exception to "an infinite repeat's future resolves
    // only by cancellation".
    let c2 = controller(100);
    let future2 = c2
        .repeat_with(None, None, false, Some(Duration::ZERO), None)
        .unwrap();
    assert!(future2.is_complete());
    assert!((c2.value() - 1.0).abs() < 1e-6, "value={}", c2.value());
    assert_eq!(c2.status(), AnimationStatus::Completed);
    crate::test_cases::dispose_controller(&c2);
}

/// `velocity()` on a reverse leg is SIGNED (negative while the
/// value falls) — see `docs/ARCHITECTURE.md`'s "Repeat sampling" mapping entry, (g).
fn repeat_reverse_leg_velocity_is_negative() {
    let c = controller(100);
    c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    c.tick_at(std::time::Duration::from_secs_f64(1.5)); // cycle index 1 (odd) -> reverse leg, mid-cycle
    assert!(
        (c.value() - 0.5).abs() < 1e-6,
        "sanity: reverse leg mid-cycle value={}",
        c.value()
    );
    assert!(
        (c.velocity() - (-1.0)).abs() < 1e-6,
        "a reverse leg's velocity must be negative: {}",
        c.velocity()
    );
    crate::test_cases::dispose_controller(&c);
}

// ---- animate_to_curved / animate_back_curved thread a curve through the run ----

fn animate_to_curved_eases_through_the_given_curve() {
    use crate::curve::Curves;
    let c = controller(100);
    c.animate_to_curved(
        1.0,
        Some(Duration::from_millis(100)),
        Arc::new(Curves::EaseInQuint),
    )
    .unwrap();
    c.tick_at(std::time::Duration::from_secs_f64(0.05)); // t=0.5 raw
    let expected = Curves::EaseInQuint.transform(0.5);
    assert!(
        (c.value() - expected).abs() < 1e-3,
        "expected the curve applied at t=0.5: got {}, want {expected}",
        c.value()
    );
    c.tick_at(std::time::Duration::from_secs_f64(0.10));
    assert_eq!(
        c.value(),
        1.0,
        "the curve must land exactly on the target at t=1.0"
    );
    assert_eq!(c.status(), AnimationStatus::Completed);
    crate::test_cases::dispose_controller(&c);
}

// ---- controller-owned run futures (issue #1161 / ADR-0064) ----

/// Order pin for a zero-duration displacement: the new run's status must
/// still be observed BEFORE the displaced run's cancellation, exactly
/// like a real-duration displacement.
fn a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable() {
    let c = AnimationController::builder(Duration::from_millis(100)).build();
    let first = c.forward().unwrap(); // a real, still-pending run

    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let order_for_status = Arc::clone(&order);
    c.subscribe_status(std::rc::Rc::new(move |_status| {
        order_for_status.lock().push("new_run_status");
    }))
    .detach();
    let order_for_cancel = Arc::clone(&order);
    first.when_complete_or_cancel(move |_outcome| {
        order_for_cancel.lock().push("displaced_run_canceled");
    });

    // A per-run zero-duration override: this settles synchronously and
    // displaces `first`, which never ticked (still at the lower bound).
    c.animate_to(1.0, Some(Duration::ZERO)).unwrap();

    assert_eq!(
        order.lock().as_slice(),
        &["new_run_status", "displaced_run_canceled"],
        "even a synchronously-settling run must publish its own status \
         before the run it displaced observes its cancellation"
    );
    crate::test_cases::dispose_controller(&c);
}

/// A trivially-finished [`Simulation`] test double: `is_done` is true
/// from the first tick, so `tick_simulation`'s completion branch runs
/// immediately without needing a real spring to settle.
struct InstantSimulation {
    value: f64,
}

impl Simulation for InstantSimulation {
    fn x(&self, _time: f64) -> f64 {
        self.value
    }
    fn dx(&self, _time: f64) -> f64 {
        0.0
    }
    fn is_done(&self, _time: f64) -> bool {
        true
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}

fn simulation_run_future_resolves_ok_when_the_simulation_finishes() {
    let c = AnimationController::builder(Duration::from_millis(100)).build();
    let future = c.animate_with(InstantSimulation { value: 0.5 }).unwrap();
    assert!(future.is_pending());

    c.tick_at(std::time::Duration::from_secs_f64(0.0));

    assert!(
        future.is_complete(),
        "tick_simulation's is_done branch must complete the run's future"
    );
    crate::test_cases::dispose_controller(&c);
}

fn stop_cancels_the_active_run() {
    let c = AnimationController::builder(Duration::from_millis(100)).build();
    let future = c.forward().unwrap();
    c.stop().unwrap();
    assert!(future.is_canceled(), "stop() must cancel the run in flight");
    crate::test_cases::dispose_controller(&c);
}

fn every_delivery_runs_with_the_controller_lock_free() {
    let c = AnimationController::builder(Duration::from_millis(100)).build();
    let inner = Rc::clone(&c.inner);
    let future = c.forward().unwrap();

    let observed = Arc::new(Mutex::new(None));
    let observed2 = Arc::clone(&observed);
    future.when_complete_or_cancel(move |_outcome| {
        *observed2.lock() = Some(inner.try_borrow_mut().is_ok());
    });

    c.tick_at(std::time::Duration::from_secs_f64(0.1));

    assert_eq!(
        observed.lock().as_ref(),
        Some(&true),
        "a delivery must run with the controller's own lock free — the \
         finish chokepoint drops it before calling deliver()"
    );
    crate::test_cases::dispose_controller(&c);
}

fn a_panicking_status_listener_leaves_the_finished_run_ok() {
    let c = AnimationController::builder(Duration::from_millis(100)).build();
    let future = c.forward().unwrap();

    // Registered on the FUTURE, not the controller: this only runs if
    // `RunDelivery` actually delivers. `future.is_complete()` alone
    // observes publication, so it cannot prove that notification survived
    // a status-listener failure. The continuation checks actual delivery.
    let seen = Arc::new(Mutex::new(None));
    let seen2 = Arc::clone(&seen);
    future.when_complete_or_cancel(move |outcome| {
        *seen2.lock() = Some(outcome);
    });

    c.subscribe_status(std::rc::Rc::new(|status| {
        assert!(
            status != AnimationStatus::Completed,
            "a status listener panics on completion"
        );
    }))
    .detach();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.tick_at(std::time::Duration::from_secs_f64(0.1));
    }));

    assert!(
        result.is_err(),
        "the listener's panic must propagate out of tick_at"
    );
    assert_eq!(
        *seen.lock(),
        Some(Ok(())),
        "RunDelivery must still run the continuation after listener failure, \
         running the continuation with the outcome published before \
         the panicking listener ran"
    );
    assert!(future.is_complete());
    crate::test_cases::dispose_controller(&c);
}

#[test]
fn controller_contract() {
    crate::test_cases::run_cases(&[
        (
            "forward from mid scales run duration",
            forward_from_mid_scales_run_duration,
        ),
        (
            "set value nan is canonicalized",
            set_value_nan_is_canonicalized,
        ),
        (
            "without ticker bounds rejects wide open ones",
            without_ticker_bounds_rejects_wide_open_ones,
        ),
        (
            "disposed controller rejects forward",
            disposed_controller_rejects_forward,
        ),
        (
            "bounded controller clamps infinite target and from to the pointed at bound",
            bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound,
        ),
        (
            "animate to curved eases through the given curve",
            animate_to_curved_eases_through_the_given_curve,
        ),
        ("stop cancels the active run", stop_cancels_the_active_run),
        (
            "simulation run future resolves ok when the simulation finishes",
            simulation_run_future_resolves_ok_when_the_simulation_finishes,
        ),
    ]);
}

#[test]
fn repeat_contract() {
    crate::test_cases::run_cases(&[
        (
            "repeat consumes all cycles in one long frame",
            repeat_consumes_all_cycles_in_one_long_frame,
        ),
        (
            "repeat with rejects equal min and max",
            repeat_with_rejects_equal_min_and_max,
        ),
        (
            "repeat value is partition invariant across a skipped cycle",
            repeat_value_is_partition_invariant_across_a_skipped_cycle,
        ),
        (
            "repeat bounce finite count and absolute time rewind",
            repeat_bounce_finite_count_and_absolute_time_rewind,
        ),
        (
            "repeat tick at infinity exhausts a finite count instead of rewinding",
            repeat_tick_at_infinity_exhausts_a_finite_count_instead_of_rewinding,
        ),
        (
            "repeat with zero period settles synchronously at the call",
            repeat_with_zero_period_settles_synchronously_at_the_call,
        ),
        (
            "repeat reverse leg velocity is negative",
            repeat_reverse_leg_velocity_is_negative,
        ),
    ]);
}

#[test]
fn failure_modes_are_contained() {
    crate::test_cases::run_cases(&[
        (
            "exhausted run identities refuse without displacing the last run",
            exhausted_run_identities_refuse_without_displacing_the_last_run,
        ),
        (
            "exhausted sample identities cancel without reissuing a stale sample",
            exhausted_sample_identities_cancel_without_reissuing_a_stale_sample,
        ),
        (
            "fling refuses a non finite velocity",
            fling_refuses_a_non_finite_velocity,
        ),
        (
            "a simulation that turns non finite mid run ends the run at the last finite value",
            a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value,
        ),
        (
            "a panicking status listener leaves the finished run ok",
            a_panicking_status_listener_leaves_the_finished_run_ok,
        ),
    ]);
}

#[test]
fn reentrancy_and_run_ordering() {
    crate::test_cases::run_cases(&[
        (
            "status callback can reenter controller without deadlock",
            status_callback_can_reenter_controller_without_deadlock,
        ),
        (
            "a zero duration run cancels the displaced run after its own status is observable",
            a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable,
        ),
        (
            "every delivery runs with the controller lock free",
            every_delivery_runs_with_the_controller_lock_free,
        ),
    ]);
}
