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
use flui_material::{InputDecoration, TextField, Theme, ThemeData};
use flui_sdk::interaction::FocusNode;
use flui_sdk::view::ViewExt;
use flui_sdk::widgets::{SizedBox, TextEditingController};

// ============================================================================
// Focus round-trip — a REAL tap, not a direct `request_focus` call
// ============================================================================

/// Tapping anywhere in the decorated area focuses the field, which reaches
/// the composed `InputDecorator` (the active-indicator color/width flips to
/// the focused branch) — and unfocusing reverts it. Exercises the full
/// production path: the press's `Listener` → `FocusNode::request_focus` →
/// `MaterialTextFieldState`'s own node listener →
/// `rebuild_handle().schedule(reason)` → a headless `tick()`.
///
/// Mutation red-check: delete `MaterialTextFieldState::init_state`'s focus-listener
/// registration (or its `rebuild.schedule(reason)` call) — `tick()` then drains
/// nothing, the decorator keeps rendering its first build's `focused: false`
/// resolution, and the "after tap" assertion below fails.
pub fn tapping_the_decorated_area_focuses_the_field_and_reaches_the_decorator() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("tap-round-trip");

    let mut laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );
    let decorated_box = laid
        .try_find_by_render_type("RenderDecoratedBox")
        .expect("TextField must compose an InputDecorator's DecoratedBox");

    let unfocused = laid
        .render_property(decorated_box, "decoration")
        .expect("decoration property");
    assert!(
        unfocused.contains(&format!("{:?}", colors.on_surface_variant)),
        "an unfocused, untapped field must render the plain M3 indicator color, got: {unfocused}"
    );

    // A real down+up inside the decorated area — not a direct
    // `FocusManager::request_focus` call. The container keeps its content
    // height inside the tight parent, so the tap aims at its own centre.
    let origin = laid.absolute_offset(decorated_box);
    let size = laid.size(decorated_box);
    let (x, y) = (origin.dx + size.width / 2.0, origin.dy + size.height / 2.0);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);
    laid.tick();

    let focused = laid
        .render_property(decorated_box, "decoration")
        .expect("decoration property");
    assert!(
        focused.contains(&format!("{:?}", colors.primary)),
        "tapping the decorated area must focus the field and reach the decorator's focused \
         indicator color, got: {focused}"
    );

    // Round trip: unfocusing must revert it, and the press's tap, which the
    // field's double-tap holds for the double-tap time, takes no focus back.
    focus_node.unfocus();
    for _ in 0..40 {
        laid.pump_for(std::time::Duration::from_millis(16));
    }
    assert!(
        !focus_node.has_focus(),
        "a focus moved on after the press stays moved"
    );

    let reverted = laid
        .render_property(decorated_box, "decoration")
        .expect("decoration property");
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

pub fn replacing_and_unmounting_the_field_withdraws_its_node_subscription() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::new();
    let previous = FocusNode::with_debug_label("previous field node");
    let replacement = FocusNode::with_debug_label("replacement field node");
    let mut laid = lay_out(
        Theme::new(
            theme.clone(),
            TextField::new(controller.clone()).focus_node(Rc::clone(&previous)),
        ),
        tight(300.0, 100.0),
    );

    laid.pump_widget(Theme::new(
        theme.clone(),
        TextField::new(controller).focus_node(Rc::clone(&replacement)),
    ));
    let _ = replacement.request_focus();
    laid.tick();
    let decorated_box = laid
        .try_find_by_render_type("RenderDecoratedBox")
        .expect("replacement field's decorated box");
    let focused = laid
        .render_property(decorated_box, "decoration")
        .expect("resolved decoration");
    assert!(
        focused.contains(&format!("{:?}", colors.primary)),
        "the effective replacement node must focus the mounted decorator: {focused}"
    );

    replacement.unfocus();
    laid.tick();
    let unfocused = laid
        .render_property(decorated_box, "decoration")
        .expect("resolved unfocused decoration");
    assert!(unfocused.contains(&format!("{:?}", colors.on_surface_variant)));
    // A retained external node may be reused elsewhere in the presentation.
    // Detached nodes suppress policy notifications, so reattach it before
    // checking that its former field subscription has been withdrawn.
    let reused_previous = laid
        .focus_manager()
        .root_scope()
        .attach_node(&previous)
        .expect("reuse the outgoing field node");
    assert_eq!(
        laid.with_build_owner_mut(|owner| owner.pending_external_builds()),
        0
    );
    let painted = laid.painted_frame_count();
    // This notifies the retained old node without requesting a root rebuild
    // or moving focus through another mounted widget.
    previous.set_skip_traversal(true);
    // Inspect accepted rebuild work before the frame drains it: a no-op
    // rebuild need not produce a new layer tree or change decoration.
    assert_eq!(
        laid.with_build_owner_mut(|owner| owner.pending_external_builds()),
        0,
        "the outgoing node must not enqueue work for the replacement field"
    );
    laid.tick();
    assert_eq!(
        laid.painted_frame_count(),
        painted,
        "the outgoing node must not schedule the replacement field"
    );
    assert_eq!(
        laid.render_property(decorated_box, "decoration")
            .expect("decoration after old-node notification"),
        unfocused
    );

    laid.pump_widget(Theme::new(theme, SizedBox::new(300.0, 100.0)));
    assert!(laid.try_find_by_render_type("RenderEditable").is_none());
    let reused_replacement = laid
        .focus_manager()
        .root_scope()
        .attach_node(&replacement)
        .expect("reuse the unmounted field node");
    let painted = laid.painted_frame_count();
    assert_eq!(
        laid.with_build_owner_mut(|owner| owner.pending_external_builds()),
        0
    );
    previous.set_skip_traversal(false);
    replacement.set_skip_traversal(true);
    assert_eq!(
        laid.with_build_owner_mut(|owner| owner.pending_external_builds()),
        0,
        "retained nodes must not enqueue work for a disposed field"
    );
    laid.tick();
    assert_eq!(
        laid.painted_frame_count(),
        painted,
        "retained nodes must not schedule a disposed field"
    );
    assert!(laid.try_find_by_render_type("RenderEditable").is_none());
    drop(reused_replacement);
    drop(reused_previous);
}

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

// ============================================================================
// Supporting line — below the indicator, outside the container
// ============================================================================

/// The error line sits below the active indicator, outside the decorated
/// container: its top is the M3 4dp gap below the container's bottom edge
/// (where the indicator is drawn), it starts where the content starts, the
/// field's content stays above the indicator, and a tap on the error line is
/// not a tap on the field.
///
/// Fails if the error line is laid out inside the container: its top is
/// then above the indicator, and the container's tap target covers it.
pub fn error_line_sits_below_the_indicator_outside_the_tap_target() {
    let focus_node = FocusNode::with_debug_label("error-line");
    let decoration = InputDecoration {
        error_text: Some("Enter a title".to_owned()),
        filled: true,
        ..InputDecoration::default()
    };

    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(TextEditingController::new())
                .decoration(decoration)
                .focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 150.0),
    );

    let container = laid.find_by_render_type("RenderDecoratedBox");
    let container_origin = laid.absolute_offset(container);
    let container_size = laid.size(container);
    // The indicator is the container's bottom border, so it ends at the
    // container's bottom edge; an unfocused error indicator is 1dp wide.
    let indicator_bottom = container_origin.dy + container_size.height;
    let indicator_top = indicator_bottom - 1.0;

    let content = laid.find_by_render_type("RenderEditable");
    let content_origin = laid.absolute_offset(content);
    let content_bottom = content_origin.dy + laid.size(content).height;
    assert!(
        content_bottom <= indicator_top,
        "the content must stay above the indicator: content bottom {content_bottom}, \
         indicator top {indicator_top}"
    );

    let error = laid
        .find_text("Enter a title")
        .expect("the error line renders");
    let error_origin = laid.absolute_offset(error);
    let error_size = laid.size(error);
    assert!(
        (error_origin.dy - (indicator_bottom + 4.0)).abs() < 1e-9,
        "the error line must start 4dp below the indicator: error top {}, indicator bottom \
         {indicator_bottom}",
        error_origin.dy
    );
    assert!(
        (error_origin.dx - content_origin.dx).abs() < 1e-9,
        "the error line must start where the content starts: error left {}, content left {}",
        error_origin.dx,
        content_origin.dx
    );

    // A tap on the error line does not focus the field; a tap on the
    // container does.
    let (error_x, error_y) = (
        error_origin.dx + error_size.width / 2.0,
        error_origin.dy + error_size.height / 2.0,
    );
    laid.dispatch_pointer_down(error_x, error_y);
    laid.dispatch_pointer_up(error_x, error_y);
    laid.tick();
    assert!(
        !focus_node.has_focus(),
        "a tap on the error line must not focus the field"
    );

    let (container_x, container_y) = (
        container_origin.dx + container_size.width / 2.0,
        container_origin.dy + container_size.height / 2.0,
    );
    laid.dispatch_pointer_down(container_x, container_y);
    laid.dispatch_pointer_up(container_x, container_y);
    laid.tick();
    assert!(
        focus_node.has_focus(),
        "a tap on the container must focus the field"
    );
}

/// A screen reader reaching the field hears its label as the field's name,
/// its hint as the placeholder, its error as an invalid state and that it is
/// required; the label
/// and hint are not announced a second time as loose text, and the error line
/// is a polite live region, read when it appears.
pub fn the_field_carries_its_label_hint_and_error_for_assistive_technology() {
    use flui_testing::a11y::{Live, Role};

    let decoration = InputDecoration {
        label_text: Some("Email".to_owned()),
        hint_text: Some("name@example.com".to_owned()),
        error_text: Some("Enter an email".to_owned()),
        ..InputDecoration::default()
    };
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            TextField::new(TextEditingController::new())
                .decoration(decoration)
                .required(true),
        ),
        tight(300.0, 150.0),
    );
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");

    let field = tree
        .find(Role::TextInput)
        .unwrap_or_else(|error| panic!("{error}\n{}", tree.describe()));
    assert_eq!(field.label(), Some("Email"), "{}", tree.describe());
    assert_eq!(
        field.raw().placeholder(),
        Some("name@example.com"),
        "{}",
        tree.describe()
    );
    assert!(field.raw().invalid().is_some(), "{}", tree.describe());
    assert!(field.raw().is_required(), "{}", tree.describe());
    assert_eq!(
        field.raw().description(),
        Some("Enter an email"),
        "the error describes the field\n{}",
        tree.describe()
    );
    assert_eq!(
        tree.find_all_by_label("Email").len(),
        1,
        "the label is announced once, as the field's name\n{}",
        tree.describe()
    );
    assert!(
        tree.find_all_by_label("name@example.com").is_empty(),
        "the hint is the placeholder, not loose text\n{}",
        tree.describe()
    );
    assert!(
        tree.nodes().any(|node| node.live() == Some(Live::Polite)),
        "the error line is a polite live region\n{}",
        tree.describe()
    );
}

// ============================================================================
// Input purpose — the field's text store reports what the field is for
// ============================================================================

/// `TextField::input_purpose` and `TextFormField::input_purpose` reach the
/// text store the input method reads, and a rebuild that changes it reaches
/// the store too.
///
/// Mutation red-check: drop `.input_purpose(view.input_purpose)` where
/// `TextField` builds its `EditableText` — the store keeps reporting `Text`.
pub fn the_input_purpose_reaches_the_text_store() {
    use flui_material::TextFormField;
    use flui_sdk::platform::TextInputPurpose;

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("purpose");
    let mut harness = flui_testing::widgets::harness::mount_with_ime(Theme::new(
        ThemeData::light(),
        SizedBox::new(300.0, 100.0).child(
            TextField::new(controller)
                .focus_node(Rc::clone(&focus_node))
                .input_purpose(TextInputPurpose::Email),
        ),
    ));
    let _ = focus_node.request_focus();
    harness.tick();
    let store = harness.active_text_store().expect("focused store");
    assert_eq!(store.status().purpose, TextInputPurpose::Email);

    let form_controller = TextEditingController::new();
    let form_focus = FocusNode::with_debug_label("form purpose");
    let mut form = flui_testing::widgets::harness::mount_with_ime(Theme::new(
        ThemeData::light(),
        SizedBox::new(300.0, 100.0).child(
            TextFormField::new(form_controller)
                .focus_node(Rc::clone(&form_focus))
                .input_purpose(TextInputPurpose::Phone),
        ),
    ));
    let _ = form_focus.request_focus();
    form.tick();
    let store = form.active_text_store().expect("focused form store");
    assert_eq!(store.status().purpose, TextInputPurpose::Phone);
}

/// `autofocus(true)` on a `TextField` and on a `TextFormField` reaches the
/// field's own node: the field holds the focus once mounted, and without it
/// nothing is focused.
pub fn an_autofocus_field_takes_the_focus_on_mount() {
    type Field = fn(Rc<FocusNode>, bool) -> flui_sdk::view::BoxedView;
    let cases: [(&str, Field); 2] = [
        ("TextField", |node, autofocus| {
            TextField::new(TextEditingController::new())
                .focus_node(node)
                .autofocus(autofocus)
                .boxed()
        }),
        ("TextFormField", |node, autofocus| {
            flui_material::TextFormField::new(TextEditingController::new())
                .focus_node(node)
                .autofocus(autofocus)
                .boxed()
        }),
    ];
    for (name, field) in cases {
        for autofocus in [true, false] {
            let node = FocusNode::with_debug_label(name);
            let mut laid = lay_out(
                Theme::new(ThemeData::light(), field(Rc::clone(&node), autofocus)),
                tight(300.0, 100.0),
            );
            laid.tick();
            assert_eq!(
                node.has_primary_focus(),
                autofocus,
                "{name} with autofocus({autofocus})"
            );
        }
    }
}

/// A field under a `Theme` highlights its selection in the theme's colours:
/// `primary` at 40 % while focused, `on_surface` at 16 % after a blur.
///
/// Mutation red-check: drop the `DefaultSelectionStyle` that `Theme::new`
/// wraps its child in; the field falls back to `SelectionStyle::default()`'s
/// blue and both assertions fail.
pub fn a_field_highlights_its_selection_in_the_theme_colours() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("themed selection");
    let mut laid = lay_out(
        Theme::new(
            theme,
            TextField::new(controller.clone()).focus_node(Rc::clone(&focus_node)),
        ),
        tight(300.0, 100.0),
    );
    let _ = focus_node.request_focus();
    controller.set_selection(0, 5);
    laid.tick();
    let editable = laid
        .try_find_by_render_type("RenderEditable")
        .expect("TextField composes an EditableText");
    assert_eq!(
        laid.render_property(editable, "selection_color"),
        Some(format!("{:?}", colors.primary.with_opacity(0.40))),
        "a focused field highlights with the theme's primary"
    );
    focus_node.unfocus();
    laid.tick();
    assert_eq!(
        laid.render_property(editable, "selection_color"),
        Some(format!("{:?}", colors.on_surface.with_opacity(0.16))),
        "a blurred field dims to the theme's on_surface"
    );
}
