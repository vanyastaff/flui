//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart`
//!   `SliverOffstage` (line 1488)
//! - Tests: `packages/flutter/test/widgets/sliver_test.dart`
//!
//! Widget → render-object mapping:
//! - `SliverOffstage` → `RenderSliverOffstage` (sliver child of `RenderViewport`)
//! - The sliver child → `RenderConstrainedBox` (box child via `SizedBox::shrink`)
//!
//! Divergence:
//! - Flutter's `SliverOffstage(offstage: true)` defaults to hidden; FLUI
//!   requires an explicit `offstage` argument to `SliverOffstage::new(bool)`,
//!   exposing the choice at the call site.
//! - Flutter verifies geometry strings and semantics-tree absence. FLUI uses
//!   render-node count (Phase-2 scope).
//!
//! Key Flutter parity behavior tested here:
//! - When `offstage = true` the child is **still laid out** (geometry phase
//!   sees `ctx.layout_child(0, constraints)`) and therefore **remains in the
//!   render tree**. Only the reported geometry is collapsed to `ZERO`. This
//!   means `render_node_count` is the same whether hidden or visible.
//!
//! Geometry oracle (offstage = false, one child, 800 × 600 surface):
//!   SliverOffstage is a transparent passthrough: child geometry is forwarded
//!   unchanged. render nodes: 1 RenderViewport + 1 RenderSliverOffstage + 1 child = 3.
//! Geometry oracle (offstage = true):
//!   Same render-node count (child still laid out); geometry reported to
//!   viewport is SliverGeometry::ZERO.

use flui_widgets::{CustomScrollView, SizedBox, SliverOffstage};

use crate::harness;

/// `SliverOffstage(offstage = false)` with a child is a transparent proxy:
/// it mounts 3 render nodes and the child is reachable.
///
/// Flutter parity: `sliver.dart` `SliverOffstage` with `offstage = false` —
/// layout, paint, and hit-test all delegate to the child unchanged.
#[test]
fn sliver_offstage_visible_with_child_builds_three_render_nodes() {
    let root = CustomScrollView::new((SliverOffstage::new(false).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverOffstage(offstage=false, child): expected 3 render nodes \
         (1 RenderViewport + 1 RenderSliverOffstage + 1 child box)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverOffstage");
}

/// `SliverOffstage(offstage = true)` with a child still mounts 3 render nodes.
///
/// Flutter parity: `RenderSliverOffstage.performLayout` always calls
/// `ctx.layout_child(0, constraints)` before returning `SliverGeometry::ZERO`,
/// so the child render object remains in the tree. This test would fail (count
/// = 2) if FLUI incorrectly detached the child on hide.
#[test]
fn sliver_offstage_hidden_child_remains_in_render_tree() {
    let root = CustomScrollView::new((SliverOffstage::new(true).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverOffstage(offstage=true, child): child must remain in render tree \
         (Flutter parity: layout still runs); expected 3 render nodes"
    );
}

/// `SliverOffstage` with no child mounts 2 render nodes regardless of flag.
#[test]
fn sliver_offstage_no_child_builds_two_render_nodes() {
    let root = CustomScrollView::new((SliverOffstage::new(false),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "SliverOffstage(no child): expected 2 render nodes \
         (1 RenderViewport + 1 RenderSliverOffstage)"
    );
}
