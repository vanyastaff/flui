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

use crate::common;

use common::{lay_out, tight};
use flui_material::{
    ButtonStyle, ElevatedButton, ElevatedButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_sdk::widgets::{Text, WidgetStateProperty};
use flui_testing::a11y::Role;

/// `_ElevatedButtonDefaultsM3`'s formatted `Debug` string for a given
/// resolved [`Color`](flui_sdk::painting::Color) — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property (`add_color("color", format!("{:?}", self.color))`,
/// `crates/flui-objects/src/proxy/physical_model.rs`), so a test can compare
/// against it without downcasting the render object.
fn color_property(color: flui_sdk::painting::Color) -> String {
    format!("{color:?}")
}

/// The highest tier still wins over a configured theme: an explicit
/// `.style(..)` override on the widget itself must resolve over the theme's
/// `elevated_button_theme`, matching Flutter's own `getProperty(widgetStyle)
/// ?? getProperty(themeStyle) ?? …` precedence.
#[test]
fn widget_level_style_wins_over_the_elevated_button_theme() {
    let themed_background = flui_sdk::painting::Color::rgb(1, 1, 1);
    let widget_background = flui_sdk::painting::Color::rgb(9, 9, 9);
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
                .on_pressed(|_cx| {})
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
// (`packages/flui-material/src/button_style_button.rs`): with no boundary
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
// `flui_sdk::widgets::Semantics::on_tap` requires `Fn() + Send + Sync + 'static`
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
            ElevatedButton::new(Text::new("Increment")).on_pressed(|_cx| {}),
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
