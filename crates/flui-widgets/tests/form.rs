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

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform_api::Clipboard as _;
use flui_view::{BoxedView, ViewExt as _};
use flui_widgets::{
    AutovalidateMode, Column, Form, FormFieldHandle, FormHandle, RawTextFormField, SizedBox,
    TextEditingController,
};

use crate::common::{LaidOut, lay_out, tight};

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

fn command() -> Modifiers {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

fn chord(ch: char) -> KeyEvent {
    KeyEventBuilder::new(Code::KeyC)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .with_modifiers(command())
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

fn at_least_three(value: &str) -> Option<String> {
    (value.chars().count() < 3).then(|| "Too short".to_owned())
}

fn fields(children: Vec<BoxedView>) -> Column {
    Column::new(children)
}

fn mount(form: Form) -> LaidOut {
    lay_out(form, tight(400.0, 300.0))
}

/// `validate()` shows the validator's message, and a valid value clears it
/// on the next `validate()`.
///
/// Also fails if the error is stored but the field never schedules its
/// rebuild: `tick` would then keep painting the old frame.
#[test]
fn validate_shows_the_validator_error_and_revalidating_a_valid_value_clears_it() {
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

/// `OnUserInteraction`: nothing at mount; the first edit validates, and so
/// does every later one.
#[test]
fn on_user_interaction_validates_only_after_the_first_edit() {
    let node = FocusNode::with_debug_label("interaction");
    let mut laid = mount(Form::new(
        RawTextFormField::with_initial_value("")
            .validator(|value| at_least_three(value))
            .autovalidate_mode(AutovalidateMode::OnUserInteraction)
            .focus_node(Rc::clone(&node)),
    ));
    assert!(
        laid.find_text("Too short").is_none(),
        "not validated at mount"
    );

    node.request_focus();
    type_text(&laid, "a");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_some(),
        "the first edit validates"
    );

    type_text(&laid, "bc");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_none(),
        "a valid edit clears it"
    );
}

/// A form-level `OnUserInteraction` validates every field after an edit to
/// any one of them.
#[test]
fn form_level_on_user_interaction_validates_every_field_after_any_edit() {
    let first = FocusNode::with_debug_label("first");
    let mut laid = mount(
        Form::new(fields(vec![
            RawTextFormField::with_initial_value("")
                .validator(required("A required"))
                .focus_node(Rc::clone(&first))
                .boxed(),
            RawTextFormField::with_initial_value("")
                .validator(required("B required"))
                .boxed(),
        ]))
        .autovalidate_mode(AutovalidateMode::OnUserInteraction),
    );
    assert!(laid.find_text("B required").is_none());

    // Settle the focus change first, so the frame after the edit carries
    // only what the edit scheduled.
    first.request_focus();
    laid.tick();
    type_text(&laid, "x");
    laid.tick();
    assert!(
        laid.find_text("A required").is_none(),
        "the edited field passes"
    );
    assert!(
        laid.find_text("B required").is_some(),
        "editing A validates B too"
    );
}

/// `OnUserInteractionIfError`: an edit shows nothing until an error is up;
/// once `validate()` has shown one, a correcting edit clears it.
///
/// Also fails if the mode is treated as `OnUserInteraction`: the first edit
/// would then show the error unasked.
#[test]
fn on_user_interaction_if_error_revalidates_only_while_an_error_is_shown() {
    let form = FormHandle::new();
    let node = FocusNode::with_debug_label("if error");
    let mut laid = mount(
        Form::new(
            RawTextFormField::with_initial_value("")
                .validator(|value| at_least_three(value))
                .autovalidate_mode(AutovalidateMode::OnUserInteractionIfError)
                .focus_node(Rc::clone(&node)),
        )
        .handle(form.clone()),
    );
    node.request_focus();
    type_text(&laid, "a");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_none(),
        "an edit before any error shows nothing"
    );

    assert!(!form.validate());
    laid.tick();
    assert!(laid.find_text("Too short").is_some());

    type_text(&laid, "bc");
    laid.tick();
    assert!(
        laid.find_text("Too short").is_none(),
        "a correcting edit clears the shown error"
    );
}

/// `Always`: the error is on the first frame, and every change re-validates.
#[test]
fn always_validates_at_mount_and_on_every_change() {
    let node = FocusNode::with_debug_label("always");
    let mut laid = mount(Form::new(
        RawTextFormField::with_initial_value("")
            .validator(required("Required"))
            .autovalidate_mode(AutovalidateMode::Always)
            .focus_node(Rc::clone(&node)),
    ));
    assert!(
        laid.find_text("Required").is_some(),
        "shown on the first frame"
    );

    node.request_focus();
    type_text(&laid, "x");
    laid.tick();
    assert!(laid.find_text("Required").is_none());
}

/// `OnUnfocus`: no error while the field has focus; Tab away shows it.
///
/// Also fails without the focus wrapper the field puts around its content:
/// nothing would observe the focus leaving.
#[test]
fn on_unfocus_validates_when_tab_leaves_the_field() {
    let first = FocusNode::with_debug_label("password");
    let second = FocusNode::with_debug_label("next");
    let mut laid = mount(Form::new(fields(vec![
        RawTextFormField::with_initial_value("")
            .validator(required("Required"))
            .autovalidate_mode(AutovalidateMode::OnUnfocus)
            .focus_node(Rc::clone(&first))
            .boxed(),
        RawTextFormField::with_initial_value("ok")
            .focus_node(Rc::clone(&second))
            .boxed(),
    ])));
    first.request_focus();
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "no error while focused"
    );

    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::empty()));
    laid.tick();
    assert!(second.has_primary_focus(), "precondition: Tab moved on");
    assert!(laid.find_text("Required").is_some(), "leaving validates");
}

/// Tab and Shift+Tab walk the fields in order; the focus wrapper each field
/// carries never takes the focus itself.
#[test]
fn tab_and_shift_tab_move_focus_between_form_fields_in_order() {
    let first = FocusNode::with_debug_label("first");
    let second = FocusNode::with_debug_label("second");
    let laid = mount(Form::new(fields(vec![
        RawTextFormField::with_initial_value("")
            .focus_node(Rc::clone(&first))
            .boxed(),
        RawTextFormField::with_initial_value("")
            .focus_node(Rc::clone(&second))
            .boxed(),
    ])));
    let is_primary = |node: &Rc<FocusNode>| {
        laid.focus_manager()
            .primary_focus()
            .is_some_and(|focused| Rc::ptr_eq(&focused, node))
    };

    first.request_focus();
    assert!(is_primary(&first));
    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::empty()));
    assert!(is_primary(&second), "Tab: first -> second");
    laid.focus_manager()
        .dispatch_key_event(&named(NamedKey::Tab, Modifiers::SHIFT));
    assert!(is_primary(&first), "Shift+Tab: second -> first");
}

/// `save()` hands each field's current value to its `on_saved`, in the
/// order the fields registered.
#[test]
fn save_calls_on_saved_with_each_fields_value_in_registration_order() {
    let form = FormHandle::new();
    let saved: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let second = FocusNode::with_debug_label("second");
    let field = |initial: &str, node: Option<&Rc<FocusNode>>| {
        let sink = Rc::clone(&saved);
        let mut field = RawTextFormField::with_initial_value(initial)
            .on_saved(move |value| sink.borrow_mut().push(value.clone()));
        if let Some(node) = node {
            field = field.focus_node(Rc::clone(node));
        }
        field.boxed()
    };
    let laid = mount(
        Form::new(fields(vec![
            field("a", None),
            field("b", Some(&second)),
            field("c", None),
        ]))
        .handle(form.clone()),
    );
    second.request_focus();
    type_text(&laid, "x");

    form.save();

    assert_eq!(*saved.borrow(), ["a", "bx", "c"]);
}

/// `reset()` puts the text back, clears the error and the interaction; the
/// next edit then validates again under `OnUserInteraction`.
///
/// Also fails if writing the initial text back into the controller counted
/// as the user's edit: the field would stay interacted and show the error.
#[test]
fn reset_restores_initial_values_and_clears_errors_and_interaction() {
    let form = FormHandle::new();
    let field = FormFieldHandle::new();
    let controller = TextEditingController::with_text("init");
    let node = FocusNode::with_debug_label("reset");
    let mut laid = mount(
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

    form.reset();
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

/// `force_error_text` is shown whatever the validator says.
#[test]
fn force_error_text_overrides_the_validator() {
    let form = FormHandle::new();
    let laid = mount(
        Form::new(
            RawTextFormField::with_initial_value("fine")
                .validator(|_| None)
                .force_error_text("Taken"),
        )
        .handle(form.clone()),
    );
    assert!(laid.find_text("Taken").is_some());
    assert!(!form.validate(), "a forced error fails validation");
}

/// A field that leaves the tree leaves the form: it no longer fails its
/// `validate()`.
#[test]
fn a_disposed_field_no_longer_takes_part_in_validate() {
    let form = FormHandle::new();
    let tree = |with_invalid: bool| {
        let first = if with_invalid {
            RawTextFormField::with_initial_value("")
                .validator(required("Required"))
                .boxed()
        } else {
            SizedBox::new(10.0, 10.0).boxed()
        };
        Form::new(fields(vec![
            first,
            RawTextFormField::with_initial_value("ok")
                .validator(required("Required"))
                .boxed(),
        ]))
        .handle(form.clone())
    };
    let mut laid = mount(tree(true));
    assert!(!form.validate(), "precondition: the empty field fails");

    laid.pump_widget(tree(false));

    assert!(form.validate(), "the removed field no longer takes part");
}

/// The form is a semantics node with the form role, and the error line is
/// a live region so an appearing error is announced.
#[test]
fn form_reports_the_form_role_and_the_error_line_is_a_live_region() {
    use flui_testing::a11y::Role;

    let mut laid = mount(Form::new(
        RawTextFormField::with_initial_value("")
            .validator(required("Required"))
            .autovalidate_mode(AutovalidateMode::Always),
    ));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");

    tree.find(Role::Form)
        .unwrap_or_else(|error| panic!("expected one form node: {error}\n{}", tree.describe()));
    let error = tree
        .find_by_label("Required")
        .unwrap_or_else(|error| panic!("expected the error line: {error}\n{}", tree.describe()));
    assert!(
        error.raw().live().is_some(),
        "the error line is a live region"
    );
}

/// Copy and paste work in a text form field, and a paste is the user's
/// edit: the field's value follows it.
///
/// Fails without the clipboard bindings: the chord is left unconsumed and
/// the clipboard stays empty.
#[test]
fn copy_then_paste_round_trips_text_in_a_text_form_field() {
    let controller = TextEditingController::with_text("abc");
    let field = FormFieldHandle::new();
    let node = FocusNode::with_debug_label("clipboard");
    let laid = mount(Form::new(
        RawTextFormField::new(controller.clone())
            .focus_node(Rc::clone(&node))
            .handle(field.clone()),
    ));
    node.request_focus();
    controller.set_selection(0, 3);

    laid.focus_manager().dispatch_key_event(&chord('c'));
    assert_eq!(laid.clipboard().read_text().as_deref(), Some("abc"));

    controller.set_caret_byte_offset(3);
    laid.focus_manager().dispatch_key_event(&chord('v'));
    assert_eq!(controller.text(), "abcabc");
    assert_eq!(field.value(), "abcabc");
    assert!(field.has_interacted_by_user(), "a paste is the user's edit");
}
