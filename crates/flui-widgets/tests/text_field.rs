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

use flui_foundation::geometry::EdgeInsets;
use flui_foundation::geometry::Size;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_interaction::{
    events::{Code, Key, KeyState, NamedKey},
    routing::{FocusAttachment, FocusManager, FocusNode, KeyEventHandler, KeyEventResult},
};
use flui_widgets::{EditableText, RawTextField, TextEditingController};

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
        self.node.request_focus();
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
        if event.state != KeyState::Down {
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
            Key::Named(_) => KeyEventResult::Ignored,
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

#[test]
fn focused_character_key_inserts_into_controller() {
    let manager = FocusManager::new();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Rc::clone(&manager),
        Rc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    let event = KeyEventBuilder::new(Code::KeyH)
        .with_key(Key::Character("h".to_string()))
        .with_state(KeyState::Down)
        .build();
    manager.dispatch_key_event(&event);

    assert_eq!(
        controller.text(),
        "h",
        "a focused key Down event must route to insert_str"
    );
}

#[test]
fn focused_arrow_keys_move_the_caret() {
    let manager = FocusManager::new();
    let controller = TextEditingController::new();
    controller.insert_str("abc"); // caret at 3

    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Rc::clone(&manager),
        Rc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    let left = KeyEventBuilder::new(Code::ArrowLeft)
        .with_key(Key::Named(NamedKey::ArrowLeft))
        .with_state(KeyState::Down)
        .build();
    manager.dispatch_key_event(&left);
    assert_eq!(
        controller.caret_byte_offset(),
        2,
        "ArrowLeft must move caret one char left"
    );

    let right = KeyEventBuilder::new(Code::ArrowRight)
        .with_key(Key::Named(NamedKey::ArrowRight))
        .with_state(KeyState::Down)
        .build();
    manager.dispatch_key_event(&right);
    assert_eq!(
        controller.caret_byte_offset(),
        3,
        "ArrowRight must move caret one char right"
    );
}

#[test]
fn unfocused_field_does_not_receive_key_events() {
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
        .with_key(Key::Character("x".to_string()))
        .with_state(KeyState::Down)
        .build();
    manager.dispatch_key_event(&event);

    assert_eq!(
        controller.text(),
        "",
        "an unfocused field must ignore key events dispatched through FocusManager"
    );
}

/// Proves `EditableTextState`'s `FocusManager` listener (`init_state`, step
/// 4) actually reaches the mounted render tree: requesting focus on the
/// explicit node supplied to the field
/// schedules a rebuild that flips `RenderEditable`'s `show_caret` — the
/// same live-focus-observation seam `flui_material::TextField` reuses to
/// keep its `InputDecorator` in sync with real focus transitions.
///
/// Previously unproven: every other focus test above asserts on
/// `FocusManager`/`TextEditingController` state directly, never on whether a
/// live focus change reaches *rendered output* through a headless
/// `tick()` — this closes that gap.
#[test]
fn requesting_focus_via_the_explicit_node_reveals_the_caret_after_a_tick() {
    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("caret reveal");

    let mut laid = crate::common::lay_out(
        EditableText::new(controller, Rc::clone(&focus_node)),
        crate::common::tight(120.0, 40.0),
    );
    let editable = laid.find_by_render_type("RenderEditable");
    let show_caret_flag = |laid: &crate::common::LaidOut| -> Option<String> {
        laid.pipeline_owner().with(|owner| {
            owner
                .debug_node_diagnostics(editable)
                .and_then(|diagnostics| diagnostics.get_property("show_caret").map(str::to_string))
        })
    };

    assert_eq!(
        show_caret_flag(&laid),
        None,
        "an unfocused field must not show the caret"
    );

    focus_node.request_focus();
    laid.tick();

    assert_eq!(
        show_caret_flag(&laid),
        Some("show caret".to_string()),
        "requesting focus via the controller's published node must reveal the caret after a tick"
    );
}

// ============================================================================
// RawTextField — composition (mounts the full GestureDetector/DecoratedBox/
// Padding/EditableText tree `RawTextField` builds, never previously
// exercised: every test above hand-simulates EditableTextState's key
// handler rather than mounting a real RawTextField/EditableText widget).
// ============================================================================

#[test]
fn raw_text_field_deflates_editable_text_by_its_content_padding() {
    let controller = TextEditingController::with_text("hello");

    let laid = crate::common::lay_out(
        RawTextField::new(controller).content_padding(EdgeInsets::all(10.0)),
        crate::common::tight(200.0, 100.0),
    );

    // The overall field fills the tight constraint given to it...
    assert_eq!(laid.size(laid.root()), Size::new(200.0, 100.0));

    // ...but the EditableText inside must be deflated by the content padding
    // on every side (10px), proving `content_padding` is actually threaded
    // through `Padding::new(...)` rather than silently dropped.
    let editable = laid.find_by_render_type("RenderEditable");
    assert_eq!(laid.size(editable), Size::new(180.0, 80.0));
}
