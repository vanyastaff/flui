//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver_fill.dart`
//!   `SliverFillViewport` (line 35)
//! - Tests: `packages/flutter/test/widgets/sliver_fill_viewport_test.dart`
//!   — "SliverFillViewport control test" (line 12), padding test (line 174)
//!
//! Widget → render-object mapping:
//! - `SliverFillViewport` → `RenderSliverFillViewport` (sliver child of
//!   `RenderViewport`)
//! - Each tile → `RenderConstrainedBox` (via `SizedBox::shrink`, sized by
//!   the fill-viewport sliver to `viewport_fraction × viewport_main_axis_extent`)
//!
//! Divergence:
//! - Flutter's `SliverFillViewport` accepts a lazy `SliverChildDelegate`.
//!   FLUI's widget is eager (all children attached up-front). The geometry
//!   behavior for the eager variant is what is tested here; a lazy path is
//!   deferred.
//! - Flutter's oracle test (line 88) asserts on geometry strings from the
//!   debug-dump (`scrollExtent: 12000.0`, 20 children × 600 px). FLUI uses
//!   render-node count and type-name lookup as the structural evidence.
//!
//! Geometry oracle (3 children, viewport_fraction = 1.0, 800 × 600 surface):
//!   item_extent = 600.0 × 1.0 = 600 px each
//!   scroll_extent = 600 × 3 = 1800 px
//!   paint_extent = 600 px (clipped to viewport height)
//!   render nodes: 1 RenderViewport + 1 RenderSliverFillViewport + 3 children = 5

use flui_view::{BoxedView, ViewExt};
use flui_widgets::{CustomScrollView, SizedBox, SliverFillViewport};

use crate::harness;

/// `SliverFillViewport(fraction=1.0)` with 3 children inside a
/// `CustomScrollView` mounts 5 render nodes.
///
/// Flutter parity: `sliver_fill_viewport_test.dart` "SliverFillViewport
/// control test" — 20 children each sized to the viewport extent. This
/// test uses 3 children to keep the count minimal while proving all children
/// are attached.
#[test]
fn sliver_fill_viewport_three_children_builds_five_render_nodes() {
    let children: Vec<BoxedView> = (0..3).map(|_| SizedBox::shrink().boxed()).collect();
    let root = CustomScrollView::new((SliverFillViewport::new(1.0, children),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        5,
        "SliverFillViewport(fraction=1.0, 3 children): expected 5 render nodes \
         (1 RenderViewport + 1 RenderSliverFillViewport + 3 children)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverFillViewport");
}

/// `SliverFillViewport` with no children mounts 2 render nodes.
///
/// Edge case: zero children; the sliver reports zero scroll extent.
#[test]
fn sliver_fill_viewport_no_children_builds_two_render_nodes() {
    let root = CustomScrollView::new((SliverFillViewport::new(1.0, Vec::<BoxedView>::new()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "SliverFillViewport(no children): expected 2 render nodes \
         (1 RenderViewport + 1 RenderSliverFillViewport)"
    );
}

/// `SliverFillViewport` with a fractional viewport (fraction < 1.0) attaches
/// all children regardless of how many fit within the viewport.
///
/// Flutter parity: `sliver_fill_viewport_test.dart` padding test (line 174) —
/// `viewportFraction: 0.5` means each child is 300 px tall (on a 600 px
/// viewport). Two children fit in the visible band. Node count is 1 + 1 + 4 = 6.
#[test]
fn sliver_fill_viewport_fractional_four_children_builds_six_render_nodes() {
    let children: Vec<BoxedView> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let root = CustomScrollView::new((SliverFillViewport::new(0.5, children),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        6,
        "SliverFillViewport(fraction=0.5, 4 children): expected 6 render nodes \
         (1 RenderViewport + 1 RenderSliverFillViewport + 4 children)"
    );
}
