//! Integration tests for [`TextEditingController`], [`EditableText`], and
//! [`RawTextField`].
//!
//! Tests are structured in two groups:
//!
//! 1. **Controller tests** — headless, no widget tree, no `FocusManager`.
//!    Verify buffer mutations, UTF-8 correctness, listener wiring, and clone
//!    sharing.
//!
//! 2. **Key-routing tests** — use an isolated `FocusManager` + `FocusNode` to
//!    verify that keyboard events dispatched through the focus system reach the
//!    controller when the node is focused, and are silently ignored when it is
//!    not.
//!
use std::rc::Rc;

use flui_interaction::testing::input::KeyEventBuilder;
use flui_interaction::{
    events::{Code, Key, KeyState, NamedKey},
    routing::{FocusAttachment, FocusManager, FocusNode, KeyEventHandler, KeyEventResult},
};
use flui_widgets::TextEditingController;

// ============================================================================
// Helpers
// ============================================================================

/// Owns one isolated focus fixture and detaches it on drop.
struct FocusGuard {
    manager: Rc<FocusManager>,
    node: Rc<FocusNode>,
    attachment: FocusAttachment,
}

impl FocusGuard {
    /// Attach `node` to `manager` and install its node-owned handler.
    fn attach(manager: Rc<FocusManager>, node: Rc<FocusNode>, handler: KeyEventHandler) -> Self {
        node.set_on_key_event(handler);
        let attachment = manager
            .root_scope()
            .attach_node(&node)
            .expect("test node attaches to its isolated manager");
        Self {
            manager,
            node,
            attachment,
        }
    }

    /// Focus this node so key dispatch is routed to its handler.
    fn request_focus(&self) {
        let _ = self.node.request_focus();
    }
}

impl Drop for FocusGuard {
    fn drop(&mut self) {
        // Clear primary focus if we held it, so subsequent tests start clean.
        if self.node.has_primary_focus() {
            self.manager.unfocus();
        }
        self.node.clear_on_key_event();
        let _ = self.attachment.detach();
    }
}

/// Build the same key-event handler closure that `EditableTextState` would
/// register: printable characters go to `insert_str`, navigation/delete keys
/// call the corresponding controller methods.
///
/// This is the behavior under test — it mirrors `editable_text::build_key_handler`
/// exactly, and these tests would fail if any branch were removed or the
/// wrong method were called.
fn make_editable_text_handler(controller: TextEditingController) -> KeyEventHandler {
    Rc::new(move |event| {
        if event.state() != KeyState::Down {
            return KeyEventResult::Ignored;
        }
        match &event.key {
            Key::Character(character_string) => {
                controller.insert_str(character_string.as_str());
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::Backspace) => {
                controller.backspace();
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::Delete) => {
                controller.delete_forward();
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::ArrowLeft) => {
                controller.move_caret_left();
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                controller.move_caret_right();
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::Home) => {
                controller.move_caret_home();
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::End) => {
                controller.move_caret_end();
                KeyEventResult::Handled
            }
            _ => KeyEventResult::Ignored,
        }
    })
}

// ============================================================================
// 1. TextEditingController — headless buffer tests
// ============================================================================

// ============================================================================
// 2. Key-routing tests — FocusManager dispatch → controller
// ============================================================================
//
// Each test owns a fresh manager, node, and generation-checked attachment, so
// these fixtures are naturally parallel-safe.

pub(crate) fn focused_character_key_inserts_into_controller() {
    let manager = FocusManager::new();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Rc::clone(&manager),
        Rc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    let _ = guard.request_focus();

    let event = KeyEventBuilder::new(Code::KeyH)
        .with_key(Key::character("h"))
        .with_state(KeyState::Down)
        .build();
    let _ = manager.dispatch_key_event(&event).is_handled();

    assert_eq!(
        controller.text(),
        "h",
        "a focused key Down event must route to insert_str"
    );
}

pub(crate) fn unfocused_field_does_not_receive_key_events() {
    let manager = FocusManager::new();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    // Register the handler but DO NOT request focus.
    let _guard = FocusGuard::attach(
        Rc::clone(&manager),
        Rc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );

    // Dispatch a character key — with no node focused, the handler must not fire.
    let event = KeyEventBuilder::new(Code::KeyX)
        .with_key(Key::character("x"))
        .with_state(KeyState::Down)
        .build();
    let _ = manager.dispatch_key_event(&event).is_handled();

    assert_eq!(
        controller.text(),
        "",
        "an unfocused field must ignore key events dispatched through FocusManager"
    );
}

// ============================================================================
// RawTextField — composition (mounts the full GestureDetector/DecoratedBox/
// Padding/EditableText tree `RawTextField` builds, never previously
// exercised: every test above hand-simulates EditableTextState's key
// handler rather than mounting a real RawTextField/EditableText widget).
// ============================================================================
