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

    // Sentinel constraints — RenderViewAdapter ignores them and drives
    // layout from its configuration (logical 320×240).
    let sentinel = BoxConstraints::tight(Size::new(px(999.0), px(999.0)));
    let size = pipeline
        .layout_dirty_root(view_id, sentinel)
        .expect("RenderViewAdapter layout_dirty_root must succeed");

    assert_eq!(
        size,
        Size::new(px(320.0), px(240.0)),
        "RenderViewAdapter must lay out from its embedded configuration, \
         not from the sentinel constraints",
    );
}
