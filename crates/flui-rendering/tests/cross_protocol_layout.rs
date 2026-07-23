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
//!   * `crates/flui-rendering/src/pipeline/owner.rs` `layout_sliver_subtree_borrowed`
//!   * `crates/flui-rendering/src/protocol/box_protocol.rs` `BoxLayoutCtxErased::layout_sliver_child`

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use flui_foundation::Diagnosticable;
use flui_objects::{RenderColoredBox, RenderSliverPadding};
use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxLayoutContext, SliverHitTestContext, SliverLayoutContext},
    parent_data::{BoxParentData, SliverParentData},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    traits::{RenderBox, RenderObject, RenderSliver},
    view::ScrollDirection,
};
use flui_tree::{Leaf, Variable};
use flui_types::{Size, geometry::px, layout::AxisDirection};

// ============================================================================
// Shared fixtures
// ============================================================================

/// Pipeline owner in the Layout phase.
fn fresh_layout_pipeline() -> PipelineOwner<flui_rendering::pipeline::Layout> {
    PipelineOwner::new().into_layout()
}

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

/// Leaf sliver that reports a deterministic geometry: 200 px scroll extent,
/// paint extent clamped to remaining paint extent. Used to confirm that the
/// cross-protocol path actually calls into the sliver's `perform_layout`.
#[derive(Debug, Default)]
struct StubLeafSliver;

impl Diagnosticable for StubLeafSliver {}

impl RenderSliver for StubLeafSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let paint = 200.0_f32.min(constraints.remaining_paint_extent);
        SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: paint,
            layout_extent: paint,
            max_paint_extent: 200.0,
            hit_test_extent: paint,
            visible: paint > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }
}

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

/// Box parent with a leaf Sliver child drives `layout_sliver_subtree_borrowed`
/// and receives the child's non-zero [`SliverGeometry`].
///
/// `layout_dirty_root` must return `Ok`, the captured geometry must have
/// `scroll_extent = 200.0`, and the parent's `needs_layout` must be cleared.
#[test]
fn cross_protocol_box_parent_lays_out_leaf_sliver_child() {
    let sc = make_sliver_constraints();
    let captured: Arc<Mutex<Option<SliverGeometry>>> = Arc::new(Mutex::new(None));

    let parent_obj: Box<dyn RenderObject<BoxProtocol>> = Box::new(BoxWithSliverChild {
        sliver_constraints: sc,
        captured: Arc::clone(&captured),
    });
    let sliver_obj: Box<dyn RenderObject<SliverProtocol>> = Box::new(StubLeafSliver);

    let mut pipeline = fresh_layout_pipeline();
    let parent_id = pipeline.render_tree_mut().insert_box(parent_obj);
    pipeline
        .render_tree_mut()
        .insert_sliver_child(parent_id, sliver_obj)
        .expect("tree must accept a Sliver child under a Box parent");

    let box_constraints = BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0));
    let result = pipeline.layout_dirty_root(parent_id, box_constraints);
    assert!(
        result.is_ok(),
        "layout_dirty_root must succeed for Box parent + leaf Sliver child: {result:?}"
    );

    let geom = captured
        .lock()
        .unwrap()
        .expect("perform_layout must have called layout_sliver_child and stored the result");

    assert_eq!(
        geom.scroll_extent, 200.0,
        "StubLeafSliver always reports scroll_extent=200.0; captured geometry must match"
    );
    assert!(
        geom.paint_extent > 0.0,
        "remaining_paint_extent=400.0 so StubLeafSliver's paint_extent (min(200,400)=200) must be >0"
    );

    let parent_node = pipeline
        .render_tree()
        .get(parent_id)
        .expect("parent must remain in the tree after layout");
    assert!(
        !parent_node.needs_layout(),
        "parent NEEDS_LAYOUT must be cleared after successful cross-protocol layout"
    );
}

// ============================================================================
// Test 1b — Positive: Box parent lays out a non-leaf Sliver child
// ============================================================================

/// Box parent with a `RenderSliverPadding` child drives the sliver non-leaf
/// walk: the padding sliver must lay out its own leaf sliver child, compose
/// the padded geometry, and return that geometry to the Box parent.
///
/// This is the next Core.2 step after W3.2b-1's leaf-only sliver bridge. On
/// the leaf-only bridge this regresses to `SliverGeometry::ZERO` and leaves
/// the Box parent dirty because `RenderSliverPadding` is gated as non-leaf.
#[test]
fn cross_protocol_box_parent_lays_out_sliver_padding_with_leaf_child() {
    let sc = make_sliver_constraints();
    let captured: Arc<Mutex<Option<SliverGeometry>>> = Arc::new(Mutex::new(None));

    let parent_obj: Box<dyn RenderObject<BoxProtocol>> = Box::new(BoxWithSliverChild {
        sliver_constraints: sc,
        captured: Arc::clone(&captured),
    });
    let padding_obj: Box<dyn RenderObject<SliverProtocol>> =
        Box::new(RenderSliverPadding::symmetric(0.0, 10.0));
    let leaf_obj: Box<dyn RenderObject<SliverProtocol>> = Box::new(StubLeafSliver);

    let mut pipeline = fresh_layout_pipeline();
    let parent_id = pipeline.render_tree_mut().insert_box(parent_obj);
    let padding_id = pipeline
        .render_tree_mut()
        .insert_sliver_child(parent_id, padding_obj)
        .expect("tree must accept RenderSliverPadding under a Box parent");
    pipeline
        .render_tree_mut()
        .insert_sliver_child(padding_id, leaf_obj)
        .expect("tree must accept a leaf Sliver child under RenderSliverPadding");

    let box_constraints = BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0));
    let result = pipeline.layout_dirty_root(parent_id, box_constraints);
    assert!(
        result.is_ok(),
        "layout_dirty_root must succeed for Box parent -> SliverPadding -> leaf Sliver: {result:?}"
    );

    let geom = captured
        .lock()
        .unwrap()
        .expect("perform_layout must have called layout_sliver_child");

    assert_eq!(
        geom.scroll_extent, 220.0,
        "10px top + 10px bottom padding must add 20px to the leaf's 200px scroll extent"
    );
    assert_eq!(
        geom.paint_extent, 220.0,
        "remaining_paint_extent=400 gives enough room for 200px leaf + 20px padding"
    );
    assert_eq!(
        geom.layout_extent, 220.0,
        "layout extent should match the fully visible padded extent"
    );

    let parent_node = pipeline
        .render_tree()
        .get(parent_id)
        .expect("parent must remain in the tree after layout");
    assert!(
        !parent_node.needs_layout(),
        "parent NEEDS_LAYOUT must be cleared after successful non-leaf sliver layout"
    );
    let padding_node = pipeline
        .render_tree()
        .get(padding_id)
        .expect("padding sliver must remain in the tree after layout");
    assert!(
        !padding_node.needs_layout(),
        "RenderSliverPadding NEEDS_LAYOUT must be cleared after its child layout succeeds"
    );
}

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

    let box_constraints = BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0));

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
