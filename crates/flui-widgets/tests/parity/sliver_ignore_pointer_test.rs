//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart`
//!   `SliverIgnorePointer` (line 1420)
//! - Tests: `packages/flutter/test/widgets/sliver_test.dart`
//!
//! Widget → render-object mapping:
//! - `SliverIgnorePointer` → `RenderSliverIgnorePointer` (sliver child of
//!   `RenderViewport`)
//! - The sliver child → `RenderConstrainedBox` (box child via `SizedBox::shrink`)
//!
//! Divergence:
//! - Flutter's `SliverIgnorePointer` has an optional `ignoringSemantics` field
//!   (defaults to `null`, inheriting from `ignoring`). FLUI defers semantics-tree
//!   coordination to the semantics-pipeline workstream; the pointer toggle is a
//!   self-contained `bool` here.
//! - Flutter verifies pointer routing via `tester.hitTest`; FLUI asserts on
//!   render-node structure (Phase-2 scope; hit-test dispatch is exercised in the
//!   interaction-widget suite).
//!
//! Key Flutter parity behavior tested here:
//! - `SliverIgnorePointer` is a pure layout/paint passthrough; it only
//!   short-circuits `hit_test`. The render tree is identical whether
//!   `ignoring = true` or `false`.
//!
//! Geometry oracle (one child, 800 × 600 surface):
//!   RenderSliverIgnorePointer delegates layout and paint to its single child;
//!   render nodes: 1 RenderViewport + 1 RenderSliverIgnorePointer + 1 child = 3.

use flui_widgets::{CustomScrollView, SizedBox, SliverIgnorePointer};

use crate::harness;

/// `SliverIgnorePointer(ignoring = false)` with a child is a transparent
/// layout/paint proxy: 3 render nodes, child fully reachable.
///
/// Flutter parity: `sliver.dart` `SliverIgnorePointer` with `ignoring = false`
/// — layout, paint, and hit-test delegate to the child unchanged.
#[test]
fn sliver_ignore_pointer_transparent_with_child_builds_three_render_nodes() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(false).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverIgnorePointer(ignoring=false, child): expected 3 render nodes \
         (1 RenderViewport + 1 RenderSliverIgnorePointer + 1 child box)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverIgnorePointer");
}

/// `SliverIgnorePointer(ignoring = true)` still mounts 3 render nodes.
///
/// Flutter parity: `RenderSliverIgnorePointer` unconditionally delegates
/// layout and paint; only `hit_test` returns `false`. The child is always in
/// the render tree. This test would fail (count = 2) if FLUI incorrectly
/// removed the child when ignoring is active.
#[test]
fn sliver_ignore_pointer_ignoring_child_remains_in_render_tree() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(true).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverIgnorePointer(ignoring=true, child): child must remain in render \
         tree (layout/paint are passthroughs); expected 3 render nodes"
    );
}

/// `SliverIgnorePointer` with no child mounts 2 render nodes.
#[test]
fn sliver_ignore_pointer_no_child_builds_two_render_nodes() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(true),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "SliverIgnorePointer(no child): expected 2 render nodes \
         (1 RenderViewport + 1 RenderSliverIgnorePointer)"
    );
}
