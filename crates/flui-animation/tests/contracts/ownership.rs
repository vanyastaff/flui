//! Listener ownership and the frame-scheduled hook: what a listener captures is
//! released once its owner is disposed or dropped, a run in flight does not
//! keep its controller alive, and the embedder's frame-scheduled hook runs
//! with the controller free to read.
//!
//! A row whose behaviour is not there yet is its own
//! `#[ignore = "contract: …"]` test, so `--run-ignored` shows it failing on
//! the assertion that names the behaviour. The hook rows run in child
//! processes: their failure mode is a deadlock.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, CurvedAnimation, Curves,
    FloatTween, ProxyAnimation, ReverseAnimation, TweenAnimation, Vsync,
};
use flui_foundation::Listenable;

#[test]
fn reentrant_settles_leave_the_next_run_for_a_later_entry() {
    const TEST: &str = "ownership::reentrant_settles_leave_the_next_run_for_a_later_entry";
    let Some(case) = child_process::selected_case() else {
        child_process::run_rows(TEST, &["finite", "instant", "zero repeat"]);
        return;
    };
    let duration = if case == "finite" {
        Duration::from_secs(1)
    } else {
        Duration::ZERO
    };
    let owner = AnimationController::builder(duration).build_on(None);
    let controller = owner.controller().clone();
    let callback_controller = controller.clone();
    let completions = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = completions.clone();
    controller.add_status_listener(std::rc::Rc::new(move |status| match status {
        AnimationStatus::Completed => {
            observed.set(observed.get() + 1);
            let _next = callback_controller.reverse().expect("reentrant reverse");
        }
        AnimationStatus::Dismissed => {
            observed.set(observed.get() + 1);
            let _next = callback_controller.forward().expect("reentrant forward");
        }
        _ => {}
    }));
    let _run = if case == "zero repeat" {
        controller.repeat(false).expect("initial repeat")
    } else {
        controller.forward().expect("initial run")
    };
    assert!(
        completions.get() <= 2,
        "one outer entry has a bounded settle budget"
    );
    assert!(
        controller.is_animating(),
        "the listener's next run remains accepted"
    );
    drop(owner);
    child_process::pass();
}

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::builder(Duration::from_secs(1)).build()
}

fn dropping_a_driven_controller_unregisters_then_cancels_its_run() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let mut run = owner.controller().forward().expect("bound owner");
    let observed = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed_in_callback = observed.clone();
    let registry = vsync.clone();
    run.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        assert!(
            registry.is_empty(),
            "seat is absent before cancellation delivery"
        );
        registry.tick_all(&flui_animation::MotionClock::new().frame(Duration::ZERO));
        let peer = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        drop(peer);
        observed_in_callback.set(observed_in_callback.get() + 1);
    });
    drop(owner);
    assert!(vsync.is_empty());
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(_))
    ));
    assert_eq!(observed.get(), 1);
}

fn dispose_then_drop_is_one_retirement() {
    let vsync = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let run = owner.controller().forward().expect("bound owner");
    let outcomes = std::rc::Rc::new(std::cell::Cell::new(0));
    let sink = outcomes.clone();
    run.when_complete_or_cancel(move |_| sink.set(sink.get() + 1));
    owner.dispose();
    owner.dispose();
    drop(owner);
    assert!(vsync.is_empty());
    assert_eq!(outcomes.get(), 1);
}

fn rebinding_mid_run_keeps_the_value_continuous() {
    for new_origin in [0, 100_000] {
        let old = Vsync::new();
        let new = Vsync::new();
        let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
        let mut clock = flui_animation::MotionClock::new();
        let _run = owner.controller().forward().expect("bound owner");
        old.tick_all(&clock.frame(Duration::ZERO));
        old.tick_all(&clock.frame(Duration::from_millis(400)));
        let displayed = owner.controller().value();
        owner.rebind(Some(&old)).expect("same seat");
        assert_eq!(old.len(), 1);
        owner.rebind(Some(&new)).expect("fresh seat");
        assert!(old.is_empty());
        let mut new_clock = flui_animation::MotionClock::new();
        new.tick_all(&new_clock.frame(Duration::from_millis(new_origin)));
        assert_eq!(
            owner.controller().value(),
            displayed,
            "migration preserves the displayed sample"
        );
        new.tick_all(&new_clock.frame(Duration::from_millis(new_origin + 100)));
        assert_eq!(owner.controller().value(), 0.5);
    }
}

fn unbound_time_run_completes_once() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner
        .controller()
        .forward()
        .expect("unbound run is admitted");
    assert_eq!(owner.controller().value(), 1.0);
    assert!(run.is_complete());
    assert_eq!(owner.controller().status(), AnimationStatus::Completed);
}

fn dropping_the_last_owner_from_its_own_listener_mid_frame() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    for channel in ["status", "value", "continuation"] {
        let registry = Vsync::new();
        let first = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let peer = AnimationController::builder(Duration::from_secs(2)).build_on(Some(&registry));
        let survivor =
            AnimationController::builder(Duration::from_secs(2)).build_on(Some(&registry));
        let first_controller = first.controller().clone();
        let peer_controller = peer.controller().clone();
        let owners = Rc::new(RefCell::new(Some((first, peer))));
        let callback_owners = owners.clone();
        let retire = Rc::new(move || {
            let outgoing = callback_owners.borrow_mut().take();
            drop(outgoing);
        });
        let peer_calls = Rc::new(Cell::new(0));
        let observed = peer_calls.clone();
        peer_controller.add_listener(Rc::new(move || observed.set(observed.get() + 1)));
        if channel == "status" {
            let retire = retire.clone();
            first_controller.add_status_listener(Rc::new(move |status| {
                if status == AnimationStatus::Completed {
                    retire();
                }
            }));
        } else if channel == "value" {
            let retire = retire.clone();
            let controller = first_controller.clone();
            first_controller.add_listener(Rc::new(move || {
                if controller.value() > 0.0 {
                    retire();
                }
            }));
        }
        let run = first_controller.forward().expect("first run");
        if channel == "continuation" {
            run.when_complete_or_cancel(move |_| retire());
        }
        let _peer_run = peer_controller.forward().expect("peer run");
        let _survivor_run = survivor.controller().forward().expect("survivor run");
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        peer_calls.set(0);
        registry.tick_all(&clock.frame(Duration::from_secs(1)));
        assert!(owners.borrow().is_none(), "{channel}: owners were released");
        assert_eq!(
            peer_calls.get(),
            0,
            "{channel}: a retired peer is skipped in this snapshot"
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(survivor.controller().value(), 0.5);
        registry.tick_all(&clock.frame(Duration::from_millis(1200)));
        assert_eq!(peer_calls.get(), 0);
        assert_eq!(survivor.controller().value(), 0.6);
    }
}

fn an_unbound_infinite_repeat_parks_then_resumes_on_a_registry() {
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner.controller().repeat(false).expect("infinite repeat");
    assert_eq!(owner.controller().value(), 0.0);
    assert!(run.is_pending());
    let registry = Vsync::new();
    owner.rebind(Some(&registry)).expect("bind parked run");
    let mut clock = flui_animation::MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(300)));
    assert_eq!(owner.controller().value(), 0.3);
    assert!(run.is_pending());
    drop(owner);
    assert!(run.is_canceled());
}

fn unbound_finite_repeat_lands_on_its_last_leg() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner
        .controller()
        .repeat_with(None, None, true, None, Some(4))
        .expect("finite repeat");
    assert_eq!(owner.controller().value(), 0.0);
    assert_eq!(owner.controller().status(), AnimationStatus::Dismissed);
    assert!(run.is_complete());
}

#[derive(Debug)]
struct NeverSettling;

impl flui_animation::Simulation for NeverSettling {
    fn x(&self, time: f64) -> f64 {
        time
    }
    fn dx(&self, _time: f64) -> f64 {
        1.0
    }
    fn is_done(&self, _time: f64) -> bool {
        false
    }
}

fn unbound_simulation_uses_the_last_finite_probe() {
    let owner = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build_on(None);
    let run = owner
        .controller()
        .animate_with(NeverSettling)
        .expect("simulation run");
    assert_eq!(owner.controller().value(), 64.0);
    assert!(run.is_complete());
}

#[test]
fn an_unbound_controller_settles_every_run_kind_at_once() {
    crate::run_table(&[
        ("finite time", unbound_time_run_completes_once),
        ("finite repeat", unbound_finite_repeat_lands_on_its_last_leg),
        ("simulation", unbound_simulation_uses_the_last_finite_probe),
        (
            "infinite repeat parks and resumes",
            an_unbound_infinite_repeat_parks_then_resumes_on_a_registry,
        ),
    ]);
}

#[test]
fn driven_controller_owns_its_seat_and_run() {
    crate::run_table(&[
        (
            "drop unregisters before cancel",
            dropping_a_driven_controller_unregisters_then_cancels_its_run,
        ),
        (
            "dispose and drop are idempotent",
            dispose_then_drop_is_one_retirement,
        ),
        (
            "migration preserves elapsed",
            rebinding_mid_run_keeps_the_value_continuous,
        ),
        (
            "last owner retires itself and a peer",
            dropping_the_last_owner_from_its_own_listener_mid_frame,
        ),
    ]);
}

/// Counts how many times the value it guards is dropped.
struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn drop_probe() -> (Arc<AtomicUsize>, DropProbe) {
    let drops = Arc::new(AtomicUsize::new(0));
    (Arc::clone(&drops), DropProbe(drops))
}

// --- controller ---------------------------------------------------------------------

#[test]
fn dispose_releases_value_listeners() {
    let controller = controller();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_listener(std::rc::Rc::new(move || {
        let _ = &capture;
    }));

    controller.dispose();

    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a disposed controller does not retain its value listeners"
    );
}

#[test]
fn last_handle_drop_releases_a_running_controller() {
    let (drops, probe) = drop_probe();
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    controller.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &probe;
    }));
    let mut run = controller.forward().expect("run starts");

    drop(controller);

    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the run in flight does not keep its controller alive"
    );
    assert!(
        matches!(
            Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ),
        "the run of a released controller is canceled"
    );
}

// --- wrappers -------------------------------------------------------------------------

type AnimationHandle = std::rc::Rc<dyn Animation<f64>>;

fn wrapper_over_a_disposed_source(wrap: fn(AnimationHandle) -> AnimationHandle) {
    let parent = controller();
    parent.set_value(0.5);
    parent.dispose();
    let (drops, value_probe) = drop_probe();
    let status_probe = DropProbe(drops.clone());
    let wrapper = wrap(std::rc::Rc::new(parent.clone()));
    wrapper.add_listener(std::rc::Rc::new(move || {
        let _ = &value_probe;
        panic!("a disposed source cannot notify a new wrapper");
    }));
    wrapper.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &status_probe;
        panic!("a disposed source cannot send a new status");
    }));
    parent.set_value(1.0);
    drop(wrapper);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "an inert wrapper releases both listener captures on owner teardown"
    );
}

fn reverse_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(ReverseAnimation::new(parent)));
}
fn curved_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| {
        std::rc::Rc::new(CurvedAnimation::new(parent, Curves::Linear))
    });
}
fn tween_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| {
        std::rc::Rc::new(TweenAnimation::new(FloatTween::new(0.0, 10.0), parent))
    });
}
fn proxy_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(ProxyAnimation::new(parent)));
}
fn switch_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(AnimationSwitch::new(parent, None)));
}

#[test]
fn wrappers_over_disposed_sources_remain_inert() {
    crate::run_table(&[
        ("reverse", reverse_over_disposed_source),
        ("curved", curved_over_disposed_source),
        ("tween", tween_over_disposed_source),
        ("proxy", proxy_over_disposed_source),
        ("switch", switch_over_disposed_source),
    ]);
}

/// Adds a status listener through `wrapper`, drops the wrapper while its parent
/// stays alive, and asserts the listener's capture was released.
fn wrapper_status_listener_dies_with_the_wrapper<W>(wrap: fn(std::rc::Rc<dyn Animation<f64>>) -> W)
where
    W: Animation<f64>,
{
    let parent = controller();
    let probe = Arc::new(());
    {
        let wrapper = wrap(std::rc::Rc::new(parent.clone()));
        let capture = Arc::clone(&probe);
        let _id = wrapper.add_status_listener(std::rc::Rc::new(move |_| {
            let _ = &capture;
        }));
    }
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "the parent does not retain a status listener added through a dropped wrapper"
    );
    parent.dispose();
}

#[test]
fn reverse_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(ReverseAnimation::new);
}

#[test]
fn curved_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        CurvedAnimation::new(parent, Curves::EaseIn)
    });
}

#[test]
fn tween_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        TweenAnimation::new(FloatTween::new(0.0, 10.0), parent)
    });
}

// --- switch ---------------------------------------------------------------------------

#[test]
fn switch_dispose_releases_callbacks() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (switched, listened) = (Arc::new(()), Arc::new(()));
    let switched_capture = Arc::clone(&switched);
    let switch = AnimationSwitch::new(
        std::rc::Rc::new(current.clone()),
        Some(std::rc::Rc::new(next.clone())),
    )
    .on_switched(move || {
        let _ = &switched_capture;
    });
    let listened_capture = Arc::clone(&listened);
    let _id = switch.add_status_listener(std::rc::Rc::new(move |_| {
        let _ = &listened_capture;
    }));

    switch.dispose();

    assert_eq!(
        (Arc::strong_count(&switched), Arc::strong_count(&listened)),
        (1, 1),
        "a disposed switch retains neither on_switched nor its status listeners"
    );
    current.dispose();
    next.dispose();
}

/// The transition-route shape: a proxy parented to a switch whose
/// `on_switched` re-parents that same proxy.
#[test]
fn proxy_parented_to_a_capturing_switch_is_freed() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (drops, probe) = drop_probe();
    {
        let proxy = std::rc::Rc::new(ProxyAnimation::new(
            std::rc::Rc::new(current.clone()) as std::rc::Rc<dyn Animation<f64>>
        ));
        let hop_proxy = std::rc::Rc::clone(&proxy);
        let target: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(next.clone());
        let switch = AnimationSwitch::new(
            std::rc::Rc::new(current.clone()),
            Some(std::rc::Rc::clone(&target)),
        )
        .on_switched(move || {
            let _ = &probe;
            hop_proxy.set_parent(std::rc::Rc::clone(&target));
        });
        proxy.set_parent(std::rc::Rc::new(switch.clone()));
        switch.dispose();
    }
    current.dispose();
    next.dispose();

    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the proxy, the switch and on_switched are released"
    );
}
