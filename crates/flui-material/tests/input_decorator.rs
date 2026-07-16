//! `InputDecorator` widget-level integration coverage — mounts a real
//! `InputDecorator` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/card.rs`/`tests/ink_well.rs` use) and probes the
//! composed [`flui_widgets::DecoratedBox`] (`RenderDecoratedBox`),
//! [`flui_widgets::MouseRegion`] (`RenderMouseRegion`), and
//! [`flui_widgets::Text`] (`RenderParagraph`) render objects it produces.
//!
//! # Hover blend is not end-to-end drivable here
//!
//! `tests/ink_well.rs`'s own module doc already established this gap:
//! `MouseRegion::on_enter`/`on_exit` require `MouseTracker::update_with_event`,
//! which only a full `AppBinding` frame pump runs — the raw
//! `HitTestResult::dispatch` this headless harness's `dispatch_pointer_move`
//! calls never reaches it. `InputDecoratorState` uses `on_enter`/`on_exit`
//! (the oracle's own `TextField._handleHover` wiring, `text_field.dart:1799`,
//! tag `3.44.0`) for the same real-behavior reason `InkWell` does — the hover
//! blend math itself is exhaustively pinned by `input_decorator.rs`'s own
//! unit tests (`hover_blend_*`); this file only proves the `MouseRegion` is
//! actually composed around the container, structurally.

#![allow(clippy::unwrap_used)] // a panic IS the failure report in test code (docs/PANIC-POLICY.md)

mod common;

use common::{lay_out, tight};
use flui_material::{
    InputDecoration, InputDecorationThemeData, InputDecorator, Theme, ThemeData, ThemeDataOverrides,
};
use flui_types::Color;
use flui_types::geometry::px;
use flui_types::styling::{BorderSide, BorderStyle};
use flui_widgets::{SizedBox, WidgetStateProperty};

/// A small render-object child standing in for a real field's content (e.g.
/// a future `EditableText`) — `SizedBox` renders as `RenderConstrainedBox`,
/// distinct from the decorator's own `RenderDecoratedBox`/`RenderParagraph`
/// nodes, so it can't be confused with them in a render-type count.
fn child_stub() -> SizedBox {
    SizedBox::new(20.0, 20.0)
}

#[test]
fn mouse_region_wraps_the_composed_decoration() {
    let theme = ThemeData::light();
    let decoration = InputDecoration {
        filled: true,
        ..Default::default()
    };
    let laid = lay_out(
        Theme::new(theme, InputDecorator::new(decoration).child(child_stub())),
        tight(300.0, 100.0),
    );

    laid.find_by_render_type("RenderMouseRegion")
        .expect("InputDecorator must wrap its content in a MouseRegion for hover tracking");
    laid.find_by_render_type("RenderDecoratedBox")
        .expect("InputDecorator must compose a DecoratedBox for the fill/underline");
}

/// All four text rows in one mount: the label floats (focused, empty) so
/// both it AND the hint render, plus the child, plus the error line
/// (replacing the unset-but-would-be helper). Proves slot presence — every
/// documented row actually reaches the render tree.
#[test]
fn slot_presence_label_hint_child_and_error_all_render() {
    let theme = ThemeData::light();
    let decoration = InputDecoration {
        label_text: Some("Email".to_string()),
        hint_text: Some("you@example.com".to_string()),
        error_text: Some("Required".to_string()),
        filled: true,
        ..Default::default()
    };
    let laid = lay_out(
        Theme::new(
            theme,
            InputDecorator::new(decoration)
                .focused(true)
                .is_empty(true)
                .child(child_stub()),
        ),
        tight(300.0, 200.0),
    );

    // Label (floating) + hint (empty && floating) + error line = 3 text rows.
    let text_nodes = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_nodes.len(),
        3,
        "expected label + hint + error rows, found {text_nodes:?}"
    );
    laid.find_by_render_type("RenderConstrainedBox")
        .expect("the child content must still be composed");
}

/// Error replaces helper: with both set, exactly one helper/error text row
/// renders, not two.
#[test]
fn error_replaces_helper_at_the_mounted_level() {
    let theme = ThemeData::light();
    let decoration = InputDecoration {
        helper_text: Some("Helper".to_string()),
        error_text: Some("Error".to_string()),
        filled: true,
        ..Default::default()
    };
    // No label/hint set, so the only text row is the helper-or-error line.
    let laid = lay_out(
        Theme::new(theme, InputDecorator::new(decoration).child(child_stub())),
        tight(300.0, 150.0),
    );

    let text_nodes = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_nodes.len(),
        1,
        "error must replace helper, not render alongside it"
    );
}

/// A disabled, filled field renders the M3 disabled fill color, not the
/// enabled default — proves the state table's `disabled` branch actually
/// reaches painted configuration, not just `default_fill_color`'s own unit
/// test in isolation.
#[test]
fn disabled_row_reaches_the_mounted_decoration_with_disabled_m3_colors() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let decoration = InputDecoration {
        filled: true,
        enabled: false,
        ..Default::default()
    };
    let laid = lay_out(
        Theme::new(theme, InputDecorator::new(decoration).child(child_stub())),
        tight(300.0, 100.0),
    );

    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("InputDecorator must compose a DecoratedBox");
    let decoration_debug = laid
        .render_property(decorated_box, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");

    let disabled_fill = colors.on_surface.with_opacity(0.04);
    assert!(
        decoration_debug.contains(&format!("{disabled_fill:?}")),
        "disabled fill color {disabled_fill:?} must reach the mounted decoration, got: \
         {decoration_debug}"
    );

    let disabled_indicator = colors.on_surface.with_opacity(0.38);
    assert!(
        decoration_debug.contains(&format!("{disabled_indicator:?}")),
        "disabled indicator color {disabled_indicator:?} must reach the mounted decoration, got: \
         {decoration_debug}"
    );
}

/// An enabled, unfilled field composes a TRANSPARENT fill — but the
/// underline still paints: `filled` only gates `_getFillColor`
/// (`input_decorator.dart:2131-2140`, tag `3.44.0`); `activeIndicatorBorder`
/// is resolved unconditionally, filled or not. Pinning both halves matters —
/// a prior version of this test asserted only the transparent fill and its
/// doc comment claimed the underline was absent too, which is false and
/// would not have caught a regression that dropped the underline entirely.
#[test]
fn unfilled_enabled_decoration_has_a_transparent_fill_but_still_paints_the_underline() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let decoration = InputDecoration::default();
    let laid = lay_out(
        Theme::new(
            theme,
            InputDecorator::new(decoration).child(SizedBox::shrink()),
        ),
        tight(300.0, 100.0),
    );

    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("InputDecorator must compose a DecoratedBox");
    let decoration_debug = laid
        .render_property(decorated_box, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");
    assert!(
        decoration_debug.contains(&format!("{:?}", Color::TRANSPARENT)),
        "an unfilled decoration must have a transparent fill, got: {decoration_debug}"
    );

    let plain_indicator_color = colors.on_surface_variant;
    assert!(
        decoration_debug.contains(&format!("{plain_indicator_color:?}")),
        "the underline must still paint its plain M3 color even when unfilled, got: \
         {decoration_debug}"
    );
}

/// `ThemeData.input_decoration_theme`'s `fill_color`/`active_indicator`
/// override the M3 default at the mounted level — proving the theme-tier
/// resolve arm in `InputDecoratorState::build` (`decoration_theme.fill_color
/// .as_ref().map_or_else(...)`, and the identical shape for
/// `active_indicator`) is actually reachable, not just correct in isolation
/// (every other `decoration_theme.*` slot in that function follows the same
/// `Option::as_ref().map_or_else(default, override)` shape, so this one
/// mounted proof stands in for the pattern).
#[test]
fn themed_fill_color_and_active_indicator_beat_the_m3_default_at_the_mounted_level() {
    let themed_fill = Color::rgb(11, 22, 33);
    let themed_indicator = BorderSide::new(Color::rgb(44, 55, 66), px(3.0), BorderStyle::Solid);
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let theme = theme.copy_with(ThemeDataOverrides {
        input_decoration_theme: Some(InputDecorationThemeData {
            fill_color: Some(WidgetStateProperty::all(Some(themed_fill))),
            active_indicator: Some(WidgetStateProperty::all(Some(themed_indicator))),
            ..Default::default()
        }),
        ..Default::default()
    });
    let decoration = InputDecoration {
        filled: true,
        ..Default::default()
    };
    let laid = lay_out(
        Theme::new(theme, InputDecorator::new(decoration).child(child_stub())),
        tight(300.0, 100.0),
    );

    let decorated_box = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("InputDecorator must compose a DecoratedBox");
    let decoration_debug = laid
        .render_property(decorated_box, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");

    assert!(
        decoration_debug.contains(&format!("{themed_fill:?}")),
        "a themed fill_color must beat the M3 default at the mounted level, got: \
         {decoration_debug}"
    );
    assert!(
        decoration_debug.contains(&format!("{:?}", themed_indicator.color)),
        "a themed active_indicator must beat the M3 default at the mounted level, got: \
         {decoration_debug}"
    );

    // The override must actually WIN, not merely coexist with the M3
    // default — the plain-state M3 fill (`surfaceContainerHighest`) must
    // not appear anywhere in the mounted decoration.
    let m3_default_fill = colors.surface_container_highest;
    assert!(
        !decoration_debug.contains(&format!("{m3_default_fill:?}")),
        "the M3 default fill must not leak through a configured theme override, got: \
         {decoration_debug}"
    );
}
