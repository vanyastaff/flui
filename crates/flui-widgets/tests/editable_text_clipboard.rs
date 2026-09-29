//! Copy, cut and paste in a mounted [`EditableText`], through the key path a
//! platform chord takes: `FocusManager::dispatch_key_event` → the field's key
//! handler leaves the chord unconsumed → `DefaultFocusTraversal`'s
//! `Shortcuts` (installed by the harness's `FocusRoot`) resolves the intent
//! against the chain the field recorded on its node. The clipboard read back
//! is the one the harness installs, the headless platform's type.

use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform_api::Clipboard as _;
use flui_widgets::{EditableText, TextEditingController};

/// The platform's command modifier, as `DefaultFocusTraversal` binds it.
fn command() -> Modifiers {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    }
}

fn chord(character: &str, modifiers: Modifiers) -> KeyEvent {
    KeyEventBuilder::new(Code::KeyC)
        .with_key(Key::Character(character.to_owned()))
        .with_state(KeyState::Down)
        .with_modifiers(modifiers)
        .build()
}

fn mount_field(
    controller: &TextEditingController,
    configure: impl FnOnce(EditableText) -> EditableText,
) -> (crate::common::harness::Harness, Rc<FocusNode>) {
    let focus_node = FocusNode::with_debug_label("clipboard field");
    let harness = crate::common::harness::mount(configure(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    )));
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus(), "precondition: focused");
    (harness, focus_node)
}

/// Copy writes the selection and keeps it; paste inserts at the caret.
///
/// Fails without the change: the chord was left unconsumed by the field and
/// bound by nothing, so the clipboard stayed empty.
pub(crate) fn copy_then_paste_round_trips_text_in_an_editable_text() {
    let controller = TextEditingController::with_text("abc");
    let (harness, _node) = mount_field(&controller, |field| field);
    controller.set_selection(0, 3);

    let consumed = harness
        .focus_manager()
        .dispatch_key_event(&chord("c", command()));
    assert!(consumed, "copy consumes the chord");
    assert_eq!(harness.clipboard().read_text().as_deref(), Some("abc"));
    assert_eq!(controller.selection(), 0..3, "copy keeps the selection");

    controller.set_caret_byte_offset(3);
    harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()));
    assert_eq!(controller.text(), "abcabc");
    assert_eq!(controller.caret_byte_offset(), 6);
}

/// A password field never hands its text to the clipboard, and the chord it
/// declines keeps bubbling.
pub(crate) fn copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed()
 {
    let controller = TextEditingController::with_text("secret");
    let (harness, _node) = mount_field(&controller, |field| field.obscure_text(true));
    controller.set_selection(0, 6);

    for key in ["c", "x"] {
        let consumed = harness
            .focus_manager()
            .dispatch_key_event(&chord(key, command()));
        assert!(
            !consumed,
            "{key}: a disabled action leaves the key unconsumed"
        );
    }
    assert_eq!(harness.clipboard().read_text(), None);
    assert_eq!(controller.text(), "secret", "cut deleted nothing");
}
