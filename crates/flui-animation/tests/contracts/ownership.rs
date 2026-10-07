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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, CurvedAnimation, Curves,
    FloatTween, ProxyAnimation, ReverseAnimation, TweenAnimation, UpdateScheduler, Vsync,
};
use flui_foundation::Listenable;

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::without_ticker(Duration::from_secs(1))
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

// --- the frame-scheduled hook -----------------------------------------------------

fn frame_hook_reads_the_controller() {
    let scheduler = UpdateScheduler::new();
    let slot: Arc<OnceLock<AnimationController>> = Arc::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let (hook_slot, hook_calls) = (Arc::clone(&slot), Arc::clone(&calls));
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        if let Some(controller) = hook_slot.get() {
            let _ = (controller.value(), controller.status());
            hook_calls.fetch_add(1, Ordering::SeqCst);
        }
    })));
    let controller = AnimationController::new(Duration::from_secs(1), &scheduler);
    assert!(slot.set(controller.clone()).is_ok(), "the slot was empty");

    let _run = controller.forward().expect("run starts");

    assert_eq!(controller.status(), AnimationStatus::Forward);
    assert!(
        calls.load(Ordering::SeqCst) > 0,
        "starting the run scheduled a frame"
    );
    controller.dispose();
}

fn frame_hook_queries_the_vsync_registry() {
    let scheduler = UpdateScheduler::new();
    let vsync = Vsync::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let (hook_vsync, hook_calls) = (vsync.clone(), Arc::clone(&calls));
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        let _ = hook_vsync.has_running();
        hook_calls.fetch_add(1, Ordering::SeqCst);
    })));
    let controller = AnimationController::new(Duration::from_secs(1), &scheduler);
    let registration = vsync.register(controller.clone());

    let _run = controller.forward().expect("run starts");

    assert_eq!(controller.status(), AnimationStatus::Forward);
    assert!(
        calls.load(Ordering::SeqCst) > 0,
        "starting the run scheduled a frame"
    );
    assert!(vsync.has_running());
    vsync.unregister(&registration);
    controller.dispose();
}

#[test]
#[ignore = "contract: the frame-scheduled hook may read the controller whose run it schedules"]
fn frame_scheduled_hook_may_read_the_controller() {
    child_process::run_single(
        "ownership::frame_scheduled_hook_may_read_the_controller",
        frame_hook_reads_the_controller,
    );
}

#[test]
#[ignore = "contract: the frame-scheduled hook may query the Vsync registry of the controller it schedules"]
fn frame_scheduled_hook_may_query_vsync() {
    child_process::run_single(
        "ownership::frame_scheduled_hook_may_query_vsync",
        frame_hook_queries_the_vsync_registry,
    );
}

// --- controller ---------------------------------------------------------------------

#[test]
#[ignore = "contract: dispose releases the controller's value listeners"]
fn dispose_releases_value_listeners() {
    let controller = controller();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_listener(Arc::new(move || {
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
#[ignore = "contract: dropping the last handle of a running controller releases it and cancels the run"]
fn last_handle_drop_releases_a_running_controller() {
    let scheduler = UpdateScheduler::new();
    let (drops, probe) = drop_probe();
    let controller = AnimationController::new(Duration::from_secs(1), &scheduler);
    controller.add_status_listener(Arc::new(move |_| {
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

/// Adds a status listener through `wrapper`, drops the wrapper while its parent
/// stays alive, and asserts the listener's capture was released.
fn wrapper_status_listener_dies_with_the_wrapper<W>(wrap: fn(Arc<dyn Animation<f64>>) -> W)
where
    W: Animation<f64>,
{
    let parent = controller();
    let probe = Arc::new(());
    {
        let wrapper = wrap(Arc::new(parent.clone()));
        let capture = Arc::clone(&probe);
        let _id = wrapper.add_status_listener(Arc::new(move |_| {
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
#[ignore = "contract: a status listener added through a ReverseAnimation dies with it"]
fn reverse_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(ReverseAnimation::new);
}

#[test]
#[ignore = "contract: a status listener added through a CurvedAnimation dies with it"]
fn curved_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        CurvedAnimation::new(parent, Curves::EaseIn)
    });
}

#[test]
#[ignore = "contract: a status listener added through a TweenAnimation dies with it"]
fn tween_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        TweenAnimation::new(FloatTween::new(0.0, 10.0), parent)
    });
}

// --- switch ---------------------------------------------------------------------------

#[test]
#[ignore = "contract: AnimationSwitch::dispose releases on_switched and its status listeners"]
fn switch_dispose_releases_callbacks() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (switched, listened) = (Arc::new(()), Arc::new(()));
    let switched_capture = Arc::clone(&switched);
    let switch = AnimationSwitch::new(Arc::new(current.clone()), Some(Arc::new(next.clone())))
        .on_switched(move || {
            let _ = &switched_capture;
        });
    let listened_capture = Arc::clone(&listened);
    let _id = switch.add_status_listener(Arc::new(move |_| {
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
#[ignore = "contract: a proxy parented to a switch that captures it is freed once the switch is disposed"]
fn proxy_parented_to_a_capturing_switch_is_freed() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (drops, probe) = drop_probe();
    {
        let proxy = Arc::new(ProxyAnimation::new(
            Arc::new(current.clone()) as Arc<dyn Animation<f64>>
        ));
        let hop_proxy = Arc::clone(&proxy);
        let target: Arc<dyn Animation<f64>> = Arc::new(next.clone());
        let switch = AnimationSwitch::new(Arc::new(current.clone()), Some(Arc::clone(&target)))
            .on_switched(move || {
                let _ = &probe;
                hop_proxy.set_parent(Arc::clone(&target));
            });
        proxy.set_parent(Arc::new(switch.clone()));
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
