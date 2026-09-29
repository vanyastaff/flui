//! Owner-entry regressions for the local post-frame lane.

use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::geometry::Offset;
use flui_interaction::InteractionDispatchError;
use flui_interaction::testing::input::{device_kind_from_button, pointer_down};
use flui_interaction::{GestureRecognizer, PointerId, TapGestureRecognizer};
use flui_testing::HeadlessBinding;

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
