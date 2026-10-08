//! `flui_material::TextFormField` in a mounted `Form`: the field's error
//! reaches the `InputDecorator`'s error line, and a reset restores the text.

use crate::common;

use std::rc::Rc;

use common::{lay_out, tight};
use flui_interaction::events::{Code, Key, KeyState};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_interaction::KeyEventResult;
use flui_material::{InputDecoration, TextFormField, Theme, ThemeData};
use flui_sdk::interaction::FocusNode;
use flui_sdk::widgets::{Form, FormHandle};

fn required(value: &str) -> Option<String> {
    value.is_empty().then(|| "Required".to_owned())
}

/// `validate()` puts the validator's message on the decorator's error line
/// (`decoration.copyWith(errorText: field.errorText)`), and a valid value
/// takes it away again.
///
/// Fails without the builder writing the field's error into the decoration:
/// nothing renders "Required".
pub fn validator_error_reaches_the_input_decorator_error_line() {
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

    let _ = node.request_focus();
    let event = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::character("a"))
        .with_state(KeyState::Down)
        .build();
    assert_eq!(
        laid.focus_manager().dispatch_key_event(&event),
        KeyEventResult::Handled
    );
    assert!(form.validate());
    laid.tick();
    assert!(
        laid.find_text("Required").is_none(),
        "a valid value clears it"
    );
}
