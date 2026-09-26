//! Copy, cut and paste in a mounted [`EditableText`], through the key path a
//! platform chord takes: `FocusManager::dispatch_key_event` → the field's key
//! handler leaves the chord unconsumed → `DefaultFocusTraversal`'s
//! `Shortcuts` (installed by the harness's `FocusRoot`) resolves the intent
//! against the chain the field recorded on its node. The clipboard read back
//! is the one the harness installs, the headless platform's type.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform_api::Clipboard as _;
use flui_widgets::{
    Actions, CallbackAction, EditableText, Focus, PasteTextIntent, TextEditingController,
};

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
#[test]
fn copy_then_paste_round_trips_text_in_an_editable_text() {
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

/// Cut writes the selection, deletes it, and leaves the caret where it began.
#[test]
fn cut_removes_the_selection_and_writes_it_to_the_clipboard() {
    let controller = TextEditingController::with_text("abc");
    let (harness, _node) = mount_field(&controller, |field| field);
    controller.set_selection(1, 2);

    harness
        .focus_manager()
        .dispatch_key_event(&chord("x", command()));

    assert_eq!(controller.text(), "ac");
    assert_eq!(harness.clipboard().read_text().as_deref(), Some("b"));
    assert_eq!(controller.caret_byte_offset(), 1);
    assert!(!controller.has_selection());
}

/// A password field never hands its text to the clipboard, and the chord it
/// declines keeps bubbling.
#[test]
fn copy_and_cut_on_an_obscured_field_leave_the_clipboard_untouched_and_the_key_unconsumed() {
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

/// Paste replaces the selection, and a single-line field drops line breaks —
/// `\r` as well as `\n`, so a Windows line ending leaves nothing behind.
#[test]
fn paste_replaces_the_selection_and_drops_line_breaks() {
    let controller = TextEditingController::with_text("0123");
    let (harness, _node) = mount_field(&controller, |field| field);
    controller.set_selection(1, 3);
    harness.clipboard().write_text("x\r\ny".to_owned());

    let consumed = harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()));

    assert!(consumed);
    assert_eq!(controller.text(), "0xy3");
    assert_eq!(controller.caret_byte_offset(), 3);
}

/// A field disabled after it was focused takes no paste: disabling releases
/// its focus, so the chord never reaches it. (The action's own `enabled`
/// check guards a dispatch that reaches the node anyway; the key path offers
/// no way to reach a disabled field, so this test does not exercise it.)
#[test]
fn paste_into_a_disabled_field_changes_nothing() {
    let controller = TextEditingController::with_text("keep");
    let focus_node = FocusNode::with_debug_label("disabled paste");
    let mut harness = crate::common::harness::mount(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();
    harness.swap_root(EditableText::new(controller.clone(), Rc::clone(&focus_node)).enabled(false));
    harness.clipboard().write_text("pasted".to_owned());

    harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()));

    assert_eq!(controller.text(), "keep");
}

/// With no text field focused the default clipboard bindings resolve to no
/// enabled action, so the chord is left unconsumed and keeps bubbling past
/// the focus root to whatever else binds it.
///
/// Fails if `DefaultFocusTraversal` consumed a clipboard chord no focused
/// widget answers.
#[test]
fn ctrl_c_with_no_text_field_focused_is_left_unconsumed() {
    let focus_node = FocusNode::with_debug_label("plain focus");
    let harness = crate::common::harness::mount(
        Focus::new(flui_widgets::SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&focus_node)),
    );
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus(), "precondition: focused");

    for key in ["c", "x", "v"] {
        let consumed = harness
            .focus_manager()
            .dispatch_key_event(&chord(key, command()));
        assert!(!consumed, "{key}: nothing answers the chord");
    }
}

/// `EditableText`'s clipboard actions are the nearest declaration of their
/// intents, so an ancestor `Actions` mapping for the same intent never
/// replaces them (no `Action.overridable`).
///
/// Fails if the field's actions were resolved after the ancestor chain: the
/// ancestor's callback would count the paste and the field stay unchanged.
#[test]
fn an_ancestor_paste_action_does_not_replace_the_fields_own() {
    let ancestor_pastes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&ancestor_pastes);
    let controller = TextEditingController::with_text("ab");
    let focus_node = FocusNode::with_debug_label("overridden field");
    let harness = crate::common::harness::mount(
        Actions::new(EditableText::new(
            controller.clone(),
            Rc::clone(&focus_node),
        ))
        .action(CallbackAction::new(move |_: &PasteTextIntent| {
            counter.set(counter.get() + 1);
        })),
    );
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus(), "precondition: focused");
    controller.set_caret_byte_offset(2);
    harness.clipboard().write_text("c".to_owned());

    let consumed = harness
        .focus_manager()
        .dispatch_key_event(&chord("v", command()));

    assert!(consumed);
    assert_eq!(controller.text(), "abc", "the field's own paste ran");
    assert_eq!(ancestor_pastes.get(), 0, "the ancestor mapping did not");
}

/// `EditableText` is a text field to assistive technology: a text-input node
/// whose value is what it shows, and a password node when obscured.
///
/// Fails without the change: the field published no semantics of its own.
#[test]
fn editable_text_reports_a_text_field_semantics_node() {
    use flui_testing::a11y::Role;

    let mut laid = crate::common::lay_out(
        EditableText::new(
            TextEditingController::with_text("hello"),
            FocusNode::with_debug_label("semantics field"),
        ),
        crate::common::tight(200.0, 40.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let field = tree.find(Role::TextInput).unwrap_or_else(|error| {
        panic!("expected one text field node: {error}\n{}", tree.describe())
    });
    assert_eq!(field.value(), Some("hello"));

    let mut laid = crate::common::lay_out(
        EditableText::new(
            TextEditingController::with_text("pw"),
            FocusNode::with_debug_label("obscured semantics field"),
        )
        .obscure_text(true),
        crate::common::tight(200.0, 40.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let field = tree.find(Role::PasswordInput).unwrap_or_else(|error| {
        panic!(
            "expected one password field node: {error}\n{}",
            tree.describe()
        )
    });
    assert_ne!(
        field.value(),
        Some("pw"),
        "the real text never reaches the tree"
    );
}
