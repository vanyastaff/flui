//! D-block PR-A1 U23 — `run_layout` wiring to `layout_dirty_root`.
//!
//! Verifies the U23 rewrite: `PipelineOwner::run_layout` now calls
//! `layout_dirty_root` per dirty entry (using cached / root
//! constraints from `cached_or_root_constraints`) instead of the
//! legacy `layout_node_with_children` no-op recursion. The result
//! is that `run_layout` actually computes geometries — previously
//! it walked the tree but invoked no per-node layout (audit-confirmed
//! no-op stub before U23).
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md §U23
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md §D1

use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner, traits::RenderObject};
use flui_types::{Size, geometry::px};

// ============================================================================
// run_layout actually lays out via layout_dirty_root + root_constraints
// ============================================================================

/// PR-A1 U23 happy path: `run_layout` on a freshly-inserted
/// Padding → ColoredBox tree (no cached state) uses
/// `root_constraints` to drive the first layout pass.
///
/// Pre-U23: `run_layout` walked the dirty queue + recursed via
/// `layout_node_with_children` which never invoked
/// `perform_layout_raw` on anyone — geometries stayed at default
/// (`Size::ZERO`). Test would have asserted `None` geometry.
///
/// Post-U23: `run_layout` calls `layout_dirty_root` per dirty entry,
/// sourcing constraints from `root_constraints`. ColoredBox lays out
/// to its preferred size; Padding wraps it.
#[test]
fn u23_run_layout_uses_root_constraints_to_drive_first_frame() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let _colored = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored child insert");

    owner.set_root_id(Some(padding_id));
    // Bind root constraints: 0..200 × 0..200 loose.
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Transition to Layout phase and run.
    let mut owner = owner.into_layout();
    owner
        .run_layout()
        .expect("first-frame run_layout must succeed");

    // ColoredBox(40×40) wrapped in Padding(5) → 50×50.
    let padding_node = owner
        .render_tree()
        .get(padding_id)
        .expect("padding still in tree");
    assert_eq!(
        padding_node.geometry_box(),
        Some(Size::new(px(50.0), px(50.0))),
        "post-run_layout Padding(5) wrapping ColoredBox(40×40) must \
         have geometry 50×50 — verifies run_layout actually invokes \
         per-node layout via layout_dirty_root (pre-U23 this was \
         None / Size::ZERO)",
    );
    assert!(
        !padding_node.needs_layout(),
        "padding NEEDS_LAYOUT must be cleared after run_layout",
    );
}

// ============================================================================
// Frame 2 — cached constraints supersede root_constraints
// ============================================================================

/// After frame 1's successful layout, every node has cached
/// `state.constraints()`. Frame 2 re-marks dirty + calls run_layout
/// again. `cached_or_root_constraints` returns the cached value
/// (not root_constraints) — verifies the priority order documented
/// on the helper.
#[test]
fn u23_run_layout_uses_cached_constraints_on_frame_two() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(2.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let _colored = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::blue(60.0, 30.0)))
        .expect("colored child insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        px(300.0),
    )));

    // Frame 1.
    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 1");
    let frame_1_size = owner
        .render_tree()
        .get(padding_id)
        .and_then(|n| n.geometry_box());
    assert_eq!(frame_1_size, Some(Size::new(px(64.0), px(34.0))));

    // Clear root_constraints to prove frame 2 doesn't depend on it.
    let mut owner = owner.into_idle();
    owner.set_root_constraints(None);

    // Mark dirty to trigger frame 2 re-layout.
    owner.mark_needs_layout(padding_id);
    let mut owner = owner.into_layout();
    owner
        .run_layout()
        .expect("frame 2 must succeed using cached constraints");

    let frame_2_size = owner
        .render_tree()
        .get(padding_id)
        .and_then(|n| n.geometry_box());
    assert_eq!(
        frame_2_size, frame_1_size,
        "frame 2 must produce the same geometry as frame 1 — \
         cached_or_root_constraints picked up state.constraints() \
         (root_constraints was cleared between frames)",
    );
}

// ============================================================================
// No constraints + non-root id — skip with warning, no Err
// ============================================================================

/// A dirty entry that's NOT the root AND has no cached constraints
/// (impossible in practice — non-root nodes get constraints from
/// their parent's perform_layout) skips with `tracing::warn!`. Test
/// verifies `run_layout` returns `Ok(())` instead of erroring.
#[test]
fn u23_run_layout_skips_dirty_entry_with_no_constraints() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    // Note: NOT set as root. NO root_constraints set.
    // Dirty queue has padding_id (from insert) but cached_or_root_constraints
    // returns None.

    let mut owner = owner.into_layout();
    owner.run_layout().expect(
        "run_layout must not fail when a dirty entry has no constraints — \
         skips with warning and continues",
    );

    // No geometry computed (skipped).
    assert!(
        owner
            .render_tree()
            .get(padding_id)
            .and_then(|n| n.geometry_box())
            .is_none(),
        "skipped entry must NOT have geometry computed",
    );
}

// ============================================================================
// root_constraints get/set round-trip
// ============================================================================

#[test]
fn u23_root_constraints_setter_round_trip() {
    let mut owner = PipelineOwner::new();
    assert_eq!(owner.root_constraints(), None);

    let c = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    owner.set_root_constraints(Some(c));
    assert_eq!(owner.root_constraints(), Some(c));

    owner.set_root_constraints(None);
    assert_eq!(owner.root_constraints(), None);
}

// ============================================================================
// PR-A1 U23 P2 review fixes — Copilot 3294417924/3294417942/3294417957
// ============================================================================

/// PR #148 Copilot review (comment_id=3294417957): setting
/// `root_constraints` should auto-mark the root dirty so the next
/// `run_layout` invocation picks up the change. Pre-fix the binding
/// had to call `mark_needs_layout(root_id)` separately — silent
/// no-relayout footgun.
#[test]
fn u23_set_root_constraints_auto_marks_root_dirty() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    owner.set_root_id(Some(padding_id));
    owner.clear_all_dirty_nodes();

    assert!(!owner.has_dirty_nodes());

    // Setting root_constraints to Some(_) auto-marks root dirty.
    let c = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    owner.set_root_constraints(Some(c));
    assert!(
        owner.has_dirty_nodes(),
        "set_root_constraints(Some(_)) must auto-mark root dirty for next run_layout",
    );

    // Idempotent: setting to the SAME value does not re-mark dirty.
    owner.clear_all_dirty_nodes();
    owner.set_root_constraints(Some(c));
    assert!(
        !owner.has_dirty_nodes(),
        "set_root_constraints with same value must NOT re-mark dirty (idempotent)",
    );

    // Setting to None does NOT mark dirty (explicit clear).
    owner.set_root_constraints(None);
    assert!(
        !owner.has_dirty_nodes(),
        "set_root_constraints(None) must NOT mark dirty (explicit clear)",
    );
}

/// PR #148 Copilot review (comment_id=3294417924): `run_layout` must
/// skip dirty-queue entries whose `NEEDS_LAYOUT` flag was already
/// cleared earlier in the iteration (e.g., parent's layout_child
/// callback already laid out the child).
///
/// Verifies via insert_child_render_object which enqueues both parent
/// and child. After run_layout:
/// 1. Parent processed first (shallow-first sort) → layout_dirty_root
///    invokes Padding's perform_layout → calls ctx.layout_child(0, c)
///    for the child → child's needs_layout is cleared via
///    layout_subtree_borrowed's success path.
/// 2. Child's dirty entry was queued separately by insert_child;
///    run_layout's iteration encounters it next; needs_layout()
///    returns false → skipped.
#[test]
fn u23_run_layout_skips_already_cleaned_dirty_entries() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let _child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut owner = owner.into_layout();
    // Pre-fix: both parent + child entries get layout_dirty_root invoked.
    //   Parent's layout calls layout_child → recurses into child →
    //   child laid out. Then run_layout dequeues child's separate
    //   entry → layout_dirty_root invoked AGAIN on child → redundant
    //   perform_layout invocation (potentially side-effectful).
    // Post-fix: child's separate entry's needs_layout() = false →
    //   skipped via trace log; no redundant invocation.
    owner.run_layout().expect("run_layout must succeed");

    // Both nodes laid out (geometry set).
    let padding_geom = owner
        .render_tree()
        .get(padding_id)
        .and_then(|n| n.geometry_box());
    assert_eq!(padding_geom, Some(Size::new(px(50.0), px(50.0))));
    // No way to count perform_layout invocations from integration test,
    // but the skip path is exercised (covered by lib-scoped test if
    // we wanted to assert the trace! event explicitly).
}
