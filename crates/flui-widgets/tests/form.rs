//! [`Form`], [`FormField`] and [`RawTextFormField`] against a mounted tree:
//! validation and its autovalidate modes, save, reset, Tab traversal between
//! fields, the form's semantics, and the clipboard in a form field.
//!
//! Frames are driven with `tick`, which does not dirty the root: an error
//! appears only if the field itself scheduled the rebuild that shows it.
//!
//! Flutter reference: `packages/flutter/test/widgets/form_test.dart`, tag
//! `3.44.0` — the cases here port its validate/save/reset/autovalidate
//! coverage onto FLUI's handles.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_widgets::{
    AutovalidateMode, Form, FormFieldHandle, FormHandle, FormHandleAlreadyAttached,
    RawTextFormField, SizedBox, TextEditingController,
};

use crate::common::{LaidOut, SignalProbe, lay_out, tight};

fn character(ch: char) -> KeyEvent {
    KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build()
}

fn named(key: NamedKey, modifiers: Modifiers) -> KeyEvent {
    KeyEventBuilder::new(Code::Tab)
        .with_key(Key::Named(key))
        .with_state(KeyState::Down)
        .with_modifiers(modifiers)
        .build()
}

fn type_text(laid: &LaidOut, text: &str) {
    for ch in text.chars() {
        laid.focus_manager().dispatch_key_event(&character(ch));
    }
}

fn required(message: &'static str) -> impl Fn(&String) -> Option<String> {
    move |value: &String| value.is_empty().then(|| message.to_owned())
}

fn mount(form: Form) -> LaidOut {
    lay_out(form, tight(400.0, 300.0))
}

/// [`mount`] below a probe, whose writer source opens the `cx` that
/// `FormHandle::save` and `reset` take, as a submit button's press would.
fn mount_probed(form: Form) -> (LaidOut, SignalProbe) {
    let probe = SignalProbe::new(move |_| form.clone());
    (lay_out(probe.view(), tight(400.0, 300.0)), probe)
}

/// `validate()` shows the validator's message, and a valid value clears it
/// on the next `validate()`.
///
/// Also fails if the error is stored but the field never schedules its
/// rebuild: `tick` would then keep painting the old frame.
pub(crate) fn validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it() {
    let form = FormHandle::new();
    let node = FocusNode::with_debug_label("name");
    let mut laid = mount(
        Form::new(
            RawTextFormField::with_initial_value("")
                .validator(required("Required"))
                .focus_node(Rc::clone(&node)),
        )
        .handle(form.clone()),
    );
    assert!(
        laid.find_text("Required").is_none(),
        "no error before validate"
    );

    assert!(!form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "validate shows the error"
    );

    node.request_focus();
    type_text(&laid, "x");
    assert!(form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "a valid value clears it"
    );
}

/// `reset()` puts the text back, clears the error and the interaction; the
/// next edit then validates again under `OnUserInteraction`.
///
/// Also fails if writing the initial text back into the controller counted
/// as the user's edit: the field would stay interacted and show the error.
pub(crate) fn reset_restores_initial_values_and_clears_errors_and_interaction() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let controller = TextEditingController::with_text("init");
    let node = FocusNode::with_debug_label("reset");
    let (mut laid, probe) = mount_probed(
        Form::new(
            RawTextFormField::new(controller.clone())
                .validator(required("Required"))
                .autovalidate_mode(AutovalidateMode::OnUserInteraction)
                .focus_node(Rc::clone(&node))
                .handle(field.clone()),
        )
        .handle(form.clone()),
    );
    let backspace = named(NamedKey::Backspace, Modifiers::empty());
    node.request_focus();
    for _ in 0..4 {
        laid.focus_manager().dispatch_key_event(&backspace);
    }
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "precondition: error up"
    );

    probe.write(|cx| form.reset(cx)).expect("same presentation");
    laid.tick();

    assert_eq!(controller.text(), "init");
    assert_eq!(field.value(), "init");
    assert!(laid.find_text("Required").is_none(), "the error is cleared");
    assert!(!field.has_interacted_by_user());
    assert!(!form.has_interacted_by_user());

    for _ in 0..4 {
        laid.focus_manager().dispatch_key_event(&backspace);
    }
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "an edit after the reset validates again"
    );
}

// ============================================================================
// Event context (ADR-0086): the handle methods take the caller's `cx` and
// hand it to the callbacks they run; a field's edit hands on its own.
// ============================================================================

pub(crate) fn a_panicking_reset_callback_does_not_disable_later_form_validation() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let controller = TextEditingController::with_text("valid");
    let (mut laid, probe) = mount_probed(
        Form::new(
            RawTextFormField::new(controller.clone())
                .handle(field.clone())
                .validator(required("Required"))
                .on_reset(|_cx| -> () { panic!("test callback failure") }),
        )
        .handle(form.clone())
        .autovalidate_mode(AutovalidateMode::OnUserInteraction),
    );
    probe
        .write(|cx| field.did_change(cx, String::new()))
        .expect("same presentation");
    laid.tick();
    assert!(
        laid.find_text("Required").is_some(),
        "precondition: the edited field shows its error"
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        probe.write(|cx| form.reset(cx)).expect("same presentation");
    }));
    assert!(result.is_err(), "callback panic propagates");
    assert_eq!(controller.text(), "valid", "the controller was reset");
    assert_eq!(field.error_text(), None, "the error state was reset");
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "the committed reset remains dirty when on_reset panics"
    );

    probe
        .write(|cx| field.did_change(cx, String::new()))
        .expect("same presentation");
    assert_eq!(field.error_text().as_deref(), Some("Required"));
}

/// One handle names one mounted form. A second simultaneous mount must fail
/// before its configuration can replace the first form's callback or owner.
///
/// Fails without an attachment lease because `Form::create_state` configures
/// the shared handle and the second `init_state` replaces its writer.
pub(crate) fn a_form_handle_refuses_a_second_simultaneous_mount_before_mutating_the_first() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let first_changes = Rc::new(Cell::new(0));
    let observed_first = Rc::clone(&first_changes);
    let mounted_form = form.clone();
    let mounted_field = field.clone();
    let first_probe = SignalProbe::new(move |_| {
        let observed_first = Rc::clone(&observed_first);
        Form::new(RawTextFormField::with_initial_value("first").handle(mounted_field.clone()))
            .handle(mounted_form.clone())
            .on_changed(move |_cx| observed_first.set(observed_first.get() + 1))
    });
    let _first_tree = lay_out(first_probe.view(), tight(400.0, 300.0));

    let duplicate_changes = Rc::new(Cell::new(0));
    let observed_duplicate = Rc::clone(&duplicate_changes);
    let duplicate_form = form.clone();
    let (duplicate_tree, log) = flui_testing::log_capture::capture(|| {
        lay_out(
            Form::new(SizedBox::shrink())
                .handle(duplicate_form)
                .on_changed(move |_cx| {
                    observed_duplicate.set(observed_duplicate.get() + 1);
                }),
            tight(400.0, 300.0),
        )
    });
    assert!(
        log.contains("FormHandle is already attached"),
        "the caller-triggerable refusal is reported without unwinding: {log}"
    );
    assert_eq!(
        form.take_attachment_error(),
        Some(FormHandleAlreadyAttached),
        "the refusal is available as a typed diagnostic"
    );

    first_probe
        .write(|cx| field.did_change(cx, "edited".to_owned()))
        .expect("the original form remains attached");
    assert_eq!(first_changes.get(), 1);
    assert_eq!(
        duplicate_changes.get(),
        0,
        "the refused form cannot replace the original callback"
    );

    drop(duplicate_tree);
    first_probe
        .write(|cx| field.did_change(cx, "still attached".to_owned()))
        .expect("disposing the refused form cannot detach the original form");
    assert_eq!(first_changes.get(), 2);
}
