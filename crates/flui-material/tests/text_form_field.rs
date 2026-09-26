//! `flui_material::TextFormField` in a mounted `Form`: the field's error
//! reaches the `InputDecorator`'s error line, and a reset restores the text.

use crate::common;

use std::rc::Rc;

use common::{lay_out, tight};
use flui_interaction::FocusNode;
use flui_interaction::events::{Code, Key, KeyState, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_material::{InputDecoration, TextFormField, Theme, ThemeData};
use flui_widgets::{Form, FormHandle, TextEditingController};

fn required(value: &str) -> Option<String> {
    value.is_empty().then(|| "Required".to_owned())
}

/// `validate()` puts the validator's message on the decorator's error line
/// (`decoration.copyWith(errorText: field.errorText)`), and a valid value
/// takes it away again.
///
/// Fails without the builder writing the field's error into the decoration:
/// nothing renders "Required".
#[test]
fn validator_error_reaches_the_input_decorator_error_line() {
    let form = FormHandle::new();
    let node = FocusNode::with_debug_label("email");
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Form::new(
                TextFormField::with_initial_value("")
                    .decoration(InputDecoration {
                        label_text: Some("Email".to_owned()),
                        ..InputDecoration::default()
                    })
                    .validator(|value| required(value))
                    .focus_node(Rc::clone(&node)),
            )
            .handle(form.clone()),
        ),
        tight(300.0, 120.0),
    );
    assert!(laid.find_text("Required").is_none());

    assert!(!form.validate());
    laid.tick();
    assert!(laid.find_text("Required").is_some(), "the error line shows");

    node.request_focus();
    let event = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character("a".to_owned()))
        .with_state(KeyState::Down)
        .build();
    laid.focus_manager().dispatch_key_event(&event);
    assert!(form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "a valid value clears it"
    );
}

/// A caller-set `error_text` shows while the field has no error of its own,
/// and the field's error replaces it once validation fails — Flutter's
/// `copyWith(errorText: null)` keeps the existing value.
///
/// Fails when the builder assigns the field's `None` error over the
/// decoration's: "Server says taken" never renders.
#[test]
fn a_caller_set_error_text_shows_until_the_field_has_its_own_error() {
    let form = FormHandle::new();
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Form::new(
                TextFormField::with_initial_value("x")
                    .decoration(InputDecoration {
                        error_text: Some("Server says taken".to_owned()),
                        ..InputDecoration::default()
                    })
                    .validator(|value| (value == "x").then(|| "Too short".to_owned())),
            )
            .handle(form.clone()),
        ),
        tight(300.0, 120.0),
    );
    assert!(
        laid.find_text("Server says taken").is_some(),
        "the caller's error shows before validation"
    );

    assert!(!form.validate());
    laid.tick();
    assert!(
        laid.find_text("Too short").is_some(),
        "the field's error shows"
    );
    assert!(
        laid.find_text("Server says taken").is_none(),
        "and replaces the caller's"
    );
}

/// `reset()` writes the initial text back into the controller.
#[test]
fn reset_restores_the_initial_value() {
    let form = FormHandle::new();
    let controller = TextEditingController::with_text("start");
    let node = FocusNode::with_debug_label("reset");
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Form::new(TextFormField::new(controller.clone()).focus_node(Rc::clone(&node)))
                .handle(form.clone()),
        ),
        tight(300.0, 120.0),
    );
    node.request_focus();
    let backspace = KeyEventBuilder::new(Code::Backspace)
        .with_key(Key::Named(NamedKey::Backspace))
        .with_state(KeyState::Down)
        .build();
    laid.focus_manager().dispatch_key_event(&backspace);
    assert_eq!(controller.text(), "star");
    assert!(form.has_interacted_by_user());

    form.reset();

    assert_eq!(controller.text(), "start");
    assert!(!form.has_interacted_by_user());
}
