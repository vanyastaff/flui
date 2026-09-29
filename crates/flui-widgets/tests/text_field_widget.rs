//! [`RawTextField`] against a mounted tree: per-field focus nodes and the
//! `on_submitted` passthrough. `tests/text_field.rs` covers the controller's
//! editing; the builder defaults stay unit tests in `src/text/text_field.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_interaction::events::{Code, Key, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::ViewExt;
use flui_view::prelude::*;
use flui_widgets::{Column, RawTextField, TextEditingController};

use crate::common::harness::mount;

/// Each field carries its own explicit node into `EditableText`, so
/// requests never resolve through controller metadata or a first-node
/// registry.
#[test]
fn explicit_focus_requests_target_each_fields_own_node() {
    let first = TextEditingController::new();
    let second = TextEditingController::new();
    let first_node = FocusNode::with_debug_label("first");
    let second_node = FocusNode::with_debug_label("second");
    let mut harness = mount(Column::new(vec![
        RawTextField::new(first)
            .focus_node(Rc::clone(&first_node))
            .into_view()
            .boxed(),
        RawTextField::new(second)
            .focus_node(Rc::clone(&second_node))
            .into_view()
            .boxed(),
    ]));
    let manager = harness.focus_manager();

    second_node.request_focus();
    assert!(second_node.has_primary_focus());
    first_node.request_focus();
    assert!(first_node.has_primary_focus());

    // Unmount: both attachment tokens are detached.
    harness.swap_root(Column::new(Vec::<flui_view::BoxedView>::new()));
    assert!(!first_node.is_attached());
    assert!(!second_node.is_attached());
    assert!(manager.primary_focus().is_none());
}

/// `RawTextField` forwards the `cx` its `EditableText` opens, so both of its
/// callbacks write signals (ADR-0086).
#[test]
fn raw_text_field_callbacks_write_through_the_forwarded_cx() {
    use crate::common::{ProbeSignals, SignalProbe};

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("raw-cx field");
    let (probe_controller, probe_node) = (controller.clone(), Rc::clone(&focus_node));
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        RawTextField::new(probe_controller.clone())
            .focus_node(Rc::clone(&probe_node))
            .on_changed(move |cx, text| count.set(cx, text.len() as u32))
            .on_submitted(move |cx, _text| count.update(cx, |n| *n += 100))
    });
    let harness = mount(probe.view());
    focus_node.request_focus();

    let typed = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character("a".to_owned()))
        .with_state(KeyState::Down)
        .build();
    harness.focus_manager().dispatch_key_event(&typed);
    assert_eq!(probe.value(), Ok(1), "on_changed wrote");

    let enter = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .build();
    harness.focus_manager().dispatch_key_event(&enter);
    assert_eq!(probe.value(), Ok(101), "on_submitted wrote");
}

/// A parent rebuilding with a different `on_submitted` closure must
/// reach the newly-mounted field — since `RawTextFieldState::build`
/// constructs a fresh `EditableText` on every call, the replacement
/// callback is simply what the next build passes, and the previous
/// one is gone.
#[test]
fn swapping_the_on_submitted_closure_via_a_rebuild_replaces_it() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("swap-submit field");
    let first_calls: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let second_calls: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let first_calls_cb = Rc::clone(&first_calls);
    let second_calls_cb = Rc::clone(&second_calls);

    let mut harness = mount(
        RawTextField::new(controller.clone())
            .focus_node(Rc::clone(&focus_node))
            .on_submitted(move |_cx, _| {
                *first_calls_cb.borrow_mut() += 1;
            }),
    );
    focus_node.request_focus();

    let event = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .build();
    harness.focus_manager().dispatch_key_event(&event);
    assert_eq!(*first_calls.borrow(), 1);
    assert_eq!(*second_calls.borrow(), 0);

    harness.swap_root(
        RawTextField::new(controller)
            .focus_node(Rc::clone(&focus_node))
            .on_submitted(move |_cx, _| {
                *second_calls_cb.borrow_mut() += 1;
            }),
    );

    harness.focus_manager().dispatch_key_event(&event);
    assert_eq!(
        *first_calls.borrow(),
        1,
        "the old closure must not fire again"
    );
    assert_eq!(
        *second_calls.borrow(),
        1,
        "the new closure, installed by the rebuild, must fire"
    );
}
