//! D-block PR-A1b3 U20 — `PipelineOwner::layout_dirty_root` disjoint-borrow
//! walk integration tests.
//!
//! These exercise the **production layout path** introduced in U20:
//! [`PipelineOwner::layout_dirty_root`] drives `perform_layout_raw`
//! against a Direct-storage [`BoxLayoutCtx`] populated with the
//! parent's child IDs + a recursive [`layout_subtree_raw`] callback. The
//! tests cover the 2-level happy path (parent + leaf child), the
//! 3-level grandchild path (Padding → Center → ColoredBox), and the
//! failure path (NodeNotFound on a stale root id).
//!
//! The full integration suite (`tests/pipeline/layout_pipeline_test.rs`
//! per plan §U25/§U26) lands in PR-A1c; this file is the U20-local smoke
//! verification.
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md §U20
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md §D1
//!   * PR #140 (U18 — protocol-erased dispatch)
//!   * PR #141 (U19 — typed bridge)
//!   * PR #143 (perform_layout_raw → Result)

use flui_foundation::RenderId;
use flui_rendering::{
    constraints::BoxConstraints,
    error::RenderError,
    objects::{RenderCenter, RenderColoredBox, RenderPadding},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, RenderObject},
};
use flui_types::{Size, geometry::px};

// ============================================================================
// Helpers
// ============================================================================

/// Builds a fresh pipeline owner in the Layout phase. Tests construct
/// the tree via `render_tree_mut` (phase-agnostic accessor) before
/// invoking [`PipelineOwner::layout_dirty_root`].
fn fresh_layout_pipeline() -> PipelineOwner<flui_rendering::pipeline::Layout> {
    PipelineOwner::new().into_layout()
}

// ============================================================================
// Happy path — 2-level tree: RenderPadding (parent) + RenderColoredBox (child)
// ============================================================================

/// Plan §U20 happy path: a 2-level tree (`Padding` wrapping a single
/// `ColoredBox`) lays out correctly through `layout_dirty_root`. The
/// padding deflates parent constraints by 20 (left=right=10) on each
/// axis, the colored box sizes to its preferred (80×40) clipped to the
/// deflated constraints, and the padding wraps it with the configured
/// insets to produce a final 100×60 box.
///
/// This exercises the **non-leaf path** of `layout_subtree_raw`:
/// pipeline builds the Direct `BoxLayoutCtx` with `child_ids =
/// [colored_box_id]` and a recursive callback, the trait-erased bridge
/// in `traits/render_box.rs` reconstructs the typed
/// `BoxLayoutCtx<Single, BoxParentData>` for `RenderPadding`, and
/// `RenderPadding::perform_layout` calls `ctx.layout_child(0,
/// deflated)` which invokes the callback → recursive
/// `layout_subtree_raw(colored_box_id, deflated)` → leaf path →
/// `RenderEntry::layout_leaf_only` produces the child size.
#[test]
fn u20_two_level_padding_with_colored_box_child() {
    let mut pipeline = fresh_layout_pipeline();

    // Build tree: Padding(all=10) → ColoredBox(preferred 80×40).
    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(10.0)));
    let _colored_box_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(80.0, 40.0)))
        .expect("child insert must succeed");

    // Parent constraints: loose 0..300 × 0..200 — leaves room for the
    // 80+20=100 wide, 40+20=60 tall padded box.
    let parent_constraints = BoxConstraints::new(px(0.0), px(300.0), px(0.0), px(200.0));

    let size = pipeline
        .layout_dirty_root(padding_id, parent_constraints)
        .expect("2-level layout_dirty_root must succeed");

    assert_eq!(
        size,
        Size::new(px(100.0), px(60.0)),
        "Padding(10) wrapping ColoredBox(80×40) must produce (80+20)×(40+20) = 100×60",
    );

    // Padding state should be populated post-layout.
    let padding_node = pipeline
        .render_tree()
        .get(padding_id)
        .expect("padding node still in tree");
    assert_eq!(
        padding_node.geometry_box(),
        Some(Size::new(px(100.0), px(60.0))),
        "padding's stored geometry must match the returned size",
    );
    assert!(
        !padding_node.needs_layout(),
        "NEEDS_LAYOUT must be cleared after successful layout",
    );
}

// ============================================================================
// Happy path — 3-level grandchild propagation: Padding → Center → ColoredBox
// ============================================================================

/// Plan §U20 edge case: a 3-level tree (`Padding` → `Center` →
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
fn u20_three_level_padding_center_colored_box_grandchild_propagation() {
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

    let parent_constraints = BoxConstraints::new(px(0.0), px(400.0), px(0.0), px(300.0));

    let size = pipeline
        .layout_dirty_root(padding_id, parent_constraints)
        .expect("3-level layout_dirty_root must succeed");

    assert_eq!(
        size,
        Size::new(px(400.0), px(300.0)),
        "Padding(20) wrapping Center wrapping ColoredBox(60×30) under \
         (0..400)×(0..300) must expand to 400×300 (Center fills the \
         deflated 360×260 + Padding adds 40 each axis)",
    );

    // Walk back: every node should have geometry set and NEEDS_LAYOUT clear.
    let padding_geom = pipeline
        .render_tree()
        .get(padding_id)
        .and_then(|n| n.geometry_box());
    let center_geom = pipeline
        .render_tree()
        .get(center_id)
        .and_then(|n| n.geometry_box());

    assert_eq!(padding_geom, Some(Size::new(px(400.0), px(300.0))));
    assert_eq!(
        center_geom,
        Some(Size::new(px(360.0), px(260.0))),
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

/// Plan §U20 failure path (adapted): `layout_dirty_root` invoked on a
/// `RenderId` that doesn't exist in the tree returns
/// `Err(RenderError::NodeNotFound)` rather than panicking.
///
/// The plan's literal phrasing names `ChildIndexOutOfBounds` for the
/// "child slice access out of bounds" case, but the U20 implementation
/// surfaces that condition via `NodeNotFound` instead: the callback
/// iterates over the parent's snapshotted `child_ids` (always within
/// bounds by construction), so a stale child-id surfaces as a
/// downstream `get(child_id) -> None` in the recursive call — i.e.,
/// `NodeNotFound`. `ChildIndexOutOfBounds` is reserved (see the variant
/// constructor doc) for future defensive checks; this test covers the
/// shape that the current implementation actually produces.
#[test]
fn u20_stale_root_id_returns_node_not_found() {
    let mut pipeline = fresh_layout_pipeline();

    let stale_id = RenderId::new(999); // never inserted

    let result = pipeline.layout_dirty_root(
        stale_id,
        BoxConstraints::tight(Size::new(px(100.0), px(100.0))),
    );

    let err = result.expect_err("layout on a non-existent id must fail");
    assert!(
        matches!(err, RenderError::NodeNotFound(id) if id == stale_id),
        "expected NodeNotFound({stale_id:?}), got {err:?}",
    );
}

// ============================================================================
// Smoke — leaf path: layout_dirty_root on a node with no children
// delegates to RenderEntry::layout_leaf_only.
// ============================================================================

/// Sanity: when `id`'s child list is empty, `layout_dirty_root` should
/// route through `RenderEntry::layout_leaf_only` and return the
/// constraint-clamped size — same path the U18/U19 leaf bridge tests
/// already cover, exercised here through the U20 entry point to prove
/// the leaf branch is reachable from the new public API.
#[test]
fn u20_leaf_path_delegates_to_layout_leaf_only() {
    let mut pipeline = fresh_layout_pipeline();

    let leaf_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderColoredBox::green(120.0, 50.0)));

    let constraints = BoxConstraints::tight(Size::new(px(120.0), px(50.0)));
    let size = pipeline
        .layout_dirty_root(leaf_id, constraints)
        .expect("leaf-only layout_dirty_root must succeed");

    assert_eq!(size, Size::new(px(120.0), px(50.0)));

    let geom = pipeline
        .render_tree()
        .get(leaf_id)
        .and_then(|n| n.geometry_box());
    assert_eq!(geom, Some(Size::new(px(120.0), px(50.0))));
}

// ============================================================================
// Idempotence — re-running layout on a clean tree returns the same size
// (interaction with U14's OnceCell→Option migration: no panic on frame 2).
// ============================================================================

/// Frame-2 regression smoke: U14 swapped `RenderState::set_constraints` /
/// `set_geometry` from `OnceCell` (panic on re-set) to `Option` (replace
/// on re-set). Two consecutive `layout_dirty_root` calls on the same
/// tree must succeed without panic — the new walk goes through the same
/// `set_*` path U14 fixed.
///
/// Covers the AE8 surface (frame-2 no panic) at the U20 entry point;
/// the full AE8 integration test lands in U29.
#[test]
fn u20_double_layout_does_not_panic_on_frame_two() {
    let mut pipeline = fresh_layout_pipeline();

    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let _child_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert must succeed");

    let c = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));

    let size1 = pipeline
        .layout_dirty_root(padding_id, c)
        .expect("frame 1 must succeed");
    let size2 = pipeline
        .layout_dirty_root(padding_id, c)
        .expect("frame 2 must succeed without OnceCell-style panic");

    assert_eq!(
        size1, size2,
        "deterministic layout: frame 1 and frame 2 must agree",
    );
    assert_eq!(size1, Size::new(px(50.0), px(50.0)));
}

// ============================================================================
// Manual RenderObject<BoxProtocol> impls (RenderViewAdapter): the U20 walk
// still drives perform_layout_raw on them via the trait dispatch.
// ============================================================================

// ============================================================================
// Review-fix regression — non-leaf perform_layout panic surfaces as Poisoned
// ============================================================================

/// PR-A1b3 review fix: a panicking user widget at NON-LEAF position
/// surfaces as `RenderError::Poisoned`, symmetric with the leaf path.
/// Pre-fix the non-leaf branch invoked `perform_layout_raw` without
/// `catch_unwind` — a panic would have unwound out of
/// `layout_dirty_root` and terminated the rendering thread. With the
/// fix, the non-leaf branch wraps `perform_layout_raw` in
/// `catch_unwind(AssertUnwindSafe(...))` mirroring
/// `RenderEntry::layout_leaf_only`'s discipline.
#[test]
fn u20_non_leaf_perform_layout_panic_surfaces_as_poisoned() {
    use flui_foundation::Diagnosticable;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        hit_testing::HitTestBehavior,
        traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
    };
    use flui_tree::Single;

    /// A non-leaf user widget that panics inside `perform_layout`.
    /// Single arity so it requires a child (i.e., goes through the
    /// NON-leaf path of `layout_subtree_raw`).
    #[derive(Debug, Default)]
    struct PanickingNonLeaf;

    impl Diagnosticable for PanickingNonLeaf {}
    impl PaintEffectsCapability for PanickingNonLeaf {}
    impl SemanticsCapability for PanickingNonLeaf {}
    impl HotReloadCapability for PanickingNonLeaf {}

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

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
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
                phase, "layout",
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

/// PR-A1b3 review fix: when the recursive callback observes a
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
fn u20_descendant_err_preserves_parent_needs_layout() {
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

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    let result = pipeline.layout_dirty_root(padding_id, constraints);

    // Outer call SUCCEEDS (Padding's perform_layout completes with
    // child_size = Size::ZERO from the swallowed callback Err).
    let size = result.expect("padding perform_layout completes even when child layout fails");
    // Padding(all=5) wrapping a Size::ZERO child = 10×10; constrained
    // to tight (100, 100) constraints clamps it back to 100×100. Either
    // way, the test only cares about NEEDS_LAYOUT preservation; assert
    // size is non-NaN as a sanity check.
    assert!(size.width.get().is_finite() && size.height.get().is_finite());

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

/// PR-A1b3 review fix: when `layout_dirty_root` is called on a
/// `RenderId` whose node is a `SliverProtocol` entry (not `Box`), it
/// surfaces as `RenderError::ProtocolMismatch` — NOT
/// `RenderError::NodeNotFound`. Pre-fix the `.get_mut(id).and_then(|n|
/// n.as_box_mut())` chain collapsed both cases into `NodeNotFound`,
/// masking the protocol-mismatch bug class.
#[test]
fn u20_sliver_node_surfaces_as_protocol_mismatch() {
    use flui_rendering::{
        constraints::SliverGeometry,
        context::{SliverHitTestContext, SliverLayoutContext},
        protocol::SliverProtocol,
        traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
    };
    use flui_tree::Leaf;

    /// Minimal sliver render-object stub for the test fixture — never
    /// laid out (the test triggers the protocol-mismatch error path
    /// before reaching perform_layout).
    #[derive(Debug, Default)]
    struct StubSliver;

    impl flui_foundation::Diagnosticable for StubSliver {}
    impl PaintEffectsCapability for StubSliver {}
    impl SemanticsCapability for StubSliver {}
    impl HotReloadCapability for StubSliver {}

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

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
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
// SubtreeBorrows thread-affinity smoke (PR #144 Copilot review legacy)
// ============================================================================

/// PR #144 Copilot review (comment_id=3294225417) fix carried forward
/// to U20.1: [`SubtreeBorrows`] (the U20.1 replacement for `TreePtr`)
/// carries the owning thread's `ThreadId` and panics with a
/// documented diagnostic on cross-thread access via
/// [`SubtreeBorrows::check_thread`].
///
/// This test verifies the legitimate worker-thread case: a pipeline
/// run inside `std::thread::spawn` succeeds because `SubtreeBorrows`
/// is constructed AND queried on the SAME (worker) thread per call.
/// The pathological cross-thread case (`SubtreeBorrows` constructed
/// on thread A, queried on thread B via a smuggled closure capture)
/// cannot be exercised from integration tests because the type is
/// private to `pipeline/owner.rs` — the guard is a defensive layer
/// validated by code reading.
#[test]
fn u20_subtree_borrows_worker_thread_pipeline_succeeds() {
    let mut pipeline = fresh_layout_pipeline();
    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let _child_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert must succeed");

    let constraints = BoxConstraints::tight(Size::new(px(50.0), px(50.0)));

    // Move pipeline into a worker thread, run layout there.
    // SubtreeBorrows is constructed inside `layout_dirty_root` using
    // the worker thread's id; subsequent `check_thread()` calls inside
    // `layout_subtree_borrowed` are on the same worker thread; guard
    // does NOT fire.
    let join = std::thread::spawn(move || pipeline.layout_dirty_root(padding_id, constraints));

    let result = join.join().expect("worker thread must not panic");
    assert!(
        result.is_ok(),
        "single-threaded worker pipeline must succeed (SubtreeBorrows \
         constructed AND queried on the same worker thread)",
    );
}

// ============================================================================
// U20.1 — 4-level deep recursion (verifies pre-acquired subtree borrows
// scale to deeper trees than the original PR #144 tests covered)
// ============================================================================

/// PR-A1b3 U20.1 deep-recursion smoke: a 4-level Padding chain
/// successfully lays out through the pre-acquired-subtree walk.
/// Verifies `collect_subtree_ids` + `get_subtree_mut` + recursive
/// `layout_subtree_borrowed` correctly handle deeper-than-typical
/// trees (the prior U20 tests topped out at 3 levels).
///
/// Math: outer Padding(10) → mid Padding(5) → inner Padding(2) →
/// ColoredBox(20×20). Parent constraints (0..200) × (0..200) — loose.
/// - ColoredBox: clamps 20×20 to its constraints → 20×20.
/// - inner Padding: wraps 20×20 + (2+2) = 24×24.
/// - mid Padding: wraps 24×24 + (5+5) = 34×34.
/// - outer Padding: wraps 34×34 + (10+10) = 54×54.
#[test]
fn u20_1_four_level_padding_chain() {
    let mut pipeline = fresh_layout_pipeline();

    let outer = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(10.0)));
    let mid = pipeline
        .render_tree_mut()
        .insert_box_child(outer, Box::new(RenderPadding::all(5.0)))
        .expect("mid insert must succeed");
    let inner = pipeline
        .render_tree_mut()
        .insert_box_child(mid, Box::new(RenderPadding::all(2.0)))
        .expect("inner insert must succeed");
    let leaf = pipeline
        .render_tree_mut()
        .insert_box_child(inner, Box::new(RenderColoredBox::green(20.0, 20.0)))
        .expect("leaf insert must succeed");

    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let size = pipeline
        .layout_dirty_root(outer, constraints)
        .expect("4-level layout must succeed under pre-acquired-subtree walk");

    assert_eq!(
        size,
        Size::new(px(54.0), px(54.0)),
        "Padding(10) → Padding(5) → Padding(2) → ColoredBox(20×20) must \
         compose to (20+4+10+20=54) × (54) — verifies recursive \
         layout_subtree_borrowed scales to deeper trees than 3 levels",
    );

    // Every node — INCLUDING the leaf — must be marked clean
    // post-layout (no descendant errors). PR #145 review fix
    // (Copilot 3294267600): prior version excluded `_leaf` from the
    // loop while the message claimed "every node".
    for id in [outer, mid, inner, leaf] {
        let node = pipeline
            .render_tree()
            .get(id)
            .expect("node must still be in tree");
        assert!(
            !node.needs_layout(),
            "depth-4 chain: every node (including leaf) must be clean post-layout",
        );
    }
}

/// `RenderViewAdapter` carries a manual (non-blanket)
/// `RenderObject<BoxProtocol>` impl that ignores the erased ctx and
/// drives layout from its embedded `ViewConfiguration`. The U20 walk
/// should still invoke `perform_layout_raw` on it correctly (the manual
/// impl chooses to ignore the ctx — that's its prerogative — but the
/// walk's dispatch shouldn't change behaviour).
///
/// Smoke check that the U20 pipeline-side ctx (Direct, no children) is
/// accepted by the adapter's manual impl.
#[test]
fn u20_render_view_adapter_layout_smoke() {
    use flui_rendering::view::{RenderView, RenderViewAdapter, ViewConfiguration};

    let mut pipeline = fresh_layout_pipeline();

    let config = ViewConfiguration::from_size(Size::new(px(320.0), px(240.0)), 1.0);
    let mut view = RenderView::with_configuration(config);
    view.prepare_initial_frame_without_owner();

    let adapter: Box<dyn RenderObject<BoxProtocol>> = Box::new(RenderViewAdapter::new(view));
    let view_id = pipeline.render_tree_mut().insert_box(adapter);

    // The incoming constraints are authoritative (live window size via
    // set_root_constraints); the mount-time configuration is a stale
    // snapshot after the first resize. Pinned end-to-end by
    // tests/root_resize_repaint.rs.
    let incoming = BoxConstraints::tight(Size::new(px(999.0), px(999.0)));
    let size = pipeline
        .layout_dirty_root(view_id, incoming)
        .expect("RenderViewAdapter layout_dirty_root must succeed");

    assert_eq!(
        size,
        Size::new(px(999.0), px(999.0)),
        "RenderViewAdapter must size from the INCOMING root constraints \
         (live window size), not from its mount-time configuration snapshot",
    );
}
