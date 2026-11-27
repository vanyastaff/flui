//! [`RawTextField`] against a mounted tree: per-field focus nodes and the
//! `on_submitted` passthrough. `tests/text_field.rs` covers the controller's
//! editing; the builder defaults stay unit tests in `src/text/text_field.rs`.

use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyPress, KeyState, NamedKey};
use flui_interaction::focus::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::prelude::*;
use flui_widgets::{RawTextField, TextEditingController};

use crate::common::harness::mount;

/// `RawTextField` forwards the `ctx` its `EditableText` opens, so both of its
/// callbacks write signals (ADR-0086).
pub(crate) fn raw_text_field_callbacks_write_through_the_forwarded_ctx() {
    use crate::common::{ProbeSignals, SignalProbe};

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("raw-ctx field");
    let (probe_controller, probe_node) = (controller, Rc::clone(&focus_node));
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        RawTextField::new(probe_controller.clone())
            .focus_node(Rc::clone(&probe_node))
            .on_changed(move |ctx, text| count.set(ctx, text.len() as u32))
            .on_submitted(move |ctx, _text| count.update(ctx, |n| *n += 100))
    });
    let harness = mount(probe.view());
    let _ = focus_node.request_focus();

    let typed = KeyEventBuilder::new(Code::KeyA)
        .key(Key::character("a"))
        .state(KeyState::Down(KeyPress::First))
        .build();
    let _ = harness
        .focus_manager()
        .dispatch_key_event(&typed)
        .is_handled();
    assert_eq!(probe.value(), Ok(1), "on_changed wrote");

    let enter = KeyEventBuilder::new(Code::Enter)
        .key(Key::Named(NamedKey::Enter))
        .state(KeyState::Down(KeyPress::First))
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

/// The Return key a platform sends for a line break (UIKit's `insertText:`
/// of a lone `"\n"`, AppKit's `insertNewline:`) submits a single-line field
/// and leaves its text as it was: no line break goes into it.
pub(crate) fn the_return_key_submits_a_single_line_field_without_a_line_break() {
    use std::cell::RefCell;

    let controller = TextEditingController::with_text("sent");
    let focus_node = FocusNode::with_debug_label("return field");
    let submitted = Rc::new(RefCell::new(Vec::<String>::new()));
    let harness = mount(
        RawTextField::new(controller.clone())
            .focus_node(Rc::clone(&focus_node))
            .on_submitted({
                let submitted = Rc::clone(&submitted);
                move |_ctx, text| submitted.borrow_mut().push(text.to_owned())
            }),
    );
    let _ = focus_node.request_focus();
    for state in [KeyState::Down(KeyPress::First), KeyState::Up] {
        let enter = KeyEventBuilder::new(Code::Enter)
            .key(Key::Named(NamedKey::Enter))
            .state(state)
            .build();
        let _ = harness.focus_manager().dispatch_key_event(&enter);
    }
    assert_eq!(*submitted.borrow(), ["sent"], "Return submitted once");
    assert_eq!(controller.text(), "sent", "no line break went in");
}

/// A `Semantics` label above a field names the field's own node, as one above
/// Flutter's `EditableText` does: the field is not a semantics boundary, so the
/// label and the field's role, value and actions are one node. The scenario
/// tester's end checks then find no unnamed input.
#[test]
fn a_semantics_label_above_a_field_names_it() {
    use flui_testing::scenario::{Tester, find, matcher};
    use flui_testing::tester::Role;
    use flui_widgets::{Column, Semantics};

    let mut t = Tester::mount(Column::new((Semantics::new()
        .label("New note")
        .identifier("new-note")
        .child(RawTextField::new(TextEditingController::new())),)));
    t.expect(&find::role(Role::TextInput), matcher::name("New note"));
    t.expect(&find::id("new-note"), matcher::name("New note"));
    // The named node is still the field: it takes text and shows it as its value.
    t.type_text(&find::id("new-note"), "Groceries");
    t.expect(
        &find::role(Role::TextInput),
        matcher::all([matcher::name("New note"), matcher::value("Groceries")]),
    );
    t.finish();
}

/// The end checks read the field: one nobody named fails them, so the case
/// above passes because the label reached the field.
#[test]
fn a_field_nobody_names_fails_the_end_checks() {
    use flui_testing::scenario::Tester;

    let t = Tester::mount(RawTextField::new(TextEditingController::new()));
    let report = t
        .try_finish()
        .expect_err("an unnamed text input fails the end checks");
    let report = report.to_string();
    assert!(
        report.contains("has no accessible name"),
        "the report names the unnamed field: {report}"
    );
}
