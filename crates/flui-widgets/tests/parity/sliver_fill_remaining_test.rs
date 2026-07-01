//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver_fill.dart`
//!   `SliverFillRemaining` (line 270)
//! - Tests: `packages/flutter/test/widgets/sliver_fill_remaining_test.dart`
//!   — "no siblings" (line 70), "one sibling" (line 97)
//!
//! Widget → render-object mapping:
//! - `SliverFillRemaining`              → `RenderSliverFillRemaining`
//!   (Flutter `hasScrollBody = false, fillOverscroll = false`)
//! - `SliverFillRemainingWithScrollable` → `RenderSliverFillRemainingWithScrollable`
//!   (Flutter `hasScrollBody = true`, the default)
//! - `RenderSliverFillRemainingAndOverscroll` exists in `flui-objects` and is
//!   deferred to a future slice (`fillOverscroll = true` path).
//!
//! Divergence:
//! - Flutter's `SliverFillRemaining` is a single widget with `hasScrollBody`
//!   and `fillOverscroll` booleans. FLUI exposes two distinct types, one per
//!   render object variant, making the choice explicit at the type level.
//! - Flutter geometry tests scroll the viewport to verify positioned offsets;
//!   FLUI asserts structural render-node counts (Phase-2 scope, same principle).
//!
//! Geometry oracle (from the "no siblings" oracle test):
//! A `SliverFillRemaining` that is the only child of a 600 px viewport fills
//! the full 600 px. Its scroll extent equals the viewport main-axis extent.
//! The render-node count proof:
//!   1 `RenderViewport` + 1 `RenderSliverFillRemaining` + 1 child box = 3.

use flui_widgets::{
    CustomScrollView, SizedBox, SliverFillRemaining, SliverFillRemainingWithScrollable,
};

use crate::harness;

/// `SliverFillRemaining` with a box child inside a `CustomScrollView` mounts
/// 3 render nodes: viewport + fill-remaining sliver + the box child.
///
/// Flutter parity: `sliver_fill_remaining_test.dart` "no siblings" (line 70) —
/// a fill-remaining with a child fills the full viewport main-axis extent.
/// Node count is the structural evidence that the child is attached.
#[test]
fn sliver_fill_remaining_with_child_builds_three_render_nodes() {
    let root = CustomScrollView::new((SliverFillRemaining::new().child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverFillRemaining(child): expected 3 render nodes \
         (1 RenderViewport + 1 RenderSliverFillRemaining + 1 child box)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverFillRemaining");
}

/// `SliverFillRemaining` with no child mounts 2 render nodes.
///
/// Edge case: childless fill-remaining is valid; the sliver reports
/// `scroll_extent = viewport_main_axis_extent` but paints nothing.
#[test]
fn sliver_fill_remaining_no_child_builds_two_render_nodes() {
    let root = CustomScrollView::new((SliverFillRemaining::new(),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "SliverFillRemaining(no child): expected 2 render nodes \
         (1 RenderViewport + 1 RenderSliverFillRemaining)"
    );
}

/// `SliverFillRemainingWithScrollable` with a box child mounts 3 render nodes.
///
/// Flutter parity: `SliverFillRemaining(hasScrollBody: true)` with a child.
/// This variant sizes the child to `remaining_paint_extent`, not intrinsic
/// extent — the render-node structure is identical to `SliverFillRemaining`
/// but the geometry semantics differ.
#[test]
fn sliver_fill_remaining_with_scrollable_child_builds_three_render_nodes() {
    let root = CustomScrollView::new((
        SliverFillRemainingWithScrollable::new().child(SizedBox::shrink()),
    ));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverFillRemainingWithScrollable(child): expected 3 render nodes \
         (1 RenderViewport + 1 RenderSliverFillRemainingWithScrollable + 1 child box)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverFillRemainingWithScrollable");
}
