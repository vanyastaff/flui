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
pub(crate) fn raw_text_field_callbacks_write_through_the_forwarded_cx() {
    use crate::common::{ProbeSignals, SignalProbe};

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("raw-cx field");
    let (probe_controller, probe_node) = (controller, Rc::clone(&focus_node));
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        RawTextField::new(probe_controller.clone())
            .focus_node(Rc::clone(&probe_node))
            .on_changed(move |cx, text| count.set(cx, text.len() as u32))
            .on_submitted(move |cx, _text| count.update(cx, |n| *n += 100))
    });
    let harness = mount(probe.view());
    let _ = focus_node.request_focus();

    let typed = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::character("a"))
        .with_state(KeyState::Down)
        .build();
    let _ = harness
        .focus_manager()
        .dispatch_key_event(&typed)
        .is_handled();
    assert_eq!(probe.value(), Ok(1), "on_changed wrote");

    let enter = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .build();
    let _ = harness
        .focus_manager()
        .dispatch_key_event(&enter)
        .is_handled();
    assert_eq!(probe.value(), Ok(101), "on_submitted wrote");
}

/// A press focuses the field at once, and the press's tap, which the
/// field's double-tap holds for the double-tap time, takes no focus back
/// from wherever focus moved meanwhile (Tab to the next control).
pub(crate) fn a_press_on_a_field_takes_no_focus_back_once_focus_moves_on() {
    use std::time::Duration;

    let field = FocusNode::with_debug_label("pressed field");
    let next = FocusNode::with_debug_label("next control");
    let view = flui_widgets::Column::new(vec![
        RawTextField::new(TextEditingController::with_text("hello"))
            .focus_node(Rc::clone(&field))
            .boxed(),
        flui_widgets::Focus::new(flui_widgets::SizedBox::new(100.0, 40.0))
            .focus_node(Rc::clone(&next))
            .boxed(),
    ]);
    let mut laid = crate::common::lay_out(view, crate::common::tight(300.0, 300.0));
    laid.dispatch_pointer_down(150.0, 8.0);
    assert!(field.has_primary_focus(), "the press focused the field");
    laid.dispatch_pointer_up(150.0, 8.0);
    let _ = next.request_focus();
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        next.has_primary_focus(),
        "the focus stays where it moved after the press"
    );
}
