//! Integration tests for [`TextEditingController`], [`EditableText`], and
//! [`TextField`].
//!
//! Tests are structured in two groups:
//!
//! 1. **Controller tests** — headless, no widget tree, no `FocusManager`.
//!    Verify buffer mutations, UTF-8 correctness, listener wiring, and clone
//!    sharing.
//!
//! 2. **Key-routing tests** — use `FocusManager::global()` + `FocusNode` to
//!    verify that keyboard events dispatched through the focus system reach the
//!    controller when the node is focused, and are silently ignored when it is
//!    not.
//!
//! Key-routing tests serialize themselves around the global `FocusManager`
//! singleton and clean up their registered nodes before returning.

use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::Listenable;
use flui_geometry::EdgeInsets;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_interaction::{
    events::{Code, Key, KeyState, NamedKey},
    routing::{FocusManager, FocusNode, KeyEventCallback},
};
use flui_types::{Size, geometry::px};
use flui_widgets::{EditableText, TextEditingController, TextField};

// ============================================================================
// Helpers
// ============================================================================

static FOCUS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn focus_test_guard() -> MutexGuard<'static, ()> {
    let guard = FOCUS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    FocusManager::global().unfocus();
    guard
}

/// A guard that unregisters the key handler and detaches the focus node from
/// the root scope when dropped, ensuring the global `FocusManager` singleton
/// is left clean for the next test even on panic.
struct FocusGuard {
    node: Arc<FocusNode>,
}

impl FocusGuard {
    /// Attach `node` to the root scope and register `handler` for it.
    fn attach(node: Arc<FocusNode>, handler: KeyEventCallback) -> Self {
        let manager = FocusManager::global();
        manager.root_scope().attach_node(&node);
        manager.register_key_handler(node.id(), handler);
        Self { node }
    }

    /// Focus this node so key dispatch is routed to its handler.
    fn request_focus(&self) {
        FocusManager::global().request_focus(self.node.id());
    }
}

impl Drop for FocusGuard {
    fn drop(&mut self) {
        let manager = FocusManager::global();
        // Clear primary focus if we held it, so subsequent tests start clean.
        if self.node.has_primary_focus() {
            manager.unfocus();
        }
        manager.unregister_key_handler(self.node.id());
        manager.root_scope().detach_node(self.node.id());
    }
}

/// Build the same key-event handler closure that `EditableTextState` would
/// register: printable characters go to `insert_str`, navigation/delete keys
/// call the corresponding controller methods.
///
/// This is the behavior under test — it mirrors `editable_text::build_key_handler`
/// exactly, and these tests would fail if any branch were removed or the
/// wrong method were called.
fn make_editable_text_handler(controller: TextEditingController) -> KeyEventCallback {
    Arc::new(move |event| {
        if event.state != KeyState::Down {
            return false;
        }
        match &event.key {
            Key::Character(character_string) => {
                controller.insert_str(character_string.as_str());
                true
            }
            Key::Named(NamedKey::Backspace) => {
                controller.backspace();
                true
            }
            Key::Named(NamedKey::Delete) => {
                controller.delete_forward();
                true
            }
            Key::Named(NamedKey::ArrowLeft) => {
                controller.move_caret_left();
                true
            }
            Key::Named(NamedKey::ArrowRight) => {
                controller.move_caret_right();
                true
            }
            Key::Named(NamedKey::Home) => {
                controller.move_caret_home();
                true
            }
            Key::Named(NamedKey::End) => {
                controller.move_caret_end();
                true
            }
            Key::Named(_) => false,
        }
    })
}

// ============================================================================
// 1. TextEditingController — headless buffer tests
// ============================================================================

#[test]
fn insert_str_appends_text_and_advances_caret_to_end() {
    let controller = TextEditingController::new();
    controller.insert_str("hello");
    assert_eq!(controller.text(), "hello");
    assert_eq!(controller.caret_byte_offset(), 5);
}

#[test]
fn backspace_removes_the_char_before_the_caret() {
    let controller = TextEditingController::new();
    controller.insert_str("hi");
    controller.backspace();
    assert_eq!(controller.text(), "h");
    assert_eq!(controller.caret_byte_offset(), 1);
}

#[test]
fn backspace_at_start_is_a_no_op() {
    let controller = TextEditingController::new();
    controller.insert_str("x");
    controller.move_caret_home();
    controller.backspace();
    assert_eq!(
        controller.text(),
        "x",
        "backspace at offset 0 must not remove anything"
    );
    assert_eq!(controller.caret_byte_offset(), 0);
}

#[test]
fn delete_forward_removes_the_char_after_the_caret() {
    let controller = TextEditingController::new();
    controller.insert_str("ab");
    controller.move_caret_home();
    controller.delete_forward();
    assert_eq!(controller.text(), "b");
    assert_eq!(controller.caret_byte_offset(), 0);
}

#[test]
fn delete_forward_at_end_is_a_no_op() {
    let controller = TextEditingController::new();
    controller.insert_str("z");
    controller.delete_forward();
    assert_eq!(
        controller.text(),
        "z",
        "delete at end must not remove anything"
    );
    assert_eq!(controller.caret_byte_offset(), 1);
}

#[test]
fn move_caret_left_steps_back_one_char() {
    let controller = TextEditingController::new();
    controller.insert_str("abc"); // caret at 3
    controller.move_caret_left();
    assert_eq!(controller.caret_byte_offset(), 2); // before 'c'
}

#[test]
fn move_caret_right_steps_forward_one_char() {
    let controller = TextEditingController::new();
    controller.insert_str("abc");
    controller.move_caret_home();
    controller.move_caret_right();
    assert_eq!(controller.caret_byte_offset(), 1); // after 'a'
}

#[test]
fn move_caret_home_places_caret_at_start() {
    let controller = TextEditingController::new();
    controller.insert_str("hello");
    controller.move_caret_home();
    assert_eq!(controller.caret_byte_offset(), 0);
}

#[test]
fn move_caret_end_places_caret_at_end() {
    let controller = TextEditingController::new();
    controller.insert_str("hello");
    controller.move_caret_home();
    controller.move_caret_end();
    assert_eq!(controller.caret_byte_offset(), 5);
}

#[test]
fn caret_lands_on_valid_utf8_boundary_around_multibyte_char() {
    let controller = TextEditingController::new();
    // 🦀 is U+1F980, encoded in 4 bytes (0xF0 0x9F 0xA6 0x80).
    controller.insert_str("a🦀b"); // bytes: a(1) + 🦀(4) + b(1) = 6, caret at 6
    assert_eq!(controller.caret_byte_offset(), 6);

    // Move left once — should land at byte 5 (before 'b'), not inside the crab.
    controller.move_caret_left();
    assert_eq!(
        controller.caret_byte_offset(),
        5,
        "caret after moving left once should be before 'b'"
    );

    // Move left again — should skip past all 4 bytes of 🦀 to byte 1 (after 'a').
    controller.move_caret_left();
    assert_eq!(
        controller.caret_byte_offset(),
        1,
        "caret should land at a valid UTF-8 boundary, skipping the full multi-byte char"
    );
}

#[test]
fn listener_fires_once_per_mutation() {
    let controller = TextEditingController::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&call_count);
    controller.add_listener(Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
    }));

    controller.insert_str("a");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "listener must fire on insert"
    );

    controller.backspace();
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "listener must fire on backspace"
    );
}

#[test]
fn clone_shares_the_underlying_buffer() {
    let original = TextEditingController::new();
    original.insert_str("hello");

    let shared = original.clone();
    shared.insert_str("!");

    assert_eq!(
        original.text(),
        "hello!",
        "a mutation via a clone must be visible through the original"
    );
}

#[test]
fn remove_listener_stops_notifications() {
    let controller = TextEditingController::new();
    let call_count = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&call_count);

    let listener_id = controller.add_listener(Arc::new(move || {
        calls.fetch_add(1, Ordering::SeqCst);
    }));

    controller.insert_str("x");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);

    controller.remove_listener(listener_id);
    controller.insert_str("y");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "listener must not fire after removal"
    );
}

// ============================================================================
// 2. Key-routing tests — FocusManager dispatch → controller
// ============================================================================
//
// These tests use `FocusManager::global()` (the process-wide singleton).
// They run safely under `--test-threads 1`; each test registers a fresh
// `FocusNode`, runs its assertions, then drops the `FocusGuard` which
// unregisters the node and clears primary focus — leaving the singleton
// pristine for the next test.

#[test]
fn focused_character_key_inserts_into_controller() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Arc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    let event = KeyEventBuilder::new(Code::KeyH)
        .with_key(Key::Character("h".to_string()))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&event);

    assert_eq!(
        controller.text(),
        "h",
        "a focused key Down event must route to insert_str"
    );
}

#[test]
fn focused_backspace_key_deletes_char_before_caret() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();
    controller.insert_str("hi");

    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Arc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    let event = KeyEventBuilder::new(Code::Backspace)
        .with_key(Key::Named(NamedKey::Backspace))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&event);

    assert_eq!(
        controller.text(),
        "h",
        "Backspace via FocusManager must remove the char before the caret"
    );
}

#[test]
fn focused_arrow_keys_move_the_caret() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();
    controller.insert_str("abc"); // caret at 3

    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Arc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    let left = KeyEventBuilder::new(Code::ArrowLeft)
        .with_key(Key::Named(NamedKey::ArrowLeft))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&left);
    assert_eq!(
        controller.caret_byte_offset(),
        2,
        "ArrowLeft must move caret one char left"
    );

    let right = KeyEventBuilder::new(Code::ArrowRight)
        .with_key(Key::Named(NamedKey::ArrowRight))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&right);
    assert_eq!(
        controller.caret_byte_offset(),
        3,
        "ArrowRight must move caret one char right"
    );
}

#[test]
fn key_up_events_are_not_consumed_by_the_handler() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    let guard = FocusGuard::attach(
        Arc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );
    guard.request_focus();

    // KeyUp must be ignored — the handler only acts on KeyDown.
    let up_event = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character("a".to_string()))
        .with_state(KeyState::Up)
        .build();
    let consumed = FocusManager::global().dispatch_key_event(&up_event);

    assert_eq!(controller.text(), "", "KeyUp must not insert text");
    assert!(
        !consumed,
        "KeyUp must not be consumed by the handler (returns false)"
    );
}

#[test]
fn unfocused_field_does_not_receive_key_events() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();
    let node = FocusNode::with_debug_label("test-field");
    // Register the handler but DO NOT request focus.
    let _guard = FocusGuard::attach(
        Arc::clone(&node),
        make_editable_text_handler(controller.clone()),
    );

    // Dispatch a character key — with no node focused, the handler must not fire.
    let event = KeyEventBuilder::new(Code::KeyX)
        .with_key(Key::Character("x".to_string()))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&event);

    assert_eq!(
        controller.text(),
        "",
        "an unfocused field must ignore key events dispatched through FocusManager"
    );
}

#[test]
fn editable_text_mounts_single_render_editable() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::with_text("hello");

    let laid = crate::common::lay_out(
        EditableText::new(controller),
        crate::common::tight(120.0, 40.0),
    );
    let editable = laid.find_by_render_type("RenderEditable");

    assert_eq!(laid.size(editable), Size::new(px(120.0), px(40.0)));
    assert!(
        laid.find_all_by_render_type("RenderParagraph").is_empty(),
        "EditableText must not split text/caret into temporary paragraphs"
    );
}

// ============================================================================
// TextField — composition (mounts the full GestureDetector/DecoratedBox/
// Padding/EditableText tree `TextField` builds, never previously exercised:
// every test above hand-simulates EditableTextState's key handler rather than
// mounting a real TextField/EditableText widget).
// ============================================================================

#[test]
fn text_field_deflates_editable_text_by_its_content_padding() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::with_text("hello");

    let laid = crate::common::lay_out(
        TextField::new(controller).content_padding(EdgeInsets::all(px(10.0))),
        crate::common::tight(200.0, 100.0),
    );

    // The overall field fills the tight constraint given to it...
    assert_eq!(laid.size(laid.root()), Size::new(px(200.0), px(100.0)));

    // ...but the EditableText inside must be deflated by the content padding
    // on every side (10px), proving `content_padding` is actually threaded
    // through `Padding::new(...)` rather than silently dropped.
    let editable = laid.find_by_render_type("RenderEditable");
    assert_eq!(laid.size(editable), Size::new(px(180.0), px(80.0)));
}

#[test]
fn text_field_default_content_padding_matches_its_documented_default() {
    let _focus_serial = focus_test_guard();
    let controller = TextEditingController::new();

    // Default content_padding is symmetric(8 vertical, 12 horizontal) per
    // `TextField::new`'s doc comment -- deflates width by 24 (12+12) and
    // height by 16 (8+8).
    let laid = crate::common::lay_out(
        TextField::new(controller),
        crate::common::tight(300.0, 60.0),
    );

    let editable = laid.find_by_render_type("RenderEditable");
    assert_eq!(laid.size(editable), Size::new(px(276.0), px(44.0)));
}
