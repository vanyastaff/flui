//! The composition-root seam (`flui_view::__runtime`, ADR-0081 §4) driven
//! from outside the crate, the way `flui-runtime`, `flui-testing` and
//! `flui-hot-reload` use it: always compiled, no cargo feature involved.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use flui_foundation::ElementId;
use flui_view::{
    __runtime::{BindingRuntime as _, FramePhaseMarker, GlobalKeyRegistryComposite},
    AppLifecycleState, GlobalKey, LifecycleClosed, WidgetsBinding,
};

#[test]
fn with_global_key_registry_resolves_this_bindings_keys_only_while_entered() {
    let binding = WidgetsBinding::new();
    let key = GlobalKey::<()>::new();
    let id = ElementId::new(7);
    binding.with_build_owner_mut(|owner| owner.register_global_key(&key, id));

    assert_eq!(key.current_element(), None);
    assert_eq!(
        binding.with_global_key_registry(|| key.current_element()),
        Some(id)
    );
    assert_eq!(key.current_element(), None);
}

#[test]
fn composite_resolves_a_key_held_by_any_member() {
    let first = WidgetsBinding::new();
    let second = WidgetsBinding::new();
    let key = GlobalKey::<()>::new();
    let id = ElementId::new(11);
    second.with_build_owner_mut(|owner| owner.register_global_key(&key, id));

    let composite = GlobalKeyRegistryComposite::assemble([&first, &second]);
    assert_eq!(composite.enter(|| key.current_element()), Some(id));
    assert_eq!(key.current_element(), None);
}

#[test]
fn draw_frame_with_phase_marker_stamps_the_finalize_phase() {
    let binding = WidgetsBinding::new();
    let marker = FramePhaseMarker::new(0_u8);
    marker.set(1);
    assert_eq!(marker.get(), 1);

    binding.draw_frame_with_phase_marker(&marker, 2);
    assert_eq!(marker.get(), 2);
}

#[test]
fn terminal_lifecycle_ladder_runs_through_the_seam() {
    use AppLifecycleState::{Detached, Resumed};

    let binding = WidgetsBinding::new();
    binding.handle_app_lifecycle_state_changed(Resumed);
    let handle = binding.lifecycle_source().handle();
    let seen = Rc::new(Cell::new(None));
    let log = Rc::clone(&seen);
    let (snapshot, _subscription) = handle
        .subscribe(move |state| log.set(Some(state)))
        .expect("an open lifecycle accepts subscriptions");
    assert_eq!(snapshot, Some(Resumed));

    binding.lifecycle_source().begin_close();
    // An ordinary commit is fenced once terminal close begins.
    assert_eq!(
        binding.lifecycle_source().commit(Resumed),
        Err(LifecycleClosed)
    );
    binding
        .lifecycle_source()
        .commit_terminal(Detached)
        .expect("terminal commit after begin_close");
    assert_eq!(binding.lifecycle_source().current(), Some(Detached));
    binding.notify_committed_lifecycle(Detached);
    assert_eq!(seen.get(), Some(Detached));

    binding.lifecycle_source().finish_close();
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
}

#[test]
fn a_frame_callback_can_install_its_replacement() {
    thread_local! {
        static BINDING: RefCell<Option<WidgetsBinding>> = const { RefCell::new(None) };
    }

    let (result_tx, result_rx) = mpsc::channel();
    thread::spawn(move || {
        BINDING.with(|slot| {
            slot.replace(Some(WidgetsBinding::new()));
        });
        let first_calls = Arc::new(AtomicUsize::new(0));
        let replacement_calls = Arc::new(AtomicUsize::new(0));

        BINDING.with(|slot| {
            let binding = slot.borrow();
            let binding = binding.as_ref().expect("binding installed above");
            let first_calls = Arc::clone(&first_calls);
            let replacement_calls = Arc::clone(&replacement_calls);
            binding.set_on_need_frame(move || {
                first_calls.fetch_add(1, Ordering::Relaxed);
                let replacement_calls = Arc::clone(&replacement_calls);
                BINDING.with(|slot| {
                    let binding = slot.borrow();
                    let binding = binding.as_ref().expect("binding outlives its callback");
                    binding.set_on_need_frame(move || {
                        replacement_calls.fetch_add(1, Ordering::Relaxed);
                    });
                });
            });
            binding.handle_build_scheduled();
            binding.handle_build_scheduled();
        });

        result_tx
            .send((
                first_calls.load(Ordering::Relaxed),
                replacement_calls.load(Ordering::Relaxed),
            ))
            .expect("the test awaits the callback counts");
    });

    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(2)),
        Ok((1, 1)),
        "the callback must replace itself without deadlocking"
    );
}

#[test]
fn a_frame_callback_can_capture_owner_local_state() {
    let binding = WidgetsBinding::new();
    let calls = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);
    binding.set_on_need_frame(move || callback_calls.set(callback_calls.get() + 1));

    binding.handle_build_scheduled();

    assert_eq!(calls.get(), 1);
}
