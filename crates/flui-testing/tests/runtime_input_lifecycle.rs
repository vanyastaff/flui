//! Addressed input advances at the production realm frame and lifecycle boundaries.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::{Offset, Point};
use flui_foundation::{ManualClock, PresentationId};
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event, make_move_event, make_move_event_for_id,
    make_up_event,
};
use flui_interaction::{GestureArenaMember, PointerId};
use flui_platform_api::{PlatformInput, PlatformWindow, WindowExecutionState};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_runtime::sink::SubmitVerdict;
use flui_runtime::testing::{ScriptedSink, TestWindow};
use flui_runtime::ui_realm::UiRealm;
use flui_scheduler::AppLifecycleState;
use flui_view::prelude::*;
use flui_widgets::{Align, GestureDetector, Listener, MouseRegion, SizedBox};

fn pump(realm: &mut UiRealm) {
    let _ = realm.pump(
        &mut ManualClock::default(),
        &mut ScriptedSink::always_presents(),
    );
}

fn pump_uncommitted(realm: &mut UiRealm) {
    let outcome = realm.pump(
        &mut ManualClock::default(),
        &mut ScriptedSink::single_shot(SubmitVerdict::Retry),
    );
    assert!(
        !outcome.presented(),
        "the initial surface did not acknowledge the tree"
    );
}

fn dispatch(realm: &UiRealm, id: PresentationId, event: PointerEvent) {
    realm.enter(|realm| {
        realm.handle_input_addressed(id, PlatformInput::Pointer(event));
    });
}

fn install_secondary(realm: &mut UiRealm) -> PresentationId {
    let window: Arc<dyn PlatformWindow> = Arc::new(TestWindow::new().focused(false));
    let presentation = realm.assemble_presentation(window);
    realm.install_presentation(presentation)
}

fn hover() -> PointerEvent {
    let mut event =
        make_move_event(Offset::new(8.0, 9.0), PointerKind::Mouse).expect("finite test position");
    if let PointerEvent::Move(update) = &mut event {
        update.buttons = flui_platform_api::pointer::PointerButtons::default();
    }
    event
}

pub(crate) fn mouse_motion_precedes_keyboard_without_a_frame() {
    assert_motion_keyboard_order(PointerKind::Mouse, false, false, false);
}

pub(crate) fn ime_commit_observes_preceding_measured_motion() {
    assert_ime_motion_order(false);
}

pub(crate) fn runtime_keyboard_barrier_preserves_scale_contacts_and_continuity() {
    use flui_interaction::events::{make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::routing::KeyEventResult;
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform_api::keyboard::Code;
    use flui_runtime::presentation::PointerResampling;

    for policy in [PointerResampling::Disabled, PointerResampling::FrameAligned] {
        let mut realm = UiRealm::for_test();
        let primary = realm.presentation_id();
        realm
            .set_pointer_resampling(primary, policy)
            .expect("policy");
        let scale = Rc::new(Cell::new(1.0));
        let ends = Rc::new(Cell::new(0));
        let updates = Rc::clone(&scale);
        let ended = Rc::clone(&ends);
        realm
            .attach_root_widget(
                &GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_scale_update(move |_, details| updates.set(details.scale))
                    .on_scale_end(move |_, _| ended.set(ended.get() + 1))
                    .child(SizedBox::new(400.0, 40.0)),
            )
            .expect("scale consumer");
        realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
        pump(&mut realm);
        let key_scale = Rc::clone(&scale);
        let reads = Rc::new(RefCell::new(Vec::new()));
        let read = Rc::clone(&reads);
        realm
            .focus_manager()
            .add_global_key_handler(Rc::new(move |_| {
                read.borrow_mut().push(key_scale.get());
                KeyEventResult::Handled
            }));
        let one = PointerId::try_from(1_u64).expect("contact");
        let two = PointerId::try_from(2_u64).expect("contact");
        for (id, x) in [(one, 100.0), (two, 300.0)] {
            dispatch(
                &realm,
                primary,
                make_down_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("down"),
            );
        }
        let key = || {
            realm.enter(|realm| {
                realm.handle_input_addressed(
                    primary,
                    PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
                );
            });
        };
        for (left, right, expected) in [(50.0, 350.0, 1.5), (25.0, 375.0, 1.75)] {
            for (id, x) in [(one, left), (two, right)] {
                dispatch(
                    &realm,
                    primary,
                    make_move_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch)
                        .expect("motion"),
                );
            }
            key();
            assert!(
                (scale.get() - expected).abs() < 1e-9,
                "{policy:?}: continuous measured scale {}",
                scale.get()
            );
            assert_eq!(ends.get(), 0, "a causal barrier cannot end live contacts");
            assert_eq!(
                *reads.borrow().last().expect("key reads scale"),
                scale.get()
            );
        }
        for (id, x) in [(one, 25.0), (two, 375.0)] {
            dispatch(
                &realm,
                primary,
                make_up_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch)
                    .expect("terminal"),
            );
        }
        assert_eq!(ends.get(), 1, "native contact terminals finish one scale");
    }
}

pub(crate) fn keyboard_motion_barrier_uses_resolved_focus_owner_during_reentrant_focus_change() {
    use flui_interaction::routing::{FocusNode, KeyEventResult};
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform_api::keyboard::Code;
    use flui_runtime::presentation::PointerResampling;
    use flui_widgets::Focus;
    use std::rc::Weak;

    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let secondary = install_secondary(&mut realm);
    realm
        .set_pointer_resampling(secondary, PointerResampling::FrameAligned)
        .expect("policy");
    let owner = Rc::new(RefCell::new(Weak::<UiRealm>::new()));
    let changing_owner = Rc::clone(&owner);
    let state = Rc::new(Cell::new(0.0));
    let moved = Rc::clone(&state);
    let secondary_state = Rc::clone(&state);
    let observations = Rc::new(RefCell::new(Vec::new()));
    let secondary_keys = Rc::clone(&observations);
    let node = FocusNode::new();
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &Focus::new(
                Listener::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pointer_move(move |_, event| {
                        let PointerEvent::Move(event) = event.global else {
                            panic!("motion");
                        };
                        moved.set(event.current().position.get().x);
                        changing_owner
                            .borrow()
                            .upgrade()
                            .expect("owner")
                            .synchronize_window_snapshot(
                                primary,
                                WindowExecutionState::Running,
                                true,
                                true,
                            );
                    })
                    .child(SizedBox::new(100.0, 40.0)),
            )
            .focus_node(Rc::clone(&node))
            .on_key_event(move |_, _| {
                secondary_keys
                    .borrow_mut()
                    .push(("secondary", secondary_state.get()));
                KeyEventResult::Handled
            }),
        )
        .expect("secondary focus consumer");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    realm.enter(|_| {
        let _ = node.request_focus();
    });
    let primary_keys = Rc::clone(&observations);
    realm
        .focus_manager()
        .add_global_key_handler(Rc::new(move |_| {
            primary_keys.borrow_mut().push(("primary", 0.0));
            KeyEventResult::Handled
        }));
    let realm = Rc::new(realm);
    *owner.borrow_mut() = Rc::downgrade(&realm);
    dispatch(
        &realm,
        secondary,
        make_down_event(Offset::new(10.0, 10.0), PointerKind::Touch).expect("down"),
    );
    dispatch(
        &realm,
        secondary,
        make_move_event(Offset::new(30.0, 10.0), PointerKind::Touch).expect("motion"),
    );
    let key = || {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                primary,
                PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
            );
        });
    };
    key();
    assert_eq!(
        *observations.borrow(),
        [("secondary", 30.0)],
        "accepted Key retains resolved focus owner while its motion changes active focus"
    );
    key();
    assert_eq!(
        *observations.borrow(),
        [("secondary", 30.0), ("primary", 0.0)],
        "later Key resolves newly active owner"
    );
}

pub(crate) fn ime_commit_survives_competing_motion_and_owner_failures() {
    assert_ime_motion_order(true);
}

fn assert_ime_motion_order(fail: bool) {
    use flui_interaction::routing::FocusNode;
    use flui_runtime::presentation::PointerResampling;
    use flui_widgets::{EditableText, TextEditingController};

    let window = flui_testing::HeadlessWindow::new(100, 40).with_text_input();
    let mut realm = UiRealm::for_test_with_text_input(window.text_input());
    let primary = realm.presentation_id();
    realm
        .set_pointer_resampling(primary, PointerResampling::FrameAligned)
        .expect("policy before contact");
    let node = FocusNode::new();
    let controller = TextEditingController::new();
    let state = Rc::new(Cell::new(0.0));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let moved = Rc::clone(&state);
    let committed = Rc::clone(&state);
    let commits = Rc::clone(&observed);
    let motion_failure = Cell::new(fail);
    let commit_failure = Cell::new(fail);
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_move(move |_, event| {
                    let PointerEvent::Move(event) = event.global else {
                        panic!("motion callback receives motion");
                    };
                    moved.set(event.current().position.get().x);
                    assert!(
                        !motion_failure.replace(false),
                        "IME preceding motion failure"
                    );
                })
                .child(
                    EditableText::new(controller.clone(), Rc::clone(&node)).on_changed(
                        move |_, text| {
                            commits
                                .borrow_mut()
                                .push((text.to_owned(), committed.get()));
                            assert!(!commit_failure.replace(false), "IME owner second failure");
                        },
                    ),
                ),
        )
        .expect("real text field attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    realm.enter(|_| {
        let _ = node.request_focus();
    });
    pump(&mut realm);
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(10.0, 10.0), PointerKind::Mouse).expect("finite contact"),
    );
    dispatch(
        &realm,
        primary,
        make_move_event(Offset::new(30.0, 10.0), PointerKind::Mouse).expect("finite motion"),
    );
    let commit = |text: &str| {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                primary,
                PlatformInput::Ime(flui_platform_api::ImeEvent::Commit(text.to_owned())),
            );
        });
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| commit("x")));
    if fail {
        assert_eq!(
            outcome
                .expect_err("first motion failure propagates")
                .downcast_ref::<&str>(),
            Some(&"IME preceding motion failure")
        );
    } else {
        outcome.expect("healthy IME commit");
    }
    assert_eq!(
        controller.text(),
        "x",
        "accepted IME edit survives motion failure"
    );
    assert_eq!(*observed.borrow(), [("x".to_owned(), 30.0)]);
    dispatch(
        &realm,
        primary,
        make_up_event(Offset::new(30.0, 10.0), PointerKind::Mouse).expect("finite terminal"),
    );
    commit("y");
    assert_eq!(controller.text(), "xy", "next IME operation recovers");
    assert_eq!(observed.borrow().len(), 2);
}

pub(crate) fn keyboard_reads_all_frozen_contacts_after_sibling_failure() {
    assert_keyboard_contact_prefix(true, false, true);
}

pub(crate) fn keyboard_barrier_keeps_reentrant_contact_motion_for_the_next_round() {
    assert_keyboard_contact_prefix(false, true, true);
}

pub(crate) fn keyboard_barrier_keeps_frozen_coalesced_motion_before_reentrant_replacement() {
    assert_keyboard_contact_prefix(false, true, false);
}

pub(crate) fn keyboard_coalesced_prefix_survives_reentrant_capture_release() {
    use flui_interaction::events::{make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::routing::KeyEventResult;
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform_api::keyboard::Code;
    use std::rc::Weak;

    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let one = PointerId::try_from(1_u64).expect("contact");
    let two = PointerId::try_from(2_u64).expect("contact");
    let unrelated = PointerId::try_from(3_u64).expect("hover");
    let owner = Rc::new(RefCell::new(Weak::<UiRealm>::new()));
    let input_owner = Rc::clone(&owner);
    let token = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&token);
    let released = Rc::clone(&token);
    let events = Rc::new(RefCell::new(Vec::new()));
    let motions = Rc::clone(&events);
    let cancels = Rc::clone(&events);
    let first = Cell::new(true);
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, dispatch| {
                    if let PointerEvent::Down(event) = dispatch.global
                        && event.pointer.id == two
                    {
                        *captured.borrow_mut() =
                            Some(dispatch.capture().expect("real Down authority"));
                    }
                })
                .on_pointer_move(move |_, event| {
                    let PointerEvent::Move(event) = event.global else {
                        panic!("motion");
                    };
                    motions.borrow_mut().push((
                        "move",
                        event.pointer.id,
                        event.current().position.get().x,
                    ));
                    if event.pointer.id == one && first.replace(false) {
                        let realm = input_owner.borrow().upgrade().expect("live owner");
                        dispatch(
                            &realm,
                            primary,
                            make_move_event_for_id(
                                two,
                                Offset::new(80.0, 10.0),
                                PointerKind::Touch,
                            )
                            .expect("new motion"),
                        );
                        let capture = released.borrow_mut().take();
                        drop(capture);
                        let mut hover = make_move_event_for_id(
                            unrelated,
                            Offset::new(90.0, 10.0),
                            PointerKind::Mouse,
                        )
                        .expect("hover");
                        if let PointerEvent::Move(event) = &mut hover {
                            event.buttons = flui_platform_api::pointer::PointerButtons::default();
                        }
                        dispatch(&realm, primary, hover);
                    }
                })
                .on_pointer_cancel(move |_, event| {
                    let PointerEvent::Cancel(event) = event.global else {
                        panic!("cancel");
                    };
                    assert_eq!(
                        event.reason,
                        flui_platform_api::pointer::CancelReason::CaptureLost
                    );
                    cancels.borrow_mut().push(("cancel", event.pointer.id, 0.0));
                })
                .child(SizedBox::new(100.0, 40.0)),
        )
        .expect("real capture consumer");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    let observed = Rc::clone(&events);
    let reads = Rc::new(RefCell::new(Vec::new()));
    let key_reads = Rc::clone(&reads);
    realm
        .focus_manager()
        .add_global_key_handler(Rc::new(move |_| {
            key_reads.borrow_mut().push(observed.borrow().clone());
            KeyEventResult::Handled
        }));
    let realm = Rc::new(realm);
    *owner.borrow_mut() = Rc::downgrade(&realm);
    for (id, x) in [(one, 10.0), (two, 30.0)] {
        dispatch(
            &realm,
            primary,
            make_down_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("down"),
        );
    }
    for (id, x) in [(one, 20.0), (two, 40.0)] {
        dispatch(
            &realm,
            primary,
            make_move_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("motion"),
        );
    }
    realm.enter(|realm| {
        realm.handle_input_addressed(
            primary,
            PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
        );
    });
    let expected = [
        ("move", one, 20.0),
        ("move", two, 40.0),
        ("move", two, 80.0),
        ("cancel", two, 0.0),
    ];
    assert_eq!(
        *events.borrow(),
        expected,
        "frozen prefix finishes before loss settles its later accepted tail"
    );
    assert_eq!(reads.borrow()[0], expected);
    dispatch(
        &realm,
        primary,
        make_up_event_for_id(two, Offset::new(80.0, 10.0), PointerKind::Touch)
            .expect("old terminal"),
    );
    assert_eq!(
        *events.borrow(),
        expected,
        "native tail cannot duplicate capture loss"
    );
}

fn assert_keyboard_contact_prefix(fail: bool, reenter: bool, resampling: bool) {
    use flui_interaction::events::{make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::routing::KeyEventResult;
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform_api::keyboard::Code;
    use flui_runtime::presentation::PointerResampling;
    use std::rc::Weak;

    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    realm
        .set_pointer_resampling(
            primary,
            if resampling {
                PointerResampling::FrameAligned
            } else {
                PointerResampling::Disabled
            },
        )
        .expect("policy");
    let one = PointerId::try_from(1_u64).expect("contact");
    let two = PointerId::try_from(2_u64).expect("contact");
    let positions = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&positions);
    let owner = Rc::new(RefCell::new(Weak::<UiRealm>::new()));
    let callback_owner = Rc::clone(&owner);
    let first = Cell::new(true);
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_move(move |_, event| {
                    let PointerEvent::Move(event) = event.global else {
                        panic!("motion");
                    };
                    log.borrow_mut()
                        .push((event.pointer.id, event.current().position.get().x));
                    if event.pointer.id == one && first.replace(false) {
                        if reenter {
                            dispatch(
                                &callback_owner.borrow().upgrade().expect("live owner"),
                                primary,
                                make_move_event_for_id(
                                    two,
                                    Offset::new(80.0, 10.0),
                                    PointerKind::Touch,
                                )
                                .expect("new measured motion"),
                            );
                        }
                        assert!(!fail, "first contact failure");
                    }
                })
                .child(SizedBox::new(100.0, 40.0)),
        )
        .expect("listener");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    let realm = Rc::new(realm);
    *owner.borrow_mut() = Rc::downgrade(&realm);
    let reads = Rc::new(RefCell::new(Vec::new()));
    let read = Rc::clone(&reads);
    let state = Rc::clone(&positions);
    realm
        .focus_manager()
        .add_global_key_handler(Rc::new(move |_| {
            read.borrow_mut().push(state.borrow().clone());
            KeyEventResult::Handled
        }));
    for (id, x) in [(one, 10.0), (two, 30.0)] {
        dispatch(
            &realm,
            primary,
            make_down_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("down"),
        );
    }
    for (id, x) in [(one, 20.0), (two, 40.0)] {
        dispatch(
            &realm,
            primary,
            make_move_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("motion"),
        );
    }
    let key = || {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                primary,
                PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
            );
        });
    };
    let outcome = catch_unwind(AssertUnwindSafe(key));
    if fail {
        assert_eq!(
            outcome
                .expect_err("first contact fails")
                .downcast_ref::<&str>(),
            Some(&"first contact failure")
        );
    } else {
        outcome.expect("healthy prefix");
    }
    let mut expected = vec![(one, 20.0), (two, 40.0)];
    assert_eq!(
        *positions.borrow(),
        expected,
        "every frozen contact precedes Key"
    );
    assert_eq!(
        reads.borrow()[0],
        expected,
        "Key observes motion-produced state"
    );
    key();
    if reenter {
        expected.push((two, 80.0));
    }
    assert_eq!(
        *positions.borrow(),
        expected,
        "reentrant measured motion belongs to next barrier"
    );
    assert_eq!(reads.borrow()[1], expected);
    for (id, x) in [(one, 20.0), (two, 80.0)] {
        dispatch(
            &realm,
            primary,
            make_up_event_for_id(id, Offset::new(x, 10.0), PointerKind::Touch).expect("terminal"),
        );
    }
    assert_eq!(
        *positions.borrow(),
        expected,
        "terminal cannot duplicate measured prefix"
    );
}

pub(crate) fn touch_motion_precedes_keyboard_without_a_frame() {
    assert_motion_keyboard_order(PointerKind::Touch, false, false, false);
}

pub(crate) fn resampled_mouse_motion_precedes_keyboard_without_a_frame() {
    assert_motion_keyboard_order(PointerKind::Mouse, true, false, false);
}

pub(crate) fn resampled_touch_motion_precedes_keyboard_without_a_frame() {
    assert_motion_keyboard_order(PointerKind::Touch, true, false, false);
}

pub(crate) fn motion_failure_keeps_following_keyboard_and_contact_terminal() {
    assert_motion_keyboard_order(PointerKind::Touch, false, true, false);
}

pub(crate) fn keyboard_failure_keeps_preceding_motion_and_contact_terminal() {
    assert_motion_keyboard_order(PointerKind::Touch, true, false, true);
}

pub(crate) fn motion_failure_precedes_competing_keyboard_failure_and_recovers() {
    assert_motion_keyboard_order(PointerKind::Touch, true, true, true);
}

fn assert_motion_keyboard_order(
    kind: PointerKind,
    resampling: bool,
    motion_fails: bool,
    key_fails: bool,
) {
    use flui_interaction::routing::KeyEventResult;
    use flui_interaction::testing::input::KeyEventBuilder;
    use flui_platform_api::keyboard::Code;
    use flui_runtime::presentation::PointerResampling;

    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    realm
        .set_pointer_resampling(
            primary,
            if resampling {
                PointerResampling::FrameAligned
            } else {
                PointerResampling::Disabled
            },
        )
        .expect("policy before contact admission");
    let events = Rc::new(RefCell::new(Vec::new()));
    let down = Rc::clone(&events);
    let movement = Rc::clone(&events);
    let up = Rc::clone(&events);
    let motion_failure = Cell::new(motion_fails);
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| down.borrow_mut().push(("down", 10.0)))
                .on_pointer_move(move |_, dispatch| {
                    let PointerEvent::Move(event) = dispatch.global else {
                        panic!("move callback receives a move");
                    };
                    movement
                        .borrow_mut()
                        .push(("move", event.current().position.get().x));
                    assert!(!motion_failure.replace(false), "causal motion failure");
                })
                .on_pointer_up(move |_, _| up.borrow_mut().push(("up", 40.0)))
                .child(SizedBox::new(100.0, 40.0)),
        )
        .expect("real listener attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    let keyboard = Rc::clone(&events);
    let key_failure = Cell::new(key_fails);
    realm
        .focus_manager()
        .add_global_key_handler(Rc::new(move |_| {
            keyboard.borrow_mut().push(("key", 0.0));
            assert!(!key_failure.replace(false), "causal keyboard failure");
            KeyEventResult::Handled
        }));
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(10.0, 10.0), kind).expect("finite down"),
    );
    for x in [20.0, 30.0] {
        dispatch(
            &realm,
            primary,
            make_move_event(Offset::new(x, 10.0), kind).expect("finite move"),
        );
    }
    assert_eq!(*events.borrow(), [("down", 10.0)], "motion is pending");
    let key = || {
        realm.enter(|realm| {
            realm.handle_input_addressed(
                primary,
                PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
            );
        });
    };
    let outcome = catch_unwind(AssertUnwindSafe(key));
    if motion_fails || key_fails {
        let failure = outcome.expect_err("input failure propagates after accepted delivery");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&if motion_fails {
                "causal motion failure"
            } else {
                "causal keyboard failure"
            }),
            "earliest input failure remains authoritative"
        );
    } else {
        outcome.expect("healthy keyboard dispatch");
    }
    let mut expected = vec![("down", 10.0)];
    if resampling {
        expected.push(("move", 20.0));
    }
    expected.extend([("move", 30.0), ("key", 0.0)]);
    assert_eq!(
        *events.borrow(),
        expected,
        "already accepted measured motion precedes keyboard without advancing a frame"
    );
    dispatch(
        &realm,
        primary,
        make_up_event(Offset::new(40.0, 10.0), kind).expect("finite terminal"),
    );
    expected.push(("up", 40.0));
    key();
    expected.push(("key", 0.0));
    pump(&mut realm);
    assert_eq!(
        *events.borrow(),
        expected,
        "contact remains admitted, terminal and healthy input follow once, with no frame duplicate"
    );
}

pub(crate) fn a_secondary_contact_move_is_delivered_by_the_next_frame() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let moves = Rc::new(Cell::new(0));
    let seen = moves.clone();
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_move(move |_, _| seen.set(seen.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(
        &realm,
        secondary,
        make_down_event(Offset::new(4.0, 6.0), PointerKind::Touch).expect("finite test position"),
    );
    dispatch(
        &realm,
        secondary,
        make_move_event(Offset::new(8.0, 9.0), PointerKind::Touch).expect("finite test position"),
    );
    assert_eq!(moves.get(), 0, "motion waits for frame cadence");
    pump(&mut realm);
    assert_eq!(
        moves.get(),
        1,
        "the secondary contact receives its queued motion"
    );
    pump(&mut realm);
    assert_eq!(moves.get(), 1, "a later frame cannot duplicate motion");
}

pub(crate) fn held_replay_preserves_hardware_history_and_drag_velocity() {
    use flui_platform_api::EventTime;
    use flui_platform_api::pointer::{
        PointerButton, PointerButtons, PointerInfo, PointerMove, PointerPosition, PointerPress,
        PointerRelease, PointerSample,
    };

    fn sample(millis: u64, x: f64) -> PointerSample {
        PointerSample::new(
            EventTime::from_nanos(1_000_000_000 + millis * 1_000_000),
            PointerPosition::try_new(Point::new(5.0 + x, 10.0)).expect("finite hardware position"),
        )
    }

    for held in [false, true, false] {
        let mut realm = UiRealm::for_test();
        let primary = realm.presentation_id();
        let motions = Rc::new(RefCell::new(Vec::<PointerMove>::new()));
        let observed = Rc::clone(&motions);
        let ends = Rc::new(RefCell::new(Vec::<f64>::new()));
        let ended = Rc::clone(&ends);
        realm
            .attach_root_widget(
                &Listener::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pointer_move(move |_, dispatch| {
                        let PointerEvent::Move(movement) = dispatch.global else {
                            panic!("a move callback receives a move");
                        };
                        observed.borrow_mut().push(movement.clone());
                    })
                    .child(
                        GestureDetector::new()
                            .behavior(HitTestBehavior::Opaque)
                            .on_horizontal_drag_end(move |_, details| {
                                ended.borrow_mut().push(details.primary_velocity);
                            })
                            .child(SizedBox::new(200.0, 40.0)),
                    ),
            )
            .expect("root attaches");
        realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
        if held {
            pump_uncommitted(&mut realm);
        } else {
            pump(&mut realm);
        }

        let pointer = PointerInfo::new(
            PointerId::new(std::num::NonZeroU64::MIN),
            PointerKind::Touch,
        );
        dispatch(
            &realm,
            primary,
            PointerEvent::Down(PointerPress::new(
                pointer,
                PointerButton::PRIMARY,
                PointerButtons::only(PointerButton::PRIMARY),
                sample(0, 0.0),
            )),
        );
        for (millis, x) in [(20, 8.0), (30, 18.0), (40, 32.0), (50, 50.0)] {
            let mut movement = PointerMove::new(
                pointer,
                PointerButtons::only(PointerButton::PRIMARY),
                sample(millis, x),
            );
            if millis == 20 {
                movement = movement.with_coalesced(vec![sample(10, 2.0)]);
            }
            // A deliberately different predicted curve must stay observable,
            // while the real drag's velocity uses hardware readings only.
            movement = movement.with_predicted(vec![sample(millis + 10, 10_000.0)]);
            dispatch(&realm, primary, PointerEvent::Move(movement));
            if !held {
                pump(&mut realm);
            }
        }
        dispatch(
            &realm,
            primary,
            PointerEvent::Up(PointerRelease::new(
                pointer,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample(50, 50.0),
            )),
        );
        if held {
            assert!(
                motions.borrow().is_empty(),
                "held input has not reached the widget"
            );
            assert!(
                ends.borrow().is_empty(),
                "the held terminal has not reached the recognizer"
            );
            pump(&mut realm);
        }

        let movements = motions.borrow();
        assert_eq!(
            movements.len(),
            if held { 1 } else { 4 },
            "held={held}: delivery cadence"
        );
        let actual: Vec<_> = movements
            .iter()
            .flat_map(|movement| {
                movement
                    .coalesced()
                    .iter()
                    .chain(std::iter::once(movement.current()))
            })
            .map(|reading| (reading.time.as_nanos(), reading.position.get().x))
            .collect();
        assert_eq!(
            actual,
            [
                (1_010_000_000, 7.0),
                (1_020_000_000, 13.0),
                (1_030_000_000, 23.0),
                (1_040_000_000, 37.0),
                (1_050_000_000, 55.0),
            ],
            "held={held}: the same real curve survives both queues"
        );
        assert_eq!(
            movements.last().expect("a move was delivered").predicted(),
            [sample(60, 10_000.0)],
            "only the latest dispatch predictions survive"
        );
        let velocities = ends.borrow();
        assert_eq!(
            velocities.len(),
            1,
            "held={held}: the terminal ends exactly one accepted drag"
        );
        assert!(
            (velocities[0] - 2_000.0).abs() < 20.0,
            "held={held}: real quadratic curve end velocity {}",
            velocities[0]
        );
        assert!(
            realm
                .presentation_gestures_for_test(primary)
                .arena()
                .is_empty(),
            "terminal delivery retires its arena"
        );
        drop(velocities);
        drop(movements);
        pump(&mut realm);
        assert_eq!(
            ends.borrow().len(),
            1,
            "a later frame cannot repeat terminal delivery"
        );
    }
}

struct AcceptLog(Rc<Cell<usize>>);

impl GestureArenaMember for AcceptLog {
    fn accept_gesture(&self, _: PointerId) {
        self.0.set(self.0.get() + 1);
    }

    fn reject_gesture(&self, _: PointerId) {}
}

pub(crate) fn a_secondary_deferred_arena_verdict_is_delivered_by_the_next_frame() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let accepted = Rc::new(Cell::new(0));
    let member = Rc::new(AcceptLog(accepted.clone()));
    let arena = realm.presentation_gestures_for_test(secondary).arena();
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let _entry = arena.add(pointer, &member);
    arena.close(pointer);
    assert_eq!(
        accepted.get(),
        0,
        "the lone verdict waits for an owner boundary"
    );
    pump(&mut realm);
    assert_eq!(
        accepted.get(),
        1,
        "the frame drains the secondary arena's accepted work"
    );
    pump(&mut realm);
    assert_eq!(accepted.get(), 1, "the verdict is delivered exactly once");
}

fn queued_hover_after_transition(paused: bool, held: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let hovers = Rc::new(Cell::new(0));
    let seen = hovers.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_hover(move |_, _| seen.set(seen.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    if held {
        pump_uncommitted(&mut realm);
    } else {
        pump(&mut realm);
    }
    dispatch(&realm, primary, hover());
    assert_eq!(hovers.get(), 0, "hover waits for frame cadence");
    if paused {
        realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Paused));
        realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Resumed));
    } else {
        realm.enter(|realm| realm.update_window_focus(primary, false));
    }
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        usize::from(!paused),
        "pause discards pending input; blur preserves hover"
    );
    dispatch(&realm, primary, hover());
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        1 + usize::from(!paused),
        "fresh hovering still works after the transition"
    );
}

pub(crate) fn host_pause_discards_a_queued_hover_before_resume() {
    queued_hover_after_transition(true, false);
}

pub(crate) fn window_blur_keeps_a_queued_hover() {
    queued_hover_after_transition(false, false);
}

pub(crate) fn host_pause_discards_a_hover_held_before_the_first_commit() {
    queued_hover_after_transition(true, true);
}

pub(crate) fn host_pause_keeps_a_completed_held_tap_for_the_first_commit() {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let downs = Rc::new(Cell::new(0));
    let ups = Rc::new(Cell::new(0));
    let down = downs.clone();
    let up = ups.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| down.set(down.get() + 1))
                .on_pointer_up(move |_, _| up.set(up.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump_uncommitted(&mut realm);
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(4.0, 6.0), PointerKind::Touch).expect("finite test position"),
    );
    dispatch(
        &realm,
        primary,
        make_up_event(Offset::new(4.0, 6.0), PointerKind::Touch).expect("finite test position"),
    );
    assert_eq!(
        (downs.get(), ups.get()),
        (0, 0),
        "the completed tap waits for commit"
    );
    realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Paused));
    realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Resumed));
    pump(&mut realm);
    assert_eq!(
        (downs.get(), ups.get()),
        (1, 1),
        "a completed tap remains accepted work across suspension"
    );
    pump(&mut realm);
    assert_eq!(
        (downs.get(), ups.get()),
        (1, 1),
        "the completed tap replays exactly once"
    );
}

fn motion_probe(count: Rc<Cell<usize>>, fail: Rc<Cell<bool>>, message: &'static str) -> Listener {
    Listener::new()
        .behavior(HitTestBehavior::Opaque)
        .on_pointer_move(move |_, _| {
            count.set(count.get() + 1);
            if fail.get() {
                panic_any(message);
            }
        })
        .child(SizedBox::new(40.0, 40.0))
}

fn failing_frame_motion_still_delivers_the_sibling(secondary_panics: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let counts = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
    let failures = [Rc::new(Cell::new(false)), Rc::new(Cell::new(false))];
    realm
        .attach_root_widget(&motion_probe(
            counts[0].clone(),
            failures[0].clone(),
            "primary motion failed",
        ))
        .expect("primary root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    let secondary = install_secondary(&mut realm);
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &motion_probe(
                counts[1].clone(),
                failures[1].clone(),
                "secondary motion failed",
            ),
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, false, true);
    pump(&mut realm);
    failures[0].set(true);
    failures[1].set(secondary_panics);
    for id in [primary, secondary] {
        dispatch(
            &realm,
            id,
            make_down_event(Offset::new(4.0, 6.0), PointerKind::Touch)
                .expect("finite test position"),
        );
        dispatch(
            &realm,
            id,
            make_move_event(Offset::new(8.0, 9.0), PointerKind::Touch)
                .expect("finite test position"),
        );
    }
    let failure = catch_unwind(AssertUnwindSafe(|| pump(&mut realm)))
        .expect_err("the frame propagates its first input failure");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"primary motion failed")
    );
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (1, 1),
        "both accepted motions complete before the first failure resumes"
    );
    for fail in failures {
        fail.set(false);
    }
    for id in [primary, secondary] {
        dispatch(
            &realm,
            id,
            make_move_event(Offset::new(12.0, 13.0), PointerKind::Touch)
                .expect("finite test position"),
        );
    }
    pump(&mut realm);
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (2, 2),
        "both contact routes recover after containment"
    );
    pump(&mut realm);
    assert_eq!(
        (counts[0].get(), counts[1].get()),
        (2, 2),
        "recovery cannot replay old motion"
    );
}

pub(crate) fn a_panicking_primary_motion_does_not_erase_the_secondary_motion() {
    failing_frame_motion_still_delivers_the_sibling(false);
}

pub(crate) fn competing_frame_motion_failures_preserve_the_first_and_recover() {
    failing_frame_motion_still_delivers_the_sibling(true);
}

struct PauseDiagnosticPanic(Arc<AtomicUsize>);

impl tracing::Subscriber for PauseDiagnosticPanic {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().ends_with("::binding") && *metadata.level() == tracing::Level::DEBUG
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}").contains(
                        "GestureBinding draining interrupted pointer state on lifecycle pause",
                    );
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic_any("pause drain diagnostic failed");
        }
    }
}

fn pause_diagnostic_failure_still_drains_motion(cancel_panics: bool) {
    let mut realm = UiRealm::for_test();
    let primary = realm.presentation_id();
    let hovers = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let downs = Rc::new(Cell::new(0));
    let seen_down = downs.clone();
    let seen_hover = hovers.clone();
    let seen_cancel = cancels.clone();
    realm
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| seen_down.set(seen_down.get() + 1))
                .on_pointer_hover(move |_, _| seen_hover.set(seen_hover.get() + 1))
                .on_pointer_cancel(move |_, _| {
                    seen_cancel.set(seen_cancel.get() + 1);
                    if cancel_panics {
                        panic_any("pointer cancel failed");
                    }
                })
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    realm.synchronize_window_snapshot(primary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(
        &realm,
        primary,
        make_down_event(Offset::new(4.0, 6.0), PointerKind::Touch).expect("finite test position"),
    );
    assert_eq!(
        downs.get(),
        1,
        "the contact is admitted and reaches its real widget route"
    );
    let mut motion = make_move_event_for_id(
        PointerId::try_from(2_u64).expect("nonzero mouse pointer"),
        Offset::new(8.0, 9.0),
        PointerKind::Mouse,
    )
    .expect("finite test position");
    if let PointerEvent::Move(update) = &mut motion {
        update.buttons = flui_platform_api::pointer::PointerButtons::default();
    }
    dispatch(&realm, primary, motion.clone());
    assert_eq!(hovers.get(), 0, "the mouse move is still queued");
    let diagnostics = Arc::new(AtomicUsize::new(0));
    let failed = catch_unwind(AssertUnwindSafe(|| {
        tracing::subscriber::with_default(PauseDiagnosticPanic(diagnostics.clone()), || {
            realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Paused));
        });
    }))
    .expect_err("pause retains its first callback or diagnostic failure");
    let first = if cancel_panics {
        "pointer cancel failed"
    } else {
        "pause drain diagnostic failed"
    };
    assert_eq!(failed.downcast_ref::<&str>(), Some(&first));
    assert!(
        diagnostics.load(Ordering::SeqCst) > 0,
        "the real pause diagnostic reached the subscriber"
    );
    assert_eq!(
        cancels.get(),
        1,
        "the accepted contact receives its terminal callback"
    );
    realm.enter(|realm| realm.update_host_lifecycle(AppLifecycleState::Resumed));
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        0,
        "a failed diagnostic cannot preserve stale motion across pause"
    );
    dispatch(&realm, primary, motion);
    pump(&mut realm);
    assert_eq!(
        hovers.get(),
        1,
        "new input remains deliverable after containment"
    );
}

pub(crate) fn a_panicking_pause_diagnostic_cannot_skip_motion_drain() {
    pause_diagnostic_failure_still_drains_motion(false);
}

pub(crate) fn a_cancel_failure_precedes_a_pause_diagnostic_failure_and_recovers() {
    pause_diagnostic_failure_still_drains_motion(true);
}

#[derive(Clone, StatelessView)]
struct ShrinkingHoverRegion {
    width: Signal<f64>,
    enters: Rc<Cell<usize>>,
    exits: Rc<Cell<usize>>,
}

impl StatelessView for ShrinkingHoverRegion {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let width = self.width.get(ctx);
        let signal = self.width;
        let enters = self.enters.clone();
        let exits = self.exits.clone();
        Align::new(flui_painting::Alignment::TOP_LEFT).child(
            MouseRegion::new()
                .on_enter(move |_, _, _| enters.set(enters.get() + 1))
                .on_exit(move |_, _, _| exits.set(exits.get() + 1))
                .on_hover(move |cx, _, _| {
                    let _ = signal.set(cx, 5.0);
                })
                .child(SizedBox::new(width, 20.0)),
        )
    }
}

pub(crate) fn a_secondary_layout_refreshes_its_stationary_hover() {
    let mut realm = UiRealm::for_test();
    let secondary = install_secondary(&mut realm);
    let graph = realm
        .presentation_widgets_for_test(secondary)
        .with_build_owner(|owner| owner.reactive().clone());
    let enters = Rc::new(Cell::new(0));
    let exits = Rc::new(Cell::new(0));
    realm
        .attach_root_widget_to_for_test(
            secondary,
            &ShrinkingHoverRegion {
                width: graph.signal(20.0),
                enters: enters.clone(),
                exits: exits.clone(),
            },
        )
        .expect("secondary root attaches");
    realm.synchronize_window_snapshot(secondary, WindowExecutionState::Running, true, true);
    pump(&mut realm);
    dispatch(&realm, secondary, hover());
    // Establish the mouse position independently of frame motion flushing:
    // this row isolates the committed-layout refresh contract.
    realm.enter(|realm| {
        realm
            .presentation_gestures_for_test(secondary)
            .flush_pending_moves();
    });
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 0),
        "the cursor enters before layout shrinks"
    );
    pump(&mut realm);
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 1),
        "the committed smaller region releases its stationary cursor"
    );
    pump(&mut realm);
    assert_eq!(
        (enters.get(), exits.get()),
        (1, 1),
        "ambient refresh does not duplicate exit"
    );
}
