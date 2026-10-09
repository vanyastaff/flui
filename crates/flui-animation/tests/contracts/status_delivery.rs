//! Status delivery: every listener sees each committed status in commit order,
//! a failing callback or controller lets its round finish and re-raises the
//! first panic afterwards, and a hop or a parent swap announces exactly the
//! status of the animation now in charge.
//!
//! Rows that hold today run in the tables. A row whose behaviour is not there
//! yet is its own `#[ignore = "contract: …"]` test, so `--run-ignored` shows
//! it failing on the assertion that names the behaviour.

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, ConstantAnimation,
    DrivenController, ProxyAnimation, StatusCallback, Vsync, curve::Curve,
};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};

use AnimationStatus::{Completed, Dismissed, Forward, Reverse};

type Log = Arc<Mutex<Vec<AnimationStatus>>>;

/// A status listener that appends every status it receives to the log.
fn recorder() -> (Log, StatusCallback) {
    let log = Log::default();
    let sink = Arc::clone(&log);
    let callback: StatusCallback = std::rc::Rc::new(move |status| {
        sink.lock().expect("status log").push(status);
    });
    (log, callback)
}

fn seen(log: &Log) -> Vec<AnimationStatus> {
    log.lock().expect("status log").clone()
}

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::builder(Duration::from_secs(1)).build()
}

fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// A linear curve that panics with `message` the first time it is sampled.
#[derive(Debug)]
struct PanicsOnce {
    message: &'static str,
    armed: AtomicBool,
}

impl Curve for PanicsOnce {
    fn transform(&self, t: f64) -> f64 {
        if self.armed.swap(false, Ordering::SeqCst) {
            panic_any(self.message);
        }
        t
    }
}

/// A controller running `0 → 1` over one second; with `panic`, its curve
/// panics with that message on the first interior frame.
fn running(registry: &Vsync, panic: Option<&'static str>) -> DrivenController {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(registry));
    let controller = owner.controller();
    let _run = match panic {
        Some(message) => controller.animate_to_curved(
            1.0,
            Some(Duration::from_secs(1)),
            Arc::new(PanicsOnce {
                message,
                armed: AtomicBool::new(true),
            }),
        ),
        None => controller.forward(),
    }
    .expect("run starts");
    owner
}

// --- callback failures -----------------------------------------------------------
//
// One policy for every callback a controller runs: the transition is committed
// before the call, the round finishes, the first panic is re-raised after the
// round, and the next operation proceeds normally.

fn panicking_status_listener_does_not_starve_later_listeners() {
    let controller = controller();
    controller.add_status_listener(std::rc::Rc::new(|_| panic_any("status listener")));
    let (second, listener) = recorder();
    controller.add_status_listener(listener);
    let (third, listener) = recorder();
    controller.add_status_listener(listener);

    let started = catch_unwind(AssertUnwindSafe(|| controller.forward()));

    assert_eq!(
        seen(&second),
        [Forward],
        "a later listener still gets the status"
    );
    assert_eq!(
        seen(&third),
        [Forward],
        "every later listener gets the status"
    );
    let payload = started.expect_err("the panic is re-raised after the round");
    assert_eq!(payload_text(payload.as_ref()), Some("status listener"));
    assert_eq!(
        controller.status(),
        Forward,
        "the run was committed before the call"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_eq!(controller.value(), 0.5, "the next frame ticks the run");
}

fn competing_status_listener_panics_re_raise_the_first() {
    let controller = controller();
    controller.add_status_listener(std::rc::Rc::new(|_| panic_any("first")));
    controller.add_status_listener(std::rc::Rc::new(|_| panic_any("second")));
    let (last, listener) = recorder();
    controller.add_status_listener(listener);

    let started = catch_unwind(AssertUnwindSafe(|| controller.forward()));

    assert_eq!(
        seen(&last),
        [Forward],
        "the round finishes past both failures"
    );
    let payload = started.expect_err("a panic is re-raised after the round");
    assert_eq!(
        payload_text(payload.as_ref()),
        Some("first"),
        "the first payload is re-raised, the second is retained"
    );
}

/// A value listener that records how often it ran.
fn value_counter(controller: &AnimationController) -> Arc<Mutex<usize>> {
    let count = Arc::new(Mutex::new(0));
    let sink = Arc::clone(&count);
    controller.add_listener(std::rc::Rc::new(move || {
        *sink.lock().expect("value count") += 1;
    }));
    count
}

fn panicking_value_listener_is_re_raised_after_the_round() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.add_listener(std::rc::Rc::new(|| panic_any("value listener")));
    let later = value_counter(&controller);

    let ticked = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    }));

    assert_eq!(
        *later.lock().expect("value count"),
        1,
        "a later value listener runs"
    );
    assert_eq!(
        controller.value(),
        0.5,
        "the sample was committed before the call"
    );
    let payload = ticked.expect_err("the panic is re-raised after the round");
    assert_eq!(payload_text(payload.as_ref()), Some("value listener"));
}

fn competing_value_listener_panics_re_raise_the_first() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.add_listener(std::rc::Rc::new(|| panic_any("first")));
    controller.add_listener(std::rc::Rc::new(|| panic_any("second")));
    let later = value_counter(&controller);

    let ticked = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    }));

    assert_eq!(*later.lock().expect("value count"), 1, "the round finishes");
    let payload = ticked.expect_err("a panic is re-raised after the round");
    assert_eq!(payload_text(payload.as_ref()), Some("first"));
}

fn panicking_continuation_during_dispose_finishes_the_dispose() {
    let vsync = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let run = controller.forward().expect("run starts");
    run.when_complete_or_cancel(|_| panic_any("first"));
    let outcome = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&outcome);
    run.when_complete_or_cancel(move |end| {
        *sink.lock().expect("continuation outcome") = Some(end.is_err());
    });

    let disposed = catch_unwind(AssertUnwindSafe(|| owner.dispose()));

    let payload = disposed.expect_err("the continuation's panic is re-raised");
    assert_eq!(payload_text(payload.as_ref()), Some("first"));
    assert_eq!(
        *outcome.lock().expect("continuation outcome"),
        Some(true),
        "a later continuation still learns the run was canceled"
    );
    assert!(
        controller.forward().is_err(),
        "the controller stays disposed"
    );
    assert!(
        !vsync.has_running(),
        "a disposed controller holds no frame open"
    );
}

fn panicking_status_listener_leaves_the_next_frame_ticking() {
    let controller = controller();
    let (log, record) = recorder();
    let armed = AtomicBool::new(true);
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        if armed.swap(false, Ordering::SeqCst) {
            panic_any("first status");
        }
        record(status);
    }));

    let _ = catch_unwind(AssertUnwindSafe(|| controller.forward()));
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_eq!(
        controller.value(),
        0.5,
        "the run advances on the next frame"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(controller.status(), Completed);
    assert_eq!(
        seen(&log),
        [Completed],
        "the listener that panicked stays subscribed"
    );
}

// --- the Vsync walk ------------------------------------------------------------

fn vsync_walk_ticks_siblings_after_a_panicking_controller() {
    let vsync = Vsync::new();
    let _first = running(&vsync, Some("first"));
    let _second = running(&vsync, Some("second"));
    let sibling = running(&vsync, None);
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );

    let walk = catch_unwind(AssertUnwindSafe(|| {
        vsync.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
        );
    }));

    assert_eq!(
        sibling.controller().value(),
        0.5,
        "a controller after a panicking one is ticked in the same frame"
    );
    let payload = walk.expect_err("the walk re-raises the contained panic");
    assert_eq!(
        payload_text(payload.as_ref()),
        Some("first"),
        "the first payload is the one re-raised"
    );
}

fn vsync_walk_ticks_the_parent_after_a_panicking_child_registry() {
    let parent = Vsync::new();
    let own = running(&parent, None);
    let child = Vsync::new();
    let _child = parent.attach_child(&child).expect("child attaches");
    let _failing = running(&child, Some("child"));
    parent.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );

    let walk = catch_unwind(AssertUnwindSafe(|| {
        parent.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
        );
    }));

    assert_eq!(
        own.controller().value(),
        0.5,
        "the parent's own controller is ticked after a child registry panicked"
    );
    let payload = walk.expect_err("the walk re-raises the contained panic");
    assert_eq!(payload_text(payload.as_ref()), Some("child"));
}

fn vsync_walk_after_a_panic_ticks_every_controller() {
    let vsync = Vsync::new();
    let failing = running(&vsync, Some("once"));
    let sibling = running(&vsync, None);
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );
    let _ = catch_unwind(AssertUnwindSafe(|| {
        vsync.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.5)),
        );
    }));

    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.75)),
    );

    assert_eq!(
        failing.controller().value(),
        0.75,
        "the controller that panicked ticks again"
    );
    assert_eq!(
        sibling.controller().value(),
        0.75,
        "its sibling ticks on the next frame"
    );
}

// --- removal and disposal during a fan-out -----------------------------------

fn status_listener_removed_by_an_earlier_listener_is_skipped() {
    let controller = controller();
    let target: std::rc::Rc<Mutex<Option<(AnimationController, ListenerId)>>> =
        std::rc::Rc::default();
    let pending = std::rc::Rc::clone(&target);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        let removal = pending.lock().expect("removal slot").take();
        if let Some((controller, id)) = removal {
            controller.remove_status_listener(id);
        }
    }));
    let (removed, listener) = recorder();
    let id = controller.add_status_listener(listener);
    *target.lock().expect("removal slot") = Some((controller.clone(), id));

    let _run = controller.forward().expect("run starts");

    assert_eq!(
        seen(&removed),
        [],
        "a listener removed earlier in the same fan-out is not called"
    );
}

fn controller_disposed_mid_fan_out_calls_no_further_listener() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let slot: std::rc::Rc<Mutex<Option<DrivenController>>> = std::rc::Rc::default();
    let pending = std::rc::Rc::clone(&slot);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        let owner = pending.lock().expect("dispose slot").take();
        if let Some(mut owner) = owner {
            owner.dispose();
        }
    }));
    let (later, listener) = recorder();
    controller.add_status_listener(listener);
    *slot.lock().expect("dispose slot") = Some(owner);

    let _run = controller.forward().expect("run starts");

    assert_eq!(
        seen(&later),
        [],
        "no listener of a disposed controller is called"
    );
    assert!(
        vsync.is_empty(),
        "callback disposal withdraws the owning seat"
    );
    assert!(matches!(
        controller.forward(),
        Err(flui_animation::AnimationError::Disposed)
    ));
}

// --- reentrant ordering --------------------------------------------------------

fn reversing_on_completed_keeps_commit_order() {
    let controller = controller();
    let slot: std::rc::Rc<Mutex<Option<AnimationController>>> = std::rc::Rc::default();
    let pending = std::rc::Rc::clone(&slot);
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        if status == Completed {
            let owner = pending.lock().expect("reverse slot").take();
            if let Some(controller) = owner {
                let _run = controller.reverse().expect("reverse from a listener");
            }
        }
    }));
    let (observer, listener) = recorder();
    controller.add_status_listener(listener);
    *slot.lock().expect("reverse slot") = Some(controller.clone());

    let _run = controller.forward().expect("run starts");
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));

    assert_eq!(controller.status(), Reverse);
    assert_eq!(
        seen(&observer),
        [Forward, Completed, Reverse],
        "a later listener observes statuses in commit order, ending at status()"
    );
}

fn reentrant_transitions_keep_the_return_to_the_original_status() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let slot = std::rc::Rc::new(Mutex::new(Some(controller.clone())));
    let pending = std::rc::Rc::clone(&slot);
    let (late, late_listener) = recorder();
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        if status == Forward {
            let owner = pending.lock().expect("reentrant owner").take();
            if let Some(owner) = owner {
                owner.add_status_listener(std::rc::Rc::clone(&late_listener));
                let _reverse = owner.reverse_from(Some(0.5)).expect("nested reverse");
                let _forward = owner.forward().expect("nested forward");
            }
        }
    }));
    let (observer, listener) = recorder();
    controller.add_status_listener(listener);

    let _run = controller.forward().expect("outer forward");

    assert_eq!(seen(&observer), [Forward, Reverse, Forward]);
    assert_eq!(
        seen(&late),
        [Reverse, Forward],
        "a late subscriber misses only the already committed round"
    );
    assert_eq!(controller.status(), Forward);
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_eq!(
        controller.value(),
        1.0,
        "the last admitted run survives delivery"
    );
    lifecycle.dispose();
}

fn reentrant_completion_delivers_its_outcome_before_the_next_status() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let run = controller.forward().expect("original run");
    let order = Arc::new(Mutex::new(Vec::new()));
    let slot = std::rc::Rc::new(Mutex::new(Some(controller.clone())));
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        if status == Completed {
            let owner = slot.lock().expect("reverse owner").take();
            if let Some(owner) = owner {
                let _next = owner.reverse().expect("replacement run");
                panic_any("completion listener");
            }
        }
    }));
    let sink = Arc::clone(&order);
    controller.add_status_listener(std::rc::Rc::new(move |status| {
        sink.lock().expect("delivery order").push(match status {
            Completed => "completed status",
            Reverse => "reverse status",
            Dismissed => "dismissed status",
            _ => "unexpected status",
        });
    }));
    let sink = Arc::clone(&order);
    run.when_complete_or_cancel(move |outcome| {
        assert!(outcome.is_ok(), "replacement cannot cancel a completed run");
        sink.lock()
            .expect("delivery order")
            .push("completed outcome");
    });

    let failure = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    }))
    .expect_err("first listener failure is resumed after delivery");
    assert_eq!(payload_text(failure.as_ref()), Some("completion listener"));
    assert_eq!(
        *order.lock().expect("delivery order"),
        ["completed status", "completed outcome", "reverse status"]
    );
    controller.tick_at(std::time::Duration::from_secs_f64(0.5));
    assert_eq!(
        controller.value(),
        0.5,
        "replacement advances on the next frame"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(
        *order.lock().expect("delivery order"),
        [
            "completed status",
            "completed outcome",
            "reverse status",
            "dismissed status"
        ]
    );
    lifecycle.dispose();
}

struct DropFailure(Arc<AtomicUsize>);

impl Drop for DropFailure {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic_any("opaque destructor failure");
    }
}

struct HostileOwnership {
    _first: DropFailure,
    _second: DropFailure,
}

fn status_failure_retains_removed_captures_and_competing_payloads() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let armed = AtomicBool::new(true);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        if armed.swap(false, Ordering::SeqCst) {
            panic_any("first status failure");
        }
    }));
    let capture = HostileOwnership {
        _first: DropFailure(Arc::clone(&drops)),
        _second: DropFailure(Arc::clone(&drops)),
    };
    let payload_drops = Arc::clone(&drops);
    let failed = controller.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &capture;
        panic_any(HostileOwnership {
            _first: DropFailure(Arc::clone(&payload_drops)),
            _second: DropFailure(Arc::clone(&payload_drops)),
        });
    }));
    let removal = std::rc::Rc::new(Mutex::new(None));
    let pending = std::rc::Rc::clone(&removal);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        let removal = pending.lock().expect("capture removal").take();
        if let Some((owner, removed)) = removal {
            let owner: AnimationController = owner;
            owner.remove_status_listener(failed);
            owner.remove_status_listener(removed);
        }
    }));
    let capture = HostileOwnership {
        _first: DropFailure(Arc::clone(&drops)),
        _second: DropFailure(Arc::clone(&drops)),
    };
    let removed = controller.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &capture;
        panic_any("removed listener must be skipped");
    }));
    *removal.lock().expect("capture removal") = Some((controller.clone(), removed));
    let (tail, listener) = recorder();
    controller.add_status_listener(listener);

    let failure = catch_unwind(AssertUnwindSafe(|| controller.forward()))
        .expect_err("first failure resumes after the healthy tail");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(seen(&tail), [Forward]);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "opaque captures and competing payloads retain failure custody"
    );
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(
        seen(&tail),
        [Forward, Completed],
        "the next frame still delivers"
    );
    lifecycle.dispose();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn status_delivery_failure_custody() {
    let rows: &[(&str, fn())] = &[
        (
            "wrapper_status",
            wrapper_status_keeps_parent_failure_custody,
        ),
        ("wrapper_value", wrapper_value_keeps_parent_failure_custody),
        ("curved_status", curved_status_keeps_parent_failure_custody),
        ("curved_value", curved_value_keeps_parent_failure_custody),
        ("tween_status", tween_status_keeps_parent_failure_custody),
        ("tween_value", tween_value_keeps_parent_failure_custody),
        ("proxy_status", proxy_status_keeps_parent_failure_custody),
        ("proxy_value", proxy_value_keeps_parent_failure_custody),
        ("switch_status", switch_status_keeps_parent_failure_custody),
        ("switch_value", switch_value_keeps_parent_failure_custody),
        ("nested_status", nested_status_keeps_parent_failure_custody),
        ("nested_value", nested_value_keeps_parent_failure_custody),
        ("drop_reverse", dropping_reverse_silences_its_tail),
        ("drop_curved", dropping_curved_silences_its_tail),
        ("drop_tween", dropping_tween_silences_its_tail),
        ("drop_proxy", dropping_proxy_silences_its_tail),
        ("drop_switch", dropping_switch_silences_its_tail),
        ("drop_nested", dropping_nested_silences_its_tail),
        (
            "status_to_value",
            status_failure_retains_removed_value_captures,
        ),
        (
            "value_removal",
            value_failure_retains_removed_value_captures,
        ),
        ("value_peer", peer_failure_retains_removed_value_captures),
        (
            "snapshot",
            status_failure_retains_removed_captures_and_competing_payloads,
        ),
        (
            "late_removal",
            status_failure_retains_a_reentrantly_removed_new_subscription,
        ),
        ("run", status_failure_retains_completed_run_captures),
        ("waiter", status_failure_retains_completed_run_waiter),
        (
            "run_competition",
            status_failure_remains_authoritative_over_run_failure,
        ),
        ("sibling", frame_failure_retains_a_siblings_run_captures),
        ("child", child_failure_retains_the_parents_run_captures),
    ];
    if let Some(selected) = child_process::selected_case() {
        let (_, row) = rows
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("selected row");
        row();
        child_process::pass();
    }
    let names: Vec<_> = rows.iter().map(|(name, _)| *name).collect();
    child_process::run_rows("status_delivery::status_delivery_failure_custody", &names);
}

fn status_failure_retains_removed_value_captures() {
    removed_value_capture_custody("status");
}

fn wrapper_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "reverse");
}

fn wrapper_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "reverse");
}

fn curved_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "curved");
}
fn curved_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "curved");
}
fn tween_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "tween");
}
fn tween_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "tween");
}
fn proxy_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "proxy");
}
fn proxy_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "proxy");
}
fn switch_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "switch");
}
fn switch_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "switch");
}
fn nested_status_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(true, "nested");
}
fn nested_value_keeps_parent_failure_custody() {
    wrapper_keeps_parent_failure_custody(false, "nested");
}

fn dropping_reverse_silences_its_tail() {
    dropping_wrapper_silences_its_tail("reverse");
}
fn dropping_curved_silences_its_tail() {
    dropping_wrapper_silences_its_tail("curved");
}
fn dropping_tween_silences_its_tail() {
    dropping_wrapper_silences_its_tail("tween");
}
fn dropping_proxy_silences_its_tail() {
    dropping_wrapper_silences_its_tail("proxy");
}
fn dropping_switch_silences_its_tail() {
    dropping_wrapper_silences_its_tail("switch");
}
fn dropping_nested_silences_its_tail() {
    dropping_wrapper_silences_its_tail("nested");
}

fn make_wrapper(
    source: std::rc::Rc<dyn Animation<f64>>,
    kind: &str,
) -> std::rc::Rc<dyn Animation<f64>> {
    match kind {
        "reverse" => std::rc::Rc::new(flui_animation::ReverseAnimation::new(source)),
        "curved" => std::rc::Rc::new(flui_animation::CurvedAnimation::new(
            source,
            flui_animation::curve::Linear,
        )),
        "tween" => std::rc::Rc::new(flui_animation::TweenAnimation::new(
            flui_animation::FloatTween::new(0.0, 10.0),
            source,
        )),
        "proxy" => std::rc::Rc::new(flui_animation::ProxyAnimation::new(source)),
        "switch" => std::rc::Rc::new(flui_animation::AnimationSwitch::new(source, None)),
        "nested" => std::rc::Rc::new(flui_animation::ReverseAnimation::new(std::rc::Rc::new(
            flui_animation::ProxyAnimation::new(std::rc::Rc::new(
                flui_animation::CurvedAnimation::new(source, flui_animation::curve::Linear),
            )),
        ))),
        _ => panic!("unknown wrapper case"),
    }
}

fn dropping_wrapper_silences_its_tail(kind: &str) {
    for status in [true, false] {
        for failed in [false, true] {
            let mut lifecycle =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
            let parent = lifecycle.controller().clone();
            let failure_id = failed.then(|| {
                if status {
                    parent.add_status_listener(std::rc::Rc::new(|_| {
                        panic_any("first parent failure")
                    }))
                } else {
                    parent.add_listener(std::rc::Rc::new(|| panic_any("first parent failure")))
                }
            });
            let wrapper = make_wrapper(std::rc::Rc::new(parent.clone()), kind);
            let owner = std::rc::Rc::new(std::cell::RefCell::new(None));
            let retire = owner.clone();
            if status {
                wrapper.add_status_listener(std::rc::Rc::new(move |_| {
                    drop(retire.borrow_mut().take());
                }));
            } else {
                wrapper.add_listener(std::rc::Rc::new(move || drop(retire.borrow_mut().take())));
            }
            let drops = Arc::new(AtomicUsize::new(0));
            let captures = failed.then(|| HostileOwnership {
                _first: DropFailure(drops.clone()),
                _second: DropFailure(drops.clone()),
            });
            let tail = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = tail.clone();
            if status {
                wrapper.add_status_listener(std::rc::Rc::new(move |_| {
                    let _ = &captures;
                    observed.set(observed.get() + 1);
                }));
            } else {
                wrapper.add_listener(std::rc::Rc::new(move || {
                    let _ = &captures;
                    observed.set(observed.get() + 1);
                }));
            }
            *owner.borrow_mut() = Some(wrapper);
            let result = catch_unwind(AssertUnwindSafe(|| {
                if status {
                    parent.forward().expect("run admitted");
                } else {
                    parent.set_value(0.5);
                }
            }));
            if failed {
                let failure = result.expect_err("parent failure is resumed");
                assert_eq!(payload_text(&*failure), Some("first parent failure"));
            } else {
                result.expect("ordinary owner release is healthy");
            }
            assert!(owner.borrow().is_none());
            assert_eq!(
                tail.get(),
                0,
                "last wrapper owner withdraws its tail: {kind}, status={status}, failed={failed}"
            );
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            if let Some(id) = failure_id {
                if status {
                    parent.remove_status_listener(id);
                } else {
                    parent.remove_listener(id);
                }
            }
            if status {
                parent.tick_at(std::time::Duration::from_secs_f64(1.0));
            } else {
                parent.set_value(1.0);
            }
            assert_eq!(
                tail.get(),
                0,
                "retired wrapper receives no next notification"
            );
            lifecycle.dispose();
        }
    }
}

fn wrapper_keeps_parent_failure_custody(status: bool, kind: &str) {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let parent = lifecycle.controller().clone();
    let failure_id = if status {
        parent.add_status_listener(std::rc::Rc::new(|_| panic_any("first parent failure")))
    } else {
        parent.add_listener(std::rc::Rc::new(|| panic_any("first parent failure")))
    };
    let source: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(parent.clone());
    let wrapper = make_wrapper(source, kind);
    let weak = std::rc::Rc::downgrade(&wrapper);
    let drops = Arc::new(AtomicUsize::new(0));
    let captures = Arc::clone(&drops);
    let retire = move || {
        let wrapper = weak.upgrade().expect("wrapper alias");
        let capture = HostileOwnership {
            _first: DropFailure(captures.clone()),
            _second: DropFailure(captures.clone()),
        };
        if status {
            let id = wrapper.add_status_listener(std::rc::Rc::new(move |_| {
                let _ = &capture;
            }));
            wrapper.remove_status_listener(id);
        } else {
            let id = wrapper.add_listener(std::rc::Rc::new(move || {
                let _ = &capture;
            }));
            wrapper.remove_listener(id);
        }
    };
    let removal_id = if status {
        wrapper.add_status_listener(std::rc::Rc::new(move |_| retire()))
    } else {
        wrapper.add_listener(std::rc::Rc::new(retire))
    };
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = calls.clone();
    if status {
        wrapper.add_status_listener(std::rc::Rc::new(move |_| observed.set(observed.get() + 1)));
    } else {
        wrapper.add_listener(std::rc::Rc::new(move || observed.set(observed.get() + 1)));
    }
    let failure = catch_unwind(AssertUnwindSafe(|| {
        if status {
            parent.forward().expect("run admitted");
        } else {
            parent.set_value(0.5);
        }
    }))
    .expect_err("parent failure resumes after wrapper tail");
    assert_eq!(payload_text(&*failure), Some("first parent failure"));
    assert_eq!(calls.get(), 1, "wrapper tail runs after parent failure");
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "wrapper retirement preserves parent custody"
    );
    if status {
        parent.remove_status_listener(failure_id);
        wrapper.remove_status_listener(removal_id);
        parent.tick_at(std::time::Duration::from_secs_f64(1.0));
    } else {
        parent.remove_listener(failure_id);
        wrapper.remove_listener(removal_id);
        parent.set_value(1.0);
    }
    assert_eq!(calls.get(), 2, "next notification still arrives");
    lifecycle.dispose();
}

fn value_failure_retains_removed_value_captures() {
    removed_value_capture_custody("value");
}

fn peer_failure_retains_removed_value_captures() {
    removed_value_capture_custody("peer");
}

fn removed_value_capture_custody(mode: &str) {
    let vsync = Vsync::new();
    let mut peer_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let peer = peer_owner.controller().clone();
    let mut driven = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = driven.controller().clone();
    let initial_peer_tick = std::cell::Cell::new(true);
    let peer_failure = peer.add_listener(std::rc::Rc::new(move || {
        if !initial_peer_tick.replace(false) {
            panic_any("first peer failure");
        }
    }));
    let drops = Arc::new(AtomicUsize::new(0));
    let captures = Arc::clone(&drops);
    let owner = std::rc::Rc::new(std::cell::RefCell::new(Some(controller.clone())));
    let pending_owner = owner;
    let wait_for_tick = mode != "status";
    let retire = move || {
        if wait_for_tick
            && pending_owner
                .borrow()
                .as_ref()
                .is_some_and(|controller| controller.value() == 0.0)
        {
            return;
        }
        let Some(controller) = pending_owner.borrow_mut().take() else {
            return;
        };
        let capture = HostileOwnership {
            _first: DropFailure(captures.clone()),
            _second: DropFailure(captures.clone()),
        };
        let id = controller.add_listener(std::rc::Rc::new(move || {
            let _ = &capture;
        }));
        controller.remove_listener(id);
    };
    let failure_id = if mode == "status" {
        let id =
            controller.add_status_listener(std::rc::Rc::new(|_| panic_any("first status failure")));
        controller.add_status_listener(std::rc::Rc::new(move |_| retire()));
        id
    } else {
        let id = controller.add_listener(std::rc::Rc::new(|| panic_any("first value failure")));
        if mode == "peer" {
            controller.remove_listener(id);
        }
        controller.add_listener(std::rc::Rc::new(retire));
        id
    };
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = calls.clone();
    controller.add_listener(std::rc::Rc::new(move || observed.set(observed.get() + 1)));
    let failure = if mode == "status" {
        catch_unwind(AssertUnwindSafe(|| {
            controller.forward().expect("run admitted");
        }))
    } else {
        controller.forward().expect("run admitted");
        if mode == "peer" {
            peer.forward().expect("peer run admitted");
            vsync.tick_all(
                &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
            );
            catch_unwind(AssertUnwindSafe(|| {
                vsync.tick_all(
                    &flui_animation::MotionClock::new()
                        .frame(std::time::Duration::from_secs_f64(0.5)),
                );
            }))
        } else {
            catch_unwind(AssertUnwindSafe(|| {
                controller.tick_at(std::time::Duration::from_secs_f64(0.5));
            }))
        }
    }
    .expect_err("first failure resumes after the healthy tail");
    assert_eq!(
        payload_text(&*failure),
        Some(match mode {
            "status" => "first status failure",
            "peer" => "first peer failure",
            _ => "first value failure",
        })
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "outgoing value captures keep first-failure custody"
    );
    if mode == "status" {
        controller.remove_status_listener(failure_id);
    } else {
        controller.remove_listener(failure_id);
    }
    peer.remove_listener(peer_failure);
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(controller.status(), Completed);
    assert_eq!(
        calls.get(),
        match mode {
            "status" => 1,
            "peer" => 3,
            _ => 2,
        },
        "next frame still delivers"
    );
    driven.dispose();
    peer_owner.dispose();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

fn status_failure_retains_a_reentrantly_removed_new_subscription() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let armed = AtomicBool::new(true);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        if armed.swap(false, Ordering::SeqCst) {
            panic_any("first status failure");
        }
    }));
    let owner = std::rc::Rc::new(Mutex::new(Some(controller.clone())));
    let captures = Arc::clone(&drops);
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        let owner = owner.lock().expect("late subscription owner").take();
        if let Some(owner) = owner {
            let capture = HostileOwnership {
                _first: DropFailure(Arc::clone(&captures)),
                _second: DropFailure(Arc::clone(&captures)),
            };
            let id = owner.add_status_listener(std::rc::Rc::new(move |_| {
                let _ = &capture;
            }));
            owner.remove_status_listener(id);
        }
    }));
    let (tail, listener) = recorder();
    controller.add_status_listener(listener);
    let failure =
        catch_unwind(AssertUnwindSafe(|| controller.forward())).expect_err("status failure");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(seen(&tail), [Forward]);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(seen(&tail), [Forward, Completed]);
    lifecycle.dispose();
}

fn install_hostile_continuation(
    controller: &AnimationController,
    drops: &Arc<AtomicUsize>,
) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&calls);
    let capture = HostileOwnership {
        _first: DropFailure(Arc::clone(drops)),
        _second: DropFailure(Arc::clone(drops)),
    };
    controller
        .forward()
        .expect("run starts")
        .when_complete_or_cancel(move |outcome| {
            let _ = &capture;
            assert!(outcome.is_ok(), "accepted completion stays completed");
            called.fetch_add(1, Ordering::SeqCst);
        });
    calls
}

fn fail_on_completion(controller: &AnimationController) {
    controller.add_status_listener(std::rc::Rc::new(|status| {
        if status == Completed {
            panic_any("first status failure");
        }
    }));
}

fn status_failure_retains_completed_run_captures() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let drops = Arc::new(AtomicUsize::new(0));
    fail_on_completion(&controller);
    let calls = install_hostile_continuation(&controller, &drops);
    let (tail, listener) = recorder();
    controller.add_status_listener(listener);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    }))
    .expect_err("status failure");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(seen(&tail), [Completed]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let _run = controller.reverse().expect("reverse starts");
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(seen(&tail), [Completed, Reverse, Dismissed]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    lifecycle.dispose();
}

struct HostileWaiter {
    _captures: HostileOwnership,
    calls: Arc<AtomicUsize>,
}

impl Wake for HostileWaiter {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn status_failure_retains_completed_run_waiter() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    fail_on_completion(&controller);
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut run = Box::pin(controller.forward().expect("waited run starts"));
    {
        let waker = Waker::from(Arc::new(HostileWaiter {
            _captures: HostileOwnership {
                _first: DropFailure(Arc::clone(&drops)),
                _second: DropFailure(Arc::clone(&drops)),
            },
            calls: Arc::clone(&calls),
        }));
        assert!(matches!(
            run.as_mut().poll(&mut Context::from_waker(&waker)),
            Poll::Pending
        ));
    }
    let failure = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    }))
    .expect_err("status failure");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(matches!(
        run.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));
    controller.tick_at(std::time::Duration::from_secs_f64(2.0));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    lifecycle.dispose();
}

fn status_failure_remains_authoritative_over_run_failure() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    fail_on_completion(&controller);
    let drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let run = controller.forward().expect("competing run starts");
    run.when_complete_or_cancel(move |_| {
        panic_any(HostileOwnership {
            _first: DropFailure(Arc::clone(&payload_drops)),
            _second: DropFailure(Arc::clone(&payload_drops)),
        })
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&calls);
    run.when_complete_or_cancel(move |outcome| {
        assert!(outcome.is_ok());
        called.fetch_add(1, Ordering::SeqCst);
    });
    let failure = catch_unwind(AssertUnwindSafe(|| {
        controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    }))
    .expect_err("first status failure");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let _reverse = controller.reverse().expect("recovery run starts");
    controller.tick_at(std::time::Duration::from_secs_f64(1.0));
    assert_eq!(controller.value(), 0.0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    lifecycle.dispose();
}

fn frame_failure_retains_a_siblings_run_captures() {
    frame_failure_retains_run_captures(false);
}

fn child_failure_retains_the_parents_run_captures() {
    frame_failure_retains_run_captures(true);
}

fn frame_failure_retains_run_captures(nested: bool) {
    let parent = Vsync::new();
    let child = Vsync::new();
    let _child = nested.then(|| parent.attach_child(&child).expect("child registry"));
    let failing_owner = AnimationController::builder(Duration::from_secs(1))
        .build_on(Some(if nested { &child } else { &parent }));
    let failing = failing_owner.controller();
    fail_on_completion(failing);
    let _run = failing.forward().expect("failing run starts");
    let mut sibling_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&parent));
    let sibling = sibling_owner.controller().clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = install_hostile_continuation(&sibling, &drops);
    parent.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );
    let failure = catch_unwind(AssertUnwindSafe(|| {
        parent.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
        );
    }))
    .expect_err("frame failure");
    assert_eq!(payload_text(failure.as_ref()), Some("first status failure"));
    assert_eq!(sibling.value(), 1.0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let _reverse = sibling.reverse().expect("sibling reverses");
    parent.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(1.0)),
    );
    parent.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(2.0)),
    );
    assert_eq!(sibling.value(), 0.0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    sibling_owner.dispose();
}

// --- AnimationSwitch -----------------------------------------------------------

#[derive(Clone, Copy)]
enum Reentry {
    Value,
    Status,
}

/// A parent that reads the switch it drives from inside its own queries, once.
struct SwitchReader {
    switch: Mutex<Option<AnimationSwitch>>,
    reentry: Reentry,
    values: ChangeNotifier,
    statuses: ChangeNotifier,
}

impl SwitchReader {
    fn read_back(&self, reentry: Reentry) {
        if !matches!(
            (self.reentry, reentry),
            (Reentry::Value, Reentry::Value) | (Reentry::Status, Reentry::Status)
        ) {
            return;
        }
        let switch = self.switch.lock().expect("switch slot").take();
        if let Some(switch) = switch {
            assert_eq!(switch.value(), 0.25);
            assert_eq!(switch.status(), Forward);
        }
    }
}

impl std::fmt::Debug for SwitchReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SwitchReader")
    }
}

impl Listenable for SwitchReader {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.values.add_listener(callback)
    }
    fn remove_listener(&self, id: ListenerId) {
        self.values.remove_listener(id);
    }
    fn remove_all_listeners(&self) {
        self.values.remove_all_listeners();
    }
}

impl Animation<f64> for SwitchReader {
    fn value(&self) -> f64 {
        self.read_back(Reentry::Value);
        0.25
    }
    fn status(&self) -> AnimationStatus {
        self.read_back(Reentry::Status);
        Forward
    }
    fn add_status_listener(&self, _callback: StatusCallback) -> ListenerId {
        self.statuses.add_listener(std::rc::Rc::new(|| {}))
    }
    fn remove_status_listener(&self, id: ListenerId) {
        self.statuses.remove_listener(id);
    }
}

fn switch_reader(reentry: Reentry) -> AnimationSwitch {
    let parent = std::rc::Rc::new(SwitchReader {
        switch: Mutex::new(None),
        reentry,
        values: ChangeNotifier::new(),
        statuses: ChangeNotifier::new(),
    });
    let switch = AnimationSwitch::new(parent.clone(), None);
    *parent.switch.lock().expect("switch slot") = Some(switch.clone());
    switch
}

fn switch_parent_value_may_read_the_switch() {
    for diagnostic in [false, true] {
        let switch = switch_reader(Reentry::Value);
        if diagnostic {
            assert!(format!("{switch:?}").contains("0.25"));
        } else {
            assert_eq!(switch.value(), 0.25);
        }
    }
}

fn switch_parent_status_may_read_the_switch() {
    for diagnostic in [false, true] {
        let switch = switch_reader(Reentry::Status);
        if diagnostic {
            assert!(format!("{switch:?}").contains("Forward"));
        } else {
            assert_eq!(switch.status(), Forward);
        }
    }
}

fn switch_hop_announces_the_new_train_status() {
    let train = controller();
    train.set_value(0.2);
    let switch = AnimationSwitch::new(
        std::rc::Rc::new(train.clone()),
        Some(std::rc::Rc::new(ConstantAnimation::completed(0.5))),
    );
    let (log, listener) = recorder();
    switch.add_status_listener(listener);

    train.set_value(0.6);

    assert_eq!(switch.value(), 0.5, "the switch hopped to the next train");
    assert_eq!(switch.status(), Completed);
    assert_eq!(
        seen(&log),
        [Completed],
        "the hop announces the new train's status once"
    );
}

// --- ProxyAnimation -------------------------------------------------------------

/// A proxy and the parent a listener installs on it from inside a notification.
type PendingSwap = (ProxyAnimation<f64>, AnimationController);

struct Parents {
    a: AnimationController,
    b: AnimationController,
    c: AnimationController,
}

/// Three parents with distinct statuses: `a` Forward, `b` Completed, `c` Dismissed.
fn parents() -> Parents {
    let a = controller();
    a.set_value(0.5);
    let b = controller();
    b.set_value(1.0);
    let c = controller();
    assert_eq!(
        (a.status(), b.status(), c.status()),
        (Forward, Completed, Dismissed)
    );
    Parents { a, b, c }
}

fn counter(proxy: &ProxyAnimation<f64>) -> Arc<Mutex<usize>> {
    let count = Arc::new(Mutex::new(0));
    let sink = Arc::clone(&count);
    proxy.add_listener(std::rc::Rc::new(move || {
        *sink.lock().expect("notification count") += 1;
    }));
    count
}

fn count(counter: &Arc<Mutex<usize>>) -> usize {
    *counter.lock().expect("notification count")
}

fn set_parent_moves_every_notification_to_the_new_parent_once() {
    let Parents { a, b, .. } = parents();
    let proxy = ProxyAnimation::new(std::rc::Rc::new(a.clone()));
    let values = counter(&proxy);
    let (log, listener) = recorder();
    proxy.add_status_listener(listener);

    proxy.set_parent(std::rc::Rc::new(b.clone()));
    assert_eq!(count(&values), 1, "one value notification per set_parent");
    assert_eq!(seen(&log), [Completed], "the new status is announced once");

    a.set_value(0.0);
    assert_eq!(
        count(&values),
        1,
        "the replaced parent's values are ignored"
    );
    assert_eq!(
        seen(&log),
        [Completed],
        "the replaced parent's status is ignored"
    );

    b.set_value(0.4);
    assert_eq!(proxy.value(), 0.4);
    assert_eq!(count(&values), 2, "the new parent's values arrive");
    assert_eq!(
        seen(&log),
        [Completed, Forward],
        "the new parent's status arrives"
    );
}

fn reentrant_set_parent_last_commit_wins() {
    let Parents { a, b, c } = parents();
    let proxy = ProxyAnimation::new(std::rc::Rc::new(a));
    let slot: std::rc::Rc<Mutex<Option<PendingSwap>>> = std::rc::Rc::default();
    let pending = std::rc::Rc::clone(&slot);
    proxy.add_listener(std::rc::Rc::new(move || {
        let swap = pending.lock().expect("swap slot").take();
        if let Some((proxy, parent)) = swap {
            proxy.set_parent(std::rc::Rc::new(parent));
        }
    }));
    let values = counter(&proxy);
    let (log, listener) = recorder();
    proxy.add_status_listener(listener);
    *slot.lock().expect("swap slot") = Some((proxy.clone(), c.clone()));

    proxy.set_parent(std::rc::Rc::new(b.clone()));

    assert_eq!(
        proxy.status(),
        Dismissed,
        "the reentrant parent is the one in charge"
    );
    assert_eq!(
        count(&values),
        2,
        "one value notification per committed set_parent"
    );
    assert_eq!(
        seen(&log).last(),
        Some(&proxy.status()),
        "the last announced status is the status of the parent in charge"
    );

    b.set_value(0.3);
    assert_eq!(
        count(&values),
        2,
        "the parent replaced mid-notification is ignored"
    );
    c.set_value(0.3);
    assert_eq!(count(&values), 3, "the reentrant parent's values arrive");
}

#[test]
fn status_delivery_contract() {
    crate::run_table(&[
        (
            "reentrant transitions retain A to B to A and respect subscription admission",
            reentrant_transitions_keep_the_return_to_the_original_status,
        ),
        (
            "reentrant completion preserves status and outcome order after failure",
            reentrant_completion_delivers_its_outcome_before_the_next_status,
        ),
        (
            "panicking status listener leaves the next frame ticking",
            panicking_status_listener_leaves_the_next_frame_ticking,
        ),
        (
            "panicking continuation during dispose finishes the dispose",
            panicking_continuation_during_dispose_finishes_the_dispose,
        ),
        (
            "vsync walk after a panic ticks every controller",
            vsync_walk_after_a_panic_ticks_every_controller,
        ),
        (
            "set_parent moves every notification to the new parent once",
            set_parent_moves_every_notification_to_the_new_parent_once,
        ),
    ]);
}

#[test]
fn status_listener_panic_finishes_the_round() {
    panicking_status_listener_does_not_starve_later_listeners();
}

#[test]
fn status_listener_panics_compete() {
    competing_status_listener_panics_re_raise_the_first();
}

#[test]
fn value_listener_panic_finishes_the_round() {
    panicking_value_listener_is_re_raised_after_the_round();
}

#[test]
fn value_listener_panics_compete() {
    competing_value_listener_panics_re_raise_the_first();
}

#[test]
fn vsync_walk_contains_a_sibling_panic() {
    vsync_walk_ticks_siblings_after_a_panicking_controller();
}

#[test]
fn vsync_walk_contains_a_child_registry_panic() {
    vsync_walk_ticks_the_parent_after_a_panicking_child_registry();
}

#[test]
fn removed_status_listener_is_skipped() {
    status_listener_removed_by_an_earlier_listener_is_skipped();
}

#[test]
fn disposed_mid_fan_out_is_silent() {
    controller_disposed_mid_fan_out_calls_no_further_listener();
}

#[test]
fn reentrant_status_keeps_commit_order() {
    reversing_on_completed_keeps_commit_order();
}

#[test]
fn switch_parent_value_reentry() {
    child_process::run_single(
        "status_delivery::switch_parent_value_reentry",
        switch_parent_value_may_read_the_switch,
    );
}

#[test]
fn switch_parent_status_reentry() {
    child_process::run_single(
        "status_delivery::switch_parent_status_reentry",
        switch_parent_status_may_read_the_switch,
    );
}

#[test]
fn switch_hop_announces_status() {
    switch_hop_announces_the_new_train_status();
}

#[test]
fn proxy_reentrant_set_parent() {
    reentrant_set_parent_last_commit_wins();
}
