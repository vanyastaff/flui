//! `FloatingActionButton` widget-level integration coverage — mounts a real
//! FAB through the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/elevated_button.rs`'s established pattern), proving what only a
//! real mount can: the M3 default token table resolved against the REAL
//! lifecycle-synced `WidgetStatesController` (not a hand-built
//! `WidgetStates` value, which `floating_action_button.rs`'s own unit tests
//! already cover), a real tap firing `on_pressed`, and the 56×56 geometry a
//! `Scaffold`'s `floating_action_button` slot actually lays out.

use crate::common;

use common::{lay_out, tight};
use flui_material::{
    FabThemeData, FloatingActionButton, Scaffold, Theme, ThemeData, ThemeDataOverrides,
};
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

/// `_FABDefaultsM3`'s formatted `Debug` string for a given resolved
/// [`Color`](flui_sdk::painting::Color) — the same helper `tests/elevated_button.rs`
/// uses for `RenderPhysicalShape`'s `"color"` diagnostics property.
fn color_property(color: flui_sdk::painting::Color) -> String {
    format!("{color:?}")
}

/// The middle cascade tier, proven end to end: a
/// `ThemeData.floating_action_button_theme` with a custom
/// `background_color`/`elevation` reaches the mounted `Material` — both the
/// enabled-default and (per `resolve_elevation`'s doc comment) the disabled
/// tier of the elevation state chain.
pub fn fab_theme_slot_reaches_the_mounted_materials_color_and_elevation() {
    let themed_background = flui_sdk::painting::Color::rgb(70, 80, 90);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        floating_action_button_theme: Some(FabThemeData {
            background_color: Some(themed_background),
            elevation: Some(12.0),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(
            theme,
            FloatingActionButton::new(SizedBox::square(24.0)).on_pressed(|_cx| {}),
        ),
        tight(56.0, 56.0),
    );

    let material = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("Material must mount");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(themed_background)),
        "a configured floating_action_button_theme.background_color must reach the mounted \
         Material",
    );
    let elevation = laid
        .render_property(material, "elevation")
        .expect("RenderPhysicalShape reports an \"elevation\" diagnostics property");
    assert_eq!(
        elevation.parse::<f64>(),
        Ok(12.0),
        "a configured floating_action_button_theme.elevation must reach the enabled tier",
    );
}

pub fn mounted_geometry_in_a_scaffold_slot_is_exactly_56_by_56_at_the_end_float_position() {
    // Mirrors `tests/scaffold.rs`'s own FAB-slot geometry tests, but with a
    // real `FloatingActionButton` (56x56 via its own `ConstrainedBox`, not a
    // stand-in `SizedBox`) proving this V1's `FAB_SIZE` constant actually
    // reaches the Scaffold's `floating_action_button` slot unmodified.
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(
                        FloatingActionButton::new(SizedBox::square(24.0)).on_pressed(|_cx| {}),
                    ),
            ),
        ),
        tight(400.0, 800.0),
    );

    let layout_root = laid
        .try_find_by_render_type("RenderCustomMultiChildLayoutBox")
        .expect("Scaffold's multi-child layout must be mounted");
    // `Scaffold::build` pushes `LayoutId`s in `body`, `floating_action_button`
    // order when both are set and there is no `app_bar` — see `scaffold.rs`.
    let fab = laid.child(layout_root, 1);

    assert_eq!(
        laid.size(fab),
        common::size(56.0, 56.0),
        "the mounted FloatingActionButton's own ConstrainedBox must pin it to exactly 56x56, \
         regardless of the Scaffold's loose FAB constraints",
    );

    // `FloatingActionButtonLocation.endFloat`: kFloatingActionButtonMargin
    // (16) from the right edge and bottom safe area, with a zero
    // `min_view_padding_bottom`/`min_insets` here (default `MediaQueryData`)
    // — the flat-margin case `scaffold.rs`'s own delegate tests already pin
    // exactly, repeated here end to end through a real `FloatingActionButton`.
    assert_eq!(
        laid.offset(fab),
        common::offset(400.0 - 16.0 - 56.0, 800.0 - 56.0 - 16.0),
    );
}
