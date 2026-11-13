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

use common::{lay_out, rgba, tight};
use flui_material::{
    ButtonStyle, ElevatedButton, ElevatedButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_sdk::widgets::{Text, WidgetStateProperty};
use flui_testing::a11y::{Action, Role};

/// The highest tier still wins over a configured theme: an explicit
/// `.style(..)` override on the widget itself must resolve over the theme's
/// `elevated_button_theme` (`widget_style ?? theme_style ?? default`).
pub fn widget_level_style_wins_over_the_elevated_button_theme() {
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
                .on_pressed(|_ctx| {})
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
        laid.render_node(material)
            .get::<flui_sdk::foundation::inspect::Rgba>("color"),
        Ok(rgba(widget_background)),
        "an explicit widget-level style must win over a configured elevated_button_theme",
    );
}

// ===========================================================================
// Accessibility semantics: the button node on the well
// ===========================================================================
//
// Flutter wraps a `ButtonStyleButton` in `Semantics(container: true, button:
// true, enabled: ..)`. Here the well's interactive node carries that
// description (`packages/flui-material/src/button_style_button.rs`), so the
// child `Text`'s label merges into it and the button is one node with the
// role, the enabled state, the click and the focus a screen reader acts on.
// `ElevatedButton` stands in for the whole `ButtonStyleButtonCore` family
// here (see this file's own module doc): `FilledButton`, `OutlinedButton`,
// `TextButton` and `IconButton` share the composition.

/// One button, one label: the child `Text`'s `RenderParagraph` label merges
/// into the button node `ButtonStyleButtonCore` describes on its well,
/// rather than forming a second, separate node.
pub fn elevated_button_with_text_child_announces_one_labelled_button_node() {
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Increment")).on_pressed(|_ctx| {}),
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

/// The button is one node whatever its state: enabled, it offers the click
/// and the focus a screen reader acts on; disabled, it says so and offers
/// no click. Either way it covers the button's box and has no child node.
pub fn style_button_is_one_button_node_enabled_or_disabled() {
    for enabled in [true, false] {
        let name = if enabled { "enabled" } else { "disabled" };
        let mut button = ElevatedButton::new(Text::new("Save"));
        if enabled {
            button = button.on_pressed(|_ctx| {});
        }
        let mut laid = lay_out(Theme::new(ThemeData::light(), button), tight(120.0, 48.0));
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let node = tree
            .find_by_label("Save")
            .unwrap_or_else(|error| panic!("{name}: no node labelled \"Save\": {error}"));
        let shown = tree.describe();
        assert_eq!(node.role(), Role::Button, "{name}:\n{shown}");
        assert_eq!(node.is_disabled(), !enabled, "{name}:\n{shown}");
        assert_eq!(
            node.supports_action(Action::Click),
            enabled,
            "{name}:\n{shown}"
        );
        if enabled {
            assert!(node.supports_action(Action::Focus), "{name}:\n{shown}");
        }
        assert!(node.child_ids().is_empty(), "{name}:\n{shown}");
        let bounds = node
            .bounds()
            .unwrap_or_else(|| panic!("{name}: no bounds:\n{shown}"));
        assert_eq!(
            (bounds.width(), bounds.height()),
            (120.0, 48.0),
            "{name}:\n{shown}"
        );
    }
}

/// Every Material control that builds its own semantics publishes the
/// identifier its author gave it (ADR-0240) as the `author_id` of the node it
/// forms, under the role it announces, and never in what it announces.
pub fn material_controls_publish_their_identifier() {
    use flui_material::{
        Checkbox, Chip, FilledButton, FilterChip, IconButton, OutlinedButton, Radio, Switch,
        TextButton, TextField,
    };
    use flui_sdk::view::prelude::*;
    use flui_sdk::widgets::{SizedBox, TextEditingController};

    /// An identifier, the role its node announces, and the control.
    type Row = (&'static str, Role, fn() -> BoxedView);
    let rows: [Row; 11] = [
        ("elevated", Role::Button, || {
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(|_ctx| {})
                .identifier("elevated")
                .boxed()
        }),
        ("filled", Role::Button, || {
            FilledButton::new(Text::new("Save"))
                .on_pressed(|_ctx| {})
                .identifier("filled")
                .boxed()
        }),
        ("outlined", Role::Button, || {
            OutlinedButton::new(Text::new("Save"))
                .on_pressed(|_ctx| {})
                .identifier("outlined")
                .boxed()
        }),
        ("text", Role::Button, || {
            TextButton::new(Text::new("Save"))
                .on_pressed(|_ctx| {})
                .identifier("text")
                .boxed()
        }),
        ("icon", Role::Button, || {
            IconButton::new(SizedBox::new(24.0, 24.0))
                .on_pressed(|_ctx| {})
                .identifier("icon")
                .boxed()
        }),
        ("checkbox", Role::CheckBox, || {
            Checkbox::new(false)
                .on_changed(|_ctx, _value| {})
                .identifier("checkbox")
                .boxed()
        }),
        ("switch", Role::Switch, || {
            Switch::new(false)
                .on_changed(|_ctx, _value| {})
                .identifier("switch")
                .boxed()
        }),
        ("radio", Role::RadioButton, || {
            Radio::new("spring", Some("spring"))
                .on_changed(|_ctx, _value| {})
                .identifier("radio")
                .boxed()
        }),
        ("chip", Role::Button, || {
            Chip::new(Text::new("Tag"))
                .on_pressed(|_ctx| {})
                .identifier("chip")
                .boxed()
        }),
        ("filter-chip", Role::Button, || {
            FilterChip::new(Text::new("Tag"))
                .on_selected(|_ctx, _selected| {})
                .identifier("filter-chip")
                .boxed()
        }),
        ("field", Role::TextInput, || {
            TextField::new(TextEditingController::new())
                .identifier("field")
                .boxed()
        }),
    ];

    let mut failed = Vec::new();
    for (identifier, role, view) in rows {
        let mut laid = lay_out(Theme::new(ThemeData::light(), view()), tight(200.0, 56.0));
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let carriers: Vec<_> = tree
            .nodes()
            .filter(|node| node.raw().author_id() == Some(identifier))
            .collect();
        let [node] = carriers.as_slice() else {
            failed.push(format!(
                "{identifier}: {} nodes carry it\n{}",
                carriers.len(),
                tree.describe()
            ));
            continue;
        };
        if node.role() != role {
            failed.push(format!("{identifier}: on a {:?} node", node.role()));
        }
        let announced = [node.label(), node.value(), node.raw().description()];
        if announced
            .iter()
            .flatten()
            .any(|text| text.contains(identifier))
        {
            failed.push(format!("{identifier}: announced in {announced:?}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
