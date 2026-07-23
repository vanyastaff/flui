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

#![allow(clippy::unwrap_used)] // a panic IS the failure report in test code (docs/PANIC-POLICY.md)

mod common;

use std::rc::Rc;

use common::{lay_out, tight};
use flui_interaction::events::{Code, Key, KeyState};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_interaction::{FocusManager, FocusNode};
use flui_material::{InputDecoration, TextField, Theme, ThemeData};
use flui_widgets::TextEditingController;

/// Dispatch a single printable-character `KeyDown` event through
/// this harness's manager — the same path a real keyboard event takes.
fn type_char(manager: &FocusManager, ch: char) {
    let event = KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build();
    manager.dispatch_key_event(&event);
}

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

/// `TextField::enabled(false)` disables both sinks from a single field —
/// `EditableText` marks its exact node ineligible (a tap cannot focus it) AND the
/// decorator renders the M3 disabled indicator color — even though the
/// `InputDecoration` passed in never set `enabled` itself, proving
/// `TextField::enabled` is the one source of truth (see the module docs on
/// `flui_material::text_field`'s "Enabled" section), not a value that must
/// be set twice to agree.
#[test]
fn disabling_the_text_field_disables_both_editable_text_and_the_decorator() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("disabled-field");

    let laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone())
                .focus_node(Rc::clone(&focus_node))
                .enabled(false),
        ),
        tight(300.0, 100.0),
    );

    // Sink 1: the exact node stays structurally attached but cannot acquire
    // focus. Identity and eligibility are separate concerns.
    assert!(focus_node.is_attached());
    assert!(!focus_node.can_request_focus());

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

    // A tap cannot focus a disabled field.
    laid.dispatch_pointer_down(150.0, 50.0);
    laid.dispatch_pointer_up(150.0, 50.0);
    assert!(!focus_node.has_primary_focus());
    assert!(laid.focus_manager().primary_focus().is_none());
}

// ============================================================================
// enabled resolution chain — decoration-only value respected, override wins
// ============================================================================

/// `InputDecoration::enabled = false` alone (no `TextField::enabled` call at
/// all — `None`) disables the field, proving the resolution chain's second
/// link (`enabled.unwrap_or(decoration.enabled)`) is actually reachable, not
/// just the `Some` branch the sibling test above exercises.
#[test]
fn decoration_only_enabled_false_is_respected_without_a_text_field_level_override() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("decoration-disabled");
    let decoration = InputDecoration {
        enabled: false,
        ..Default::default()
    };

    let laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone())
                .focus_node(Rc::clone(&focus_node))
                .decoration(decoration),
        ),
        tight(300.0, 100.0),
    );

    assert!(!focus_node.can_request_focus());
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");
    let decoration_debug = laid.render_property(decorated_box, "decoration").unwrap();
    let disabled_indicator = colors.on_surface.with_opacity(0.38);
    assert!(
        decoration_debug.contains(&format!("{disabled_indicator:?}")),
        "decoration-only enabled=false must reach the decorator's disabled indicator, got: \
         {decoration_debug}"
    );
}

/// `TextField::enabled(true)` overrides a conflicting
/// `InputDecoration::enabled = false` — the resolution chain's first link
/// (`Some` wins outright) beats the second, matching the oracle's
/// `widget.enabled ?? decoration?.enabled ?? true` precedence
/// (`text_field.dart:1183`, tag `3.44.0`).
///
/// Mutation red-check: swap the `unwrap_or` argument order (i.e. resolve
/// `decoration.enabled` before `view.enabled`) — this field would render
/// disabled despite the explicit `enabled(true)` override, and the first
/// assertion below fails.
#[test]
fn text_field_enabled_override_wins_over_a_conflicting_decoration_enabled() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("enabled-override");
    let decoration = InputDecoration {
        enabled: false,
        ..Default::default()
    };

    let laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone())
                .focus_node(Rc::clone(&focus_node))
                .decoration(decoration)
                .enabled(true),
        ),
        tight(300.0, 100.0),
    );

    assert!(focus_node.can_request_focus());
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");
    let decoration_debug = laid.render_property(decorated_box, "decoration").unwrap();
    let disabled_indicator = colors.on_surface.with_opacity(0.38);
    assert!(
        !decoration_debug.contains(&format!("{disabled_indicator:?}")),
        "the override must actually win, not merely coexist with the disabled default, got: \
         {decoration_debug}"
    );
}

// ============================================================================
// Unmount — the exact node listener must not leak
// ============================================================================

/// Unmounting a `TextField` removes the listener installed directly on its
/// exact node. This also proves the external node is detached from the
/// presentation rather than left reachable through a stale owner.
///
/// The `TextField` is a `Column` child here, not the mounted root — removing
/// it from the children list goes through ordinary list reconciliation
/// (proven elsewhere, e.g. `flui-widgets/src/text/text_field.rs`'s own
/// `a_tap_focuses_the_fields_own_node_not_the_first_registered`, to
/// correctly dispose a removed child), unlike swapping the ROOT widget's
/// type outright via `pump_widget`/`swap_root_view`, which this harness
/// documents as a same-type configuration replacement, not a full
/// deactivate-and-remount.
///
/// Mutation red-check: delete `MaterialTextFieldState::dispose`'s
/// `FocusNode::remove_listener(id)` call — the count after
/// removal no longer matches the pre-mount baseline and the final assertion
/// fails.
#[test]
fn unmounting_removes_the_listener_from_the_exact_focus_node() {
    use flui_view::{IntoView, ViewExt};
    use flui_widgets::Column;

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("unmount-listener");

    let before_mount = focus_node.listener_count();
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Column::new(vec![
                TextField::new(controller)
                    .focus_node(Rc::clone(&focus_node))
                    .into_view()
                    .boxed(),
            ]),
        ),
        tight(300.0, 100.0),
    );
    let while_mounted = focus_node.listener_count();
    assert!(
        while_mounted > before_mount,
        "mounting a TextField must register its own node listener"
    );
    assert!(focus_node.is_attached());

    // Remove the TextField from the Column's children — an ordinary child
    // removal, not a root-type swap.
    laid.pump_widget(Theme::new(
        ThemeData::light(),
        Column::new(Vec::<flui_view::BoxedView>::new()),
    ));

    let after_removal = focus_node.listener_count();
    assert_eq!(
        after_removal, before_mount,
        "removing a TextField must remove its exact-node listener, not leak it"
    );
    assert!(!focus_node.is_attached());
}

// ============================================================================
// Disable-while-focused: focus is lost, and re-enabling does not restore it
// ============================================================================

/// Disabling a focused field unfocuses it (`EditableTextState::did_update_view`)
/// — and re-enabling afterward renders the UNFOCUSED indicator, not a stale
/// focused one, matching Flutter: a field that loses focus while disabled
/// does not regain it merely by becoming enabled again.
///
#[test]
fn disabling_a_focused_field_then_re_enabling_renders_the_unfocused_indicator() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("disable-round-trip");

    let mut laid = lay_out(
        Theme::new(
            theme.clone(),
            TextField::new(controller.clone()).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );

    // Focus it via a real tap.
    laid.dispatch_pointer_down(150.0, 50.0);
    laid.dispatch_pointer_up(150.0, 50.0);
    laid.tick();
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");
    let focused_decoration = laid.render_property(decorated_box, "decoration").unwrap();
    assert!(
        focused_decoration.contains(&format!("{:?}", colors.primary)),
        "sanity: the field must actually be focused before disabling it, got: {focused_decoration}"
    );

    // Disable while focused.
    laid.pump_widget(Theme::new(
        theme.clone(),
        TextField::new(controller.clone())
            .focus_node(Rc::clone(&focus_node))
            .enabled(false),
    ));
    assert!(!focus_node.has_primary_focus());
    assert!(laid.focus_manager().primary_focus().is_none());

    // Re-enable: focus must NOT be restored automatically.
    laid.pump_widget(Theme::new(
        theme,
        TextField::new(controller).focus_node(Rc::clone(&focus_node)),
    ));
    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");
    let reenabled_decoration = laid.render_property(decorated_box, "decoration").unwrap();
    assert!(
        reenabled_decoration.contains(&format!("{:?}", colors.on_surface_variant)),
        "re-enabling a field that lost focus while disabled must render the UNFOCUSED \
         indicator, got: {reenabled_decoration}"
    );
    assert!(
        !reenabled_decoration.contains(&format!("{:?}", colors.primary)),
        "the re-enabled field must not still show the focused indicator, got: \
         {reenabled_decoration}"
    );
}

/// Disabling a focused field notifies listeners on the exact retained node.
/// No controller metadata or manager-wide ID comparison participates in the
/// transition.
#[test]
fn disabling_a_focused_field_notifies_the_exact_node() {
    use std::cell::RefCell;

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("disable-notify");

    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(controller.clone()).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );
    focus_node.request_focus();
    laid.tick();
    assert!(focus_node.has_primary_focus());

    let observations = Rc::new(RefCell::new(Vec::new()));
    let observations_for_spy = Rc::clone(&observations);
    let weak_node = Rc::downgrade(&focus_node);
    let spy_id = focus_node.add_listener(Rc::new(move || {
        if let Some(node) = weak_node.upgrade() {
            observations_for_spy
                .borrow_mut()
                .push((node.has_primary_focus(), node.can_request_focus()));
        }
    }));

    laid.pump_widget(Theme::new(
        ThemeData::light(),
        TextField::new(controller)
            .focus_node(Rc::clone(&focus_node))
            .enabled(false),
    ));

    focus_node.remove_listener(spy_id);

    assert!(
        observations
            .borrow()
            .iter()
            .any(|&(has_primary_focus, can_request_focus)| {
                !has_primary_focus && !can_request_focus
            }),
        "the retained node must notify after it loses focus and becomes ineligible"
    );
}

// ============================================================================
// Whole-area tap — a point clearly outside EditableText's own rect
// ============================================================================

/// A tap inside the decorator's default content padding — outside
/// `EditableText`'s own padded content rect entirely — still focuses the
/// field, proving the tap target really is the whole decorated box, not
/// just wherever the inner text happens to sit (a center-tap, as the other
/// focus tests use, would likely land inside `EditableText`'s own rect and
/// couldn't tell the two apart).
#[test]
fn tapping_the_padding_margin_outside_the_text_rect_also_focuses_the_field() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("padding-tap");

    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(controller).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );

    // (3, 3) sits inside the decorator's default content padding (8px top /
    // 12px left, `default_content_padding`) — well outside EditableText's
    // own padded content rect, but still within the decorator's own
    // fill/border/`MouseRegion`, which spans the FULL box.
    laid.dispatch_pointer_down(3.0, 3.0);
    laid.dispatch_pointer_up(3.0, 3.0);

    assert!(
        focus_node.has_primary_focus(),
        "a tap inside the padding margin, outside the inner text rect, must still focus the field"
    );
}

// ============================================================================
// Live plumbing — typing reaches the decorator's hint visibility
// ============================================================================

/// Typing a character while focused clears the hint row — proving
/// `MaterialTextFieldState`'s own controller listener (not
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
/// Mutation red-check: delete `MaterialTextFieldState::init_state`'s controller
/// listener registration — the decorator's `is_empty` stays pinned at its
/// last-rebuilt value (`true`), the hint row never disappears, and the
/// final paragraph-count assertion below fails (verified directly: the
/// assertion trips with the registration removed and passes with it
/// restored).
#[test]
fn typing_while_focused_clears_the_hint_row_in_the_decorator() {
    let theme = ThemeData::light();
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("typing");
    let decoration = InputDecoration {
        hint_text: Some("you@example.com".to_string()),
        ..Default::default()
    };

    let mut laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone())
                .focus_node(Rc::clone(&focus_node))
                .decoration(decoration),
        ),
        tight(300.0, 100.0),
    );

    // Empty field, no label: the hint renders as its own paragraph, plus the
    // `EditableText` interior's own (empty) paragraph slot.
    let hint_visible = laid.find_all_by_render_type("RenderParagraph").len();
    assert_eq!(hint_visible, 1, "an empty field with a hint must render it");

    // Resolve the focus transition (and whatever it dirties) on its own
    // tick, before any typing happens.
    focus_node.request_focus();
    laid.tick();
    assert_eq!(
        laid.find_all_by_render_type("RenderParagraph").len(),
        1,
        "focusing an empty field must not, by itself, hide the hint"
    );

    // Now type, on a fresh tick with no other pending rebuild to ride on.
    type_char(&laid.focus_manager(), 'h');
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
