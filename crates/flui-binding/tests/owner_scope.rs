//! Owner-entry regressions for the local post-frame lane.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_interaction::testing::input::{device_kind_from_button, pointer_down};
use flui_interaction::{GestureRecognizer, PointerId, TapGestureRecognizer};
use flui_interaction::{HitTestResult, InteractionDispatchError};
use flui_types::Offset;
use flui_types::geometry::px;
use flui_view::BuildOwner;

#[test]
fn pointer_route_runs_inside_the_binding_owner_scope() {
    let mut binding = HeadlessBinding::new();
    let mut owner = BuildOwner::new();
    binding.install_build_capabilities(&mut owner);
    let handle = owner
        .post_frame_handle()
        .expect("the binding installs its post-frame capability");
    let fired = Rc::new(Cell::new(false));
    let callback_fired = Rc::clone(&fired);
    let event = pointer_down(Offset::new(px(4.0), px(7.0)), device_kind_from_button(0));

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
fn interaction_registration_requires_the_binding_owner_scope() {
    let binding = HeadlessBinding::new();
    let handle = binding.interaction_dispatch_handle();

    assert!(matches!(
        handle.register_pointer(|_| {}),
        Err(InteractionDispatchError::InactiveRealm)
    ));

    binding.enter_owner_scope(|| {
        handle
            .register_pointer(|_| {})
            .expect("owner scope activates the binding interaction lane");
    });
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
    recognizer.add_pointer(pointer, Offset::new(px(4.0), px(7.0)));
    assert!(binding.arena().is_open(pointer));

    let event = pointer_down(Offset::new(px(4.0), px(7.0)), device_kind_from_button(0));
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

struct CountingMember(Arc<AtomicUsize>);

impl flui_interaction::sealed::CustomGestureRecognizer for CountingMember {
    fn on_arena_accept(&self, _pointer: PointerId) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn on_arena_reject(&self, _pointer: PointerId) {}
}

#[test]
fn pointer_event_boundary_drains_a_lone_deferred_winner() {
    let binding = HeadlessBinding::new();
    let accepted = Arc::new(AtomicUsize::new(0));
    let event = pointer_down(Offset::new(px(4.0), px(7.0)), device_kind_from_button(0));

    binding.dispatch_pointer(&event, |_| {
        binding.arena().add(
            PointerId::PRIMARY,
            Arc::new(CountingMember(accepted.clone())),
        );
        HitTestResult::new()
    });

    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    assert!(binding.arena().is_empty());
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
    let event = pointer_down(Offset::new(px(2.0), px(3.0)), device_kind_from_button(0));

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
