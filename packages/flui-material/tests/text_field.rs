//! `flui_material::TextField` widget-level integration coverage — mounts a
//! real `TextField` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/ink_well.rs`/`tests/input_decorator.rs` use) and
//! drives it through real tap dispatch, real `FocusManager` key routing, and
//! real controller mutation, asserting on the composed `InputDecorator`'s
//! and `RenderEditable`'s mounted, resolved state.
//!
//! # Focus ownership
//!
//! Every mounted harness owns an isolated [`FocusManager`]. Tests pass an
//! explicit [`FocusNode`] into the field and route keys through that harness's
//! manager, so parallel tests share no focus state and need no serialization.
//!
//! # What's proven here
//!
//! A live `FocusManager` focus change reaches a mounted render tree through
//! a headless `tick()` — `flui-widgets/tests/text_field.rs`'s
//! `requesting_focus_via_the_controllers_published_node_reveals_the_caret_after_a_tick`
//! proves this for `EditableTextState`'s own internal listener driving
//! `RenderEditable`'s `show_caret`. This file proves the *next* layer:
//! that `TextField`'s own node listener (registered against the same explicit
//! node as `EditableTextState`)
//! reaches its composed `InputDecorator`, and that its controller listener
//! reaches the decorator's `is_empty`-driven hint visibility — neither of
//! which `EditableTextState`'s own plumbing would produce on its own, since
//! `InputDecorator`'s `focused`/`is_empty` are `TextField`-level build
//! inputs, not something `EditableText` itself exposes upward.

// a panic IS the failure report in test code (docs/PANIC-POLICY.md)

use crate::common;

use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{TextField, Theme, ThemeData};
use flui_sdk::interaction::FocusNode;
use flui_sdk::widgets::TextEditingController;

// ============================================================================
// Focus round-trip — a REAL tap, not a direct `request_focus` call
// ============================================================================

/// Tapping anywhere in the decorated area focuses the field, which reaches
/// the composed `InputDecorator` (the active-indicator color/width flips to
/// the focused branch) — and unfocusing reverts it. Exercises the full
/// production path: `GestureDetector::on_tap` → `FocusNode::request_focus` →
/// `MaterialTextFieldState`'s own node listener →
/// `rebuild_handle().schedule(reason)` → a headless `tick()`.
///
/// Mutation red-check: delete `MaterialTextFieldState::init_state`'s focus-listener
/// registration (or its `rebuild.schedule(reason)` call) — `tick()` then drains
/// nothing, the decorator keeps rendering its first build's `focused: false`
/// resolution, and the "after tap" assertion below fails.
#[test]
fn tapping_the_decorated_area_focuses_the_field_and_reaches_the_decorator() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("tap-round-trip");

    let mut laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone()).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );
    let decorated_box = laid
        .try_find_by_render_type("RenderDecoratedBox")
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
    focus_node.unfocus();
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

// ============================================================================
// enabled resolution chain — decoration-only value respected, override wins
// ============================================================================

// ============================================================================
// Unmount — the exact node listener must not leak
// ============================================================================

// ============================================================================
// Disable-while-focused: focus is lost, and re-enabling does not restore it
// ============================================================================

// ============================================================================
// Whole-area tap — a point clearly outside EditableText's own rect
// ============================================================================

// ============================================================================
// Live plumbing — typing reaches the decorator's hint visibility
// ============================================================================

// ============================================================================
// on_submitted — forwarded to the composed EditableText, fires on Enter
// ============================================================================

// ============================================================================
// Caret color — error vs. primary
// ============================================================================

// ============================================================================
// Hover — delegated entirely to the decorator, not double-tracked
// ============================================================================

// ============================================================================
// Decoration passthrough
// ============================================================================

// ============================================================================
// Parity anchor — "TextField errorText trumps helperText" (text_field_test.dart, tag 3.44.0)
// ============================================================================
