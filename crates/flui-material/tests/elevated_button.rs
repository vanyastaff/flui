//! `ElevatedButton` widget-level integration coverage — mounts a real button
//! through the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/ink_well.rs`/`tests/material.rs`'s established pattern) and drives
//! real pointer dispatch. Hit-testing runs inside `enter_owner_scope` (see
//! `common::LaidOut::route_event`'s doc comment) since `Material`'s clip
//! resolves through the owner-lane path-clipper registry — mounting without
//! it would silently degrade to the whole-box fallback clip instead of
//! erroring, which is exactly the trap that module's doc comment warns
//! about.
//!
//! `ElevatedButton` stands in for the whole `ButtonStyleButtonCore`
//! composition here; `FilledButton`/`OutlinedButton`/`TextButton` share the
//! identical composition path (only their `default_style` tables differ,
//! covered by each file's own unit tests), so one button's worth of
//! integration coverage is enough to prove the wiring, not four.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{
    ButtonStyle, ElevatedButton, ElevatedButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_testing::a11y::Role;
use flui_widgets::{Text, WidgetStateProperty};

/// `_ElevatedButtonDefaultsM3`'s formatted `Debug` string for a given
/// resolved [`Color`](flui_types::Color) — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property (`add_color("color", format!("{:?}", self.color))`,
/// `crates/flui-objects/src/proxy/physical_model.rs`), so a test can compare
/// against it without downcasting the render object.
fn color_property(color: flui_types::Color) -> String {
    format!("{color:?}")
}

#[test]
fn tap_fires_on_pressed_and_the_button_mounts_a_material_surface() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Save")).on_pressed(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        tight(120.0, 48.0),
    );

    assert!(
        laid.try_find_by_render_type("RenderPhysicalShape")
            .is_some(),
        "ElevatedButton must compose a Material (RenderPhysicalShape) surface",
    );

    laid.dispatch_pointer_down(60.0, 24.0);
    laid.dispatch_pointer_up(60.0, 24.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on an enabled ElevatedButton must fire on_pressed exactly once",
    );
}

#[test]
fn a_button_with_no_press_handler_is_disabled_and_a_tap_dispatch_is_a_no_op() {
    // No `.on_pressed(..)`: `ButtonStyleButtonCore::is_interactive` is
    // false, so the inner `InkWell` never gets an `on_tap` closure at all
    // (unit-tested directly at the construction level by
    // `elevated_button::tests::is_disabled_when_no_press_handler_is_set`).
    // What only an end-to-end mount can prove: a real pointer down+up
    // dispatched at a disabled button's composed
    // ConstrainedBox/Material/InkWell/Padding stack does not panic and
    // leaves the composition mounted — a regression guard against any of
    // those four layers assuming an `on_tap` closure is always present.
    let laid = lay_out(
        Theme::new(ThemeData::light(), ElevatedButton::new(Text::new("Save"))),
        tight(120.0, 48.0),
    );
    let material_before = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("a disabled ElevatedButton must still mount its Material surface");

    laid.dispatch_pointer_down(60.0, 24.0);
    laid.dispatch_pointer_up(60.0, 24.0);

    let material_after = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("the Material surface must survive a tap dispatch");
    assert_eq!(
        material_before, material_after,
        "the disabled button's render tree must not be torn down or rebuilt \
         under a tap dispatch it does not react to",
    );
}

/// Mutation-honest coverage for `ButtonStyleButtonCoreState::init_state`'s
/// `WidgetState::Disabled` sync (`crates/flui-material/src/button_style_button.rs`)
/// — driven through the REAL `create_state`/`init_state` lifecycle of a
/// mounted `ElevatedButton`, not a hand-constructed `WidgetStates` value.
/// Deleting that sync line leaves every unit test in `elevated_button.rs`
/// green (they all resolve against a states value they construct
/// themselves), because `_ElevatedButtonDefaultsM3`'s `background_color`
/// closure only produces a DIFFERENT color for `WidgetState::Disabled` —
/// only an end-to-end mount, whose `states.value()` is fed by the real
/// lifecycle hook, can tell whether that bit actually got set.
#[test]
fn a_handler_less_button_resolves_the_disabled_background_color_through_the_real_lifecycle() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(theme, ElevatedButton::new(Text::new("Save"))),
        tight(120.0, 48.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("a disabled ElevatedButton must still mount its Material surface");
    let resolved_color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");

    assert_eq!(
        resolved_color,
        color_property(colors.on_surface.with_opacity(0.12)),
        "a button with no on_pressed handler must resolve _ElevatedButtonDefaultsM3's disabled \
         background color (onSurface@12%) — which only happens if init_state actually set \
         WidgetState::Disabled before the first build; without that sync this resolves to the \
         enabled default (surfaceContainerLow) instead",
    );
}

/// Companion coverage for `did_update_view`'s re-sync branch: an ENABLED
/// button (real `on_pressed`) resolves the enabled background first, then a
/// root swap to a handler-less `ElevatedButton` (same element identity,
/// `did_update_view` fires, not `init_state` again) must re-resolve the
/// disabled background. Mutation-honest the same way as the test above:
/// deleting `did_update_view`'s `WidgetState::Disabled` re-sync leaves the
/// enabled color stuck after the swap.
#[test]
fn did_update_view_resyncs_disabled_when_the_press_handler_is_removed() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let mut laid = lay_out(
        Theme::new(
            theme.clone(),
            ElevatedButton::new(Text::new("Save")).on_pressed(|| {}),
        ),
        tight(120.0, 48.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("Material must mount");
    let enabled_color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");
    assert_eq!(
        enabled_color,
        color_property(colors.surface_container_low),
        "an enabled button must resolve _ElevatedButtonDefaultsM3's enabled background color",
    );

    // Root swap to the SAME widget shape minus `.on_pressed(..)`:
    // reconciliation keeps element/render identity, so this exercises
    // `did_update_view`, not a fresh `init_state`.
    laid.pump_widget(Theme::new(theme, ElevatedButton::new(Text::new("Save"))));

    let material_after_swap = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("Material must still be mounted after the swap");
    assert_eq!(
        material, material_after_swap,
        "the swap must reconcile onto the same render node (did_update_view), not remount",
    );
    let disabled_color = laid
        .render_property(material_after_swap, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");
    assert_eq!(
        disabled_color,
        color_property(colors.on_surface.with_opacity(0.12)),
        "removing the press handler must re-sync WidgetState::Disabled via did_update_view, \
         re-resolving the disabled background color",
    );
}

/// The middle cascade tier, proven end to end: a `ThemeData.elevated_button_theme`
/// with a custom `background_color` must reach the mounted `Material`'s
/// resolved color — this is the wiring `button_style_button.rs`'s
/// `theme_style` seam exists for; before it was wired, this exact scenario
/// silently resolved the M3 default instead (`theme_style` was hardcoded
/// `None` at every call site).
#[test]
fn elevated_button_theme_slot_reaches_the_mounted_materials_background_color() {
    let themed_background = flui_types::Color::rgb(11, 22, 33);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        elevated_button_theme: Some(ElevatedButtonThemeData {
            style: Some(ButtonStyle {
                background_color: Some(WidgetStateProperty::all(Some(themed_background))),
                ..Default::default()
            }),
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(
            theme,
            ElevatedButton::new(Text::new("Save")).on_pressed(|| {}),
        ),
        tight(120.0, 48.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("ElevatedButton must compose a Material surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(themed_background)),
        "a configured elevated_button_theme.style.background_color must reach the mounted \
         Material — the middle (theme) tier of the widget/theme/default cascade",
    );
}

/// The highest tier still wins over a configured theme: an explicit
/// `.style(..)` override on the widget itself must resolve over the theme's
/// `elevated_button_theme`, matching Flutter's own `getProperty(widgetStyle)
/// ?? getProperty(themeStyle) ?? …` precedence.
#[test]
fn widget_level_style_wins_over_the_elevated_button_theme() {
    let themed_background = flui_types::Color::rgb(1, 1, 1);
    let widget_background = flui_types::Color::rgb(9, 9, 9);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        elevated_button_theme: Some(ElevatedButtonThemeData {
            style: Some(ButtonStyle {
                background_color: Some(WidgetStateProperty::all(Some(themed_background))),
                ..Default::default()
            }),
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(
            theme,
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(|| {})
                .style(ButtonStyle {
                    background_color: Some(WidgetStateProperty::all(Some(widget_background))),
                    ..Default::default()
                }),
        ),
        tight(120.0, 48.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("ElevatedButton must compose a Material surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(widget_background)),
        "an explicit widget-level style must win over a configured elevated_button_theme",
    );
}

// ===========================================================================
// Accessibility semantics — `ButtonStyleButtonCore`'s `Semantics` wrapper
// ===========================================================================
//
// Red before `ButtonStyleButtonCore::build` wrapped its composition in
// `Semantics(container: true, button: true, enabled: ..)`
// (`crates/flui-material/src/button_style_button.rs`): with no boundary
// under `ElevatedButton`, `RenderParagraph`'s label (once it started
// publishing one) had nowhere non-root to merge into and `find_by_label`
// failed with `A11yQueryError::NotFound`. `ElevatedButton` again stands in
// for the whole `ButtonStyleButtonCore` family here (see this file's own
// module doc) — `FilledButton`/`OutlinedButton`/`TextButton`/`IconButton`/
// `FloatingActionButton` share the identical wrapper.
//
// The a11y tap-action round trip `crates/flui-widgets/tests/semantics.rs`
// covers for a hand-built `Semantics::on_tap` handler is NOT mirrored here:
// `ButtonStyleButtonCore::on_pressed` is `Rc<dyn Fn()>` (owner-local, per
// ADR-0027 — see that field's own doc comment), while
// `flui_widgets::Semantics::on_tap` requires `Fn() + Send + Sync + 'static`
// (see that builder's module doc, "The `Send + Sync` bound on action
// handlers comes from storage, not from threading"). Routing `on_pressed`
// through the `Semantics` wrapper's own tap action would need a `Send +
// Sync`-compatible callback shape for the whole button family — a change to
// `PressCallback` itself, well outside this change's scope (adding the
// `Semantics` wrapper). A screen-reader "double-tap to activate" therefore
// still reaches this button only via the platform's synthesized pointer
// tap, not FLUI's own semantics-action dispatch, until that follow-up lands.

/// One button, one label: the child `Text`'s `RenderParagraph` label merges
/// into the `Semantics(container: true, button: true)` boundary
/// `ButtonStyleButtonCore` wraps around the whole composition, rather than
/// forming a second, separate node.
#[test]
fn elevated_button_with_text_child_announces_one_labelled_button_node() {
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Increment")).on_pressed(|| {}),
        ),
        tight(120.0, 48.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node = tree
        .find_by_label("Increment")
        .unwrap_or_else(|error| panic!("expected one node labelled \"Increment\": {error}"));

    assert_eq!(node.role(), Role::Button, "Tree was:\n{}", tree.describe());
    assert!(
        !node.is_disabled(),
        "an ElevatedButton with on_pressed set must announce enabled. Tree was:\n{}",
        tree.describe()
    );
    assert!(
        node.child_ids().is_empty(),
        "the child paragraph's label must merge into the button's own node, not form a \
         separate child node. Tree was:\n{}",
        tree.describe()
    );
}

/// The disabled counterpart: no `on_pressed` must announce `enabled: false`
/// on the very same merged node, matching `ButtonStyle.enabled =>
/// onPressed != null` (`button_style_button.dart`).
#[test]
fn elevated_button_without_on_pressed_announces_disabled() {
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Increment")),
        ),
        tight(120.0, 48.0),
    );
    laid.enable_semantics();
    laid.pump();

    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node = tree
        .find_by_label("Increment")
        .unwrap_or_else(|error| panic!("expected one node labelled \"Increment\": {error}"));

    assert!(
        node.is_disabled(),
        "an ElevatedButton with no on_pressed handler must announce disabled. Tree was:\n{}",
        tree.describe()
    );
}
