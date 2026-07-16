//! `AppBar` widget-level integration coverage — mounts a real app bar through
//! the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/material.rs`/`tests/elevated_button.rs`'s established pattern).
//!
//! `AppBar` composes `Theme::of` (M3 token defaults) and `MediaQuery::of`
//! (the top safe-area inset) — both ambient reads that only resolve through
//! a real mount, so these tests prove the composition end to end rather than
//! re-checking `app_bar.rs`'s own unit-tested `resolve_style` formula.

mod common;

use common::{lay_out, loose};
use flui_material::{AppBar, Theme, ThemeData};
use flui_types::EdgeInsets;
use flui_types::geometry::px;
use flui_widgets::{MediaQuery, MediaQueryData, Text};

/// `_ElevatedButtonDefaultsM3`'s sibling formatting helper (see
/// `tests/elevated_button.rs`'s `color_property`): the exact `Debug` string
/// `RenderPhysicalShape` writes into its `"color"` diagnostics property, so a
/// test can compare against a resolved `Color` without downcasting.
fn color_property(color: flui_types::Color) -> String {
    format!("{color:?}")
}

#[test]
fn standalone_app_bar_consumes_the_top_padding_itself() {
    // No Scaffold at all: an AppBar mounted directly under a MediaQuery that
    // reports a 24px top safe-area inset (a notch/status bar) must reserve
    // that inset on its own — the "consumes the top inset itself" contract
    // (`app_bar.rs`'s module docs).
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(24.0), px(0.0), px(0.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(media_query, AppBar::new().title(Text::new("Title"))),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0 + 24.0),
        "a primary AppBar must add the ambient MediaQuery top padding to its own \
         toolbar_height, unassisted by any Scaffold",
    );
}

#[test]
fn app_bar_with_no_top_padding_is_exactly_the_toolbar_height() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new().title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root).height,
        px(56.0),
        "with a zero MediaQuery padding, the app bar's height must be exactly \
         the default toolbar_height",
    );
}

#[test]
fn theme_defaults_apply_surface_background_and_zero_elevation() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(
            theme,
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new().title(Text::new("Title")),
            ),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("AppBar must compose a Material (RenderPhysicalShape) surface");

    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(colors.surface)),
        "an AppBar with no background_color override must resolve _AppBarDefaultsM3's \
         ColorScheme.surface",
    );
    assert_eq!(
        laid.render_property(material, "elevation"),
        Some("0".to_string()),
        "an AppBar with no elevation override must resolve _AppBarDefaultsM3's 0.0",
    );
}

#[test]
fn background_color_override_replaces_the_theme_default() {
    let overridden = flui_types::Color::rgb(10, 20, 30);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                AppBar::new()
                    .title(Text::new("Title"))
                    .background_color(overridden),
            ),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("AppBar must compose a Material (RenderPhysicalShape) surface");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(overridden)),
        "an explicit background_color must win over the theme default",
    );
}
