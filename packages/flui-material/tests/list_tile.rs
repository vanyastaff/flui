//! `ListTile` widget-level integration coverage — mounts a real `ListTile`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/ink_well.rs` use) and proves the whole-tile tap
//! target, the `enabled`/theme cascades, and slot presence/absence actually
//! reach a mounted tree, not just `resolve_style` computed in isolation.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{ListTile, Radio, Theme, ThemeData};
use flui_sdk::view::IntoView;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, MergeSemantics, Text};
use flui_testing::a11y::Role;

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
            ListTile::new()
                .title(Text::new("Inbox"))
                .on_tap(move |_cx| {
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
    laid.dispatch_pointer_down(origin.dx + 2.0, origin.dy + 2.0);
    laid.dispatch_pointer_up(origin.dx + 2.0, origin.dy + 2.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap inside the content padding (not on the title's own glyphs) must still fire \
         on_tap — the whole mounted tile, not just the title, is the tap target"
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
                    .leading(Radio::new("spring", Some("spring")).on_changed(|_cx, _| {}))
                    .title(Text::new("Spring"))
                    .on_tap(|_cx| {}),
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
