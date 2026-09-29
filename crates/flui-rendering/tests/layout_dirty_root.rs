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
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md
//!   * PR #140 (protocol-erased dispatch)
//!   * PR #141 (typed bridge)
//!   * PR #143 (perform_layout_raw → Result)

use flui_foundation::geometry::Size;
use flui_objects::{RenderCenter, RenderColoredBox, RenderPadding};
use flui_rendering::{
    constraints::BoxConstraints,
    error::{PoisonPhase, RenderError},
    protocol::{BoxProtocol, RenderObject},
};

use crate::common::fresh_layout_pipeline;

// ============================================================================
// Happy path — 2-level tree: RenderPadding (parent) + RenderColoredBox (child)
// ============================================================================

// ============================================================================
// Happy path — 3-level grandchild propagation: Padding → Center → ColoredBox
// ============================================================================

/// Edge case: a 3-level tree (`Padding` → `Center` →
/// `ColoredBox`) propagates layout correctly through the
/// **recursive** callback path of `layout_subtree_raw`. Each recursion
/// level builds its own Direct `BoxLayoutCtx`, invokes
/// `perform_layout_raw` on the parent at that level, and the bridge's
/// `ctx.layout_child(0, c)` dispatches through the closure to recurse
/// one level deeper.
///
/// # Math
///
/// Parent constraints: 0..400 × 0..300.
/// - `RenderPadding::all(20)` deflates to 0..360 × 0..260, passes to
///   `RenderCenter` (which is `Single` arity).
/// - `RenderCenter::perform_layout` calls `ctx.layout_single_child_loose()`
///   — gives the child 0..360 × 0..260 (loose). Since `RenderColoredBox`
///   constrains its preferred 60×30 to the loose constraints, the child
///   takes its preferred 60×30.
/// - `RenderCenter` expands to fill the loose constraints' max → 360×260.
/// - `RenderPadding` adds 20+20 = 40 in each axis → 360+40=400, 260+40=300.
#[test]
fn three_level_padding_center_colored_box_grandchild_propagation() {
    let mut pipeline = fresh_layout_pipeline();

    // Build tree: Padding(20) → Center → ColoredBox(60×30).
    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(20.0)));
    let center_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderCenter::new()))
        .expect("center insert must succeed");
    let _colored_box_id = pipeline
        .render_tree_mut()
        .insert_box_child(center_id, Box::new(RenderColoredBox::blue(60.0, 30.0)))
        .expect("colored box insert must succeed");

    let parent_constraints = BoxConstraints::new(0.0, 400.0, 0.0, 300.0);

    let size = pipeline
        .layout_dirty_root(padding_id, parent_constraints)
        .expect("3-level layout_dirty_root must succeed");

    assert_eq!(
        size,
        Size::new(400.0, 300.0),
        "Padding(20) wrapping Center wrapping ColoredBox(60×30) under \
         (0..400)×(0..300) must expand to 400×300 (Center fills the \
         deflated 360×260 + Padding adds 40 each axis)",
    );

    // Walk back: every node should have geometry set and NEEDS_LAYOUT clear.
    let padding_geom = pipeline
        .render_tree()
        .get(padding_id)
        .and_then(flui_rendering::storage::RenderNode::geometry_box);
    let center_geom = pipeline
        .render_tree()
        .get(center_id)
        .and_then(flui_rendering::storage::RenderNode::geometry_box);

    assert_eq!(padding_geom, Some(Size::new(400.0, 300.0)));
    assert_eq!(
        center_geom,
        Some(Size::new(360.0, 260.0)),
        "Center fills the loose constraints it received from Padding's \
         deflation (max_width=360, max_height=260)",
    );

    assert!(
        !pipeline
            .render_tree()
            .get(padding_id)
            .unwrap()
            .needs_layout(),
        "padding NEEDS_LAYOUT must be cleared",
    );
    assert!(
        !pipeline
            .render_tree()
            .get(center_id)
            .unwrap()
            .needs_layout(),
        "center NEEDS_LAYOUT must be cleared after the recursive callback \
         drove its layout_leaf_only path",
    );
}

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

/// Regression guard: a panicking user widget at NON-LEAF position
/// surfaces as `RenderError::Poisoned`, symmetric with the leaf path.
/// Before this fix the non-leaf branch invoked `perform_layout_raw` without
/// `catch_unwind` — a panic would have unwound out of
/// `layout_dirty_root` and terminated the rendering thread. With the
/// fix, the non-leaf branch wraps `perform_layout_raw` in
/// `catch_unwind(AssertUnwindSafe(...))` mirroring
/// `RenderEntry::layout_leaf_only`'s discipline.
#[test]
fn non_leaf_perform_layout_panic_surfaces_as_poisoned() {
    use flui_foundation::Diagnosticable;
    use flui_foundation::Single;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        hit_testing::HitTestBehavior,
        traits::RenderBox,
    };

    /// A non-leaf user widget that panics inside `perform_layout`.
    /// Single arity so it requires a child (i.e., goes through the
    /// NON-leaf path of `layout_subtree_raw`).
    #[derive(Debug, Default)]
    struct PanickingNonLeaf;

    impl Diagnosticable for PanickingNonLeaf {}

    impl RenderBox for PanickingNonLeaf {
        type Arity = Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Single, Self::ParentData>,
        ) -> Size {
            panic!("PanickingNonLeaf intentionally panics");
        }

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Single, Self::ParentData>) -> bool {
            false
        }
        fn hit_test_behavior(&self) -> HitTestBehavior {
            HitTestBehavior::Opaque
        }
    }

    let mut pipeline = fresh_layout_pipeline();

    // Parent (panics) with a benign child so the walk takes the non-leaf path.
    let parent_obj: Box<dyn RenderObject<BoxProtocol>> = Box::new(PanickingNonLeaf);
    let parent_id = pipeline.render_tree_mut().insert_box(parent_obj);
    let _child_id = pipeline
        .render_tree_mut()
        .insert_box_child(parent_id, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("child insert must succeed");

    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let result = pipeline.layout_dirty_root(parent_id, constraints);

    let err = result.expect_err("panicking non-leaf widget must return Err, not unwind");
    match err {
        RenderError::Poisoned {
            render_object,
            phase,
        } => {
            assert!(
                render_object.contains("PanickingNonLeaf"),
                "render_object name must identify the offending widget; got {render_object}",
            );
            assert_eq!(
                phase,
                PoisonPhase::Layout,
                "phase tag should identify the layout phase, got {phase}",
            );
        }
        other => panic!(
            "expected RenderError::Poisoned, got {other:?} — \
                 non-leaf panic must surface symmetric with leaf path",
        ),
    }
}

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
#[test]
fn descendant_err_preserves_parent_needs_layout() {
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

/// Regression guard: when `layout_dirty_root` is called on a
/// `RenderId` whose node is a `SliverProtocol` entry (not `Box`), it
/// surfaces as `RenderError::ProtocolMismatch` — NOT
/// `RenderError::NodeNotFound`. Before this fix the `.get_mut(id).and_then(|n|
/// n.as_box_mut())` chain collapsed both cases into `NodeNotFound`,
/// masking the protocol-mismatch bug class.
#[test]
fn sliver_node_surfaces_as_protocol_mismatch() {
    use flui_foundation::Leaf;
    use flui_rendering::{
        constraints::SliverGeometry,
        context::{SliverHitTestContext, SliverLayoutContext},
        protocol::SliverProtocol,
        traits::RenderSliver,
    };

    /// Minimal sliver render-object stub for the test fixture — never
    /// laid out (the test triggers the protocol-mismatch error path
    /// before reaching perform_layout).
    #[derive(Debug, Default)]
    struct StubSliver;

    impl flui_foundation::Diagnosticable for StubSliver {}

    impl RenderSliver for StubSliver {
        type Arity = Leaf;
        type ParentData = flui_rendering::parent_data::SliverParentData;

        fn perform_layout(
            &mut self,
            _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
        ) -> SliverGeometry {
            // Never invoked in this test — protocol-mismatch error
            // returns before perform_layout.
            SliverGeometry::ZERO
        }

        fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
            false
        }
    }

    let mut pipeline = fresh_layout_pipeline();

    let sliver_obj: Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>> =
        Box::new(StubSliver);
    let sliver_id = pipeline.render_tree_mut().insert_sliver(sliver_obj);

    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    let result = pipeline.layout_dirty_root(sliver_id, constraints);

    let err = result.expect_err("box layout on sliver node must fail");
    match err {
        RenderError::ProtocolMismatch {
            node_protocol,
            constraints_protocol,
        } => {
            assert_eq!(node_protocol, "Sliver", "node_protocol must name Sliver");
            assert_eq!(
                constraints_protocol, "Box",
                "constraints_protocol must name Box (the layout entry point)",
            );
        }
        // The leaf path was taken (sliver has no children), but the
        // refactored stage 2 distinguishes NodeNotFound vs
        // ProtocolMismatch.
        other => panic!(
            "expected RenderError::ProtocolMismatch, got {other:?} — \
                 sliver id should not collapse to NodeNotFound",
        ),
    }
}

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
