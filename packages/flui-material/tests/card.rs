//! `Card` widget-level integration coverage — mounts a real `Card` through
//! the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/material.rs`/`tests/elevated_button.rs` use) and probes the
//! composed [`Material`](flui_material::Material) (`RenderPhysicalShape`)
//! and [`Padding`](flui_sdk::widgets::Padding) (`RenderPadding`) render objects it
//! produces, proving `_CardDefaultsM3` actually reaches paint configuration
//! rather than just being computed in isolation.

use crate::common;

use common::{lay_out, tight};
use flui_material::{Card, CardThemeData, Theme, ThemeData, ThemeDataOverrides};
use flui_sdk::painting::Color;
use flui_sdk::widgets::ColoredBox;

/// `_CardDefaultsM3`'s formatted `Debug` string for a resolved
/// [`Color`](flui_sdk::painting::Color) — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property, mirroring `tests/elevated_button.rs`'s identical helper.
fn color_property(color: Color) -> String {
    format!("{color:?}")
}

/// The middle cascade tier, proven end to end: a `ThemeData.card_theme` with
/// custom `color`/`elevation` reaches the mounted `Material`, per field —
/// an unset `shape` on the same theme slot must still fall through to the
/// M3 default independently (proven via `card.rs`'s own unit tests; this
/// mount only needs to prove the theme tier is reachable at all).
pub fn card_theme_slot_reaches_the_mounted_materials_color_and_elevation() {
    let themed_color = Color::rgb(77, 88, 99);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        card_theme: Some(CardThemeData {
            color: Some(themed_color),
            elevation: Some(15.0),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(theme, Card::new(ColoredBox::new(Color::rgb(1, 2, 3)))),
        tight(200.0, 200.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(themed_color)),
        "a configured card_theme.color must reach the mounted Material",
    );
    let elevation = laid
        .render_property(material, "elevation")
        .expect("RenderPhysicalShape reports an \"elevation\" diagnostics property");
    assert_eq!(elevation.parse::<f64>(), Ok(15.0));
}
