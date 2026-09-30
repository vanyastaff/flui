//! The composition-root seam (`flui_view::__runtime`, ADR-0081 §4) driven
//! from outside the crate, the way `flui-runtime`, `flui-testing` and
//! `flui-hot-reload` use it: always compiled, no cargo feature involved.

use std::{cell::Cell, rc::Rc};

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
