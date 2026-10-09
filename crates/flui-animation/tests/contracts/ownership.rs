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

fn rebinding_retires_the_old_registry_after_committing_the_new_clock() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture {
        controller: AnimationController,
        seen: Rc<Cell<f64>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.seen.set(self.controller.value());
            panic!("registry retirement");
        }
    }

    let old = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
    let seen = Rc::new(Cell::new(-1.0));
    let capture = Capture {
        controller: owner.controller().clone(),
        seen: seen.clone(),
    };
    old.set_frame_requester(Some(Rc::new(move || {
        let _ = &capture;
    })));
    let mut run = owner.controller().forward().expect("bound run");
    drop(old);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        owner.rebind(None).expect("release old clock");
    }))
    .expect_err("outgoing registry capture fails");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"registry retirement"));
    assert!(!owner.is_bound());
    assert_eq!(
        owner.controller().value(),
        1.0,
        "losing the clock settles the run"
    );
    assert_eq!(
        seen.get(),
        1.0,
        "retirement observes the committed clock transition"
    );
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));

    let next = Vsync::new();
    owner.rebind(Some(&next)).expect("recover with a new clock");
    let reverse = owner.controller().reverse().expect("next run");
    let mut clock = flui_animation::MotionClock::new();
    next.tick_all(&clock.frame(Duration::ZERO));
    next.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!(owner.controller().value(), 0.0);
    assert!(reverse.is_complete());
}

fn rebinding_preserves_delivery_failure_before_outgoing_retirement() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("outgoing registry retirement");
        }
    }

    for binding in ["missing", "replacement"] {
        let old = Vsync::new();
        let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
        let retired = Rc::new(Cell::new(0));
        let capture = Capture(retired.clone());
        old.set_frame_requester(Some(Rc::new(move || {
            let _ = &capture;
        })));
        let run = owner.controller().forward().expect("initial bound run");
        drop(old);

        let replacement = Vsync::new();
        let healthy = Rc::new(Cell::new(0));
        if binding == "replacement" {
            replacement.set_frame_requester(Some(Rc::new(|| panic!("clock delivery"))));
        } else {
            owner
                .controller()
                .add_listener(Rc::new(|| panic!("clock delivery")));
            let observed = healthy.clone();
            owner
                .controller()
                .add_listener(Rc::new(move || observed.set(observed.get() + 1)));
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner
                .rebind((binding == "replacement").then_some(&replacement))
                .expect("clock transition");
        }))
        .expect_err("clock delivery fails");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"clock delivery"));
        assert_eq!(
            retired.get(),
            0,
            "opaque outgoing captures retain first-failure custody"
        );
        assert_eq!(owner.is_bound(), binding == "replacement");
        if binding == "missing" {
            assert_eq!(owner.controller().value(), 1.0);
            assert!(run.is_complete());
            assert_eq!(healthy.get(), 1, "healthy settlement delivery finishes");
            owner.controller().remove_all_listeners();
            owner.rebind(Some(&replacement)).expect("restore clock");
        } else {
            assert_eq!(replacement.len(), 1);
            assert!(!run.is_complete(), "accepted run survives the failed wake");
            replacement.set_frame_requester(None);
        }
        let next_run = owner
            .controller()
            .reverse()
            .expect("next operation after recovery");
        let mut clock = flui_animation::MotionClock::new();
        replacement.tick_all(&clock.frame(Duration::ZERO));
        replacement.tick_all(&clock.frame(Duration::from_secs(1)));
        assert_eq!(owner.controller().value(), 0.0);
        assert!(next_run.is_complete());
    }
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

fn disposal_registry_custody(failure: &str) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture {
        controller: AnimationController,
        drops: Rc<Cell<usize>>,
        refused: Rc<Cell<bool>>,
        panics: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            self.refused.set(self.controller.forward().is_err());
            assert!(!self.panics, "registry retirement");
        }
    }

    let registry = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller().clone();
    let drops = Rc::new(Cell::new(0));
    let refused = Rc::new(Cell::new(false));
    let capture = Capture {
        controller: controller.clone(),
        drops: drops.clone(),
        refused: refused.clone(),
        panics: failure != "none",
    };
    registry.set_frame_requester(Some(Rc::new(move || {
        let _ = &capture;
    })));
    let mut run = controller.forward().expect("bound run");
    let deliveries = Rc::new(Cell::new(0));
    if failure == "cancellation" {
        run.when_complete_or_cancel(|_| panic!("run cancellation"));
    }
    let healthy = deliveries.clone();
    run.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        healthy.set(healthy.get() + 1);
    });
    drop(registry);

    let result = catch_unwind(AssertUnwindSafe(|| owner.dispose()));
    if failure == "none" {
        assert!(result.is_ok());
    } else {
        let payload = result.expect_err("the first failure resumes after cleanup");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&if failure == "registry" {
                "registry retirement"
            } else {
                "run cancellation"
            })
        );
    }
    assert!(!owner.is_bound());
    assert!(controller.forward().is_err(), "the kernel stays closed");
    assert_eq!(
        deliveries.get(),
        1,
        "healthy cancellation delivery finishes"
    );
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(_))
    ));
    if failure == "cancellation" {
        assert_eq!(
            drops.get(),
            0,
            "outgoing registry retains first-failure custody"
        );
    } else {
        assert_eq!(drops.get(), 1);
        assert!(refused.get(), "retirement observes the closed kernel");
    }
    owner.dispose();
    assert_eq!(deliveries.get(), 1, "the next disposal is inert");
}

fn healthy_disposal_commits_before_registry_retirement() {
    disposal_registry_custody("none");
}

fn failing_registry_retirement_observes_the_disposed_kernel() {
    disposal_registry_custody("registry");
}

fn cancellation_failure_retains_the_outgoing_registry() {
    disposal_registry_custody("cancellation");
}

fn disposal_closes_the_kernel_before_retiring_the_registry() {
    crate::run_table(&[
        (
            "healthy retirement",
            healthy_disposal_commits_before_registry_retirement,
        ),
        (
            "registry retirement failure",
            failing_registry_retirement_observes_the_disposed_kernel,
        ),
        (
            "cancellation and retirement competition",
            cancellation_failure_retains_the_outgoing_registry,
        ),
    ]);
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
            "disposal closes the kernel before registry retirement",
            disposal_closes_the_kernel_before_retiring_the_registry,
        ),
        (
            "rebind preserves delivery failure before outgoing retirement",
            rebinding_preserves_delivery_failure_before_outgoing_retirement,
        ),
        (
            "rebind commits the clock before retiring the outgoing registry",
            rebinding_retires_the_old_registry_after_committing_the_new_clock,
        ),
        (
            "run restart and rate resume request samples",
            runs_and_rate_changes_request_samples,
        ),
        (
            "nested mute and rebind preserve the addressed driver",
            nested_mute_and_rebind_address_the_live_driver,
        ),
        (
            "wake failure preserves delivery and recovery",
            wake_failure_preserves_delivery_and_recovery,
        ),
        (
            "wake replacement retires captures outside borrows",
            wake_replacement_retires_captures_outside_borrows,
        ),
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

fn counted_registry() -> (Vsync, std::rc::Rc<std::cell::Cell<usize>>) {
    let registry = Vsync::new();
    let wakes = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = wakes.clone();
    registry.set_frame_requester(Some(std::rc::Rc::new(move || {
        observed.set(observed.get() + 1);
    })));
    (registry, wakes)
}

fn wake_replacement_retires_captures_outside_borrows() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Capture {
        registry: Vsync,
        retired: Rc<Cell<usize>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert!(
                self.registry.has_running(),
                "capture can reenter the registry during retirement"
            );
            self.retired.set(self.retired.get() + 1);
        }
    }
    let registry = Vsync::new();
    let retired = Rc::new(Cell::new(0));
    let capture = Capture {
        registry: registry.clone(),
        retired: retired.clone(),
    };
    let wakes = Rc::new(Cell::new(0));
    let replacement_wakes = wakes.clone();
    registry.set_frame_requester(Some(Rc::new(move || {
        let observed = replacement_wakes.clone();
        capture
            .registry
            .set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
        let _ = &capture;
    })));
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    owner.controller().forward().unwrap();
    assert_eq!(retired.get(), 1, "the replaced hook releases its captures");
    assert_eq!(
        wakes.get(),
        1,
        "replacement delivers the accepted running demand"
    );
    owner.controller().forward().unwrap();
    assert_eq!(wakes.get(), 2);
    registry.set_frame_requester(None);
    let run = owner.controller().forward().unwrap();
    let observed = wakes.clone();
    registry.set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
    assert_eq!(
        wakes.get(),
        3,
        "missing hook keeps demand for later installation"
    );
    let mut clock = flui_animation::MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(run.is_complete());
}

fn runs_and_rate_changes_request_samples() {
    use flui_animation::{MotionClock, PlaybackRate};
    let (registry, wakes) = counted_registry();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    assert_eq!(wakes.get(), 0, "an idle registration does not wake");
    owner.controller().forward().unwrap();
    let run = owner.controller().forward().unwrap();
    owner.controller().set_playback_rate(PlaybackRate::NORMAL);
    assert_eq!(
        wakes.get(),
        2,
        "a same-status restart requests its frame; an unchanged rate adds no demand"
    );
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    owner.controller().set_playback_rate(PlaybackRate::PAUSED);
    assert_eq!(wakes.get(), 3);
    registry.tick_all(&clock.frame(Duration::from_millis(600)));
    assert!(!registry.has_running(), "pause releases continuous demand");
    registry.tick_all(&clock.frame(Duration::from_secs(10)));
    owner.controller().set_playback_rate(PlaybackRate::NORMAL);
    assert_eq!(
        wakes.get(),
        4,
        "resume requests the rate application sample"
    );
    registry.tick_all(&clock.frame(Duration::from_secs(11)));
    assert!(
        (owner.controller().value() - 0.6).abs() < 1e-9,
        "pause time is excluded"
    );
    registry.tick_all(&clock.frame(Duration::from_millis(11_400)));
    assert!(run.is_complete());
    assert_eq!(owner.controller().value(), 1.0);
    assert_eq!(
        wakes.get(),
        4,
        "sampling does not manufacture new wake requests"
    );
}

fn nested_mute_and_rebind_address_the_live_driver() {
    let (a, a_wakes) = counted_registry();
    let (b, b_wakes) = counted_registry();
    let child = Vsync::new();
    let seat = a.attach_child(&child).expect("child admitted");
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    a.set_muted(true);
    owner.controller().forward().unwrap();
    assert_eq!(
        a_wakes.get(),
        0,
        "an unmuted descendant cannot wake a muted ancestor"
    );
    a.set_muted(false);
    assert_eq!(a_wakes.get(), 1, "unmute requests the retained run");
    child.set_muted(true);
    owner.controller().forward().unwrap();
    assert_eq!(a_wakes.get(), 1);
    a.detach_child(&seat);
    child.set_muted(false);
    assert_eq!(
        a_wakes.get(),
        1,
        "a detached child cannot wake its old parent"
    );
    owner.rebind(Some(&b)).unwrap();
    assert_eq!(b_wakes.get(), 1, "a live run requests its new driver");
    owner.controller().forward().unwrap();
    assert_eq!((a_wakes.get(), b_wakes.get()), (1, 2));
    let observer = owner.controller().clone();
    drop(owner);
    observer.set_playback_rate(flui_animation::PlaybackRate::NORMAL);
    assert_eq!(
        (a_wakes.get(), b_wakes.get()),
        (1, 2),
        "retired observer clones cannot wake either driver"
    );
}

fn wake_failure_preserves_delivery_and_recovery() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    for competing in [false, true] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let inspected = owner.controller().clone();
        registry.set_frame_requester(Some(Rc::new(move || {
            assert!(inspected.is_animating(), "run admission precedes wake");
            assert_eq!(inspected.status(), AnimationStatus::Forward);
            panic!("wake failure");
        })));
        owner
            .controller()
            .add_status_listener(Rc::new(move |status| {
                assert!(
                    !(competing && status == AnimationStatus::Forward),
                    "listener failure"
                );
            }));
        let delivered = Rc::new(Cell::new(0));
        let observed = delivered.clone();
        owner
            .controller()
            .add_status_listener(Rc::new(move |_| observed.set(observed.get() + 1)));
        let failure = catch_unwind(AssertUnwindSafe(|| owner.controller().forward()));
        let payload = failure.expect_err("wake failure propagates after delivery");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"wake failure"));
        assert_eq!(
            delivered.get(),
            1,
            "healthy status tail is delivered despite both failures"
        );
        assert!(
            owner.controller().is_animating(),
            "accepted run survives wake failure"
        );
        let recovered = Rc::new(Cell::new(0));
        let observed = recovered.clone();
        registry.set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
        let run = owner.controller().forward().unwrap();
        assert_eq!(
            recovered.get(),
            2,
            "replacement and same-status restart remain deliverable"
        );
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_secs(1)));
        assert!(run.is_complete());
    }
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
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_listener(std::rc::Rc::new(move || {
        let _ = &capture;
    }));

    lifecycle.dispose();

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
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let parent = lifecycle.controller().clone();
    parent.set_value(0.5);
    lifecycle.dispose();
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
    drop(parent);
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
    drop(current);
    drop(next);
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
    drop(current);
    drop(next);

    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the proxy, the switch and on_switched are released"
    );
}
