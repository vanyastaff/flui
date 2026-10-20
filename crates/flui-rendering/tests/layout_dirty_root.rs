//! `PipelineOwner::layout_dirty_root` disjoint-borrow walk integration tests.
//!
//! These exercise the **production layout path**:
//! [`PipelineOwner::layout_dirty_root`] drives `perform_layout_raw`
//! against a Direct-storage [`BoxLayoutCtx`] populated with the
//! parent's child IDs + a recursive [`layout_subtree_raw`] callback. The
//! tests cover the 2-level happy path (parent + leaf child), the
//! 3-level grandchild path (Padding → Center → ColoredBox), and the
//! failure path (NodeNotFound on a stale root id).
//!
//! A broader integration suite (`tests/pipeline/layout_pipeline_test.rs`)
//! covers more ground; this file is a smoke-level verification of the
//! disjoint-borrow walk itself.
//!
//! Refs:
//!   * PR #140 (protocol-erased dispatch)
//!   * PR #141 (typed bridge)
//!   * PR #143 (perform_layout_raw → Result)

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::constraints::BoxConstraints;

use crate::common::fresh_layout_pipeline;

// ============================================================================
// Happy path — 2-level tree: RenderPadding (parent) + RenderColoredBox (child)
// ============================================================================

// ============================================================================
// Happy path — 3-level grandchild propagation: Padding → Center → ColoredBox
// ============================================================================

// ============================================================================
// Failure path — stale root id surfaces RenderError::NodeNotFound
// ============================================================================

// ============================================================================
// Smoke — leaf path: layout_dirty_root on a node with no children
// delegates to RenderEntry::layout_leaf_only.
// ============================================================================

// ============================================================================
// Idempotence — re-running layout on a clean tree returns the same size
// (interaction with an earlier OnceCell→Option state-storage migration:
// no panic on frame 2).
// ============================================================================

// ============================================================================
// Manual RenderObject<BoxProtocol> impls (RenderViewAdapter): the layout walk
// still drives perform_layout_raw on them via the trait dispatch.
// ============================================================================

// ============================================================================
// Review-fix regression — non-leaf perform_layout panic surfaces as Poisoned
// ============================================================================

// ============================================================================
// Review-fix regression — descendant Err preserves parent NEEDS_LAYOUT
// ============================================================================

/// Regression guard: when the recursive callback observes a
/// descendant `Err`, the outer parent's `NEEDS_LAYOUT` must STAY SET
/// so the next dirty walk re-runs the subtree. Pre-fix the parent
/// was unconditionally `clear_needs_layout`'d after a successful
/// `perform_layout_raw`, leaving the parent CLEAN with stale geometry
/// derived from the `Size::ZERO` returned by the swallowed-error
/// callback.
///
/// Trigger: `RenderPadding` (Single arity, non-leaf) with a child id
/// that points to a node which was inserted but then removed from the
/// tree before the layout walk. The recursive callback's stage 1
/// snapshot reads the stale id from `parent.children()`, then the
/// stage 4 `(*ptr).get_mut(stale_id)` returns `None` →
/// `RenderError::NodeNotFound`. Callback swallows to `Size::ZERO` +
/// flips the `descendant_error_flag`. Padding's perform_layout
/// completes (0+padding = small size); stage 6 skips
/// `clear_needs_layout` per the flag.
pub(crate) fn descendant_err_preserves_parent_needs_layout() {
    let mut pipeline = fresh_layout_pipeline();

    // Build Padding → Child. Insert both, then REMOVE the child from
    // the slab (without removing the link from Padding.children()) —
    // simulates a torn tree state where parent.children() contains a
    // stale id.
    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let child_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(20.0, 20.0)))
        .expect("child insert must succeed");

    // Remove the child node from the slab WITHOUT updating the
    // parent's children list — this synthesises the stale-id condition.
    // (In production this shouldn't happen; the test deliberately
    // constructs it.)
    pipeline
        .render_tree_mut()
        .remove_shallow(child_id)
        .expect("shallow remove must succeed");
    // remove_shallow also strips the child from parent.children() per
    // its impl, so we have to re-add it manually to simulate the stale
    // link.
    let padding_node = pipeline
        .render_tree_mut()
        .get_mut(padding_id)
        .expect("padding node must exist");
    padding_node.add_child(child_id);

    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let result = pipeline.layout_dirty_root(padding_id, constraints);

    // Outer call SUCCEEDS (Padding's perform_layout completes with
    // child_size = Size::ZERO from the swallowed callback Err).
    let size = result.expect("padding perform_layout completes even when child layout fails");
    // Padding(all=5) wrapping a Size::ZERO child = 10×10; constrained
    // to tight (100, 100) constraints clamps it back to 100×100. Either
    // way, the test only cares about NEEDS_LAYOUT preservation; assert
    // size is non-NaN as a sanity check.
    assert!(size.width.is_finite() && size.height.is_finite());

    // CRITICAL ASSERTION: padding's NEEDS_LAYOUT must STAY SET because
    // the descendant errored during the walk (pre-fix this would have
    // been cleared, leading to stale layout persisting indefinitely).
    let padding_node = pipeline
        .render_tree()
        .get(padding_id)
        .expect("padding node must still exist");
    assert!(
        padding_node.needs_layout(),
        "padding NEEDS_LAYOUT must remain SET when a descendant errored \
             during the walk, so the next dirty pass re-runs the subtree",
    );
}

// ============================================================================
// Review-fix regression — Sliver protocol mismatch surfaces as ProtocolMismatch
// ============================================================================

// ============================================================================
// SubtreeArena thread-affinity smoke
// ============================================================================

// `subtree_borrows_worker_thread_pipeline_succeeds` used to prove that
// handing a freshly-built, not-yet-shared `PipelineOwner` to a worker
// thread and running layout there was a legitimate pattern -- as long as
// `SubtreeArena` was constructed and queried on that same worker thread.
// It no longer compiles, and that is the point: `PipelineOwner` stores a
// `RenderTree` holding `Box<dyn RenderObject<P>>`, which is no longer
// `Send` (a render object may now hold non-`Send` state). Moving a
// `PipelineOwner` -- by value, not just by shared reference -- to any
// thread other than the one it was created on is a compile error, full
// stop; "single-owner, single-thread" is no longer a convention a runtime
// check (`SubtreeArena::check_thread`, since deleted along with `NodePtr`'s
// manual `unsafe impl Send/Sync` -- confinement needs no runtime check
// when the type system already refuses to compile the violation) merely
// encouraged, it is the only shape the type system allows. There is no
// successor test: the capability this test exercised (worker-thread
// layout of an unshared owner) is gone by design, not replaced.

// ============================================================================
// 4-level deep recursion (verifies pre-acquired subtree borrows
// scale to deeper trees than the original thread-affinity tests covered)
// ============================================================================

// ============================================================================
// Dirty-root order — element-mounted nodes lay out parents first
// ============================================================================

/// Lays its child out tight at its own size less a shared inset, so a new
/// inset hands the child new constraints.
#[derive(Debug)]
struct InsetParent(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl flui_foundation::Diagnosticable for InsetParent {}

impl flui_rendering::traits::RenderBox for InsetParent {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let size = ctx.constraints().biggest();
        let inset = f64::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed));
        let child = Size::new(size.width - inset, size.height - inset);
        ctx.layout_child(0, BoxConstraints::tight(child))?;
        Ok(size)
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }
}

/// Counts its layouts.
#[derive(Debug)]
struct CountingLeaf(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl flui_foundation::Diagnosticable for CountingLeaf {}

impl flui_rendering::traits::RenderBox for CountingLeaf {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(ctx.constraints().biggest())
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }
}

/// Nodes mounted the way elements mount them (inserted parentless, then
/// adopted) carry their tree depth, so a relayout boundary marked before its
/// ancestor still lays out after it, once, against the ancestor's new
/// constraints.
pub(crate) fn adopted_nodes_lay_out_parents_before_children() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    use crate::common::BoxedRenderObject;

    let no_inset = || Arc::new(AtomicU64::new(0.0_f64.to_bits()));
    let inset = no_inset();
    let leaf_layouts = Arc::new(AtomicUsize::new(0));
    let mut owner = flui_rendering::pipeline::PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    );
    let root = owner.insert(Box::new(InsetParent(no_inset())) as BoxedRenderObject);
    let a = owner.insert(Box::new(InsetParent(Arc::clone(&inset))) as BoxedRenderObject);
    let b = owner.insert(Box::new(InsetParent(no_inset())) as BoxedRenderObject);
    let c = owner.insert(Box::new(CountingLeaf(Arc::clone(&leaf_layouts))) as BoxedRenderObject);
    owner.adopt_render_child(root, a);
    owner.adopt_render_child(a, b);
    owner.adopt_render_child(b, c);
    let depths: Vec<_> = [root, a, b, c]
        .iter()
        .map(|&id| owner.render_tree().depth(id))
        .collect();
    assert_eq!(depths, [Some(0), Some(1), Some(2), Some(3)]);

    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("first layout");
    assert_eq!(leaf_layouts.load(Ordering::Relaxed), 1);

    owner.mark_needs_layout(c);
    inset.store(10.0_f64.to_bits(), Ordering::Relaxed);
    owner.mark_needs_layout(a);
    owner.run_layout().expect("relayout");

    assert_eq!(
        leaf_layouts.load(Ordering::Relaxed),
        2,
        "the leaf lays out once, under its ancestor's new constraints"
    );
    let leaf_size = owner
        .render_tree()
        .get(c)
        .and_then(|node| node.as_box())
        .map(|entry| entry.state().size());
    assert_eq!(leaf_size, Some(Size::new(90.0, 90.0)));
}
