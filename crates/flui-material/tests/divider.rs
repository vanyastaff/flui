//! `Divider`/`VerticalDivider` widget-level integration coverage — mounts a
//! real `Divider` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/card.rs` uses) and proves the M3 geometry
//! (height/thickness/indents) and the theme cascade actually reach a mounted
//! tree, not just `resolve_style` computed in isolation.

mod common;

use common::{lay_out, loose};
use flui_material::{
    Divider, DividerThemeData, Theme, ThemeData, ThemeDataOverrides, VerticalDivider,
};
use flui_types::Color;

/// `_DividerDefaultsM3`'s full geometry table reaches the mounted tree: the
/// filled line is `1.0` thick and inset by `indent`/`end_indent` on the
/// left/right, under a loose (not tight) root so the divider's own
/// intrinsic `16.0` height request isn't overridden by a forced parent size.
#[test]
fn default_geometry_matches_the_m3_token_table() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Divider::new().indent(8.0).end_indent(12.0),
        ),
        loose(400.0),
    );

    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("Divider must compose a decorated (filled) line");
    assert_eq!(
        laid.size(decorated).height.get(),
        1.0,
        "_DividerDefaultsM3.thickness (1.0) must set the filled line's height"
    );

    let width = laid.size(decorated).width.get();
    assert_eq!(
        width,
        400.0 - 8.0 - 12.0,
        "indent (8.0) and end_indent (12.0) must both reduce the line's width from the \
         400px root"
    );
}

/// `VerticalDivider`'s geometry mirrors `Divider`'s on the TRANSPOSED axis:
/// thickness sets the line's WIDTH (not height), and `indent`/`end_indent`
/// reduce the line's HEIGHT (not width) — Flutter parity: `Divider`'s
/// `height`/margin-`top`/margin-`bottom` axis vs. `VerticalDivider`'s
/// `width`/margin-`left`/margin-`right` axis (`divider.dart`, oracle tag
/// `3.44.0`). A width/height mapping that was accidentally left transposed
/// (or copy-pasted from `Divider` unchanged) fails this test: swap
/// `laid.size(decorated).width`/`.height` below and both assertions flip
/// from matching to mismatching the M3 token values.
#[test]
fn vertical_divider_default_geometry_matches_the_m3_token_table_on_the_transposed_axis() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            VerticalDivider::new().indent(8.0).end_indent(12.0),
        ),
        loose(400.0),
    );

    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("VerticalDivider must compose a decorated (filled) line");
    assert_eq!(
        laid.size(decorated).width.get(),
        1.0,
        "_DividerDefaultsM3.thickness (1.0) must set the filled line's WIDTH for \
         VerticalDivider"
    );

    let height = laid.size(decorated).height.get();
    assert_eq!(
        height,
        400.0 - 8.0 - 12.0,
        "indent (8.0) and end_indent (12.0) must both reduce the line's HEIGHT (not width) \
         from the 400px root"
    );
}

/// The theme tier's `color` reaches the mounted line's decoration — proving
/// `ThemeData.divider_theme` is actually consulted, not just computed in
/// `resolve_style` isolation.
#[test]
fn themed_color_beats_the_m3_default() {
    let themed_color = Color::rgb(44, 55, 66);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        divider_theme: Some(DividerThemeData {
            color: Some(themed_color),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(Theme::new(theme, Divider::new()), loose(400.0));

    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("Divider must compose a decorated (filled) line");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");

    assert!(
        decoration.contains(&format!("{themed_color:?}")),
        "a configured divider_theme.color must reach the mounted line — got {decoration:?}"
    );
}

/// A widget-level `.color(...)` override wins over a configured
/// `divider_theme.color` — the standard widget → theme → default cascade.
#[test]
fn widget_color_override_wins_over_the_divider_theme() {
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        divider_theme: Some(DividerThemeData {
            color: Some(Color::rgb(1, 1, 1)),
            ..Default::default()
        }),
        ..Default::default()
    });
    let widget_color = Color::rgb(9, 9, 9);

    let laid = lay_out(
        Theme::new(theme, Divider::new().color(widget_color)),
        loose(400.0),
    );

    let decorated = laid
        .find_by_render_type("RenderDecoratedBox")
        .expect("Divider must compose a decorated (filled) line");
    let decoration = laid
        .render_property(decorated, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");

    assert!(
        decoration.contains(&format!("{widget_color:?}")),
        "an explicit Divider::color override must win over divider_theme.color — got \
         {decoration:?}"
    );
}
