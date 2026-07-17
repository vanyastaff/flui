//! `ListTile` widget-level integration coverage — mounts a real `ListTile`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/ink_well.rs` use) and proves the whole-tile tap
//! target, the `enabled`/theme cascades, and slot presence/absence actually
//! reach a mounted tree, not just `resolve_style` computed in isolation.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose, tight};
use flui_material::{ListTile, ListTileThemeData, Theme, ThemeData, ThemeDataOverrides};
use flui_types::Color;
use flui_view::IntoView;
use flui_widgets::{Icon, IconData, IconTheme, IconThemeData, MediaQuery, MediaQueryData, Text};

/// `ListTile::build` reads `SafeArea`, which panics without an ambient
/// `MediaQuery` (`tests/app_bar.rs`'s own tests wrap the same way) — every
/// test mounts under this default `MediaQueryData` (no system insets) so the
/// `Theme`/`ListTile` under test is the only thing varying per case.
fn themed(theme: ThemeData, child: impl IntoView) -> MediaQuery {
    MediaQuery::new(MediaQueryData::default(), Theme::new(theme, child))
}

/// A tap anywhere on the tile — including inside the content padding gutter,
/// well away from `title`'s own text glyphs — fires `on_tap`: the whole tile
/// is the tap target, not just the title. Flutter parity: `ListTile.build`
/// wraps its ENTIRE content (padding included) in a single `InkWell`
/// (`list_tile.dart`, oracle tag `3.44.0`), mirroring `tests/card.rs`'s
/// `default_corner_radius_reaches_the_mounted_material` pattern of probing
/// the mounted surface rather than the title's own bounds.
#[test]
fn whole_tile_tap_fires_from_a_point_inside_the_content_padding() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            ListTile::new().title(Text::new("Inbox")).on_tap(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        tight(400.0, 56.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");
    let origin = laid.absolute_offset(material);

    // `_LisTileDefaultsM3.contentPadding` starts at `left: 16.0` — a point
    // 2px from the tile's left edge sits inside that padding gutter, well
    // before any title glyph begins.
    laid.dispatch_pointer_down(origin.dx.get() + 2.0, origin.dy.get() + 2.0);
    laid.dispatch_pointer_up(origin.dx.get() + 2.0, origin.dy.get() + 2.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap inside the content padding (not on the title's own glyphs) must still fire \
         on_tap — the whole mounted tile, not just the title, is the tap target"
    );
}

/// `enabled(false)` swallows a tap entirely: the same point that fires
/// `on_tap` on an enabled tile produces zero calls once disabled — Flutter
/// parity: `InkWell(onTap: enabled ? onTap : null)` (`list_tile.dart`, oracle
/// tag `3.44.0`).
#[test]
fn disabled_tile_swallows_a_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        themed(
            ThemeData::light(),
            ListTile::new()
                .title(Text::new("Inbox"))
                .enabled(false)
                .on_tap(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                }),
        ),
        tight(400.0, 56.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");
    let origin = laid.absolute_offset(material);

    laid.dispatch_pointer_down(origin.dx.get() + 20.0, origin.dy.get() + 20.0);
    laid.dispatch_pointer_up(origin.dx.get() + 20.0, origin.dy.get() + 20.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "enabled(false) must swallow the tap — on_tap must not fire"
    );
}

/// A `ListTile` with only a title (no leading/subtitle/trailing) still
/// mounts a single-line tile at the default one-line height (`56.0`) —
/// proving the composition tolerates every slot being absent, not just
/// every slot being present.
#[test]
fn title_only_tile_mounts_at_the_one_line_height() {
    // Loose (not tight) constraints: a tight incoming height would force
    // the tile to exactly that height regardless of its own min-height
    // request, masking the very default this test exists to pin.
    let laid = lay_out(
        themed(ThemeData::light(), ListTile::new().title(Text::new("Solo"))),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");

    assert_eq!(
        laid.size(material).height.get(),
        56.0,
        "a title-only tile (no subtitle) must mount at the one-line M3 default height"
    );
}

/// A `ListTile` with leading, title, subtitle, and trailing all present
/// mounts without dropping any slot — each slot's content reaches the tree.
#[test]
fn every_slot_present_mounts_a_two_line_tile() {
    let laid = lay_out(
        themed(
            ThemeData::light(),
            ListTile::new()
                .leading(Text::new("L"))
                .title(Text::new("Title"))
                .subtitle(Text::new("Subtitle"))
                .trailing(Text::new("T")),
        ),
        loose(400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");

    assert_eq!(
        laid.size(material).height.get(),
        72.0,
        "leading+title+subtitle+trailing (two lines, not three) must mount at the two-line \
         M3 default height"
    );

    let text_runs = laid.find_all_by_render_type("RenderParagraph");
    assert_eq!(
        text_runs.len(),
        4,
        "all four slots (leading, title, subtitle, trailing) must mount their own text run"
    );
}

/// `ListTile` MERGES its resolved icon color into the ambient `IconTheme`
/// rather than replacing it — an app-level `IconTheme(size: ..)` above the
/// tile must still reach a bare `Icon` in `leading`, matching Flutter's
/// `IconTheme.merge` (`list_tile.dart` `:1008-1009`, oracle tag `3.44.0`).
/// Probed by comparing the mounted glyph's `RenderParagraph` height across
/// two distinct ambient sizes (far from `IconThemeData::fallback`'s
/// `24.0`): if `ListTile` replaced the ambient theme instead of merging
/// into it, both mounts would collapse to the SAME `24.0` fallback and this
/// height comparison would fail to distinguish them.
#[test]
fn ambient_icon_theme_size_reaches_a_bare_leading_icon_through_the_tile() {
    fn mounted_glyph_height(ambient_size: f32) -> f32 {
        let laid = lay_out(
            themed(
                ThemeData::light(),
                IconTheme::new(
                    IconThemeData {
                        size: Some(ambient_size),
                        ..IconThemeData::default()
                    },
                    ListTile::new().leading(Icon::new(IconData::new(0xE87D))),
                ),
            ),
            loose(400.0),
        );

        let glyph = laid
            .find_by_render_type("RenderParagraph")
            .expect("the leading Icon must mount its glyph as a RenderParagraph");
        laid.size(glyph).height.get()
    }

    let small = mounted_glyph_height(10.0);
    let large = mounted_glyph_height(80.0);

    assert!(
        large > small,
        "a larger ambient IconTheme size (80.0) must mount a taller glyph box than a smaller \
         one (10.0) — got small={small}, large={large}. Equal heights mean the ambient size \
         never reached the icon (ListTile replaced the ambient IconTheme instead of merging \
         into it)."
    );
}

/// The theme tier's `tile_color` reaches the mounted `Material` fill —
/// proving `ThemeData.list_tile_theme` is actually consulted, not just
/// computed in `resolve_style` isolation.
#[test]
fn list_tile_theme_slot_reaches_the_mounted_materials_color() {
    let themed_color = Color::rgb(11, 22, 33);
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        list_tile_theme: Some(ListTileThemeData {
            tile_color: Some(themed_color),
            ..Default::default()
        }),
        ..Default::default()
    });

    let laid = lay_out(
        themed(theme, ListTile::new().title(Text::new("Themed"))),
        tight(400.0, 56.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("ListTile must compose a Material surface");
    let color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");

    assert_eq!(
        color,
        format!("{themed_color:?}"),
        "a configured list_tile_theme.tile_color must reach the mounted Material"
    );
}
