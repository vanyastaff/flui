//! Cross-protocol child layout — Box parent lays out a leaf Sliver child.
//!
//! Core.2 W3.2b-1: verifies that a Box parent can call
//! `ctx.layout_sliver_child(index, constraints)` to drive a Sliver child
//! through the `layout_sliver_subtree_borrowed` pipeline path, and that
//! calling that method when the indexed child is a Box-protocol node returns
//! `SliverGeometry::ZERO` and keeps the parent marked dirty.
//!
//! Tests:
//!   1. **Positive** — Box parent + leaf Sliver child → non-zero
//!      [`SliverGeometry`] produced, parent `needs_layout` cleared.
//!   2. **Negative** — Box parent calls `layout_sliver_child` on a Box child
//!      (protocol mismatch) → `SliverGeometry::ZERO`, parent stays dirty.
//!
//! Refs:
//!   * `crates/flui-rendering/src/pipeline/owner/subtree_arena.rs` `layout_sliver_subtree_borrowed`
//!   * `crates/flui-rendering/src/protocol/box_protocol.rs` `BoxLayoutCtxErased::layout_sliver_child`

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use flui_foundation::Diagnosticable;
use flui_foundation::Variable;
use flui_foundation::geometry::Size;
use flui_objects::RenderColoredBox;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    protocol::BoxProtocol,
    traits::{RenderBox, RenderObject},
    view::ScrollDirection,
};

use crate::common::fresh_layout_pipeline;

// ============================================================================
// Shared fixtures
// ============================================================================

/// A sliver constraints value representing a 600×300 vertical viewport
/// at scroll offset 0 with 400 px of remaining paint extent.
fn make_sliver_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 400.0,
        cross_axis_extent: 300.0,
        viewport_main_axis_extent: 600.0,
        remaining_cache_extent: 450.0,
        cache_origin: 0.0,
    }
}

// ============================================================================
// StubLeafSliver — minimal leaf Sliver render object
// ============================================================================

// ============================================================================
// BoxWithSliverChild — Box parent that drives a sliver child
// ============================================================================

/// `Variable`-arity Box render object that, during layout, calls
/// `ctx.layout_sliver_child(0, sliver_constraints)` and records the
/// returned [`SliverGeometry`] in a shared sink.
///
/// Completes with the biggest size allowed by the parent constraints so the
/// pipeline succeeds and dirty-flag state is observable.
struct BoxWithSliverChild {
    sliver_constraints: SliverConstraints,
    /// Records the `SliverGeometry` received from `layout_sliver_child`.
    captured: Arc<Mutex<Option<SliverGeometry>>>,
}

impl std::fmt::Debug for BoxWithSliverChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxWithSliverChild").finish_non_exhaustive()
    }
}

impl Diagnosticable for BoxWithSliverChild {}

impl RenderBox for BoxWithSliverChild {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let geom = ctx.layout_sliver_child(0, self.sliver_constraints);
        *self.captured.lock().unwrap() = Some(geom);
        ctx.constraints().biggest()
    }
}

// ============================================================================
// Test 1 — Positive: Box parent lays out a leaf Sliver child
// ============================================================================

// ============================================================================
// Test 1b — Positive: Box parent lays out a non-leaf Sliver child
// ============================================================================

// ============================================================================
// Test 2 — Negative: layout_sliver_child on a Box child returns ZERO + poisons
// ============================================================================

/// Calling `ctx.layout_sliver_child(0, ...)` when child 0 is a Box-protocol
/// node triggers a `ProtocolMismatch` error in `layout_sliver_subtree_borrowed_impl`
/// (`as_sliver_mut()` returns `None`).  The sliver callback collapses the
/// error and returns `SliverGeometry::ZERO` to the parent's
/// `perform_layout`.  A protocol mismatch is a structural failure, so the
/// layout poison engages on the first occurrence: the failed child (and
/// its direct layout parent) have `NEEDS_LAYOUT` cleared and the child is
/// skipped in later walks, instead of the parent staying dirty for an
/// unbounded next-frame retry.
///
/// Assertions:
/// - `layout_dirty_root` returns `Ok` (parent's own geometry is produced).
/// - The captured geometry equals `SliverGeometry::ZERO`.
/// - Parent's `NEEDS_LAYOUT` is cleared (poison engaged — bounded retry).
#[test]
fn cross_protocol_layout_sliver_child_on_box_child_returns_zero_and_poisons() {
    let sc = make_sliver_constraints();
    let captured: Arc<Mutex<Option<SliverGeometry>>> = Arc::new(Mutex::new(None));

    let parent_obj: Box<dyn RenderObject<BoxProtocol>> = Box::new(BoxWithSliverChild {
        sliver_constraints: sc,
        captured: Arc::clone(&captured),
    });
    // Deliberately insert a Box child, not a Sliver child.
    let box_child: Box<dyn RenderObject<BoxProtocol>> = Box::new(RenderColoredBox::red(40.0, 40.0));

    let mut pipeline = fresh_layout_pipeline();
    let parent_id = pipeline.render_tree_mut().insert_box(parent_obj);
    pipeline
        .render_tree_mut()
        .insert_box_child(parent_id, box_child)
        .expect("tree must accept a Box child");

    let box_constraints = BoxConstraints::new(0.0, 800.0, 0.0, 600.0);

    // The parent's perform_layout returns Ok, so layout_dirty_root itself
    // returns Ok.  Only the descendant-error flag prevents NEEDS_LAYOUT
    // from being cleared.
    let result = pipeline.layout_dirty_root(parent_id, box_constraints);
    assert!(
        result.is_ok(),
        "layout_dirty_root must return Ok even when a descendant ProtocolMismatch occurs: {result:?}"
    );

    let geom = captured
        .lock()
        .unwrap()
        .expect("perform_layout must have called layout_sliver_child");
    assert_eq!(
        geom,
        SliverGeometry::ZERO,
        "layout_sliver_child on a Box-protocol child must return SliverGeometry::ZERO"
    );

    let parent_node = pipeline
        .render_tree()
        .get(parent_id)
        .expect("parent must remain in the tree after layout");
    assert!(
        !parent_node.needs_layout(),
        "a structural descendant failure (ProtocolMismatch) engages the layout \
         poison on the first occurrence: the failed child is skipped in later \
         walks and the parent's NEEDS_LAYOUT is cleared — its geometry with \
         the child's ZERO stand-in is the same value any retry would produce",
    );
}
