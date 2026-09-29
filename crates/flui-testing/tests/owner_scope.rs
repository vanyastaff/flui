//! Owner-entry regressions for the local post-frame lane.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::testing::input::{device_kind_from_button, pointer_down};
use flui_interaction::{GestureRecognizer, PointerId, TapGestureRecognizer};
use flui_interaction::{HitTestResult, InteractionDispatchError};
use flui_testing::HeadlessBinding;
use flui_view::BuildOwner;

#[test]
fn pointer_route_runs_inside_the_binding_owner_scope() {
    let mut binding = HeadlessBinding::new();
    let mut owner = BuildOwner::new();
    binding.install_build_capabilities(&mut owner);
    let handle = owner
        .local_post_frame_handle()
        .expect("the binding installs its owner-local post-frame capability")
        .clone();
    let fired = Rc::new(Cell::new(false));
    let callback_fired = Rc::clone(&fired);
    let event = pointer_down(Offset::new(4.0, 7.0), device_kind_from_button(0));

    binding.dispatch_pointer(&event, move |_| {
        handle
            .schedule_local(move |_| callback_fired.set(true))
            .expect("the pointer route is an owner entry");
        HitTestResult::new()
    });
    binding.pump_frame(Duration::ZERO);

    assert!(fired.get(), "the queued local callback must be drained");
}

#[test]
fn interaction_targets_are_isolated_between_headless_bindings() {
    let first = HeadlessBinding::new();
    let second = HeadlessBinding::new();
    let first_handle = first.interaction_dispatch_handle();
    let second_handle = second.interaction_dispatch_handle();

    let target = first.enter_owner_scope(|| {
        first_handle
            .register_pointer(|_| {})
            .expect("first binding registers its own target")
    });

    second.enter_owner_scope(|| {
        assert!(matches!(
            second_handle.replace_pointer(target, |_| {}),
            Err(InteractionDispatchError::WrongRealm)
        ));
    });
}

#[test]
fn pointer_route_panic_still_runs_the_down_arena_lifecycle() {
    let binding = HeadlessBinding::new();
    let pointer = PointerId::PRIMARY;
    let recognizer = TapGestureRecognizer::new(binding.arena().clone());
    recognizer.add_pointer(pointer, Offset::new(4.0, 7.0), Offset::new(4.0, 7.0));
    assert!(binding.arena().is_open(pointer));

    let event = pointer_down(Offset::new(4.0, 7.0), device_kind_from_button(0));
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        binding.dispatch_pointer(&event, |_| panic!("route panic"));
    }));

    let payload = unwind.expect_err("the route panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"route panic"));
    assert!(
        !binding.arena().is_open(pointer),
        "Down must close the arena before the route panic resumes"
    );
}

struct PanickingPayloadDrop;

impl Drop for PanickingPayloadDrop {
    fn drop(&mut self) {
        panic!("secondary payload drop panic");
    }
}

struct LifecyclePanicsWithHostilePayload;

impl flui_interaction::sealed::CustomGestureRecognizer for LifecyclePanicsWithHostilePayload {
    fn on_arena_accept(&self, _pointer: PointerId) {
        panic_any(PanickingPayloadDrop);
    }

    fn on_arena_reject(&self, _pointer: PointerId) {}
}

#[test]
fn hostile_secondary_lifecycle_payload_cannot_replace_the_route_panic() {
    let binding = HeadlessBinding::new();
    let pointer = PointerId::PRIMARY;
    binding
        .arena()
        .add(pointer, Arc::new(LifecyclePanicsWithHostilePayload));
    let event = pointer_down(Offset::new(2.0, 3.0), device_kind_from_button(0));

    let unwind = catch_unwind(AssertUnwindSafe(|| {
        binding.dispatch_pointer(&event, |_| panic!("first route panic"));
    }));
    let payload = unwind.expect_err("the first route panic must propagate");

    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"first route panic"),
        "dropping a hostile secondary payload must not replace the route panic"
    );
    assert!(!binding.arena().is_open(pointer));
}
