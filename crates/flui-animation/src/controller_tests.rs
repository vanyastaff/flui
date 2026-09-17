//! Test module for `AnimationController` (split out of `controller.rs` via
//! `#[path = "controller_tests.rs"] mod tests;` -- kept as a sibling file
//! because this crate's test module is itself larger than most production
//! files in the workspace; splitting it stops it from dominating
//! `controller.rs`'s own line count and lets an editor/reviewer open
//! "the tests" and "the implementation" as two separate, right-sized files.
//! `ticker_completer_resolution_never_bypasses_the_finish_chokepoint`'s own
//! `include_str!("controller.rs")` still reads the (now test-free)
//! production file directly, so its production-vs-test-code split logic is
//! no longer needed there.

use super::*;
use flui_scheduler::UpdateScheduler;
use std::sync::atomic::{AtomicUsize, Ordering};

// Several tests assert exact per-tick progress, and `time_dilation_scales_progress`
// mutates the *global* `time_dilation`. Serialize all controller tests so the
// dilation mutation can never corrupt a sibling's progress assertions under a
// parallel `cargo test` run.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> parking_lot::MutexGuard<'static, ()> {
    SERIAL.lock()
}

fn controller(ms: u64) -> AnimationController {
    let scheduler = UpdateScheduler::new();
    AnimationController::new(Duration::from_millis(ms), &scheduler)
}

/// Migration-premise pin: every production
/// `AnimationController` today is built with a private, never-pumped
/// `Arc::new(UpdateScheduler::new())` — the "wall-clock fallback" comments in
/// `flui-widgets` (`scrollable.rs`, `navigator/binding.rs`) claim the
/// ticker on that private scheduler fires on its own. It cannot: nothing
/// ever calls `handle_begin_frame`/`execute_frame` on a scheduler no one
/// else holds, so the ticker's transient callback registers and then
/// simply never runs.
///
/// This test proves that premise BEFORE `without_ticker` (a controller
/// with no scheduler at all) replaces those throwaway-scheduler call
/// sites: it drives a *different*, actually-pumped scheduler for many
/// frames and asserts the controller's own private scheduler never once
/// produced a tick. If this assertion ever fails, some path pumps a
/// controller's private scheduler after all, `without_ticker` would be a
/// real behavior change, and the widget migration must stop and be
/// re-examined — not proceed on this premise.
#[test]
fn a_controllers_private_unpumped_scheduler_never_advances_without_vsync() {
    let _serial = serial();
    let private_scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::from_millis(100), &private_scheduler);
    c.forward().unwrap();

    // Drive an UNRELATED, actually-pumped scheduler for many frames —
    // this is what a real event loop's realm-owned `UpdateScheduler` does
    // every frame. It must have zero effect on `c`, which is wired to
    // `private_scheduler` alone.
    let other_scheduler = UpdateScheduler::new();
    for _ in 0..120 {
        other_scheduler.execute_frame();
    }
    std::thread::sleep(Duration::from_millis(20));

    assert_eq!(
        c.value(),
        0.0,
        "a controller wired to a private, never-pumped UpdateScheduler must not \
         advance no matter how many frames an unrelated scheduler runs — \
         the ticker's callback is registered on `private_scheduler`'s own \
         transient queue, which nothing ever drains"
    );
    assert_eq!(
        private_scheduler.frame_count(),
        0,
        "precondition: the controller's own private scheduler was never pumped"
    );
    c.dispose();
}

#[test]
fn creation_starts_dismissed_at_lower_bound() {
    let _serial = serial();
    let c = controller(100);
    assert_eq!(c.value(), 0.0);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

// ---- remaining-fraction duration scaling (Flutter `_animateToInternal`) ----

#[test]
fn reverse_mid_flight_keeps_full_range_velocity() {
    let _serial = serial();
    let c = controller(1000);
    c.forward().unwrap();
    c.tick_at(0.6);
    assert!((c.value() - 0.6).abs() < 1e-4);

    // reverse() restarts the ticker (elapsed re-zeroes) and the leg
    // covers 0.6 of the range in 0.6s — NOT the full second. Pre-fix
    // the lerp ran start->target over the full duration, so the same
    // distance took longer and velocity dropped at the turn.
    c.reverse().unwrap();
    c.tick_at(0.3);
    assert!(
        (c.value() - 0.3).abs() < 1e-3,
        "0.3s into the 0.6s reverse leg must sit at 0.3, got {}",
        c.value()
    );
    // Past the scaled leg's end (0.6s + float headroom) → dismissed.
    c.tick_at(0.7);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

#[test]
fn forward_from_mid_scales_run_duration() {
    let _serial = serial();
    let c = controller(100);
    c.forward_from(Some(0.5)).unwrap();
    // Half the range remains -> 50ms run. 25ms in = halfway -> 0.75.
    c.tick_at(0.025);
    assert!(
        (c.value() - 0.75).abs() < 1e-3,
        "constant velocity from 0.5: got {}",
        c.value()
    );
    c.tick_at(0.05);
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.dispose();
}

#[test]
fn forward_at_upper_bound_settles_immediately() {
    let _serial = serial();
    let c = controller(100);
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let s2 = Arc::clone(&statuses);
    let _id = c.add_status_listener(Arc::new(move |s| s2.lock().push(s)));

    c.forward_from(Some(1.0)).unwrap();
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!(!c.is_animating(), "no ticker run for a zero-distance leg");
    assert_eq!(
        statuses.lock().as_slice(),
        &[AnimationStatus::Completed],
        "settles with the final status only — no transient Forward",
    );
    c.dispose();
}

#[test]
fn reverse_at_lower_bound_settles_immediately() {
    let _serial = serial();
    let c = controller(100);
    c.reverse().unwrap();
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    assert!(!c.is_animating());
    c.dispose();
}

#[test]
fn explicit_animate_to_duration_is_not_scaled() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);
    // Explicit 100ms run from 0.5 -> 1.0: 50ms in = halfway -> 0.75.
    c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
    c.tick_at(0.05);
    assert!(
        (c.value() - 0.75).abs() < 1e-3,
        "an explicit per-run duration is used as-is, got {}",
        c.value()
    );
    c.dispose();
}

#[test]
fn forward_sets_running_status() {
    let _serial = serial();
    let c = controller(100);
    c.forward().unwrap();
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.dispose();
}

#[test]
fn is_animating_is_ticker_based_not_status_based() {
    let _serial = serial();
    let c = controller(100);
    // Flutter `_internalSetValue` parity: an interior set_value reports a
    // directional status, but a stopped controller must not claim to be
    // animating (Flutter's isAnimating is ticker-based).
    c.set_value(0.5);
    assert_eq!(c.status(), AnimationStatus::Forward);
    assert!(!c.is_animating(), "stopped controller must not animate");

    c.forward().unwrap();
    assert!(c.is_animating(), "running controller must animate");

    c.stop().unwrap();
    assert!(!c.is_animating(), "stop() must end animating");
    c.dispose();
}

#[test]
fn set_value_nan_is_canonicalized() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);
    c.set_value(f32::NAN);
    assert_eq!(
        c.value(),
        0.0,
        "NaN must canonicalize to the lower bound, not poison the value"
    );
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

#[test]
fn reset_returns_to_lower_bound() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);
    assert_eq!(c.value(), 0.5);
    c.reset().unwrap();
    assert_eq!(c.value(), 0.0);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

#[test]
fn custom_bounds_clamp() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::with_bounds(Duration::from_millis(100), &scheduler, 10.0, 20.0)
        .unwrap();
    assert_eq!(c.value(), 10.0);
    c.set_value(15.0);
    assert_eq!(c.value(), 15.0);
    c.set_value(100.0);
    assert_eq!(c.value(), 20.0);
    c.dispose();
}

#[test]
fn invalid_bounds_rejected() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let r = AnimationController::with_bounds(Duration::from_millis(100), &scheduler, 20.0, 10.0);
    assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
}

/// `without_ticker` builds a controller with no scheduler attached at
/// all — it must still advance via `tick_at` (the widget-layer `Vsync`
/// driving path), exactly like a controller built against a private,
/// never-pumped `UpdateScheduler`.
#[test]
fn without_ticker_advances_via_tick_at_only() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    assert_eq!(c.value(), 0.0);

    c.forward().unwrap();
    c.tick_at(0.05);
    assert!((c.value() - 0.5).abs() < 1e-4);

    c.dispose();
}

/// `without_ticker_bounds` is a BOUNDED constructor: bounded means
/// finite (#1183). This test used to assert the OPPOSITE — that a
/// wide-open `(NEG_INFINITY, INFINITY)` pair was ACCEPTED, landing
/// `value() == NEG_INFINITY` — that assertion is deliberately inverted
/// here, not preserved: unboundedness is now a constructor fact
/// ([`AnimationController::unbounded_without_ticker`]), not a bound
/// value, so the same wide-open pair this test used to accept must now
/// be rejected, and the unbounded shape starts at `0.0`, never `-inf`.
#[test]
fn without_ticker_bounds_rejects_wide_open_ones() {
    let _serial = serial();
    let rejected = AnimationController::without_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
    assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

    let wide_open = AnimationController::without_ticker_bounds(
        Duration::from_millis(1),
        f32::NEG_INFINITY,
        f32::INFINITY,
    );
    assert!(
        matches!(wide_open, Err(AnimationError::InvalidBounds(_))),
        "a wide-open pair is unboundedness spelled as bounds -- reject it; \
         use unbounded_without_ticker instead"
    );

    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    assert_eq!(
        c.value(),
        0.0,
        "unbounded_without_ticker starts at 0.0, never -inf"
    );
    c.dispose();
}

/// `with_detached_ticker` is the shape the throwaway-`UpdateScheduler` sites
/// migrated to: unlike `without_ticker`, this controller has a REAL
/// `Ticker`, so `is_animating()` reports `true` mid-run exactly as it
/// would with a live (but never-pumped) scheduler attached — while still
/// advancing only via `tick_at`, never on its own, because the ticker's
/// scheduler is `None`.
#[test]
fn with_detached_ticker_reports_animating_and_advances_via_tick_at_only() {
    let _serial = serial();
    let c = AnimationController::with_detached_ticker(Duration::from_millis(100));
    assert_eq!(c.value(), 0.0);
    assert!(
        !c.is_animating(),
        "a freshly built, un-started controller must not report animating"
    );

    c.forward().unwrap();
    assert!(
        c.is_animating(),
        "a detached ticker must still report is_animating() == true once started -- \
         this is exactly the behavior without_ticker cannot provide"
    );
    c.tick_at(0.05);
    assert!((c.value() - 0.5).abs() < 1e-4);

    c.stop().unwrap();
    assert!(
        !c.is_animating(),
        "stop() must end animating for a detached ticker just as it does for a scheduled one"
    );
    c.dispose();
}

/// `with_detached_ticker_bounds` is the same shape with custom bounds --
/// mirrors `without_ticker_bounds`'s coverage of bounds validation. See
/// that test's own doc for why the wide-open-pair assertion below is a
/// deliberate inversion of what this test used to check, not a
/// preserved pin.
#[test]
fn with_detached_ticker_bounds_rejects_wide_open_ones() {
    let _serial = serial();
    let rejected =
        AnimationController::with_detached_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
    assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

    let wide_open = AnimationController::with_detached_ticker_bounds(
        Duration::from_millis(1),
        f32::NEG_INFINITY,
        f32::INFINITY,
    );
    assert!(
        matches!(wide_open, Err(AnimationError::InvalidBounds(_))),
        "a wide-open pair is unboundedness spelled as bounds -- reject it; \
         use unbounded_with_detached_ticker instead"
    );

    let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1));
    assert_eq!(
        c.value(),
        0.0,
        "unbounded_with_detached_ticker starts at 0.0, never -inf"
    );
    c.dispose();
}

#[test]
fn disposed_controller_rejects_forward() {
    let _serial = serial();
    let c = controller(100);
    c.dispose();
    assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
}

// ---- #1183: unboundedness is a constructor fact ----------------------

/// Bounded means finite: every bounded constructor (and `builder.rs`'s
/// `.bounds()`, tested separately in that module) must reject any bound
/// that is `NaN`, infinite, or a half-open pair (one finite, one
/// infinite) -- unboundedness is `unbounded*`, not a bound value.
#[test]
fn bounds_constructors_reject_non_finite_bounds() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let cases: &[(f32, f32)] = &[
        (f32::NAN, 1.0),
        (0.0, f32::NAN),
        (f32::NEG_INFINITY, f32::INFINITY),
        (f32::NEG_INFINITY, 5.0),
        (5.0, f32::INFINITY),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
        (-f32::MAX, f32::MAX),
    ];
    for &(lower, upper) in cases {
        assert!(
            matches!(
                AnimationController::with_bounds(
                    Duration::from_millis(1),
                    &scheduler,
                    lower,
                    upper
                ),
                Err(AnimationError::InvalidBounds(_))
            ),
            "with_bounds({lower}, {upper}) must be rejected"
        );
        assert!(
            matches!(
                AnimationController::without_ticker_bounds(Duration::from_millis(1), lower, upper),
                Err(AnimationError::InvalidBounds(_))
            ),
            "without_ticker_bounds({lower}, {upper}) must be rejected"
        );
        assert!(
            matches!(
                AnimationController::with_detached_ticker_bounds(
                    Duration::from_millis(1),
                    lower,
                    upper
                ),
                Err(AnimationError::InvalidBounds(_))
            ),
            "with_detached_ticker_bounds({lower}, {upper}) must be rejected"
        );
    }
}

/// Pin: a finite, merely non-default range must still be accepted and
/// start at its own `lower_bound` -- the tightened validation rejects
/// non-finite bounds, not merely-unusual finite ones.
#[test]
fn bounds_constructors_still_accept_a_finite_non_default_range() {
    let _serial = serial();
    let c = AnimationController::without_ticker_bounds(Duration::from_millis(1), -1.0, 3.0)
        .expect("finite (-1, 3) satisfies lower < upper");
    assert_eq!(c.value(), -1.0);
    c.dispose();
}

/// `unbounded_without_ticker`'s documented contract: `value() == 0.0`,
/// initial status `Forward` (Flutter's `_internalSetValue` rule at
/// `0.0`, direction defaulted `Forward` -- see the mapping entry for the
/// recorded cost), zero velocity, and not animating until driven.
#[test]
fn unbounded_without_ticker_starts_at_zero_forward_and_idle() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    assert_eq!(c.value(), 0.0);
    assert_eq!(c.status(), AnimationStatus::Forward);
    assert_eq!(c.velocity(), 0.0);
    assert!(!c.is_animating());
    c.dispose();
}

/// `set_value` at a non-bound value on an unbounded controller keeps the
/// keep-direction status (`Forward`, unchanged from construction) and
/// must fire no status listener -- status equality alone is vacuous
/// under `take_status_change`'s dedup, so this counts callbacks (v3
/// delta 5).
#[test]
fn unbounded_set_value_at_a_non_bound_value_fires_no_status_change() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    let status_changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&status_changes);
    let _id = c.add_status_listener(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    c.set_value(123.0);
    assert_eq!(c.status(), AnimationStatus::Forward);
    assert_eq!(
        status_changes.load(Ordering::SeqCst),
        0,
        "status was already Forward at construction (last_reported_status is initialized \
         from the SAME value, not hard-coded Dismissed); set_value at a non-bound value \
         must not re-fire it"
    );
    c.dispose();
}

/// `set_value` with a non-finite input on an UNBOUNDED controller is a
/// FULL no-op -- not merely "unchanged value" but no `stop_running` and
/// no notification either: a poisoned drag's `set_value(NaN)` must not
/// cancel a live fling out from under it. Pinned alongside the BOUNDED
/// canonicalization (`NaN` -> lower bound, `+inf` -> upper bound) it
/// does not disturb, and the shared latch (`non_finite_warned`) firing
/// once across three non-finite calls.
#[test]
fn set_value_non_finite_is_a_full_no_op_on_unbounded_but_canonicalizes_on_bounded() {
    let _serial = serial();

    // Bounded: pin -- `+inf` clamps to the upper bound (mirrors the
    // existing `set_value_nan_is_canonicalized` pin for `NaN`).
    let c = controller(100);
    c.set_value(f32::INFINITY);
    assert_eq!(
        c.value(),
        1.0,
        "set_value(+inf) on a bounded controller clamps to upper"
    );
    c.dispose();

    // Unbounded: a live run must survive set_value(NaN) untouched --
    // the run is still installed and still ticking afterward.
    let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));
    c.animate_to(500.0, None).unwrap();
    assert!(c.is_animating(), "precondition: a live run is installed");
    let generation_before = c.run_generation();

    c.set_value(f32::NAN);
    assert_eq!(
        c.value(),
        0.0,
        "set_value(NaN) on an unbounded controller must not move value"
    );
    assert!(
        c.is_animating(),
        "set_value(NaN) on an unbounded controller must not stop the live run"
    );
    assert_eq!(c.run_generation(), generation_before);

    c.set_value(f32::INFINITY);
    assert_eq!(
        c.value(),
        0.0,
        "set_value(+inf) on an unbounded controller must not move value"
    );
    c.set_value(f32::NEG_INFINITY);
    assert_eq!(
        c.value(),
        0.0,
        "set_value(-inf) on an unbounded controller must not move value"
    );
    assert!(
        c.is_animating(),
        "none of the three non-finite calls may have stopped the run"
    );
    c.dispose();
}

/// The "received a non-finite value" latch is set-once: `false` before
/// any non-finite `set_value`, `true` after the FIRST one, and still
/// `true` (not re-armed) after two more. This is the mechanism a
/// hand-rolled `tracing::Subscriber` would otherwise have to prove
/// indirectly by counting emissions; reading the field directly is the
/// cheaper pin for a plain bool.
#[test]
fn non_finite_warn_latch_sets_once_across_three_calls() {
    let _serial = serial();
    let c = controller(100);
    assert!(
        !c.debug_non_finite_warned(),
        "a fresh controller has never seen a non-finite value"
    );

    c.set_value(f32::NAN);
    assert!(
        c.debug_non_finite_warned(),
        "the latch must be set after the first non-finite set_value"
    );

    c.set_value(f32::INFINITY);
    c.set_value(f32::NEG_INFINITY);
    assert!(
        c.debug_non_finite_warned(),
        "the latch must stay set (not toggle) across two further non-finite calls"
    );
    c.dispose();
}

/// Every bound-targeting run start is refused on an unbounded
/// controller (there is no finite bound to run to), and the refusal is
/// a NO-OP in every observable respect: value, status, direction
/// (probed via `stop()`'s reported status against a fresh twin that
/// never received the refused call), `run_generation`, `is_animating`,
/// both listener kinds, a concurrently live run's own future staying
/// pending throughout, AND the live run's per-run mode (curve/duration)
/// staying installed -- probed by ticking the refused controller and
/// its untouched twin by the same elapsed time and comparing values.
/// That last probe is load-bearing, not redundant with the others: a
/// refusal that ran `clear_run_modes()` before returning `Err` (instead
/// of after every check) would still report the same value/status/
/// generation immediately afterward -- `clear_run_modes` touches
/// `run_curve`/`run_duration`/`simulation`/`repeat`, none of which
/// `value`/`status` read synchronously -- and would only diverge from
/// the twin on the NEXT tick, once the live run's now-missing curve
/// changes its interpolation.
#[test]
fn unbounded_refusals_leave_the_controller_completely_untouched() {
    let _serial = serial();

    type Attempt = Box<dyn Fn(&AnimationController) -> Result<TickerFuture, AnimationError>>;
    let attempts: Vec<(&str, Attempt)> = vec![
        ("forward()", Box::new(AnimationController::forward)),
        (
            "forward_from(Some(0.5))",
            Box::new(|c: &AnimationController| c.forward_from(Some(0.5))),
        ),
        ("reverse()", Box::new(AnimationController::reverse)),
        (
            "animate_to(NaN)",
            Box::new(|c: &AnimationController| c.animate_to(f32::NAN, None)),
        ),
        (
            "animate_to(inf)",
            Box::new(|c: &AnimationController| c.animate_to(f32::INFINITY, None)),
        ),
        (
            "animate_back(NaN)",
            Box::new(|c: &AnimationController| c.animate_back(f32::NAN, None)),
        ),
        (
            "fling(1.0)",
            Box::new(|c: &AnimationController| c.fling(1.0)),
        ),
        (
            "fling(-1.0)",
            Box::new(|c: &AnimationController| c.fling(-1.0)),
        ),
        (
            "repeat(false)",
            Box::new(|c: &AnimationController| c.repeat(false)),
        ),
        (
            "repeat_with(None, Some(1.0), ..)",
            Box::new(|c: &AnimationController| c.repeat_with(None, Some(1.0), false, None, None)),
        ),
    ];

    for (name, attempt) in attempts {
        let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));
        let twin = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));

        // The live run carries a per-run CURVE and an explicit per-run
        // DURATION -- both cleared by `clear_run_modes()` -- so a
        // refusal that ran it too early is observable on the next tick,
        // not just immediately. An identical run on both `c`/`twin`: a
        // corrupted `direction` on `c` (the `fling_with` ordering bug
        // this issue also fixed) shows up as a divergent `stop()`
        // status against `twin`, which never sees the refused call.
        let curve = Arc::new(NotIdentityAtEndpoints) as Arc<dyn Curve + Send + Sync>;
        let live = c
            .animate_to_curved(200.0, Some(Duration::from_millis(500)), Arc::clone(&curve))
            .unwrap();
        twin.animate_to_curved(200.0, Some(Duration::from_millis(500)), curve)
            .unwrap();
        assert!(c.is_animating(), "precondition: a live run is installed");

        let value_hits = Arc::new(AtomicUsize::new(0));
        let vh = Arc::clone(&value_hits);
        let _vid = c.add_listener(Arc::new(move || {
            vh.fetch_add(1, Ordering::SeqCst);
        }));
        let status_hits = Arc::new(AtomicUsize::new(0));
        let sh = Arc::clone(&status_hits);
        let _sid = c.add_status_listener(Arc::new(move |_| {
            sh.fetch_add(1, Ordering::SeqCst);
        }));

        let value_before = c.value();
        let status_before = c.status();
        let generation_before = c.run_generation();

        let result = attempt(&c);
        assert!(
            matches!(result, Err(AnimationError::NonFiniteTarget(_))),
            "{name} must be refused with NonFiniteTarget on an unbounded controller, got {result:?}"
        );

        assert_eq!(c.value(), value_before, "{name}: value must be untouched");
        assert_eq!(
            c.status(),
            status_before,
            "{name}: status must be untouched"
        );
        assert_eq!(
            c.run_generation(),
            generation_before,
            "{name}: run_generation must be untouched"
        );
        assert!(
            c.is_animating(),
            "{name}: the live run must still be installed and ticking"
        );
        assert_eq!(
            value_hits.load(Ordering::SeqCst),
            0,
            "{name}: a refused call must fire no value listener"
        );
        assert_eq!(
            status_hits.load(Ordering::SeqCst),
            0,
            "{name}: a refused call must fire no status listener"
        );
        assert!(
            live.is_pending(),
            "{name}: the live animate_to_curved run's future must still be pending"
        );

        // Tick the refused controller and its untouched twin by the
        // SAME elapsed time: if the refusal ran `clear_run_modes()`
        // before its own check (wiping `c`'s curve/duration), `c` would
        // now interpolate LINEARLY over its base 1000ms duration while
        // `twin` still eases over its explicit 500ms -- landing on
        // different values at the same elapsed time.
        c.tick_at(0.25);
        twin.tick_at(0.25);
        assert_eq!(
            c.value(),
            twin.value(),
            "{name}: a refused call must not have cleared the live run's own curve/duration \
             -- the refused controller and its untouched twin must land on the same value \
             after the same elapsed time"
        );

        c.stop().unwrap();
        twin.stop().unwrap();
        assert_eq!(
            c.status(),
            twin.status(),
            "{name}: a refused call must not corrupt direction -- stop() on the refused \
             controller must report the same status a twin that never saw it reports"
        );
    }
}

/// On a BOUNDED controller, a non-finite `target`/`from` is refused too
/// (a declared behavior change: `clamp` alone passes `NaN` straight
/// through unchanged) -- nothing is mutated by the refusal.
#[test]
fn bounded_controller_refuses_non_finite_target_and_from() {
    let _serial = serial();

    let c = controller(100);
    assert!(matches!(
        c.animate_to(f32::NAN, None),
        Err(AnimationError::NonFiniteTarget(_))
    ));
    assert_eq!(
        c.value(),
        0.0,
        "a refused animate_to(NaN) must not move value"
    );

    let c = controller(100);
    assert!(matches!(
        c.forward_from(Some(f32::NAN)),
        Err(AnimationError::NonFiniteTarget(_))
    ));
    assert_eq!(c.value(), 0.0);

    let c = controller(100);
    c.set_value(0.5);
    assert!(matches!(
        c.reverse_from(Some(f32::NAN)),
        Err(AnimationError::NonFiniteTarget(_))
    ));
    assert_eq!(
        c.value(),
        0.5,
        "a refused reverse_from(NaN) must not move value"
    );
}

/// `+-inf` still CLAMPS on a bounded controller (Flutter's own "go to
/// the end" idiom) -- only a bound-less direction refuses.
#[test]
fn bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound() {
    let _serial = serial();

    // `target` clamps to the finite upper bound and the run proceeds
    // normally from there (clamping the target does not mean an
    // instant settle: `value` still starts at the entry value and
    // reaches `1.0` only once the run completes).
    let c = controller(100);
    c.animate_to(f32::INFINITY, None).unwrap();
    c.tick_at(0.1);
    assert_eq!(
        c.value(),
        1.0,
        "animate_to(+inf) clamps to the finite upper bound"
    );
    c.dispose();

    // `from` clamping to the upper bound makes `forward_from`'s own
    // target (also `upper_bound`) already reached -- this settles
    // SYNCHRONOUSLY (zero distance), unlike the `animate_to` case above.
    let c = controller(100);
    c.forward_from(Some(f32::INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        1.0,
        "forward_from(Some(+inf)) clamps to the finite upper bound"
    );
    c.dispose();

    // Mirror case: `reverse_from`'s `from` clamps to the LOWER bound,
    // which is also `reverse`'s own target -- settles SYNCHRONOUSLY at
    // the lower bound, the reverse-direction twin of the case above.
    let c = controller(100);
    c.reverse_from(Some(f32::NEG_INFINITY)).unwrap();
    assert_eq!(
        c.value(),
        0.0,
        "reverse_from(Some(-inf)) clamps to the finite lower bound"
    );
    c.dispose();
}

/// On unbounded: `animate_to`/`animate_with`/`repeat_with` with an
/// explicit finite range all still run (pin) -- the refusal is about
/// bound-*targeting*, not about the controller being unbounded per se.
#[test]
fn unbounded_controller_runs_finite_targeted_operations() {
    let _serial = serial();

    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    c.animate_to(200.0, None).unwrap();
    c.tick_at(0.1);
    assert_eq!(
        c.value(),
        200.0,
        "animate_to still runs on an unbounded controller"
    );
    c.dispose();

    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 1.0);
    let sim = SpringSimulation::new(spring, 0.0, 50.0, 10.0);
    c.animate_with(sim).unwrap();
    assert!(
        c.value().is_finite(),
        "animate_with still runs on an unbounded controller"
    );
    c.dispose();

    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    c.repeat_with(Some(0.0), Some(1.0), false, None, None)
        .expect("an explicit finite range must run on an unbounded controller");
    c.dispose();
}

/// `animate_to` with an INFINITE `target` on an unbounded controller
/// must be refused exactly like a `NaN` one (both clamp to a
/// non-finite result, since neither bound is finite). Unlike
/// `forward`/`reverse` (blanket-refused on unbounded regardless of
/// their argument -- see the battery test above), `animate_to` is only
/// refused when `target` ITSELF is non-finite, so this is the
/// distinguishing case for that half of the rule.
#[test]
fn unbounded_controller_refuses_an_infinite_animate_to_target_too() {
    let _serial = serial();

    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    assert!(matches!(
        c.animate_to(f32::INFINITY, None),
        Err(AnimationError::NonFiniteTarget(_))
    ));
    assert_eq!(c.value(), 0.0, "a refused animate_to must not move value");
}

/// `reset()` on an unbounded controller lands on `0.0` (flutter#76014's
/// requested defined beginning), never `-inf` -- and never fails for
/// non-finiteness.
#[test]
fn unbounded_reset_lands_on_zero_not_negative_infinity() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    c.set_value(500.0);
    c.reset().unwrap();
    assert_eq!(c.value(), 0.0);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

/// The FIRST `stop()` on a fresh, never-run unbounded controller reports
/// `Completed` -- direction defaults `Forward`, and
/// `settled_status_directed` falls to the direction-only branch because
/// neither infinite bound is ever "at". Every `jump_to` on a fresh
/// `Scrollable` triggers exactly this event through the stop hook. A
/// COUNTING status listener (not a bare `status()` read) proves this is
/// a real emitted transition, not two equal reads either side of a
/// no-op: `status()` alone cannot tell "started Forward, `stop()`
/// notified nothing because a mutant no-ops it" apart from "started
/// Forward, `stop()` correctly transitioned to Completed".
#[test]
fn unbounded_first_stop_on_a_fresh_controller_reports_completed() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    let completed_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&completed_count);
    let _id = c.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Completed {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    }));

    c.stop().unwrap();

    assert_eq!(c.status(), AnimationStatus::Completed);
    assert_eq!(
        completed_count.load(Ordering::SeqCst),
        1,
        "stop() must emit exactly one Completed status transition"
    );
    c.dispose();
}

// ---- repeat_with's effective-range check ------------------------------

/// A caller-supplied NaN endpoint is an any-controller range-shape
/// error. The OLD `lo >= hi` check did not catch NaN (`NaN >= hi` is
/// `false`), so `repeat_with(Some(NaN), ..)` reached
/// `inner.value.clamp(lo, hi)` with `lo` itself NaN, which panicked
/// inside `f32::clamp`'s own `assert!(min <= max)`.
#[test]
fn repeat_with_rejects_a_nan_endpoint_on_a_bounded_controller() {
    let _serial = serial();
    let c = controller(100);
    let r = c.repeat_with(Some(f32::NAN), Some(1.0), false, None, None);
    assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    assert_eq!(c.value(), 0.0, "a refused repeat_with must not move value");
}

/// `repeat`/`repeat_with` whose EFFECTIVE range defaults through an
/// unbounded controller's own infinite bound is refused with
/// `NonFiniteTarget`, distinct from `InvalidBounds`'s range-SHAPE errors
/// above -- a partially-specified range (`Some(0.0), None`) hits the
/// same refusal on the still-infinite side.
#[test]
fn repeat_with_refuses_a_non_finite_effective_range_on_unbounded() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
    let r = c.repeat_with(Some(0.0), None, false, None, None);
    assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
    assert_eq!(c.value(), 0.0);
}

// ---- #1183: span overflow -----------------------------------------------

/// A `target - value` span overflowing `f32` is refused at the call
/// rather than let `tick_time_based` interpolate an infinite range:
/// unguarded, `set_value(-f32::MAX)` then `animate_to(f32::MAX)`
/// installs a run whose `range` is `+inf`.
#[test]
fn animate_to_refuses_a_span_that_overflows_f32() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    c.set_value(-f32::MAX);
    let r = c.animate_to(f32::MAX, None);
    assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
    assert_eq!(
        c.value(),
        -f32::MAX,
        "a refused animate_to must not move value"
    );
    c.dispose();
}

// ---- #1183: fling_with's ordering and non-finite refusals ---------------

/// `fling_with` refuses a non-finite `velocity` before ANY mutation:
/// unguarded, `NaN < 0.0` is `false`, so a NaN velocity took the
/// Forward branch and built a spring whose `SpringSimulation` never
/// reaches `is_done` for a NaN target.
#[test]
fn fling_refuses_a_non_finite_velocity() {
    let _serial = serial();
    let c = controller(100);
    let r = c.fling(f32::NAN);
    assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
    assert!(!c.is_animating());
}

/// `fling_with` used to write `inner.direction` BEFORE its
/// `InvalidSpring` check, so a refused fling still corrupted direction,
/// observable only via a LATER `stop()` (an interior value keeps the
/// running status via direction, not a bound). Fixed by computing
/// `direction` into a local and assigning it only after every check
/// passes.
#[test]
fn fling_with_refused_for_invalid_spring_leaves_direction_untouched() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5); // interior value: settled_status_directed falls to direction
    let underdamped = SpringDescription::with_damping_ratio(1.0, 500.0, 0.5);

    let r = c.fling_with(-1.0, Some(underdamped));
    assert!(matches!(r, Err(AnimationError::InvalidSpring(_))));

    c.stop().unwrap();
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "direction must still be the controller's original Forward -- a corrupted \
         Reverse would report Dismissed instead"
    );
}

// ---- #1183: no path reads NaN -- simulation and tick endpoints ---------

/// A scratch [`Simulation`] whose `x()` is finite at `t = 0` (so
/// `drive_simulation` accepts it) but returns NaN once mid-run.
struct GoesNanMidRun {
    nan_at: f32,
}

impl Simulation for GoesNanMidRun {
    fn x(&self, time: f32) -> f32 {
        if time >= self.nan_at { f32::NAN } else { time }
    }
    fn dx(&self, _time: f32) -> f32 {
        1.0
    }
    fn is_done(&self, _time: f32) -> bool {
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
#[test]
fn a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value() {
    let _serial = serial();
    let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
    let vsync = crate::vsync::Vsync::new();
    let _reg = vsync.register(c.clone());

    let mut future = c.animate_with(GoesNanMidRun { nan_at: 1.0 }).unwrap();
    // `Vsync` anchors a run's `t = 0` on the FIRST `tick_all` it sees
    // after the run starts (registration alone reads no clock) -- so
    // the first call establishes the anchor at elapsed 0, exactly like
    // a real frame loop's first pumped frame after `animate_with`.
    vsync.tick_all(0.0);
    vsync.tick_all(0.5);
    assert_eq!(
        c.value(),
        0.5,
        "precondition: the run is progressing normally"
    );
    assert!(vsync.has_running());

    vsync.tick_all(1.0); // sim.x(1.0) is NaN
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
    c.dispose();
}

/// `drive_simulation` refuses a simulation whose FIRST sample
/// (`x(0.0)`) is already non-finite, before any mutation.
#[test]
fn drive_simulation_refuses_a_non_finite_initial_sample() {
    let _serial = serial();
    let c = controller(100);
    let r = c.animate_with(GoesNanMidRun { nan_at: 0.0 });
    assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
    assert_eq!(c.value(), 0.0);
}

/// `tick_time_based` reads `start_value`/`target_value`
/// directly at the exact endpoints rather than computing
/// `start + range * eased_t` there -- a curve that is not the identity
/// at its own endpoints (this uses a curve whose `transform` shifts
/// every input) must still land EXACTLY on `start_value` at `t == 0`,
/// matching Flutter's `_InterpolationSimulation.x` parity.
#[test]
fn tick_time_based_reads_start_value_exactly_at_t_zero() {
    let _serial = serial();
    let c =
        AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0).unwrap();
    c.set_value(-1.0);
    c.animate_to_curved(3.0, None, Arc::new(NotIdentityAtEndpoints))
        .unwrap();
    c.tick_at(0.0);
    assert_eq!(
        c.value(),
        -1.0,
        "t == 0 must read start_value exactly, regardless of the curve"
    );
}

/// A curve whose `transform` is NOT the identity at `0.0`/`1.0` --
/// `tick_time_based`'s endpoint special-case must still land exactly on
/// `start_value`/`target_value` there despite this.
#[derive(Debug)]
struct NotIdentityAtEndpoints;

impl Curve for NotIdentityAtEndpoints {
    fn transform(&self, t: f32) -> f32 {
        0.1 + t * 0.8
    }
}

// ---- run-generation: bumps per run-start, stable across ticks ----

#[test]
fn run_generation_bumps_per_run_not_per_tick() {
    let _serial = serial();
    let c = controller(100);
    let g0 = c.run_generation();

    c.forward().unwrap();
    let g1 = c.run_generation();
    assert_eq!(g1, g0 + 1, "forward() establishes a new run");

    // Ticking advances the SAME run — the generation must not move, so the
    // binding does not spuriously re-anchor mid-run.
    c.tick_at(0.02);
    c.tick_at(0.05);
    assert_eq!(c.run_generation(), g1, "tick_at must not start a new run");

    c.reverse().unwrap();
    assert_eq!(
        c.run_generation(),
        g1 + 1,
        "reverse() establishes a new run"
    );

    // A settle-only path (reset) does NOT bump — the controller is not
    // running afterwards, so there is no run to anchor.
    c.reset().unwrap();
    assert_eq!(c.run_generation(), g1 + 1, "reset() does not start a run");

    c.animate_to(1.0, Some(Duration::from_millis(50))).unwrap();
    assert_eq!(c.run_generation(), g1 + 2, "animate_to starts a new run");

    c.reset().unwrap();
    c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
        .unwrap();
    assert_eq!(c.run_generation(), g1 + 3, "repeat starts a new run");

    c.fling(1.0).unwrap();
    assert_eq!(c.run_generation(), g1 + 4, "fling starts a new run");

    c.dispose();
}

// ---- B1: animate_to actually advances + does not clobber base duration ----

#[test]
fn animate_to_advances_value_across_ticks() {
    let _serial = serial();
    let c = controller(100); // base 100ms
    c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
    assert_eq!(c.value(), 0.0);
    c.tick_at(0.05); // 50ms of 100ms -> ~0.5
    assert!((c.value() - 0.5).abs() < 1e-3, "value={}", c.value());
    c.tick_at(0.10); // 100ms -> complete
    assert_eq!(c.value(), 1.0);
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.dispose();
}

#[test]
fn animate_to_does_not_clobber_base_duration() {
    let _serial = serial();
    let c = controller(100); // base 100ms
    c.animate_to(1.0, Some(Duration::from_millis(20))).unwrap();
    c.tick_at(0.02); // completes the 20ms run
    assert_eq!(c.value(), 1.0);
    // Base duration must be intact: a fresh forward run still takes 100ms.
    c.reset().unwrap();
    c.forward().unwrap();
    c.tick_at(0.05); // 50ms of the BASE 100ms -> ~0.5, not already complete
    assert!(
        (c.value() - 0.5).abs() < 1e-3,
        "base duration was clobbered: value={}",
        c.value()
    );
    c.dispose();
}

// ---- B1c: status listener may re-enter the controller without deadlock ----

#[test]
fn status_callback_can_reenter_controller_without_deadlock() {
    let _serial = serial();
    let c = controller(100);
    let reentered = Arc::new(AtomicUsize::new(0));
    let c2 = c.clone();
    let r2 = Arc::clone(&reentered);
    c.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Completed {
            // Re-enter: read + mutate the controller from within the status
            // callback. Under the old notify-under-lock code this deadlocked.
            let _ = c2.value();
            let _ = c2.reverse();
            r2.fetch_add(1, Ordering::SeqCst);
        }
    }));
    c.forward().unwrap();
    c.tick_at(0.10); // complete -> fires Completed -> callback re-enters
    assert_eq!(reentered.load(Ordering::SeqCst), 1);
    c.dispose();
}

// ---- value listeners fire on tick (regression for the dead ticker) ----

#[test]
fn value_listeners_fire_on_tick() {
    let _serial = serial();
    let c = controller(100);
    let ticks = Arc::new(AtomicUsize::new(0));
    let t2 = Arc::clone(&ticks);
    c.add_listener(Arc::new(move || {
        t2.fetch_add(1, Ordering::SeqCst);
    }));
    c.forward().unwrap();
    c.tick_at(0.05);
    c.tick_at(0.08);
    assert!(ticks.load(Ordering::SeqCst) >= 2);
    c.dispose();
}

/// `forward_from(Some(x))` jumps `value` to `x` before starting a REAL
/// (non-settling) run — that jump must notify exactly once, at the
/// call, same as Flutter's `forward(from:)` going through the `value=`
/// setter. Plain `forward()` (no `from`) does not jump the value at all,
/// so it must not notify at the call — only later ticks do.
///
/// Red-check: pass `ValueChange::Unchanged` unconditionally at
/// `forward_from`'s real-run `finish` call (its pre-fix shape) — the
/// first assertion reads `0`, not `1`.
#[test]
fn forward_from_notifies_once_at_the_call_iff_from_moved_the_value() {
    let _serial = serial();
    let c = controller(100);
    let fires = Arc::new(AtomicUsize::new(0));
    let f2 = Arc::clone(&fires);
    c.add_listener(Arc::new(move || {
        f2.fetch_add(1, Ordering::SeqCst);
    }));

    c.forward_from(Some(0.5)).unwrap();
    assert_eq!(
        fires.load(Ordering::SeqCst),
        1,
        "the from-jump (0.0 -> 0.5) must notify exactly once at the call"
    );
    assert!((c.value() - 0.5).abs() < 1e-6);
    c.dispose();

    let c2 = controller(100);
    let fires2 = Arc::new(AtomicUsize::new(0));
    let f2b = Arc::clone(&fires2);
    c2.add_listener(Arc::new(move || {
        f2b.fetch_add(1, Ordering::SeqCst);
    }));
    c2.forward().unwrap();
    assert_eq!(
        fires2.load(Ordering::SeqCst),
        0,
        "forward() with no `from` does not jump the value, so it must not \
         notify at the call — only a later tick does"
    );
    c2.dispose();
}

// ---- repeat with a finite count stops + completes ----

#[test]
fn repeat_with_finite_count_stops() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
        .unwrap();
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.tick_at(0.010); // cycle 1 boundary -> restart
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.tick_at(0.020); // cycle 2 boundary -> count reached -> stop
    assert_eq!(c.status(), AnimationStatus::Completed);
    // Further ticks do not advance a stopped controller.
    let v = c.value();
    c.tick_at(0.030);
    assert_eq!(c.value(), v);
    c.dispose();
}

#[test]
fn repeat_consumes_all_cycles_in_one_long_frame() {
    let _serial = serial();
    let c = controller(100);
    // count = 4, period = 10ms. A single 45ms frame (a dropped-frame
    // catch-up) spans 4 whole cycles, so the repeat must already be
    // exhausted — not still Forward as the old one-cycle-per-tick path left
    // it after the first boundary.
    c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(4))
        .unwrap();
    assert_eq!(c.status(), AnimationStatus::Forward);
    c.tick_at(0.045); // 4.5 cycles elapsed in one frame
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "all four cycles retired in one long frame -> exhausted"
    );
    c.dispose();
}

/// A bounce repeat over a CUSTOM interior range (not the controller's
/// true bounds) must exhaust on the FINAL retired leg's direction, not
/// whichever leg was active when the exhausting tick began.
/// `settled_status` has no bound check, so the old bounds-first fallback
/// that used to mask a stale `inner.direction` here no longer does.
///
/// `repeat_with(0.2, 0.8, reverse: true, period: 100ms, count: 2)`
/// starts at `0.2`, direction `Forward` (leg 1: `0.2 -> 0.8`). A single
/// 250ms tick spans both retired cycles at once (leg 1 forward, leg 2
/// reverse), landing on leg 2's endpoint `0.2` — so the run's direction
/// at exhaustion is `Reverse`, ending `Dismissed`.
///
/// Red-check: make `repeat_landing` ignore parity (e.g. always return
/// the `Forward` leg's `max` regardless of `index`) — status reads
/// `Completed`, not `Dismissed`.
#[test]
fn bounce_repeat_over_an_interior_range_exhausts_on_the_final_legs_status() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(
        Some(0.2),
        Some(0.8),
        true,
        Some(Duration::from_millis(100)),
        Some(2),
    )
    .unwrap();
    assert_eq!(
        c.status(),
        AnimationStatus::Forward,
        "sanity: leg 1 starts forward"
    );

    c.tick_at(0.25);

    assert!(
        (c.value() - 0.2).abs() < 1e-6,
        "value should land on leg 2's endpoint (repeat_min), got {}",
        c.value()
    );
    assert_eq!(
        c.status(),
        AnimationStatus::Dismissed,
        "the exhausting leg (leg 2) ran Reverse, so the run must end Dismissed"
    );
    c.dispose();
}

#[test]
fn animate_to_current_value_settles_immediately() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);
    // Animating to the value we are already at must settle at once instead
    // of running the ticker for `duration` re-notifying an unchanged value.
    c.animate_to(0.5, Some(Duration::from_millis(100))).unwrap();
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
    c.dispose();
}

#[test]
fn repeat_with_rejects_inverted_range() {
    let _serial = serial();
    let c = controller(100);
    // min >= max (within bounds) is rejected like `with_bounds` does.
    let r = c.repeat_with(Some(0.8), Some(0.2), false, None, None);
    assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    c.dispose();
}

/// The `min == max` equality case, distinct from `repeat_with_rejects_inverted_range`'s
/// `min > max` — Flutter permits this degenerate range (its own dropped
/// `min: 1.0, max: 1.0` oracle sub-case, `animation_controller_test.dart`
/// "calling repeat with specified min and max values" @ 3.44.0); FLUI
/// rejects it, the mapping entry's rationale.
#[test]
fn repeat_with_rejects_equal_min_and_max() {
    let _serial = serial();
    let c = controller(100);
    let r = c.repeat_with(Some(0.5), Some(0.5), false, None, None);
    assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    c.dispose();
}

#[test]
fn repeat_with_clamps_range_into_bounds() {
    let _serial = serial();
    let c = controller(100);
    // Out-of-bounds min/max are clamped into [0, 1]; the run never
    // leaves the controller bounds.
    c.repeat_with(
        Some(-5.0),
        Some(5.0),
        false,
        Some(Duration::from_millis(10)),
        None,
    )
    .unwrap();
    // The run starts at the CURRENT value (0.0, this controller's
    // default) clamped into the range — not "at the clamped min",
    // which only coincides here because the current value already is
    // the lower bound.
    assert_eq!(
        c.value(),
        0.0,
        "current value 0.0 clamped into [0, 1] is still 0.0"
    );
    c.tick_at(0.005); // mid-cycle
    assert!(
        c.value() >= 0.0 && c.value() <= 1.0,
        "stays within bounds: {}",
        c.value()
    );
    c.dispose();
}

// ---- repeat sampling is a pure function of elapsed time (#1078) ----

/// The issue's own reproduction: a repeating run's value/status at any
/// `tick_at(t)` depends only on the elapsed time since the run started,
/// never on how many intervening ticks partitioned the way there —
/// `tick_at(1.25)` must equal `tick_at(1.0); tick_at(1.25)`, for both
/// restart and bounce.
#[test]
fn repeat_value_is_partition_invariant_across_a_skipped_cycle() {
    let _serial = serial();

    // Restart mode: skip cycle 0's boundary tick entirely.
    let direct = AnimationController::without_ticker(Duration::from_secs(1));
    direct
        .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    direct.tick_at(1.25);
    let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
    partitioned
        .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    partitioned.tick_at(1.0);
    partitioned.tick_at(1.25);
    assert!(
        (direct.value() - 0.25).abs() < 1e-6,
        "value={}",
        direct.value()
    );
    assert_eq!(direct.value(), partitioned.value());
    assert_eq!(direct.status(), partitioned.status());
    direct.dispose();
    partitioned.dispose();

    // Bounce mode: same elapsed time, opposite leg (cycle index 1 is
    // the reverse leg).
    let direct = AnimationController::without_ticker(Duration::from_secs(1));
    direct
        .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    direct.tick_at(1.25);
    let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
    partitioned
        .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    partitioned.tick_at(1.0);
    partitioned.tick_at(1.25);
    assert!(
        (direct.value() - 0.75).abs() < 1e-6,
        "value={}",
        direct.value()
    );
    assert_eq!(direct.value(), partitioned.value());
    assert_eq!(direct.status(), partitioned.status());
    direct.dispose();
    partitioned.dispose();
}

/// Partition invariance holds for any number of skipped cycles (odd and
/// even), and over a custom `min`/`max` range, not only the
/// controller's own bounds.
#[test]
fn repeat_value_is_partition_invariant_over_custom_bounds_and_multiple_skipped_cycles() {
    let _serial = serial();
    for &t in &[1.25_f64, 2.25, 3.25] {
        let direct = AnimationController::without_ticker(Duration::from_secs(1));
        direct
            .repeat_with(
                Some(0.2),
                Some(0.8),
                true,
                Some(Duration::from_secs(1)),
                None,
            )
            .unwrap();
        direct.tick_at(t);

        let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
        partitioned
            .repeat_with(
                Some(0.2),
                Some(0.8),
                true,
                Some(Duration::from_secs(1)),
                None,
            )
            .unwrap();
        let mut elapsed = 0.0_f64;
        while elapsed < t {
            elapsed = (elapsed + 1.0).min(t);
            partitioned.tick_at(elapsed);
        }

        assert!(
            (direct.value() - partitioned.value()).abs() < 1e-6,
            "t={t}: direct={} partitioned={}",
            direct.value(),
            partitioned.value()
        );
        assert_eq!(direct.status(), partitioned.status(), "t={t}");
        direct.dispose();
        partitioned.dispose();
    }
}

/// Sampling the same elapsed time twice is idempotent.
#[test]
fn repeat_tick_at_the_same_time_twice_is_idempotent() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_secs(1));
    c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    c.tick_at(2.25);
    let (value, status) = (c.value(), c.status());
    c.tick_at(2.25);
    assert_eq!(c.value(), value);
    assert_eq!(c.status(), status);
    c.dispose();
}

/// The at-call value for a RESTART repeat starting exactly at `max`
/// must be the pure function sampled at elapsed time zero (the phase
/// wraps to `min`, exactly Flutter's `_startSimulation` setting
/// `_value = x(0.0)`), not the bare clamped current value. Storing `v`
/// directly contradicted the model, its own comment, AND Flutter: it
/// read `1.0` at the call, then `tick_at(0.0)` — the very first tick,
/// no time elapsed — recomputed via the sampler and got `0.0`, a
/// one-frame discontinuity the whole change exists to remove. The tick
/// still notifies value listeners (every tick does, as in Flutter's
/// `_tick`); what it must not do is move the value.
#[test]
fn repeat_restart_at_call_value_from_max_has_no_discontinuity_at_the_first_tick() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(1.0);
    c.repeat(false).unwrap();
    assert!(
        (c.value() - 0.0).abs() < 1e-6,
        "the phase wraps at the call itself: value={}",
        c.value()
    );
    assert_eq!(c.status(), AnimationStatus::Forward);

    let value_fires = Arc::new(AtomicUsize::new(0));
    let vf = Arc::clone(&value_fires);
    c.add_listener(Arc::new(move || {
        vf.fetch_add(1, Ordering::SeqCst);
    }));
    c.tick_at(0.0);
    assert!(
        (c.value() - 0.0).abs() < 1e-6,
        "no discontinuity: the first tick must agree with the at-call value"
    );
    assert_eq!(
        value_fires.load(Ordering::SeqCst),
        1,
        "a tick is a frame: value listeners fire once per tick even when the sample repeats"
    );
    c.dispose();
}

/// The value/status at the call are the pure function sampled at
/// elapsed time zero, computed BEFORE any tick — a bounce starting
/// exactly at `max` reports the reverse leg immediately (unaffected by
/// the restart-mode fix above: bounce mode's phase wrap lands back on
/// `max`, not `min`). Flutter parity: `_startSimulation` runs `x(0.0)`
/// (which flips direction) before `_status` is computed
/// (`AnimationController._startSimulation` @ 3.44.0).
#[test]
fn repeat_bounce_from_max_reports_reverse_status_at_the_call() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(1.0);
    c.repeat(true).unwrap();
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    assert_eq!(c.status(), AnimationStatus::Reverse);
    c.dispose();
}

/// Flutter oracle: `animation_controller_test.dart` "calling repeat
/// with reverse set to true makes the animation alternate between
/// lowerBound and upperBound values on each repeat" (@ 3.44.0) — the
/// two sub-cases that start from a boundary/interior value (the
/// `value == 0.0` sub-case is a restart-equivalent already covered by
/// `repeat_value_is_partition_invariant_across_a_skipped_cycle`).
#[test]
fn repeat_bounce_flutter_oracle_reverse_from_max_and_mid() {
    let _serial = serial();

    // value == max at the call reports the reverse leg immediately.
    let c = controller(100);
    c.set_value(1.0);
    c.repeat(true).unwrap();
    c.tick_at(0.025);
    assert!((c.value() - 0.75).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.125);
    assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
    c.dispose();

    // value == 0.5 (mid-range) at the call.
    let c = controller(100);
    c.set_value(0.5);
    c.repeat(true).unwrap();
    c.tick_at(0.05);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.15);
    assert!((c.value() - 0.0).abs() < 1e-6, "value={}", c.value());
    c.dispose();
}

/// Flutter oracle: `animation_controller_test.dart` "calling repeat
/// with specified min and max values between 0 and 1..." (@ 3.44.0) —
/// the `min == max` degenerate sub-case is skipped: FLUI rejects it
/// (`repeat_with_rejects_inverted_range`), where Flutter permits it.
#[test]
fn repeat_bounce_flutter_oracle_interior_range() {
    let _serial = serial();

    // value 0.0 is below `min` at the call — the silent clamp lands on
    // 0.5 (Flutter's `x(0.0)` parity), not a rejection.
    let c = controller(100);
    c.repeat_with(Some(0.5), Some(1.0), true, None, None)
        .unwrap();
    assert!(
        (c.value() - 0.5).abs() < 1e-6,
        "silent clamp: value={}",
        c.value()
    );
    c.tick_at(0.05);
    assert!((c.value() - 0.75).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.10);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.20);
    assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
    c.dispose();

    // The same 200ms checkpoint, sampled in a SINGLE tick from the call
    // with no intervening boundary ticks: partition invariance must
    // give the identical answer the sequential port above established.
    // A two-cycle skip in one frame is exactly where the old
    // incremental model (advance-by-one-cycle-endpoint) diverged from
    // pure sampling.
    let jumped = controller(100);
    jumped
        .repeat_with(Some(0.5), Some(1.0), true, None, None)
        .unwrap();
    jumped.tick_at(0.20);
    assert!(
        (jumped.value() - 0.5).abs() < 1e-6,
        "value={}",
        jumped.value()
    );
    jumped.dispose();

    let c = controller(100);
    c.set_value(0.2);
    c.repeat_with(Some(0.2), Some(0.6), true, None, None)
        .unwrap();
    c.tick_at(0.05);
    assert!((c.value() - 0.4).abs() < 1e-6, "value={}", c.value());
    c.dispose();
}

/// Flutter oracle: `animation_controller_test.dart` "calling repeat
/// with negative min value and positive max value..." (@ 3.44.0) — a
/// repeat range that does not start at the controller's own bounds, on
/// a controller whose own bounds are not `[0, 1]`.
#[test]
fn repeat_restart_flutter_oracle_custom_controller_bounds() {
    let _serial = serial();
    let c =
        AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0).unwrap();
    c.set_value(1.0);
    c.repeat_with(Some(1.0), Some(3.0), false, None, None)
        .unwrap();
    assert!(
        (c.value() - 1.0).abs() < 1e-6,
        "value at call={}",
        c.value()
    );
    c.tick_at(0.05);
    assert!((c.value() - 2.0).abs() < 1e-6, "value={}", c.value());
    c.dispose();

    let c =
        AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0).unwrap();
    c.set_value(0.0);
    c.repeat_with(Some(-1.0), Some(3.0), false, None, None)
        .unwrap();
    assert!(
        (c.value() - 0.0).abs() < 1e-6,
        "value at call={}",
        c.value()
    );
    c.tick_at(0.025);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    c.dispose();
}

/// A finite count is measured from the phase origin, not from a fresh
/// cycle 0: a run started mid-cycle (value 0.5, half a period's phase)
/// exhausts `count` boundaries later — half a period after the call,
/// not a full period later (Flutter: `_exitTimeInSeconds = count*period
/// - _initialT`). Lands on the leg's own endpoint, `Completed` — the
/// improved replacement for Flutter's `% 1.0`-wrapped oracle (see
/// `docs/ARCHITECTURE.md`'s "Repeat sampling" mapping entry).
#[test]
fn repeat_restart_finite_count_exhausts_from_the_phase_origin() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);
    c.repeat_with(None, None, false, None, Some(1)).unwrap();
    c.tick_at(0.05);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.dispose();
}

/// Flutter oracle: `animation_controller_test.dart` "calling repeat by
/// setting count as valid with reverse as true..." (@ 3.44.0). Its
/// harness ticks are ABSOLUTE frame timestamps
/// (`scheduler_tester.dart`'s `tick` calls `handleBeginFrame` with the
/// argument directly), so `tick(100ms)` then `tick(60ms)` REWINDS the
/// clock to 60ms — the oracle's `0.6` sample is elapsed 60ms, not a
/// cumulative 160ms. Pure sampling makes that rewind exact, with no
/// `toStringAsFixed` rounding needed. The exhaustion assertion is
/// FLUI's own addition — the oracle never ticks that far.
#[test]
fn repeat_bounce_flutter_oracle_finite_count_and_absolute_time_rewind() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(None, None, true, None, Some(4)).unwrap();
    c.tick_at(0.025);
    assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.05);
    assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
    c.tick_at(0.099);
    assert!((c.value() - 0.99).abs() < 1e-3, "value={}", c.value());
    c.tick_at(0.10);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    // The harness's absolute-time rewind: elapsed 60ms, not a
    // cumulative 160ms.
    c.tick_at(0.06);
    assert!((c.value() - 0.6).abs() < 1e-6, "value={}", c.value());
    c.dispose();

    // The non-rewound interpretation, for contrast: elapsed 160ms lands
    // on the reverse leg's 0.4, not the forward leg's 0.6.
    let c2 = controller(100);
    c2.repeat_with(None, None, true, None, Some(4)).unwrap();
    c2.tick_at(0.16);
    assert!((c2.value() - 0.4).abs() < 1e-6, "value={}", c2.value());
    c2.dispose();

    // Exhaustion at exactly 400ms lands on the 4th (odd-indexed)
    // cycle's reverse-leg endpoint.
    let c3 = controller(100);
    c3.repeat_with(None, None, true, None, Some(4)).unwrap();
    c3.tick_at(0.4);
    assert!((c3.value() - 0.0).abs() < 1e-6, "value={}", c3.value());
    assert_eq!(c3.status(), AnimationStatus::Dismissed);
    c3.dispose();
}

/// The exhaustion boundary is checked in integer nanoseconds, not f64:
/// `0.3 >= 3.0 * 0.1` is `false` in f64 arithmetic, which would leave a
/// 3-count 100ms repeat `Forward` one frame past the boundary it
/// should have exhausted at.
#[test]
fn repeat_exhaustion_boundary_is_exact_at_a_float_unsafe_ratio() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(None, None, false, None, Some(3)).unwrap();
    c.tick_at(0.3);
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "3 * 100ms sampled at exactly 300ms must already be exhausted"
    );
    c.dispose();
}

/// `Duration::try_from_secs_f64` returning `Err` (an out-of-range
/// `cycle`, e.g. `tick_at(f64::INFINITY)` or an extreme `time_dilation`
/// overflowing the division) must saturate to a very large elapsed
/// time, never to zero — a zero-rewind would let a pathological input
/// never exhaust a finite repeat while `forward()` on the same input
/// completes normally. Red-check: `.map_or(0, |d| d.as_nanos())`
/// instead of `.unwrap_or(Duration::MAX)` — this repeat never exhausts.
#[test]
fn repeat_tick_at_infinity_exhausts_a_finite_count_instead_of_rewinding() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(None, None, false, Some(Duration::from_millis(100)), Some(2))
        .unwrap();
    c.tick_at(f64::INFINITY);
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
    c.dispose();
}

/// A zero effective period settles SYNCHRONOUSLY at the call — Android's
/// rule ("0 duration animator, ignore the repeat count and skip to the
/// end"); Compose rejects it, Flutter asserts. A later `tick_at`
/// changes nothing (no run was ever installed), and `run_generation` is
/// untouched.
#[test]
fn repeat_with_zero_period_settles_synchronously_at_the_call() {
    let _serial = serial();

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
    c.tick_at(1.0);
    assert!(
        (c.value() - 1.0).abs() < 1e-6,
        "a later tick must change nothing"
    );
    c.dispose();

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
    c2.dispose();
}

/// `count: Some(0)` is a degenerate case distinct from a zero
/// PERIOD: zero cycles run AT ALL, regardless of period, so there is no
/// cycle to land on — the value at the call is the CLAMPED CURRENT
/// value, unchanged, not a landing jump. Web Animations semantics (an
/// empty active interval finishes at once); Flutter asserts
/// `count > 0`, Compose throws for `iterations < 1`. Repair, not
/// reject, is the house rule.
#[test]
fn repeat_with_zero_count_settles_at_the_current_value_with_no_landing_jump() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.3);
    let generation_before = c.run_generation();
    let future = c
        .repeat_with(None, None, true, Some(Duration::from_millis(100)), Some(0))
        .unwrap();
    assert!(future.is_complete());
    assert!(
        (c.value() - 0.3).abs() < 1e-6,
        "zero cycles must not jump the value: {}",
        c.value()
    );
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert_eq!(c.run_generation(), generation_before);
    c.tick_at(1.0);
    assert!(
        (c.value() - 0.3).abs() < 1e-6,
        "a later tick must change nothing"
    );
    c.dispose();
}

/// `tick_repeat` NEVER reads `run_curve` at all — this pins that a
/// repeat interpolates linearly regardless of what a prior
/// `animate_to_curved` leaves behind, not that `repeat_with`'s
/// `clear_run_modes()` call is what protects it (that call stays green
/// even with the call deleted, since the curve field is structurally
/// unreachable from `tick_repeat`; see
/// `a_leftover_fling_simulation_does_not_leak_into_a_following_repeats_velocity`
/// below for what `clear_run_modes()` actually protects).
#[test]
fn a_leftover_curve_does_not_shape_a_following_repeat() {
    use crate::curve::Curves;
    let _serial = serial();
    let c = controller(100);
    // A zero-distance curved run settles synchronously but still
    // installs the curve in `run_curve` — exactly the leftover state a
    // real `animate_to_curved` interruption would leave behind.
    c.animate_to_curved(
        0.0,
        Some(Duration::from_millis(100)),
        Arc::new(Curves::EaseInQuint),
    )
    .unwrap();

    c.repeat_with(None, None, false, Some(Duration::from_millis(100)), None)
        .unwrap();
    c.tick_at(0.05);
    assert!(
        (c.value() - 0.5).abs() < 1e-6,
        "a repeat interpolates linearly, no curve — value={}",
        c.value()
    );
    c.dispose();
}

/// `repeat_with`'s `clear_run_modes()` call IS pinned by this: a
/// leftover `simulation` from a prior `fling` would short-circuit
/// `velocity()` (`sim.dx(cycle)` instead of `range / duration`) if it
/// survived into the repeat. Red-check: delete the `clear_run_modes()`
/// call in `repeat_with` — `velocity()` reads the stale fling spring's
/// `dx` instead of the repeat's own `range / period`.
#[test]
fn a_leftover_fling_simulation_does_not_leak_into_a_following_repeats_velocity() {
    let _serial = serial();
    let c = controller(100);
    c.fling(1.0).unwrap(); // installs `simulation`
    c.repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    c.tick_at(0.25);
    assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
    assert!(
        (c.velocity() - 1.0).abs() < 1e-6,
        "a leftover fling simulation must not leak into the repeat's velocity: {}",
        c.velocity()
    );
    c.dispose();
}

/// `set_duration` must not retime an ACTIVE repeat: the period is
/// resolved ONCE at `repeat_with` (`period.unwrap_or(duration)`), not
/// read live on every tick — Flutter parity (`period ??= duration`,
/// captured by the simulation at the call).
#[test]
fn set_duration_during_an_active_repeat_leaves_the_running_period_unchanged() {
    let _serial = serial();
    let c = controller(100);
    // Period defaults from `duration` (no explicit `period` argument) —
    // the shape that used to be re-read live.
    c.repeat_with(None, None, false, None, None).unwrap();
    c.set_duration(Duration::from_millis(200));
    c.tick_at(0.05); // half of the ORIGINAL 100ms period
    assert!(
        (c.value() - 0.5).abs() < 1e-6,
        "set_duration mid-repeat must not retime the running period: value={}",
        c.value()
    );
    c.dispose();
}

/// `velocity()` on a reverse leg is SIGNED (a deliberate divergence
/// from Flutter's `_RepeatingSimulation.dx`, which is always positive)
/// — see `docs/ARCHITECTURE.md`'s "Repeat sampling" mapping entry, (g).
/// Previously uncited/untested: the mapping entry's citation of
/// `reverse_mid_flight_keeps_full_range_velocity` as this behavior's
/// "sibling repeat coverage" named a test that has no `.velocity()`
/// call at all.
#[test]
fn repeat_reverse_leg_velocity_is_negative() {
    let _serial = serial();
    let c = controller(100);
    c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    c.tick_at(1.5); // cycle index 1 (odd) -> reverse leg, mid-cycle
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
    c.dispose();
}

/// `velocity()` after a mid-run `set_value` must be 0.0, not the stale
/// interrupted run's rate: `set_value` clears `active_run` via
/// `stop_running()` but reports a *directional* running status at an
/// interior value, so the `status.is_running()` gate `velocity()` used to
/// consult read "running" and computed `(target_value - start_value) /
/// current_duration()` from a run that no longer exists. `active_run.is_none()`
/// is the always-consistent "is a run installed" fact (see `walk_probe`'s
/// doc) — the same predicate `tick_at` already gates on.
#[test]
fn velocity_after_a_mid_run_set_value_is_zero_not_the_stale_runs_rate() {
    let _serial = serial();
    let c = controller(1000);
    c.forward().unwrap();
    c.tick_at(0.5);
    assert!(
        (c.value() - 0.5).abs() < 1e-6,
        "sanity: value={}",
        c.value()
    );

    c.set_value(0.2);
    assert_eq!(c.status(), AnimationStatus::Forward);
    assert!(!c.is_animating(), "set_value mid-run must stop the run");
    assert_eq!(
        c.velocity(),
        0.0,
        "velocity after a mid-run set_value must be 0.0, not the stale run's rate"
    );

    // A subsequent run installs a fresh active_run and reports again.
    c.forward().unwrap();
    assert!(
        c.velocity() != 0.0,
        "a fresh forward() must report a live velocity"
    );
    c.dispose();
}

/// Unbounded mirror of
/// `velocity_after_a_mid_run_set_value_is_zero_not_the_stale_runs_rate`:
/// `animate_to` (a finite target on an unbounded controller) interrupted by
/// `set_value` reported the same stale-run velocity before the fix.
#[test]
fn velocity_after_a_mid_run_set_value_is_zero_on_unbounded() {
    let _serial = serial();
    let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));
    c.animate_to(500.0, None).unwrap();
    c.tick_at(0.5);
    assert!(
        (c.value() - 250.0).abs() < 1e-3,
        "sanity: value={}",
        c.value()
    );

    c.set_value(200.0);
    assert!(!c.is_animating(), "set_value mid-run must stop the run");
    assert_eq!(c.velocity(), 0.0);

    c.animate_to(500.0, None).unwrap();
    assert!(
        c.velocity() != 0.0,
        "a fresh animate_to must report a live velocity"
    );
    c.dispose();
}

/// A frame that spans several repeat cycles at once fires status
/// listeners by PARITY, not once per retired cycle: an even number of
/// skipped bounce legs cancels out (no net direction flip), an odd
/// number fires exactly one status change, and a restart repeat's
/// cycle boundary never changes status at all (`take_status_change`
/// dedups the repeated `Forward` write). A tick that advances a live
/// run fires the value listener exactly once, regardless of how many
/// cycles it retired.
#[test]
fn repeat_multi_cycle_frame_notification_counts() {
    let _serial = serial();

    // Restart: a single tick spanning 3.5 cycles never changes status
    // and fires exactly one value notification.
    let c = controller(100);
    c.repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
        .unwrap();
    let status_fires = Arc::new(AtomicUsize::new(0));
    let sf = Arc::clone(&status_fires);
    c.add_status_listener(Arc::new(move |_| {
        sf.fetch_add(1, Ordering::SeqCst);
    }));
    let value_fires = Arc::new(AtomicUsize::new(0));
    let vf = Arc::clone(&value_fires);
    c.add_listener(Arc::new(move || {
        vf.fetch_add(1, Ordering::SeqCst);
    }));
    c.tick_at(3.5);
    assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
    assert_eq!(
        status_fires.load(Ordering::SeqCst),
        0,
        "restart never changes status mid-repeat"
    );
    assert_eq!(
        value_fires.load(Ordering::SeqCst),
        1,
        "one tick, one value notification"
    );
    c.dispose();

    // Bounce, EVEN number of retired cycles (2): direction is back to
    // Forward, so no net status change.
    let c = controller(100);
    c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    let status_fires = Arc::new(AtomicUsize::new(0));
    let sf = Arc::clone(&status_fires);
    c.add_status_listener(Arc::new(move |_| {
        sf.fetch_add(1, Ordering::SeqCst);
    }));
    c.tick_at(2.5);
    assert_eq!(
        status_fires.load(Ordering::SeqCst),
        0,
        "an even number of skipped bounce cycles fires no status change"
    );
    c.dispose();

    // Bounce, ODD number of retired cycles (3): exactly one status
    // change.
    let c = controller(100);
    c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
        .unwrap();
    let status_fires = Arc::new(AtomicUsize::new(0));
    let sf = Arc::clone(&status_fires);
    c.add_status_listener(Arc::new(move |_| {
        sf.fetch_add(1, Ordering::SeqCst);
    }));
    c.tick_at(3.5);
    assert_eq!(
        status_fires.load(Ordering::SeqCst),
        1,
        "an odd number of skipped bounce cycles fires exactly one status change"
    );
    c.dispose();
}

// ---- set_value recomputes status at the bounds ----

#[test]
fn set_value_recomputes_status() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(1.0);
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.set_value(0.0);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

// ---- time dilation slows progress ----

/// Restores the global time dilation on drop so a failed assertion cannot
/// leak a non-default dilation into sibling tests.
struct DilationRestore(f64);
impl Drop for DilationRestore {
    fn drop(&mut self) {
        let _ = flui_scheduler::config::set_time_dilation(self.0);
    }
}

#[test]
fn time_dilation_scales_progress() {
    use flui_scheduler::config::{set_time_dilation, time_dilation};
    let _serial = serial();
    let _restore = DilationRestore(time_dilation());
    set_time_dilation(2.0).unwrap(); // half speed
    let c = controller(100); // 100ms
    c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
    c.tick_at(0.10); // 100ms raw -> dilated 50ms -> ~0.5, NOT complete
    let value = c.value();
    c.dispose();
    assert!((value - 0.5).abs() < 1e-3, "value={value}");
}

// ---- set_value parity: stops an active run, change-detects status ----

#[test]
fn set_value_stops_the_ticker_so_a_later_frame_does_not_clobber_it() {
    // Flutter parity: `AnimationController`'s `value=` setter calls
    // `stop()` before `_internalSetValue`. Drive a REAL frame through the
    // scheduler (not a direct `tick_at` call) so this exercises the same
    // path production code does: an auto-scheduling ticker re-registers
    // itself with the scheduler every frame while active, and only
    // `ticker.stop()` deregisters it.
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::from_millis(100), &scheduler);

    c.forward().unwrap();
    scheduler.execute_frame();
    assert!(
        c.value() > 0.0,
        "sanity: the ticker actually drove a frame, got value={}",
        c.value()
    );

    c.set_value(0.5);
    assert!(
        !c.is_animating(),
        "set_value must stop the ticker like Flutter's value= setter"
    );

    // The "next vsync": if the ticker were still registered, this frame
    // would recompute the value from the stale run and clobber 0.5.
    scheduler.execute_frame();
    assert_eq!(
        c.value(),
        0.5,
        "a frame after set_value must not overwrite the value that was just set"
    );
    c.dispose();
}

#[test]
fn set_value_reports_completed_status_at_upper_bound() {
    let _serial = serial();
    let c = controller(100);
    c.forward().unwrap();
    c.set_value(1.0);
    assert_eq!(c.value(), 1.0);
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert!(!c.is_animating(), "set_value stops the run before settling");
    c.dispose();
}

#[test]
fn status_listener_is_not_refired_for_an_unchanged_status() {
    // Flutter parity: `AnimationController._checkStatusChanged` only
    // notifies status listeners when `status` actually differs from
    // `_lastReportedStatus`. Without this, a 120Hz gesture-driven
    // `set_value` loop (or an interior set_value immediately followed by
    // `forward()` in the same direction) would re-fire `Forward` every
    // frame and a one-shot status listener would misfire repeatedly.
    let _serial = serial();
    let c = controller(100);
    let fire_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fire_count);
    c.add_status_listener(Arc::new(move |_status| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    // set_value(0.5) is the FIRST transition (Dismissed -> Forward): fires once.
    c.set_value(0.5);
    assert_eq!(fire_count.load(Ordering::SeqCst), 1);

    // forward() keeps the same Forward status (direction was already
    // Forward, value already interior) — must NOT re-fire.
    c.forward().unwrap();
    assert_eq!(
        fire_count.load(Ordering::SeqCst),
        1,
        "forward() after an already-Forward set_value must not re-fire the same status"
    );

    // A second set_value that keeps the status Forward must also not re-fire.
    c.set_value(0.6);
    assert_eq!(
        fire_count.load(Ordering::SeqCst),
        1,
        "a same-status set_value must not re-fire (mirrors a 120Hz gesture drag)"
    );

    // A genuine status change (interior -> Completed) DOES fire.
    c.set_value(1.0);
    assert_eq!(fire_count.load(Ordering::SeqCst), 2);
    c.dispose();
}

// ---- animate_to_curved / animate_back_curved thread a curve through the run ----

#[test]
fn animate_to_curved_eases_through_the_given_curve() {
    use crate::curve::Curves;
    let _serial = serial();
    let c = controller(100);
    c.animate_to_curved(
        1.0,
        Some(Duration::from_millis(100)),
        Arc::new(Curves::EaseInQuint),
    )
    .unwrap();
    c.tick_at(0.05); // t=0.5 raw
    let expected = Curves::EaseInQuint.transform(0.5);
    assert!(
        (c.value() - expected).abs() < 1e-3,
        "expected the curve applied at t=0.5: got {}, want {expected}",
        c.value()
    );
    c.tick_at(0.10);
    assert_eq!(
        c.value(),
        1.0,
        "the curve must land exactly on the target at t=1.0"
    );
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.dispose();
}

#[test]
fn plain_animate_to_after_a_curved_run_is_linear_again() {
    // The per-run curve must not leak into a later plain (linear) run.
    use crate::curve::Curves;
    let _serial = serial();
    let c = controller(100);
    c.animate_to_curved(
        1.0,
        Some(Duration::from_millis(100)),
        Arc::new(Curves::EaseInQuint),
    )
    .unwrap();
    c.tick_at(0.10);
    c.reset().unwrap();

    c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
    c.tick_at(0.05);
    assert!(
        (c.value() - 0.5).abs() < 1e-3,
        "a plain animate_to after a curved run must be linear again: got {}",
        c.value()
    );
    c.dispose();
}

#[test]
fn animate_back_curved_eases_toward_the_lower_bound() {
    use crate::curve::Curves;
    let _serial = serial();
    let c = controller(100);
    c.set_value(1.0);
    c.animate_back_curved(
        0.0,
        Some(Duration::from_millis(100)),
        Arc::new(Curves::EaseInQuint),
    )
    .unwrap();
    c.tick_at(0.05);
    let expected = 1.0 - Curves::EaseInQuint.transform(0.5);
    assert!(
        (c.value() - expected).abs() < 1e-3,
        "got {}, want {expected}",
        c.value()
    );
    c.dispose();
}

// ---- reentrant ticker restart via a status listener (issue #1059) ----

/// The canonical "chain the next animation" idiom: a status listener
/// calls `forward()` again once the run it is reacting to completes.
/// The controller starts at the UPPER bound and runs `reverse()` first —
/// calling `forward()` immediately after a forward run lands exactly on
/// the upper bound already (a real, zero-distance settle, Flutter
/// parity: see `forward_at_upper_bound_settles_immediately`), which
/// would never reach `restart_ticker` at all. `restart_ticker`'s
/// `ticker.start(new_callback)` then runs while the SAME ticker's own
/// callback is still the one dispatching — this run's own — tick
/// (`tick_time_based` already stopped the ticker before firing status,
/// so `restart_ticker`'s own `ticker.stop()` is a no-op, but
/// `ticker.start` is not: it installs the chained run's callback into a
/// slot a `TickerLease` still holds checked out). This is the SAME
/// "restart inside tick" scenario `flui-scheduler`'s own
/// `restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick`
/// pins directly on `Ticker`, reached here through the real production
/// call chain instead of a hand-rolled reentrant probe.
#[test]
fn status_listener_chaining_forward_ticks_once_per_frame_and_stop_fully_stops_it() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::from_millis(1), &scheduler);
    c.set_value(1.0);

    let tick_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&tick_count);
    c.add_listener(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    let restarted = Arc::new(AtomicUsize::new(0));
    let restart_flag = Arc::clone(&restarted);
    let chained = c.clone();
    c.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Dismissed && restart_flag.fetch_add(1, Ordering::SeqCst) == 0
        {
            // A LONG chained run on purpose: the point of the second
            // half of this test is that `stop()` cancels a chain that
            // is still in flight. A chained run short enough to finish
            // on the next frame stops itself (`tick_time_based` calls
            // `ticker.stop()` before firing its status), which would
            // leave nothing for `stop()` to cancel and make every
            // assertion below hold with or without the fix.
            chained
                .animate_to(1.0, Some(Duration::from_secs(10)))
                .unwrap();
        }
    }));

    c.reverse().unwrap();
    std::thread::sleep(Duration::from_millis(5));
    scheduler.execute_frame();

    assert_eq!(
        restarted.load(Ordering::SeqCst),
        1,
        "sanity: the first run must complete and the status listener \
         must have chained a restart"
    );
    assert_eq!(
        scheduler.transient_callback_count(),
        1,
        "the chained restart must leave exactly ONE live tick \
         registration — not zero (the new run's callback lost) and not \
         two (the old run's registration orphaned alongside it)"
    );

    std::thread::sleep(Duration::from_millis(5));
    scheduler.execute_frame();

    assert_eq!(
        tick_count.load(Ordering::SeqCst),
        2,
        "exactly one value notification per frame across both runs — a \
         duplicated tick chain would notify twice on the second frame"
    );
    assert_eq!(
        scheduler.transient_callback_count(),
        1,
        "the chained run is still in flight, so there is exactly one \
         live registration for `stop()` to cancel below"
    );

    c.stop().unwrap();
    assert_eq!(
        scheduler.transient_callback_count(),
        0,
        "stop() must fully cancel the single live tick chain, not just \
         one half of a duplicated pair"
    );
    assert!(!c.is_animating());

    // A surviving orphaned registration from a duplicated chain would
    // still fire here even after `stop()`.
    scheduler.execute_frame();
    assert_eq!(tick_count.load(Ordering::SeqCst), 2);

    c.dispose();
}

// ---- controller-owned run futures (issue #1161 / ADR-0064) ----

#[test]
fn forward_future_resolves_ok_when_the_run_completes_via_tick_at() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();
    assert!(future.is_pending());

    c.tick_at(0.1);

    assert!(
        future.is_complete(),
        "a run that reaches its target normally must resolve Ok, not \
         stay pending"
    );
    c.dispose();
}

#[test]
fn zero_duration_forward_completes_before_forward_returns() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::ZERO, &scheduler);

    let value_fires = Arc::new(AtomicUsize::new(0));
    let vf = Arc::clone(&value_fires);
    c.add_listener(Arc::new(move || {
        vf.fetch_add(1, Ordering::SeqCst);
    }));
    let status_fires = Arc::new(AtomicUsize::new(0));
    let sf = Arc::clone(&status_fires);
    c.add_status_listener(Arc::new(move |_status| {
        sf.fetch_add(1, Ordering::SeqCst);
    }));
    let generation_before = c.run_generation();

    let future = c.forward().unwrap();

    assert!(
        future.is_complete(),
        "a zero-duration forward() must return an already-complete \
         future — no ticker run to wait on"
    );
    assert_eq!(c.value(), 1.0, "value snaps to the upper bound at the call");
    assert_eq!(c.status(), AnimationStatus::Completed);
    assert_eq!(
        value_fires.load(Ordering::SeqCst),
        1,
        "the value moved (0.0 -> 1.0), so the value listener fires once"
    );
    assert_eq!(status_fires.load(Ordering::SeqCst), 1);
    assert_eq!(
        c.run_generation(),
        generation_before,
        "a synchronous settle installs no run and must not bump run_generation"
    );

    scheduler.execute_frame();
    assert_eq!(
        (
            value_fires.load(Ordering::SeqCst),
            status_fires.load(Ordering::SeqCst)
        ),
        (1, 1),
        "no ticker was ever installed, so a later frame changes nothing"
    );
    c.dispose();
}

#[test]
fn zero_duration_reverse_settles_dismissed_at_the_call() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::ZERO, &scheduler);
    c.set_value(1.0);

    let future = c.reverse().unwrap();

    assert!(future.is_complete());
    assert_eq!(c.value(), 0.0);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

/// `animate_to(x, Some(Duration::ZERO))` / `animate_back(x, Some(Duration::ZERO))`
/// are the documented "set a value with a direction" (flutter#158233's
/// accepted workaround): the METHOD picks the end status, not the
/// travel — `animate_to` toward a SMALLER value still ends `Completed`,
/// `animate_back` toward a LARGER one still ends `Dismissed`.
#[test]
fn animate_to_with_zero_duration_is_a_directional_set() {
    let _serial = serial();
    let c = controller(100);

    c.set_value(0.7);
    c.animate_to(0.3, Some(Duration::ZERO)).unwrap();
    assert_eq!(c.value(), 0.3);
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "animate_to toward a SMALLER value still ends Completed"
    );

    c.set_value(0.1);
    c.animate_back(0.3, Some(Duration::ZERO)).unwrap();
    assert_eq!(c.value(), 0.3);
    assert_eq!(
        c.status(),
        AnimationStatus::Dismissed,
        "animate_back toward a LARGER value still ends Dismissed"
    );
    c.dispose();
}

/// `animate_to`'s status is `Forward` regardless of whether `target` is
/// above or below the current value — a REAL (non-settling) run, so the
/// transient running status is observable before the run completes.
#[test]
fn animate_to_below_the_current_value_runs_forward() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.7);

    c.animate_to(0.3, Some(Duration::from_millis(100))).unwrap();
    assert_eq!(
        c.status(),
        AnimationStatus::Forward,
        "animate_to is Forward regardless of travel direction"
    );

    c.tick_at(0.1);
    assert_eq!(c.value(), 0.3);
    assert_eq!(c.status(), AnimationStatus::Completed);
    c.dispose();
}

/// The mirror of [`animate_to_below_the_current_value_runs_forward`] for
/// `animate_back`.
#[test]
fn animate_back_above_the_current_value_runs_reverse() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.1);

    c.animate_back(0.3, Some(Duration::from_millis(100)))
        .unwrap();
    assert_eq!(
        c.status(),
        AnimationStatus::Reverse,
        "animate_back is Reverse regardless of travel direction"
    );

    c.tick_at(0.1);
    assert_eq!(c.value(), 0.3);
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    c.dispose();
}

/// Run-end status is the run's direction, with **no bound check**:
/// `animate_to(lower_bound)` from mid-range still ends `Completed`.
/// Flutter's `_tick` rule (`animation_controller.dart:948-950` @ 3.44.0).
#[test]
fn animate_to_the_lower_bound_ends_completed() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(0.5);

    c.animate_to(0.0, Some(Duration::from_millis(100))).unwrap();
    c.tick_at(0.1);

    assert_eq!(c.value(), 0.0);
    assert_eq!(
        c.status(),
        AnimationStatus::Completed,
        "the run's direction was Forward, so it ends Completed even \
         though the value landed on the LOWER bound"
    );
    c.dispose();
}

/// Order pin for a zero-duration displacement: the new run's status must
/// still be observed BEFORE the displaced run's cancellation, exactly
/// like a real-duration displacement
/// (`a_new_runs_status_listener_fires_before_the_displaced_runs_cancellation`).
#[test]
fn a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    let c = AnimationController::new(Duration::from_millis(100), &scheduler);
    let first = c.forward().unwrap(); // a real, still-pending run

    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let order_for_status = Arc::clone(&order);
    c.add_status_listener(Arc::new(move |_status| {
        order_for_status.lock().push("new_run_status");
    }));
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
    c.dispose();
}

/// A settle that does not move the value must not fire value listeners —
/// Flutter: `if (value != target) { …; notifyListeners(); }`
/// (`animation_controller.dart:675-678`).
#[test]
fn a_zero_distance_settle_does_not_notify_value_listeners() {
    let _serial = serial();
    let c = controller(100);
    c.set_value(1.0); // already at the upper bound

    let value_fires = Arc::new(AtomicUsize::new(0));
    let vf = Arc::clone(&value_fires);
    c.add_listener(Arc::new(move || {
        vf.fetch_add(1, Ordering::SeqCst);
    }));

    c.forward().unwrap(); // zero-distance settle: the value does not move

    assert_eq!(
        value_fires.load(Ordering::SeqCst),
        0,
        "a settle that does not move the value must not notify value listeners"
    );
    c.dispose();
}

/// `dispose()` mid-run leaves `status` untouched (Flutter parity) — a
/// proxy reading `status()` on replay must see the status the run had,
/// not a manufactured settle. The frame-loop leak that would otherwise
/// follow is closed on `tick_at` instead: it is a no-op after dispose.
#[test]
fn a_disposed_controller_neither_ticks_nor_holds_the_frame_loop() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    c.forward().unwrap();
    c.tick_at(0.05); // mid-run
    let status_before = c.status();
    assert_eq!(status_before, AnimationStatus::Forward, "sanity: mid-run");

    c.dispose();
    assert_eq!(
        c.status(),
        status_before,
        "dispose() must leave status untouched"
    );

    let value_before = c.value();
    c.tick_at(0.10);
    assert_eq!(
        c.value(),
        value_before,
        "tick_at after dispose must not advance the value"
    );
    assert_eq!(
        c.status(),
        status_before,
        "tick_at after dispose must not change status either"
    );
}

/// `tick_at` after a mid-run `set_value` must be a no-op — `set_value`
/// calls `stop_running()` (clearing `active_run`) but reports a
/// directional *running* status at an interior value (Flutter parity,
/// `settled_status_keep_direction`), so `status.is_running()` alone
/// cannot tell "a run is installed" from "set_value just stopped one and
/// reported a running-looking status anyway". `tick_at`'s guard must
/// read `active_run.is_none()`, not `!status.is_running()`.
///
/// Red-check: revert `tick_at`'s guard to `!inner.status.is_running()` —
/// the final assertion sees `1.0`, not `0.2` (the stopped run's own
/// `start_value..target_value` recomputed at `t = 1.0`).
#[test]
fn tick_at_after_set_value_mid_run_is_a_no_op() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    c.forward().unwrap();
    c.tick_at(0.05);
    assert!(
        (c.value() - 0.5).abs() < 1e-3,
        "sanity: halfway through the run, got {}",
        c.value()
    );

    c.set_value(0.2);
    assert_eq!(
        c.status(),
        AnimationStatus::Forward,
        "sanity: set_value at an interior value keeps the directional running status"
    );

    c.tick_at(0.10);
    assert_eq!(
        c.value(),
        0.2,
        "tick_at after a mid-run set_value must be a no-op, not recompute \
         from the run set_value already stopped"
    );
    c.dispose();
}

/// flutter#1913: status coalescing must not swallow an intermediate
/// status. `forward()` from the lower bound with no tick between calls
/// reports `Forward` (a real run: distance and duration are both
/// nonzero); the immediately-following `reverse()` finds the value still
/// at the lower bound (nothing ticked) and settles `Dismissed` at zero
/// distance. The net value/status end up back where they started, but
/// both intermediate transitions must still be delivered.
#[test]
fn forward_then_reverse_with_no_tick_delivers_the_intermediate_status() {
    let _serial = serial();
    let c = controller(100);

    let statuses: Arc<Mutex<Vec<AnimationStatus>>> = Arc::new(Mutex::new(Vec::new()));
    let s2 = Arc::clone(&statuses);
    c.add_status_listener(Arc::new(move |status| s2.lock().push(status)));

    c.forward().unwrap();
    c.reverse().unwrap();

    assert_eq!(
        statuses.lock().as_slice(),
        &[AnimationStatus::Forward, AnimationStatus::Dismissed],
        "coalescing must not drop the intermediate Forward just because \
         the net status ends back at Dismissed"
    );
    c.dispose();
}

/// A trivially-finished [`Simulation`] test double: `is_done` is true
/// from the first tick, so `tick_simulation`'s completion branch runs
/// immediately without needing a real spring to settle.
struct InstantSimulation {
    value: f32,
}

impl Simulation for InstantSimulation {
    fn x(&self, _time: f32) -> f32 {
        self.value
    }
    fn dx(&self, _time: f32) -> f32 {
        0.0
    }
    fn is_done(&self, _time: f32) -> bool {
        true
    }
    fn tolerance(&self) -> Tolerance {
        Tolerance::DEFAULT
    }
}

#[test]
fn simulation_run_future_resolves_ok_when_the_simulation_finishes() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.animate_with(InstantSimulation { value: 0.5 }).unwrap();
    assert!(future.is_pending());

    c.tick_at(0.0);

    assert!(
        future.is_complete(),
        "tick_simulation's is_done branch must complete the run's future"
    );
    c.dispose();
}

#[test]
fn finite_repeat_future_completes_when_the_count_is_exhausted() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(10));
    let future = c
        .repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
        .unwrap();
    assert!(future.is_pending());

    c.tick_at(0.010); // first cycle retires; one more to go
    assert!(future.is_pending(), "one of two cycles is not exhaustion");
    c.tick_at(0.020); // second cycle retires -> exhausted
    assert!(
        future.is_complete(),
        "a finite repeat must complete its future once its count is exhausted"
    );
    c.dispose();
}

#[test]
fn infinite_repeat_future_only_resolves_via_stop() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(10));
    let future = c.repeat(false).unwrap();
    assert!(future.is_pending());

    // Several cycles retire; an infinite repeat has no natural end.
    c.tick_at(0.010);
    c.tick_at(0.020);
    c.tick_at(0.100);
    assert!(
        future.is_pending(),
        "an infinite repeat's future must stay pending through any \
         number of retired cycles"
    );

    c.stop().unwrap();
    assert!(
        future.is_canceled(),
        "stop() is the only thing that resolves an infinite repeat's future"
    );
    c.dispose();
}

#[test]
fn stop_cancels_the_active_run() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();
    c.stop().unwrap();
    assert!(future.is_canceled(), "stop() must cancel the run in flight");
    c.dispose();
}

#[test]
fn set_value_cancels_the_active_run() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();
    c.set_value(0.3);
    assert!(
        future.is_canceled(),
        "set_value() must cancel the run in flight"
    );
    c.dispose();
}

#[test]
fn reset_cancels_the_active_run() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();
    c.reset().unwrap();
    assert!(
        future.is_canceled(),
        "reset() must cancel the run in flight"
    );
    c.dispose();
}

#[test]
fn dispose_cancels_the_active_run() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();
    c.dispose();
    assert!(
        future.is_canceled(),
        "dispose() must cancel the run in flight"
    );
}

#[test]
fn a_new_runs_status_listener_fires_before_the_displaced_runs_cancellation() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let first = c.forward().unwrap();

    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let order_for_status = Arc::clone(&order);
    c.add_status_listener(Arc::new(move |_status| {
        order_for_status.lock().push("new_run_status");
    }));
    let order_for_cancel = Arc::clone(&order);
    first.when_complete_or_cancel(move |_outcome| {
        order_for_cancel.lock().push("displaced_run_canceled");
    });

    c.reverse().unwrap();

    assert_eq!(
        order.lock().as_slice(),
        &["new_run_status", "displaced_run_canceled"],
        "the new run's status must be observed before the displaced \
         run's cancellation is delivered"
    );
    c.dispose();
}

#[test]
fn zero_distance_start_returns_a_complete_future_and_cancels_the_displaced_run() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let first = c.forward().unwrap(); // 0.0 -> 1.0, a real run
    c.tick_at(0.05); // partway; still pending
    assert!(first.is_pending());

    // Already at the target -> the zero-distance settle path, no new
    // ticker run.
    let settled = c.forward_from(Some(1.0)).unwrap();

    assert!(
        settled.is_complete(),
        "a zero-distance start must return an already-complete future"
    );
    assert!(
        first.is_canceled(),
        "the zero-distance settle must still cancel whatever run it displaced"
    );
    c.dispose();
}

#[test]
fn every_delivery_runs_with_the_controller_lock_free() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let inner = Arc::clone(&c.inner);
    let future = c.forward().unwrap();

    let observed = Arc::new(Mutex::new(None));
    let observed2 = Arc::clone(&observed);
    future.when_complete_or_cancel(move |_outcome| {
        // PORT-CHECK-OK-LOCK: plain data, no significant drop
        *observed2.lock() = Some(inner.try_lock().is_some());
    });

    c.tick_at(0.1);

    assert_eq!(
        observed.lock().as_ref(),
        Some(&true),
        "a delivery must run with the controller's own lock free — the \
         finish chokepoint drops it before calling deliver()"
    );
    c.dispose();
}

#[test]
fn a_completed_listener_that_starts_a_new_run_leaves_the_finished_run_ok() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let first = c.forward().unwrap();

    let chained = c.clone();
    let restarted = Arc::new(AtomicUsize::new(0));
    let restart_flag = Arc::clone(&restarted);
    c.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Completed && restart_flag.fetch_add(1, Ordering::SeqCst) == 0
        {
            chained.forward_from(Some(0.0)).unwrap();
        }
    }));

    c.tick_at(0.1); // completes `first`; the listener above chains a new run

    assert_eq!(
        restarted.load(Ordering::SeqCst),
        1,
        "sanity: the listener must have chained a restart"
    );
    assert!(
        first.is_complete(),
        "the finished run's own future must resolve Ok even though a \
         listener started a new run before delivery ran"
    );
    c.dispose();
}

#[test]
fn a_panicking_status_listener_leaves_the_finished_run_ok() {
    let _serial = serial();
    let c = AnimationController::without_ticker(Duration::from_millis(100));
    let future = c.forward().unwrap();

    // Registered on the FUTURE, not the controller: this only runs if
    // `TickerDelivery` actually delivers. `future.is_complete()` alone
    // (the durable state `publish` writes) would stay green even with
    // `Drop for TickerDelivery` emptied out, since `finish` never
    // reaches its own `delivery.deliver()` line when `fire_status`
    // panics — only the unwind dropping the `delivery` parameter runs
    // it. This continuation is the oracle for that drop actually firing.
    let seen = Arc::new(Mutex::new(None));
    let seen2 = Arc::clone(&seen);
    future.when_complete_or_cancel(move |outcome| {
        // PORT-CHECK-OK-LOCK: plain data, no significant drop
        *seen2.lock() = Some(outcome);
    });

    c.add_status_listener(Arc::new(|status| {
        assert!(
            status != AnimationStatus::Completed,
            "a status listener panics on completion"
        );
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.tick_at(0.1);
    }));

    assert!(
        result.is_err(),
        "the listener's panic must propagate out of tick_at"
    );
    assert_eq!(
        *seen.lock(),
        Some(Ok(())),
        "TickerDelivery must still deliver on drop through the unwind, \
         running the continuation with the outcome published before \
         the panicking listener ran"
    );
    assert!(future.is_complete());
    c.dispose();
}

#[test]
fn when_complete_or_cancel_chaining_ticks_once_per_frame_and_stop_fully_stops_it() {
    let _serial = serial();
    let scheduler = UpdateScheduler::new();
    // A REAL (1ms) base duration, not `Duration::ZERO`: a zero-duration
    // `reverse()` completes AT THE CALL (issue #1171) and fires this
    // `when_complete_or_cancel` continuation at REGISTRATION time,
    // before `execute_frame()` ever runs (see
    // `zero_duration_reverse_settles_dismissed_at_the_call`), which
    // would collapse the two-frame structure this test relies on
    // (per-frame tick counts, `stop()` canceling a chain still in
    // flight). A short real duration keeps the first leg genuinely
    // pending across the `sleep` + `execute_frame()` pump below, exactly
    // like the sibling status-listener version of this test
    // (`status_listener_chaining_forward_ticks_once_per_frame_and_stop_fully_stops_it`).
    // The chained leg below still takes its own explicit 10s duration
    // regardless of this controller's base duration, so it is still in
    // flight when `stop()` cancels it.
    let c = AnimationController::new(Duration::from_millis(1), &scheduler);
    c.set_value(1.0);

    let tick_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&tick_count);
    c.add_listener(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    let restarted = Arc::new(AtomicUsize::new(0));
    let restart_flag = Arc::clone(&restarted);
    let chained = c.clone();
    let future = c.reverse().unwrap();
    future.when_complete_or_cancel(move |outcome| {
        if outcome.is_ok() && restart_flag.fetch_add(1, Ordering::SeqCst) == 0 {
            // A LONG chained run on purpose — see the sibling
            // status-listener version of this test for why.
            chained
                .animate_to(1.0, Some(Duration::from_secs(10)))
                .unwrap();
        }
    });

    std::thread::sleep(Duration::from_millis(5));
    scheduler.execute_frame();

    assert_eq!(
        restarted.load(Ordering::SeqCst),
        1,
        "sanity: the first run must complete and the continuation must \
         have chained a restart"
    );
    assert_eq!(
        scheduler.transient_callback_count(),
        1,
        "the chained restart must leave exactly ONE live tick registration"
    );

    std::thread::sleep(Duration::from_millis(5));
    scheduler.execute_frame();

    assert_eq!(
        tick_count.load(Ordering::SeqCst),
        2,
        "exactly one value notification per frame across both runs"
    );
    assert_eq!(scheduler.transient_callback_count(), 1);

    c.stop().unwrap();
    assert_eq!(
        scheduler.transient_callback_count(),
        0,
        "stop() must fully cancel the single live tick chain"
    );
    assert!(!c.is_animating());

    scheduler.execute_frame();
    assert_eq!(tick_count.load(Ordering::SeqCst), 2);

    c.dispose();
}

/// Source guard: `TickerDelivery::deliver` must be called from exactly
/// one place — inside `AnimationController::finish` — and no
/// `TickerCompleter`/`TickerDelivery`-producing call
/// (`.complete()`/`.cancel()`/`stop_running(`/`.map(TickerCompleter::..)`)
/// or a direct `active_run = ` assignment may appear anywhere else
/// without being bound to a name (a `let _ = ..`, a bare unbound
/// statement, or either wrapped in `drop(..)`) — every one of those
/// shapes delivers (or drops a completer that would have delivered)
/// under whatever lock is live at that statement instead of routing
/// through `finish`.
///
/// Checked per STATEMENT, not per line: physical lines are stripped of
/// `//` comments (never `://`) and doc-comment-only lines are dropped
/// entirely, then accumulated until a `;`, `{`, or `}` is seen. Only a
/// `;`-terminated accumulation is a real statement — a bare tail
/// expression (no trailing `;`, e.g. `stop_running`'s own
/// `self.active_run.take().map(TickerCompleter::cancel)` return value)
/// is a function's return, not a discard, and is excluded by
/// construction: it never accumulates a trailing `;` of its own before
/// the enclosing `}` ends the accumulation instead. This is what makes a
/// rustfmt-wrapped `let _ = inner\n    .active_run\n    .take()\n    .map(TickerCompleter::cancel);`
/// visible as one unit regardless of where the formatter broke the
/// lines (whitespace before a `.` is removed after joining, so a wrapped
/// `.active_run\n.take()` still reads `active_run.take(`), which a
/// per-line check cannot see.
///
/// Known limit, by construction: a `{` or `}` ends an accumulation
/// without checking it, so a tracked call that sits to the LEFT of a
/// brace in the same statement — `if let Some(old) = inner.active_run.take() { .. }`,
/// `match inner.active_run.take() { .. }` — is not seen. Those shapes are
/// bound (the value has a name inside the block), so they are outside
/// what this guard claims; do not cite it against them.
#[test]
fn ticker_completer_resolution_never_bypasses_the_finish_chokepoint() {
    // `controller.rs`'s test module now lives in this sibling file
    // (`#[path = "controller_tests.rs"] mod tests;`), so `include_str!` here
    // reads ONLY production code -- no boundary to find, and no risk of this
    // test's own body (which spells out the exact patterns it searches for,
    // in match strings and panic messages) flagging itself.
    let production = include_str!("controller.rs");

    // `.deliver()` is called from exactly one place: inside `finish`.
    let deliver_count = production.matches(".deliver()").count();
    assert_eq!(
        deliver_count, 1,
        "`.deliver()` must be called from exactly one place in this file \
         (AnimationController::finish); found {deliver_count} call site(s)"
    );
    let finish_start = production
        .find("fn finish(")
        .expect("AnimationController::finish must exist");
    let finish_body_start = production[finish_start..]
        .find('{')
        .map(|i| finish_start + i)
        .expect("fn finish must have a body");
    let mut depth = 0i32;
    let mut finish_body_end = None;
    for (i, ch) in production[finish_body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    finish_body_end = Some(finish_body_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let finish_body_end = finish_body_end.expect("fn finish's body braces must balance");
    let deliver_pos = production
        .find(".deliver()")
        .expect("just counted at least one occurrence above");
    assert!(
        (finish_body_start..=finish_body_end).contains(&deliver_pos),
        "the one `.deliver()` call must be inside `AnimationController::finish`'s \
         own body (byte range {finish_body_start}..={finish_body_end}), found at \
         byte {deliver_pos}"
    );

    // Tracked call shapes that must never appear unbound.
    let tracked: [&str; 6] = [
        ".complete()",
        ".cancel()",
        "stop_running(",
        ".map(TickerCompleter::",
        "active_run.replace(",
        "active_run.take(",
    ];

    let mut buffer = String::new();
    for line in production.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue; // prose, not code — never joined in
        }
        // Strip a trailing `//` comment, but not a `://` inside a URL.
        let bytes = line.as_bytes();
        let mut code_part = line;
        let mut search_from = 0;
        while let Some(rel) = line[search_from..].find("//") {
            let at = search_from + rel;
            if at > 0 && bytes[at - 1] == b':' {
                search_from = at + 2;
                continue;
            }
            code_part = &line[..at];
            break;
        }

        for ch in code_part.chars() {
            buffer.push(ch);
            if ch == ';' || ch == '{' || ch == '}' {
                let statement: String = if ch == ';' {
                    // Re-join a wrapped method chain so `.active_run\n.take()`
                    // reads `active_run.take(` again: the tracked patterns
                    // are written without whitespace before the `.`.
                    buffer[..buffer.len() - 1]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .replace(" .", ".")
                } else {
                    String::new() // `{`/`}` boundary: not a value statement
                };
                buffer.clear();

                if statement.is_empty() {
                    continue;
                }
                let is_bound = statement.starts_with("let ");

                let discards_via_let_underscore = statement.starts_with("let _ =")
                    && tracked.iter().any(|pat| statement.contains(pat));
                assert!(
                    !discards_via_let_underscore,
                    "discards a TickerCompleter/TickerDelivery result with \
                     `let _ =`, which delivers under whatever lock is held at \
                     this statement instead of going through \
                     `AnimationController::finish`: {statement:?}"
                );

                // Covers a bare `foo().map(TickerCompleter::cancel);`, a
                // bare `stop_running();`, and either wrapped in
                // `drop(..)` — none of them bind the result anywhere.
                let bare_discard = !is_bound && tracked.iter().any(|pat| statement.contains(pat));
                assert!(
                    !bare_discard,
                    "calls a TickerCompleter/TickerDelivery-producing operation \
                     with no binding at all, delivering under whatever lock is \
                     held at this statement instead of going through \
                     `AnimationController::finish`: {statement:?}"
                );

                let direct_assignment = statement.contains("active_run = ");
                assert!(
                    !direct_assignment,
                    "assigns `active_run` directly with `=`, which drops \
                     whatever completer was there before, inline, under \
                     whatever lock is held at this statement instead of \
                     routing it through `.replace()` + \
                     `AnimationController::finish`: {statement:?}"
                );
            }
        }
        buffer.push(' '); // preserve the line break as whitespace
    }
}
