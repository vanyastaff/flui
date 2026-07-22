//! U21b — soundness of the borrowed intrinsic walk under a cyclic edge.
//!
//! Regression guard for the SAFETY-GATE finding on
//! `build_intrinsic_child_parent_data` (`subtree_arena.rs`): when a widget
//! queries a child's intrinsic DURING its own `perform_layout`, the query runs
//! the *borrowed* intrinsic walk, which derives a shared `&RenderNode` from a
//! raw `NodePtr` for each of the queried node's children to read their parent
//! data. `links().children()` lists are NOT statically acyclic
//! (`LayoutCycleGuard`, [`layout_cycle_guard`]), so a cyclic edge can name a slot
//! whose `&mut` is live on an ancestor frame. Reading it ungated is aliasing UB
//! (Stacked/Tree Borrows). The fix gates every deref on
//! `SubtreeArena::is_in_flight` and skips in-flight slots.
//!
//! This test constructs exactly that aliasing setup and drives it; under miri
//! it FAILS (UB detected) without the `is_in_flight` gate and passes with it.
//!
//! Refs:
//!   * crates/flui-rendering/src/pipeline/owner/subtree_arena.rs
//!     (`build_intrinsic_child_parent_data`)
//!   * tests/layout_cycle_guard.rs (the layout-cycle siblings)

use flui_foundation::Diagnosticable;
use flui_objects::RenderColoredBox;
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestBehavior,
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    traits::RenderBox,
};
use flui_tree::Single;
use flui_types::{Size, geometry::px};

/// A widget whose `perform_layout` queries child 0's max-intrinsic-width
/// BEFORE laying it out. The intrinsic query routes through the borrowed walk
/// (`box_intrinsic_query_borrowed`) while THIS node's layout in-flight flag is
/// set — the precondition under which the walk must skip an in-flight cyclic
/// child instead of dereferencing it.
#[derive(Debug, Default)]
struct ChildIntrinsicQueryingWidget;

impl Diagnosticable for ChildIntrinsicQueryingWidget {}

impl RenderBox for ChildIntrinsicQueryingWidget {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        // Borrowed intrinsic walk on child 0, fired while this node is in-flight.
        let _ = ctx.child_max_intrinsic_width(0, f32::INFINITY);
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints);
        constraints.smallest()
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        false
    }

    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }
}

/// The querying widget (W1) lays out its child (W2 = a colored box). W2's
/// `children()` list is given a cyclic edge back to W1. During W1's layout
/// (W1 in-flight), W1 queries W2's intrinsic; the borrowed walk on W2 builds
/// W2's per-child parent-data slice, which names W1 — an in-flight ancestor.
/// The walk must skip W1 (`is_in_flight`) rather than alias the live `&mut`.
///
/// Asserts the walk completes without panic or UB. Under miri this is the
/// regression check: it reports aliasing UB without the `is_in_flight` gate.
#[test]
fn u21b_borrowed_intrinsic_walk_skips_in_flight_cyclic_child() {
    let mut pipeline = PipelineOwner::new().into_layout();

    let w1 = pipeline
        .render_tree_mut()
        .insert_box(Box::new(ChildIntrinsicQueryingWidget));
    let w2 = pipeline
        .render_tree_mut()
        .insert_box_child(w1, Box::new(RenderColoredBox::red(20.0, 20.0)))
        .expect("w2 insert");
    // Inject the cycle: W2's children list now names its own ancestor W1.
    pipeline
        .render_tree_mut()
        .get_mut(w2)
        .expect("w2 in tree")
        .add_child(w1);

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    let result = pipeline.layout_dirty_root(w1, constraints);

    assert!(
        result.is_ok(),
        "the borrowed intrinsic walk must skip the in-flight cyclic child \
         (is_in_flight gate) and complete without panic or aliasing UB; got \
         {result:?}",
    );
}
