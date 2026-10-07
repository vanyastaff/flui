//! Frame-path state: reads from inside a callout see the last commit, a nested
//! commit is not overwritten by the commit that called out, and a frame
//! survives user code that disposes or releases the controllers it is walking.
//!
//! The rows run in child processes: a regression here shows as a deadlock or
//! an abort, not a failed assertion.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, TickerFuture, Vsync, VsyncRegistration,
};
use flui_foundation::Listenable;

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::without_ticker(Duration::from_secs(1))
}

fn poll(run: &mut TickerFuture) -> Poll<Result<(), flui_animation::TickerCanceled>> {
    Pin::new(run).poll(&mut Context::from_waker(Waker::noop()))
}

fn counter() -> (Arc<Mutex<usize>>, impl Fn() + Send + Sync + 'static) {
    let count = Arc::new(Mutex::new(0));
    let sink = Arc::clone(&count);
    (count, move || *sink.lock().expect("tick count") += 1)
}

fn ticks(count: &Arc<Mutex<usize>>) -> usize {
    *count.lock().expect("tick count")
}

fn reads_inside_a_status_listener_see_the_commit() {
    let controller = controller();
    let observed = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&observed);
    let reader = controller.clone();
    controller.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Completed {
            *sink.lock().expect("observed read") =
                Some((reader.value(), reader.status(), format!("{reader:?}")));
        }
    }));
    let _run = controller.forward().expect("run starts");
    controller.tick_at(1.0);

    let (value, status, debug) = observed
        .lock()
        .expect("observed read")
        .take()
        .expect("the listener saw Completed");
    assert_eq!((value, status), (1.0, AnimationStatus::Completed));
    assert!(debug.contains("Completed"), "{debug}");
}

fn nested_commit_is_not_overwritten() {
    let controller = controller();
    let nested = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&nested);
    let slot = Arc::new(Mutex::new(Some(controller.clone())));
    controller.add_status_listener(Arc::new(move |status| {
        if status != AnimationStatus::Completed {
            return;
        }
        let owner = slot.lock().expect("retarget slot").take();
        if let Some(controller) = owner {
            let _run = controller
                .animate_to(0.3, Some(Duration::from_secs(1)))
                .expect("retarget from a listener");
            *sink.lock().expect("nested commit") = Some(controller.status());
        }
    }));
    let _run = controller.forward().expect("run starts");

    controller.tick_at(1.0);

    let nested = nested
        .lock()
        .expect("nested commit")
        .take()
        .expect("the listener retargeted");
    assert_eq!(
        controller.status(),
        nested,
        "the nested commit stays published"
    );
    assert!(controller.status().is_running());
    assert_eq!(
        controller.value(),
        1.0,
        "the new run starts where the last ended"
    );
}

fn dispose_from_a_value_listener_mid_walk() {
    let vsync = Vsync::new();
    let disposed = controller();
    let sibling = controller();
    let _registrations = [
        vsync.register(disposed.clone()),
        vsync.register(sibling.clone()),
    ];
    let mut run = disposed.forward().expect("run starts");
    let _sibling_run = sibling.forward().expect("run starts");
    let reader = disposed.clone();
    disposed.add_listener(Arc::new(move || {
        if reader.value() >= 0.5 {
            reader.dispose();
        }
    }));

    vsync.tick_all(0.0);
    vsync.tick_all(0.5);
    assert!(
        matches!(poll(&mut run), Poll::Ready(Err(_))),
        "disposing from a listener cancels the run"
    );
    vsync.tick_all(0.75);

    assert_eq!(
        disposed.value(),
        0.5,
        "a disposed controller is not ticked again"
    );
    assert_eq!(
        sibling.value(),
        0.75,
        "the rest of the registry keeps ticking"
    );
}

fn last_owner_released_from_its_own_listener_mid_walk() {
    let vsync = Vsync::new();
    let first = AnimationController::without_ticker(Duration::from_millis(500));
    let second = controller();
    let registrations: Vec<VsyncRegistration> = vec![
        vsync.register(first.clone()),
        vsync.register(second.clone()),
    ];
    let _first_run = first.forward().expect("run starts");
    let _second_run = second.forward().expect("run starts");
    let (second_ticks, count) = counter();
    second.add_listener(Arc::new(count));

    let owners = Arc::new(Mutex::new(Some((
        vec![first.clone(), second],
        registrations,
    ))));
    let registry = vsync.clone();
    first.add_status_listener(Arc::new(move |status| {
        if status != AnimationStatus::Completed {
            return;
        }
        let released = owners.lock().expect("owners").take();
        if let Some((controllers, registrations)) = released {
            for registration in &registrations {
                registry.unregister(registration);
            }
            drop(controllers);
        }
    }));
    drop(first);

    vsync.tick_all(0.0);
    let before = ticks(&second_ticks);
    vsync.tick_all(0.5);
    assert_eq!(
        ticks(&second_ticks),
        before,
        "a controller released earlier in the walk is not ticked"
    );
    assert_eq!(vsync.len(), 0);
    assert!(!vsync.has_running());
    vsync.tick_all(0.75);
    assert_eq!(ticks(&second_ticks), before);
}

#[test]
fn frame_path_reentry() {
    let cases: &[(&str, fn())] = &[
        (
            "reads inside a status listener see the commit",
            reads_inside_a_status_listener_see_the_commit,
        ),
        (
            "nested commit is not overwritten",
            nested_commit_is_not_overwritten,
        ),
        (
            "dispose from a value listener mid walk",
            dispose_from_a_value_listener_mid_walk,
        ),
        (
            "last owner released from its own listener mid walk",
            last_owner_released_from_its_own_listener_mid_walk,
        ),
    ];
    if let Some(selected) = child_process::selected_case() {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        child_process::pass();
    }
    let names: Vec<_> = cases.iter().map(|(name, _)| *name).collect();
    child_process::run_rows("frame_path::frame_path_reentry", &names);
}
