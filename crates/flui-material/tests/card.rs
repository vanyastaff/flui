//! `Card` widget-level integration coverage — mounts a real `Card` through
//! the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/material.rs`/`tests/elevated_button.rs` use) and probes the
//! composed [`Material`](flui_material::Material) (`RenderPhysicalShape`)
//! and [`Padding`](flui_widgets::Padding) (`RenderPadding`) render objects it
//! produces, proving `_CardDefaultsM3` actually reaches paint configuration
//! rather than just being computed in isolation.

mod common;

use common::{lay_out, tight};
use flui_material::{Card, Theme, ThemeData};
use flui_types::Color;
use flui_widgets::ColoredBox;

/// `_CardDefaultsM3`'s formatted `Debug` string for a resolved
/// [`Color`](flui_types::Color) — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property, mirroring `tests/elevated_button.rs`'s identical helper.
fn color_property(color: Color) -> String {
    format!("{color:?}")
}

#[test]
fn default_material_matches_card_defaults_m3() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(theme, Card::new(ColoredBox::new(Color::rgb(1, 2, 3)))),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material (RenderPhysicalShape) surface");

    let color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");
    assert_eq!(
        color,
        color_property(colors.surface_container_low),
        "_CardDefaultsM3.color is ColorScheme.surfaceContainerLow"
    );

    let elevation = laid
        .render_property(material, "elevation")
        .expect("RenderPhysicalShape reports an \"elevation\" diagnostics property");
    assert_eq!(
        elevation.parse::<f32>(),
        Ok(1.0),
        "_CardDefaultsM3 constructs with elevation: 1.0"
    );

    let clip_behavior = laid
        .render_property(material, "clip_behavior")
        .expect("RenderPhysicalShape reports a \"clip_behavior\" diagnostics property");
    assert_eq!(
        clip_behavior, "None",
        "_CardDefaultsM3 constructs with clipBehavior: Clip.none"
    );
}

/// `_CardDefaultsM3`'s margin (`EdgeInsets.all(4.0)`) reaches the composed
/// `Padding`: under a fixed-size root the `Material` must sit inset by
/// exactly 4 logical pixels on every side, positioned at `(4, 4)`.
#[test]
fn default_margin_insets_the_material_by_four_pixels() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(1, 2, 3))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");

    assert_eq!(
        laid.offset(material),
        common::offset(4.0, 4.0),
        "_CardDefaultsM3's margin (EdgeInsets.all(4.0)) must inset the Material by 4px"
    );
}

/// An explicit `.margin(...)` override reaches the same `Padding`, replacing
/// the `_CardDefaultsM3` default rather than composing with it.
#[test]
fn margin_override_replaces_the_default_inset() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(1, 2, 3)))
                .margin(flui_types::EdgeInsets::all(flui_types::geometry::px(10.0))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");

    assert_eq!(
        laid.offset(material),
        common::offset(10.0, 10.0),
        "an overridden margin must replace _CardDefaultsM3's 4px default"
    );
}

/// The child actually renders inside the `Material` surface: a `ColoredBox`
/// child mounts its `RenderDecoratedBox` as the `Material`'s only child.
#[test]
fn child_mounts_inside_the_material_surface() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(9, 9, 9))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");
    let child = laid.only_child(material);

    let decoration = laid
        .render_property(child, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");
    assert!(
        decoration.contains(&color_property(Color::rgb(9, 9, 9))),
        "the ColoredBox child must mount as the Material's child with its own color intact, \
         not be dropped or repainted over — got decoration {decoration:?}"
    );
    assert_eq!(
        laid.size(child),
        laid.size(material),
        "a childless-margin ColoredBox must fill the Material's whole content area"
    );
}
