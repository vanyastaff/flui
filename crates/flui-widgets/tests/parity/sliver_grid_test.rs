//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/lib/src/widgets/sliver.dart` — `SliverGrid`
//! (line 739) over `RenderSliverGrid`.
//!
//! Widget → render-object mapping:
//! - `SliverGrid` → `RenderSliverGrid` (sliver child of `RenderViewport`)
//! - `Viewport` → `RenderViewport` (root box)
//!
//! Divergence: Flutter's `SliverGrid` accepts a lazy child delegate; FLUI's
//! `SliverGrid` is eager (all children attached up-front). The geometry
//! behaviour of the eager variant is tested here; the lazy path is deferred.
//!
//! Geometry oracle: with `SliverGridDelegateWithFixedCrossAxisCount(2)` on an
//! 800 × 600 viewport: tile_width = 800 / 2 = 400 px, tile_height = 400 px
//! (aspect_ratio = 1.0). Two rows fit within the 600 px viewport height, so
//! all four tiles are in the visible band and receive a layout call.

use std::sync::Arc;

use flui_view::{BoxedView, ViewExt};
use flui_widgets::{SizedBox, SliverGrid, SliverGridDelegateWithFixedCrossAxisCount, Viewport};

use crate::harness;

/// A 2-column `SliverGrid` with 4 eager children inside a `Viewport` builds
/// the correct render-node count.
///
/// Expected: 1 `RenderViewport` + 1 `RenderSliverGrid` + 4 tile
/// `RenderConstrainedBox` nodes = 6 total.
///
/// Flutter parity: `sliver.dart` `SliverGrid` over `RenderSliverGrid` —
/// eager children attached; delegate computes tile geometry.
#[test]
fn sliver_grid_two_columns_four_tiles_builds_six_render_nodes() {
    let delegate = Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2));
    // Tile children: SizedBox::shrink gives 0×0; the grid delegate overrides
    // their cross/main extents to 400×400 via tight sliver constraints.
    let tiles: Vec<BoxedView> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let root = Viewport::new((SliverGrid::new(delegate, tiles).boxed(),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        6,
        "SliverGrid(2 cols, 4 tiles): expected 6 render nodes \
         (1 RenderViewport + 1 RenderSliverGrid + 4 tile nodes)"
    );
}

/// An empty `SliverGrid` (no children) renders exactly 2 nodes.
///
/// Edge case: the grid delegate is valid but child_count is 0; the sliver
/// reports a zero scroll extent and the viewport renders nothing inside it.
#[test]
fn sliver_grid_empty_children_renders_two_nodes() {
    let delegate = Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(3));
    let root = Viewport::new((SliverGrid::new(delegate, Vec::<BoxedView>::new()).boxed(),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "empty SliverGrid: expected 2 render nodes (1 RenderViewport + 1 RenderSliverGrid)"
    );
}
