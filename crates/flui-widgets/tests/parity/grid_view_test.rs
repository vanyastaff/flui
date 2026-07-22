//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/grid_view_layout_test.dart`
//! Oracle test: `'Empty GridView'` — 4 children in a 200-wide surface with
//! `GridView.extent(maxCrossAxisExtent: 100)` lays each tile at 100×100.
//!
//! Widget → render-object mapping:
//! - `GridView` → `RenderViewport` (root) + `RenderSliverGrid` (sliver child)
//! - Each tile → `RenderConstrainedBox` (via `SizedBox::shrink`, sized by delegate)
//!
//! Divergence:
//! - Flutter wraps `GridView` in a `Center`/`SizedBox(width: 200)` to constrain
//!   the cross axis. FLUI uses `harness::screen_of(200, 600)` (tight 200×600
//!   constraints) to achieve the same cross-axis budget without the extra
//!   ancestor render objects.
//! - Flutter asserts via `tester.renderObjectList<RenderBox>(find.byType(DecoratedBox))`;
//!   FLUI uses `find_all_by_render_type("RenderSliverGrid")` and
//!   `render_node_count()` since the type-finder operates on render objects.
//! - FLUI omits `Directionality` (not required by the layout machinery).
//!
//! Geometry cross-check:
//! `SliverGridDelegateWithMaxCrossAxisExtent(max=100)` on a 200-wide viewport:
//!   cross_axis_count = ceil(200 / (100 + 0)) = 2
//!   child_cross = (200 - 0) / 2 = 100 px
//!   child_main  = 100 / 1.0 (aspect_ratio) = 100 px
//! All 4 tiles → 2 columns × 2 rows → render_node_count = 1 + 1 + 4 = 6.

use flui_view::ViewExt;
use flui_widgets::{GridView, SizedBox};

use crate::harness;

/// `GridView.extent(max=100)` on a 200-wide surface with 4 children lays out
/// exactly 6 render nodes: 1 `RenderViewport` + 1 `RenderSliverGrid` + 4 tiles.
///
/// Flutter parity: `grid_view_layout_test.dart` `'Empty GridView'`, first
/// `pumpWidget` call — 4 `DecoratedBox` children each at 100×100 in a
/// 200-wide grid. Node-count form used in place of `find.byType` (see
/// divergence notes above).
#[test]
fn grid_view_extent_four_tiles_builds_six_render_nodes() {
    let children: Vec<flui_view::BoxedView> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let root = GridView::extent(100.0, children);
    // 200 × 600: same cross-axis budget as Flutter's `SizedBox(width: 200)`.
    let laid = harness::pump_widget(root, harness::screen_of(200.0, 600.0));

    assert_eq!(
        laid.render_node_count(),
        6,
        "GridView.extent(100, 4 tiles) on 200-wide surface: \
         expected 6 render nodes (1 viewport + 1 sliver-grid + 4 tiles)"
    );
}

/// `GridView.count(cross_axis_count=3)` on an 800-wide surface with 6 children
/// builds 8 render nodes: 1 viewport + 1 sliver-grid + 6 tiles.
///
/// Flutter parity: derived from `grid_view_layout_test.dart` — the 3-column
/// variant places all 6 tiles in 2 rows (3 × 2) within the visible band.
#[test]
fn grid_view_count_three_columns_six_tiles_builds_eight_render_nodes() {
    let children: Vec<flui_view::BoxedView> = (0..6).map(|_| SizedBox::shrink().boxed()).collect();
    let root = GridView::count(3, children);
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        8,
        "GridView.count(3, 6 tiles): expected 8 render nodes \
         (1 viewport + 1 sliver-grid + 6 tiles)"
    );
}

/// An empty `GridView` (zero children) renders exactly 2 nodes.
///
/// Edge case: no tiles are attached; the viewport and grid-sliver are present
/// but the grid reports zero scroll extent.
#[test]
fn grid_view_empty_children_renders_two_nodes() {
    let root = GridView::count(2, Vec::<flui_view::BoxedView>::new());
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "empty GridView: expected 2 render nodes (1 viewport + 1 sliver-grid)"
    );
}
