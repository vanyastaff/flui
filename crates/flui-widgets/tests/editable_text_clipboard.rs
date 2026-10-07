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

pub(crate) fn select_all_replaces_the_complete_unicode_document_without_reporting_selection_as_an_edit()
 {
    crate::common::cases::run_cases(
        "select all",
        &[
            ("plain field", select_all_plain as fn()),
            ("obscured field", select_all_obscured),
        ],
    );
}

fn select_all_plain() {
    select_all_replacement(false);
}

fn select_all_obscured() {
    select_all_replacement(true);
}

fn select_all_replacement(obscured: bool) {
    use std::cell::Cell;
    let controller = TextEditingController::new();
    let changes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&changes);
    let (harness, _node) = mount_field(&controller, move |field| {
        field
            .obscure_text(obscured)
            .on_changed(move |_cx, _text| counted.set(counted.get() + 1))
    });
    let text = "A😀e\u{301}東京";
    for scalar in text.chars() {
        assert!(
            harness
                .focus_manager()
                .dispatch_key_event(&chord(&scalar.to_string(), Modifiers::empty()))
        );
    }
    let edits = changes.get();
    let other_command = if command() == Modifiers::META {
        Modifiers::CONTROL
    } else {
        Modifiers::META
    };
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", other_command))
    );
    assert!(
        !controller.has_selection(),
        "the other platform's command chord does not select text"
    );
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("A", command())),
        "Caps Lock does not defeat select all"
    );
    assert_eq!(
        controller.selection(),
        0..text.len(),
        "the whole UTF-8 document is selected"
    );
    assert_eq!(controller.selected_text(), text);
    assert_eq!(controller.text(), text, "selecting never changes text");
    assert_eq!(changes.get(), edits, "selection does not call on_changed");
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("文", Modifiers::empty()))
    );
    assert_eq!(
        controller.text(),
        "文",
        "a real typed scalar replaces every selected grapheme"
    );
    assert_eq!(controller.caret_byte_offset(), "文".len());
    assert!(!controller.has_selection());
    assert_eq!(changes.get(), edits + 1, "replacement is one user edit");
    assert_eq!(
        harness.clipboard().read_text(),
        None,
        "selection requires no clipboard operation"
    );
}

pub(crate) fn select_all_without_a_focused_text_field_leaves_the_key_unconsumed() {
    let controller = TextEditingController::with_text("retained");
    let (harness, node) = mount_field(&controller, |field| field);
    node.set_can_request_focus(false);
    assert!(!node.has_primary_focus());
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
    );
    assert!(!controller.has_selection());
    assert_eq!(controller.text(), "retained");
}

pub(crate) fn select_all_defers_to_an_active_composition_and_recovers_after_commit() {
    let controller = TextEditingController::new();
    let node = FocusNode::new();
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&node),
    ));
    node.request_focus();
    harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
        text: "東京".to_owned(),
        cursor: Some(("東京".len(), "東京".len())),
    });
    let before = controller.selection();
    assert!(controller.is_composing());
    assert!(
        !harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
    );
    assert_eq!(controller.selection(), before, "IME retains its selection");
    assert_eq!(controller.text(), "東京");
    harness.dispatch_ime(&flui_platform_api::ImeEvent::Commit("東京".to_owned()));
    assert!(!controller.is_composing());
    assert!(
        harness
            .focus_manager()
            .dispatch_key_event(&chord("a", command()))
    );
    assert_eq!(
        controller.selection(),
        0.."東京".len(),
        "select all resumes after commit"
    );
}

pub(crate) fn paste_rechecks_focus_after_committing_composition() {
    let controller = TextEditingController::new();
    let node = FocusNode::new();
    let retiring = node.clone();
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), node.clone())
            .on_changed(move |_cx, _text| retiring.unfocus()),
    );
    node.request_focus();
    harness.clipboard().write_text("paste".into());
    harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
        text: "composition".into(),
        cursor: None,
    });
    assert!(
        node.has_primary_focus(),
        "preedit does not report a committed edit"
    );
    harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()));
    assert_eq!(
        controller.text(),
        "composition",
        "the composition committed, but an unfocused field cannot paste"
    );
}
