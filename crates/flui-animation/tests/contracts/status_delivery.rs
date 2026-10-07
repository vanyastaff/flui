//! Status delivery: every listener sees each committed status in commit order,
//! a failing callback or controller lets its round finish and re-raises the
//! first panic afterwards, and a hop or a parent swap announces exactly the
//! status of the animation now in charge.
//!
//! Rows that hold today run in the tables. A row whose behaviour is not there
//! yet is its own `#[ignore = "contract: …"]` test, so `--run-ignored` shows
//! it failing on the assertion that names the behaviour.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, ConstantAnimation,
    ProxyAnimation, StatusCallback, Vsync, curve::Curve,
};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};

use AnimationStatus::{Completed, Dismissed, Forward, Reverse};

type Log = Arc<Mutex<Vec<AnimationStatus>>>;

/// A status listener that appends every status it receives to the log.
fn recorder() -> (Log, StatusCallback) {
    let log = Log::default();
    let sink = Arc::clone(&log);
    let callback: StatusCallback = Arc::new(move |status| {
        sink.lock().expect("status log").push(status);
    });
    (log, callback)
}

fn seen(log: &Log) -> Vec<AnimationStatus> {
    log.lock().expect("status log").clone()
}

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::without_ticker(Duration::from_secs(1))
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
fn running(panic: Option<&'static str>) -> AnimationController {
    let controller = controller();
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
    controller
}

// --- callback failures -----------------------------------------------------------
//
// One policy for every callback a controller runs: the transition is committed
// before the call, the round finishes, the first panic is re-raised after the
// round, and the next operation proceeds normally.

fn panicking_status_listener_does_not_starve_later_listeners() {
    let controller = controller();
    controller.add_status_listener(Arc::new(|_| panic_any("status listener")));
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
    controller.tick_at(0.5);
    assert_eq!(controller.value(), 0.5, "the next frame ticks the run");
}

fn competing_status_listener_panics_re_raise_the_first() {
    let controller = controller();
    controller.add_status_listener(Arc::new(|_| panic_any("first")));
    controller.add_status_listener(Arc::new(|_| panic_any("second")));
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
    controller.add_listener(Arc::new(move || {
        *sink.lock().expect("value count") += 1;
    }));
    count
}

fn panicking_value_listener_is_re_raised_after_the_round() {
    let controller = controller();
    let _run = controller.forward().expect("run starts");
    controller.add_listener(Arc::new(|| panic_any("value listener")));
    let later = value_counter(&controller);

    let ticked = catch_unwind(AssertUnwindSafe(|| controller.tick_at(0.5)));

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
    controller.add_listener(Arc::new(|| panic_any("first")));
    controller.add_listener(Arc::new(|| panic_any("second")));
    let later = value_counter(&controller);

    let ticked = catch_unwind(AssertUnwindSafe(|| controller.tick_at(0.5)));

    assert_eq!(*later.lock().expect("value count"), 1, "the round finishes");
    let payload = ticked.expect_err("a panic is re-raised after the round");
    assert_eq!(payload_text(payload.as_ref()), Some("first"));
}

fn panicking_continuation_during_dispose_finishes_the_dispose() {
    let vsync = Vsync::new();
    let controller = controller();
    let _registration = vsync.register(controller.clone());
    let run = controller.forward().expect("run starts");
    run.when_complete_or_cancel(|_| panic_any("first"));
    let outcome = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&outcome);
    run.when_complete_or_cancel(move |end| {
        *sink.lock().expect("continuation outcome") = Some(end.is_err());
    });

    let disposed = catch_unwind(AssertUnwindSafe(|| controller.dispose()));

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
    controller.add_status_listener(Arc::new(move |status| {
        if armed.swap(false, Ordering::SeqCst) {
            panic_any("first status");
        }
        record(status);
    }));

    let _ = catch_unwind(AssertUnwindSafe(|| controller.forward()));
    controller.tick_at(0.5);
    assert_eq!(
        controller.value(),
        0.5,
        "the run advances on the next frame"
    );
    controller.tick_at(1.0);
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
    let first = running(Some("first"));
    let second = running(Some("second"));
    let sibling = running(None);
    let _registrations = [
        vsync.register(first),
        vsync.register(second),
        vsync.register(sibling.clone()),
    ];
    vsync.tick_all(0.0);

    let walk = catch_unwind(AssertUnwindSafe(|| vsync.tick_all(0.5)));

    assert_eq!(
        sibling.value(),
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
    let own = running(None);
    let _own = parent.register(own.clone());
    let child = Vsync::new();
    let _child = parent.attach_child(&child).expect("child attaches");
    let _failing = child.register(running(Some("child")));
    parent.tick_all(0.0);

    let walk = catch_unwind(AssertUnwindSafe(|| parent.tick_all(0.5)));

    assert_eq!(
        own.value(),
        0.5,
        "the parent's own controller is ticked after a child registry panicked"
    );
    let payload = walk.expect_err("the walk re-raises the contained panic");
    assert_eq!(payload_text(payload.as_ref()), Some("child"));
}

fn vsync_walk_after_a_panic_ticks_every_controller() {
    let vsync = Vsync::new();
    let failing = running(Some("once"));
    let sibling = running(None);
    let _registrations = [
        vsync.register(failing.clone()),
        vsync.register(sibling.clone()),
    ];
    vsync.tick_all(0.0);
    let _ = catch_unwind(AssertUnwindSafe(|| vsync.tick_all(0.5)));

    vsync.tick_all(0.75);

    assert_eq!(
        failing.value(),
        0.75,
        "the controller that panicked ticks again"
    );
    assert_eq!(sibling.value(), 0.75, "its sibling ticks on the next frame");
}

// --- removal and disposal during a fan-out -----------------------------------

fn status_listener_removed_by_an_earlier_listener_is_skipped() {
    let controller = controller();
    let target: Arc<Mutex<Option<(AnimationController, ListenerId)>>> = Arc::default();
    let pending = Arc::clone(&target);
    controller.add_status_listener(Arc::new(move |_| {
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
    let controller = controller();
    let slot: Arc<Mutex<Option<AnimationController>>> = Arc::default();
    let pending = Arc::clone(&slot);
    controller.add_status_listener(Arc::new(move |_| {
        let owner = pending.lock().expect("dispose slot").take();
        if let Some(controller) = owner {
            controller.dispose();
        }
    }));
    let (later, listener) = recorder();
    controller.add_status_listener(listener);
    *slot.lock().expect("dispose slot") = Some(controller.clone());

    let _run = controller.forward().expect("run starts");

    assert_eq!(
        seen(&later),
        [],
        "no listener of a disposed controller is called"
    );
}

// --- reentrant ordering --------------------------------------------------------

fn reversing_on_completed_keeps_commit_order() {
    let controller = controller();
    let slot: Arc<Mutex<Option<AnimationController>>> = Arc::default();
    let pending = Arc::clone(&slot);
    controller.add_status_listener(Arc::new(move |status| {
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
    controller.tick_at(1.0);

    assert_eq!(controller.status(), Reverse);
    assert_eq!(
        seen(&observer),
        [Forward, Completed, Reverse],
        "a later listener observes statuses in commit order, ending at status()"
    );
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
        self.statuses.add_listener(Arc::new(|| {}))
    }
    fn remove_status_listener(&self, id: ListenerId) {
        self.statuses.remove_listener(id);
    }
}

fn switch_reader(reentry: Reentry) -> AnimationSwitch {
    let parent = Arc::new(SwitchReader {
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
    let switch = switch_reader(Reentry::Value);
    assert_eq!(switch.value(), 0.25);
}

fn switch_parent_status_may_read_the_switch() {
    let switch = switch_reader(Reentry::Status);
    assert_eq!(switch.status(), Forward);
}

fn switch_hop_announces_the_new_train_status() {
    let train = controller();
    train.set_value(0.2);
    let switch = AnimationSwitch::new(
        Arc::new(train.clone()),
        Some(Arc::new(ConstantAnimation::completed(0.5))),
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
    proxy.add_listener(Arc::new(move || {
        *sink.lock().expect("notification count") += 1;
    }));
    count
}

fn count(counter: &Arc<Mutex<usize>>) -> usize {
    *counter.lock().expect("notification count")
}

fn set_parent_moves_every_notification_to_the_new_parent_once() {
    let Parents { a, b, .. } = parents();
    let proxy = ProxyAnimation::new(Arc::new(a.clone()));
    let values = counter(&proxy);
    let (log, listener) = recorder();
    proxy.add_status_listener(listener);

    proxy.set_parent(Arc::new(b.clone()));
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
    let proxy = ProxyAnimation::new(Arc::new(a));
    let slot: Arc<Mutex<Option<PendingSwap>>> = Arc::default();
    let pending = Arc::clone(&slot);
    proxy.add_listener(Arc::new(move || {
        let swap = pending.lock().expect("swap slot").take();
        if let Some((proxy, parent)) = swap {
            proxy.set_parent(Arc::new(parent));
        }
    }));
    let values = counter(&proxy);
    let (log, listener) = recorder();
    proxy.add_status_listener(listener);
    *slot.lock().expect("swap slot") = Some((proxy.clone(), c.clone()));

    proxy.set_parent(Arc::new(b.clone()));

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
#[ignore = "contract: a status listener's panic is re-raised after every listener got the status"]
fn status_listener_panic_finishes_the_round() {
    panicking_status_listener_does_not_starve_later_listeners();
}

#[test]
#[ignore = "contract: of two status-listener panics the first is re-raised after the round"]
fn status_listener_panics_compete() {
    competing_status_listener_panics_re_raise_the_first();
}

#[test]
#[ignore = "contract: a value listener's panic is re-raised after the round"]
fn value_listener_panic_finishes_the_round() {
    panicking_value_listener_is_re_raised_after_the_round();
}

#[test]
#[ignore = "contract: of two value-listener panics the first is re-raised after the round"]
fn value_listener_panics_compete() {
    competing_value_listener_panics_re_raise_the_first();
}

#[test]
#[ignore = "contract: the Vsync walk ticks siblings after a panicking controller and re-raises the first payload"]
fn vsync_walk_contains_a_sibling_panic() {
    vsync_walk_ticks_siblings_after_a_panicking_controller();
}

#[test]
#[ignore = "contract: the Vsync walk ticks the parent registry after a panicking child registry"]
fn vsync_walk_contains_a_child_registry_panic() {
    vsync_walk_ticks_the_parent_after_a_panicking_child_registry();
}

#[test]
#[ignore = "contract: a status listener removed during a fan-out is not called"]
fn removed_status_listener_is_skipped() {
    status_listener_removed_by_an_earlier_listener_is_skipped();
}

#[test]
#[ignore = "contract: a controller disposed during a fan-out calls no further listener"]
fn disposed_mid_fan_out_is_silent() {
    controller_disposed_mid_fan_out_calls_no_further_listener();
}

#[test]
#[ignore = "contract: a status started from a listener reaches later listeners in commit order"]
fn reentrant_status_keeps_commit_order() {
    reversing_on_completed_keeps_commit_order();
}

#[test]
#[ignore = "contract: a switch's parent may read the switch from value()"]
fn switch_parent_value_reentry() {
    child_process::run_single(
        "status_delivery::switch_parent_value_reentry",
        switch_parent_value_may_read_the_switch,
    );
}

#[test]
#[ignore = "contract: a switch's parent may read the switch from status()"]
fn switch_parent_status_reentry() {
    child_process::run_single(
        "status_delivery::switch_parent_status_reentry",
        switch_parent_status_may_read_the_switch,
    );
}

#[test]
#[ignore = "contract: a switch hop announces the new train's status"]
fn switch_hop_announces_status() {
    switch_hop_announces_the_new_train_status();
}

#[test]
#[ignore = "contract: a set_parent made from a notification wins and is the last status announced"]
fn proxy_reentrant_set_parent() {
    reentrant_set_parent_last_commit_wins();
}
