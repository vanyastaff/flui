//! `ListTile` widget-level integration coverage — mounts a real `ListTile`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/ink_well.rs` use) and proves the whole-tile tap
//! target, the `enabled`/theme cascades, and slot presence/absence actually
//! reach a mounted tree, not just `resolve_style` computed in isolation.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose, tight};
use flui_material::{ListTile, ListTileThemeData, Radio, Theme, ThemeData, ThemeDataOverrides};
use flui_testing::a11y::Role;
use flui_types::Color;
use flui_view::IntoView;
use flui_widgets::{
    Icon, IconData, IconTheme, IconThemeData, MediaQuery, MediaQueryData, MergeSemantics, Text,
};

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
        .try_find_by_render_type("RenderPhysicalShape")
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
        .try_find_by_render_type("RenderPhysicalShape")
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
        .try_find_by_render_type("RenderPhysicalShape")
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
        .try_find_by_render_type("RenderPhysicalShape")
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
            .try_find_by_render_type("RenderParagraph")
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
        .try_find_by_render_type("RenderPhysicalShape")
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

/// A `Radio` in a real `ListTile` still announces as a radio button.
///
/// This is the composition a user actually writes, and the one nothing else in
/// this crate mounts: `ListTile` publishes `.enabled(..)` and `Radio` publishes
/// `.enabled(..)` too, so `is_compatible_with` reads the overlap as a conflict
/// and the two form **separate** nodes instead of merging. The radio therefore
/// keeps a node of its own — and that node announces as a radio, because
/// `Radio` publishes `.in_mutually_exclusive_group(true)`.
///
/// It mounts through this file's [`themed`] helper deliberately. `ListTile::build`
/// composes `SafeArea`, which reads `MediaQuery::of` and panics with no ambient
/// `MediaQuery`; the widget-layer harness installs none, so a `ListTile` mounted
/// bare degrades mid-build, the radio below `SafeArea` never mounts, and the
/// resulting `[GenericContainer, Button]` is an artifact of that dead subtree —
/// not a measurement of what a user's tile announces.
///
/// The reference splits a bare tile the same way — by its predicate, traced in
/// `crates/flui-semantics/ARCHITECTURE.md`; no reference test mounts that bare
/// composition, so that half is inference at the tag. Its one-node oracle
/// (`test/material/radio_list_tile_test.dart`, `testWidgets('RadioListTile
/// semantics')`, oracle tag `3.44.0`) is `RadioListTile`'s doing, which wraps its
/// `ListTile` in `MergeSemantics` (`material/radio_list_tile.dart`); the
/// reference's own compatibility predicate still gives the radio a node of its
/// own under the tile, and the merge folds it afterwards. The like-for-like
/// composition is pinned by the next test; this one pins the bare tile.
#[test]
fn a_radio_inside_a_list_tile_announces_as_a_radio_button() {
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            ListTile::new()
                .leading(Radio::new("spring", Some("spring")).on_changed(|_| {}))
                .title(Text::new("Spring"))
                .on_tap(|| {}),
        ),
        tight(400.0, 56.0),
    );
    laid.enable_semantics();
    laid.pump();

    let roles: Vec<Role> = laid
        .a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect();

    assert!(
        roles.contains(&Role::Button),
        "the tile's own tap target announces as a button; losing it would mean the \
         tile stopped publishing `.button(..)`. Roles: {roles:?}"
    );
    assert!(
        roles.contains(&Role::RadioButton),
        "the radio keeps a node of its own (the tile's `.enabled(..)` conflicts with \
         the radio's), and that node is a radio button because `Radio` publishes the \
         mutually-exclusive-group flag. Roles: {roles:?}"
    );
}

/// `MergeSemantics` over the same tile is the reference's own composition —
/// `RadioListTile` wraps its `ListTile` in `MergeSemantics` (`material/
/// radio_list_tile.dart`, oracle tag `3.44.0`) so the whole tile is one
/// interactive entity — and here the tile and the radio share **one** node,
/// as `test/material/radio_list_tile_test.dart`'s `testWidgets('RadioListTile
/// semantics')` asserts of the reference. That merged node carries the tile's
/// `IsButton` beside the radio's checkable flags, so `resolve_role`'s
/// precedence is load-bearing for it: measured, the node resolves `Button`
/// with the checkable arms moved back below `IsButton`, and `RadioButton` with
/// them above it.
///
/// The roles beyond the root are asserted exactly rather than with `contains`,
/// because the claim is the node *count*: a second node would mean the merge
/// did not happen, and a `Button` in its place would mean the precedence
/// regressed. The root's own role is the cascade's fallback for a flag-free
/// node and is not this test's claim, so it is split off rather than pinned. What the merged
/// node still lacks against the oracle — its `'Title'` label, its `tap` action
/// — is recorded in `crates/flui-semantics/ARCHITECTURE.md`, not asserted here.
#[test]
fn merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button() {
    let mut laid = lay_out(
        themed(
            ThemeData::light(),
            MergeSemantics::new().child(
                ListTile::new()
                    .leading(Radio::new("spring", Some("spring")).on_changed(|_| {}))
                    .title(Text::new("Spring"))
                    .on_tap(|| {}),
            ),
        ),
        tight(400.0, 56.0),
    );
    laid.enable_semantics();
    laid.pump();

    let roles: Vec<Role> = laid
        .a11y_tree()
        .expect("semantics enabled before the frame")
        .nodes()
        .map(|node| node.role())
        .collect();

    let (_root, rest) = roles
        .split_first()
        .expect("the a11y tree always carries its root");
    assert_eq!(
        rest,
        [Role::RadioButton],
        "beyond the root, exactly one merged node for the tile and its radio, resolving \
         RadioButton: a Button here means the checkable arms fell below IsButton again, \
         and a second node means the tile and the radio stopped merging. Roles: {roles:?}"
    );
}
