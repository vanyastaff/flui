//! `flui_material::TextField` widget-level integration coverage — mounts a
//! real `TextField` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/ink_well.rs`/`tests/input_decorator.rs` use) and
//! drives it through real tap dispatch, real `FocusManager` key routing, and
//! real controller mutation, asserting on the composed `InputDecorator`'s
//! and `RenderEditable`'s mounted, resolved state.
//!
//! # `FocusManager` is a process-wide singleton
//!
//! Every test that touches focus takes [`focus_test_guard`], which serializes
//! on a private [`Mutex`] and clears `FocusManager::global()`'s primary
//! focus first — the same pattern `flui-widgets/tests/text_field.rs` uses,
//! so tests in this file cannot interleave with each other's focus state
//! even under a parallel test runner.
//!
//! # What's proven here that wasn't proven anywhere else
//!
//! `flui-widgets/tests/text_field.rs` already proved that a live
//! `FocusManager` focus change reaches a mounted `RenderEditable` (its own
//! `show_caret` flag) through a headless `tick()` — the finding that
//! de-risked this file's approach; see that test's doc comment. This file
//! proves the *next* layer: that `TextField`'s own `FocusManager` listener
//! (registered against the field's own published node, not
//! `EditableTextState`'s internal one) reaches its composed
//! `InputDecorator`, and that its controller listener reaches the
//! decorator's `is_empty`-driven hint visibility — neither of which
//! `EditableTextState`'s own plumbing would produce on its own, since
//! `InputDecorator`'s `focused`/`is_empty` are `TextField`-level build
//! inputs, not something `EditableText` itself exposes upward.

#![allow(clippy::unwrap_used)] // a panic IS the failure report in test code (docs/PANIC-POLICY.md)

mod common;

use std::sync::{Mutex, MutexGuard};

use common::{lay_out, tight};
use flui_interaction::events::{Code, Key, KeyState};
use flui_interaction::routing::FocusManager;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_material::{InputDecoration, TextField, Theme, ThemeData};
use flui_widgets::TextEditingController;

// ============================================================================
// Focus-test serialization
// ============================================================================

static FOCUS_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serialize on the process-wide `FocusManager` singleton and start every
/// test from a clean, unfocused state — mirrors
/// `flui-widgets/tests/text_field.rs`'s own `focus_test_guard`.
fn focus_test_guard() -> MutexGuard<'static, ()> {
    let guard = FOCUS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    FocusManager::global().unfocus();
    guard
}

/// Dispatch a single printable-character `KeyDown` event through
/// `FocusManager::global()` — routes to whichever node currently holds
/// primary focus, the same path a real keyboard event takes.
fn type_char(ch: char) {
    let event = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build();
    FocusManager::global().dispatch_key_event(&event);
}

// ============================================================================
// Focus round-trip — a REAL tap, not a direct `request_focus` call
// ============================================================================

/// Tapping anywhere in the decorated area focuses the field, which reaches
/// the composed `InputDecorator` (the active-indicator color/width flips to
/// the focused branch) — and unfocusing reverts it. Exercises the full
/// production path: `GestureDetector::on_tap` → `focus_field` →
/// `FocusManager::request_focus` → `TextFieldState`'s own focus listener →
/// `rebuild_handle().schedule()` → a headless `tick()`.
///
/// Mutation red-check: delete `TextFieldState::init_state`'s focus-listener
/// registration (or its `rebuild.schedule()` call) — `tick()` then drains
/// nothing, the decorator keeps rendering its first build's `focused: false`
/// resolution, and the "after tap" assertion below fails.
#[test]
fn tapping_the_decorated_area_focuses_the_field_and_reaches_the_decorator() {
    let _focus_serial = focus_test_guard();
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();

    let mut laid = lay_out(
        Theme::new(theme, TextField::new(controller.clone())),
        tight(300.0, 100.0),
    );
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");

    let unfocused = laid.render_property(decorated_box, "decoration").unwrap();
    assert!(
        unfocused.contains(&format!("{:?}", colors.on_surface_variant)),
        "an unfocused, untapped field must render the plain M3 indicator color, got: {unfocused}"
    );

    // A real down+up inside the decorated area — not a direct
    // `FocusManager::request_focus` call.
    laid.dispatch_pointer_down(150.0, 50.0);
    laid.dispatch_pointer_up(150.0, 50.0);
    laid.tick();

    let focused = laid.render_property(decorated_box, "decoration").unwrap();
    assert!(
        focused.contains(&format!("{:?}", colors.primary)),
        "tapping the decorated area must focus the field and reach the decorator's focused \
         indicator color, got: {focused}"
    );

    // Round trip: unfocusing must revert it.
    FocusManager::global().unfocus();
    laid.tick();

    let reverted = laid.render_property(decorated_box, "decoration").unwrap();
    assert!(
        reverted.contains(&format!("{:?}", colors.on_surface_variant)),
        "unfocusing must revert the decorator to the plain indicator color, got: {reverted}"
    );
}

// ============================================================================
// enabled: both sinks agree
// ============================================================================

/// `TextField::enabled(false)` disables both sinks from a single field —
/// `EditableText` withholds its focus node (a tap cannot focus it) AND the
/// decorator renders the M3 disabled indicator color — even though the
/// `InputDecoration` passed in never set `enabled` itself, proving
/// `TextField::enabled` is the one source of truth (see the module docs on
/// `flui_material::text_field`'s "Enabled" section), not a value that must
/// be set twice to agree.
#[test]
fn disabling_the_text_field_disables_both_editable_text_and_the_decorator() {
    let _focus_serial = focus_test_guard();
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();

    let laid = lay_out(
        Theme::new(theme, TextField::new(controller.clone()).enabled(false)),
        tight(300.0, 100.0),
    );

    // Sink 1: EditableText withholds its focus node.
    assert_eq!(
        controller.focus_node_id(),
        None,
        "a disabled TextField's EditableText must not publish a focus node"
    );

    // Sink 2: the decorator renders the disabled M3 indicator color.
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");
    let decoration_debug = laid.render_property(decorated_box, "decoration").unwrap();
    let disabled_indicator = colors.on_surface.with_opacity(0.38);
    assert!(
        decoration_debug.contains(&format!("{disabled_indicator:?}")),
        "a disabled TextField's decorator must render the M3 disabled indicator color, got: \
         {decoration_debug}"
    );

    // A tap cannot focus a disabled field — `focus_field` reads
    // `controller.focus_node_id()`, which is `None`.
    laid.dispatch_pointer_down(150.0, 50.0);
    laid.dispatch_pointer_up(150.0, 50.0);
    assert_eq!(
        FocusManager::global().primary_focus(),
        None,
        "tapping a disabled TextField must not focus anything"
    );
}

// ============================================================================
// Live plumbing — typing reaches the decorator's hint visibility
// ============================================================================

/// Typing a character while focused clears the hint row — proving
/// `TextFieldState`'s own controller listener (not
/// `EditableTextState`'s independent one, which only drives the rendered
/// text/caret) reaches the decorator's `is_empty` build input.
///
/// The focus transition itself is resolved (via its own `tick()`) *before*
/// typing, specifically so the typing `tick()` has no other pending rebuild
/// to ride on — isolating the controller listener as the only thing that
/// could still be dirtying `TextField`'s element at that point. Skipping
/// this and ticking once after both focusing and typing would let a focus-
/// triggered rebuild read the by-then-already-mutated text and pass for the
/// wrong reason, even with the controller listener deleted.
///
/// Mutation red-check: delete `TextFieldState::init_state`'s controller
/// listener registration — the decorator's `is_empty` stays pinned at its
/// last-rebuilt value (`true`), the hint row never disappears, and the
/// final paragraph-count assertion below fails (verified directly: the
/// assertion trips with the registration removed and passes with it
/// restored).
#[test]
fn typing_while_focused_clears_the_hint_row_in_the_decorator() {
    let _focus_serial = focus_test_guard();
    let theme = ThemeData::light();
    let controller = TextEditingController::new();
    let decoration = InputDecoration {
        hint_text: Some("you@example.com".to_string()),
        ..Default::default()
    };

    let mut laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone()).decoration(decoration),
        ),
        tight(300.0, 100.0),
    );

    // Empty field, no label: the hint renders as its own paragraph, plus the
    // `EditableText` interior's own (empty) paragraph slot.
    let hint_visible = laid.find_all_by_render_type("RenderParagraph").len();
    assert_eq!(hint_visible, 1, "an empty field with a hint must render it");

    // Resolve the focus transition (and whatever it dirties) on its own
    // tick, before any typing happens.
    let node_id = controller
        .focus_node_id()
        .expect("EditableText publishes its focus node on mount");
    FocusManager::global().request_focus(node_id);
    laid.tick();
    assert_eq!(
        laid.find_all_by_render_type("RenderParagraph").len(),
        1,
        "focusing an empty field must not, by itself, hide the hint"
    );

    // Now type, on a fresh tick with no other pending rebuild to ride on.
    type_char('h');
    laid.tick();

    assert_eq!(
        controller.text(),
        "h",
        "the keystroke must reach the controller"
    );
    let hint_after_typing = laid.find_all_by_render_type("RenderParagraph").len();
    assert_eq!(
        hint_after_typing, 0,
        "typing must clear is_empty and hide the hint row in the mounted decorator"
    );
}

// ============================================================================
// Caret color — error vs. primary
// ============================================================================

/// Flutter parity: the oracle's `cursorColor = _hasError ? _errorColor :
/// ... theme.colorScheme.primary` (`text_field.dart:1637-1641`, tag
/// `3.44.0`). `InputDecoration::error_text` presence is the only input —
/// no focus, no typing required.
#[test]
fn caret_color_is_error_colored_with_error_text_and_primary_otherwise() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;

    let with_error = lay_out(
        Theme::new(
            theme,
            TextField::new(TextEditingController::new()).decoration(InputDecoration {
                error_text: Some("Required".to_string()),
                ..Default::default()
            }),
        ),
        tight(300.0, 100.0),
    );
    let editable = with_error.find_by_render_type("RenderEditable").unwrap();
    let error_caret = with_error.render_property(editable, "caret_color").unwrap();
    assert!(
        error_caret.contains(&format!("{:?}", colors.error)),
        "an errored field's caret must be error-colored, got: {error_caret}"
    );

    let theme = ThemeData::light();
    let without_error = lay_out(
        Theme::new(theme, TextField::new(TextEditingController::new())),
        tight(300.0, 100.0),
    );
    let editable = without_error.find_by_render_type("RenderEditable").unwrap();
    let plain_caret = without_error
        .render_property(editable, "caret_color")
        .unwrap();
    assert!(
        plain_caret.contains(&format!("{:?}", colors.primary)),
        "a field with no error must have a primary-colored caret, got: {plain_caret}"
    );
}

// ============================================================================
// Hover — delegated entirely to the decorator, not double-tracked
// ============================================================================

/// `TextField` composes exactly ONE `MouseRegion` — the `InputDecorator`'s
/// own self-tracked hover — proving `TextField` did not add a second, outer
/// `MouseRegion` of its own (see `text_field.rs`'s module docs, "Hover").
#[test]
fn text_field_does_not_double_track_hover_with_its_own_mouse_region() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(TextEditingController::new()),
        ),
        tight(300.0, 100.0),
    );

    let mouse_regions = laid.find_all_by_render_type("RenderMouseRegion");
    assert_eq!(
        mouse_regions.len(),
        1,
        "TextField must delegate hover entirely to InputDecorator's own MouseRegion, not add a \
         second one, found {mouse_regions:?}"
    );
}

// ============================================================================
// Decoration passthrough
// ============================================================================

/// Label, hint, and helper all reach the mounted tree through `TextField`'s
/// `decoration` builder — passthrough proof, not a re-test of the
/// decorator's own row-selection logic (already pinned in
/// `tests/input_decorator.rs`).
#[test]
fn label_hint_and_helper_flow_through_the_decoration_builder() {
    let decoration = InputDecoration {
        label_text: Some("Email".to_string()),
        // No hint: with a label present and the field non-empty, the hint
        // would be suppressed anyway (`should_show_hint`) — omitted here so
        // this test counts only rows this decoration actually contributes.
        helper_text: Some("We'll never share it".to_string()),
        ..Default::default()
    };
    let controller = TextEditingController::with_text("a@b.com");

    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(controller).decoration(decoration),
        ),
        tight(300.0, 150.0),
    );

    // Label (floats: non-empty) + helper = 2 text rows, alongside the
    // EditableText interior (its own RenderEditable, not a RenderParagraph).
    let text_rows = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_rows.len(),
        2,
        "expected label + helper rows, found {text_rows:?}"
    );
    laid.find_by_render_type("RenderEditable")
        .expect("the EditableText interior must still be composed");
}

// ============================================================================
// Parity anchor — "TextField errorText trumps helperText" (text_field_test.dart, tag 3.44.0)
// ============================================================================

/// Named after the oracle's own `text_field_test.dart` test
/// (`'TextField errorText trumps helperText'`, tag `3.44.0`) — asserted
/// through `TextField`, not directly on `InputDecorator` (already covered
/// there by `error_replaces_helper_at_the_mounted_level`).
#[test]
fn text_field_error_text_trumps_helper_text() {
    let decoration = InputDecoration {
        helper_text: Some("Helper".to_string()),
        error_text: Some("Error".to_string()),
        ..Default::default()
    };

    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(TextEditingController::new()).decoration(decoration),
        ),
        tight(300.0, 150.0),
    );

    // No label/hint set (empty controller, no hint_text — see
    // `should_show_hint`, false without `hint_text`), so the only text row
    // possible is the helper-or-error line.
    let text_rows = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_rows.len(),
        1,
        "error must replace helper, not render alongside it, found {text_rows:?}"
    );
}
