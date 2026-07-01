//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/scroll_view.dart`
//!   `CustomScrollView` (line 718)
//! - Tests: `packages/flutter/test/widgets/custom_scroll_view_test.dart`
//!
//! Widget → render-object mapping:
//! - `CustomScrollView` → `RenderViewport` (root box, via the composed `Viewport`)
//! - Each sliver child is wired directly into the viewport as a sliver child.
//!
//! Divergence:
//! - Flutter wraps `CustomScrollView` in a `Directionality`; FLUI's layout
//!   pipeline has no text-direction concept at this level.
//! - Flutter tests cover keyboard scroll, focus, and overscroll physics; those
//!   require `ScrollController` integration not yet exercised in this harness.
//!   Geometry/structure assertions are used here (Phase-2 scope).
//!
//! Geometry oracle: `CustomScrollView` is a pure composition over `Viewport` —
//! the render-node count mirrors what `Viewport::new(slivers)` would produce
//! directly. There is no extra render node from the stateless widget layer.

use flui_view::{BoxedView, ViewExt};
use flui_widgets::{CustomScrollView, SizedBox, SliverFixedExtentList, SliverToBoxAdapter};

use crate::harness;

/// A `CustomScrollView` composing a `SliverToBoxAdapter` (with one box child)
/// followed by a `SliverFixedExtentList` (two items) builds 6 render nodes.
///
/// Expected:
/// - 1 `RenderViewport`
/// - 1 `RenderSliverToBoxAdapter` + 1 `RenderConstrainedBox` (its box child)
/// - 1 `RenderSliverFixedExtentList` + 2 `RenderConstrainedBox` (its items)
///
/// Total: 6 nodes.
///
/// Flutter parity: `custom_scroll_view_test.dart` — `CustomScrollView` with
/// heterogeneous sliver children; the render-node count is the structural
/// evidence that all slivers and their children are wired through the viewport.
#[test]
fn custom_scroll_view_two_slivers_builds_six_render_nodes() {
    let items: Vec<BoxedView> = (0..2).map(|_| SizedBox::shrink().boxed()).collect();
    let root = CustomScrollView::new((
        SliverToBoxAdapter::new().child(SizedBox::shrink()),
        SliverFixedExtentList::new(50.0, items),
    ));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        6,
        "CustomScrollView(SliverToBoxAdapter + SliverFixedExtentList[2]): \
         expected 6 render nodes \
         (1 viewport + 1 STBA + 1 STBA-child + 1 SFEL + 2 SFEL-items)"
    );

    // Structural: the root viewport must be reachable by type name.
    let _viewport = laid.find_by_render_type("RenderViewport");
}

/// A `CustomScrollView` with no slivers builds exactly 1 render node.
///
/// Edge case: an empty sliver sequence is valid; the viewport renders with
/// zero scroll extent and reports a single `RenderViewport`.
#[test]
fn custom_scroll_view_no_slivers_builds_one_render_node() {
    let root = CustomScrollView::new(Vec::<BoxedView>::new());
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        1,
        "empty CustomScrollView: expected exactly 1 render node (RenderViewport)"
    );
}
