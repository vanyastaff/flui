//! [`RawTextField`] against a mounted tree: per-field focus nodes and the
//! `on_submitted` passthrough. `tests/text_field.rs` covers the controller's
//! editing; the builder defaults stay unit tests in `src/text/text_field.rs`.

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_interaction::events::{Code, Key, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::prelude::*;
use flui_widgets::{RawTextField, TextEditingController};

use crate::common::harness::mount;

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
