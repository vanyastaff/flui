//! Owner-entry regressions for the local post-frame lane.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_interaction::InteractionDispatchError;
use flui_interaction::testing::input::{device_kind_from_button, pointer_down};
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
