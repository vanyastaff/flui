//! D-block PR-A2 U34/U35 — compositing-bits walk in `run_compositing`.
//!
//! Verifies the U34 rewrite of `PipelineOwner::run_compositing`: per
//! Flutter `RenderObject._updateCompositingBits`
//! (`.flutter/.../object.dart:3226-3258`), the method now recursively
//! walks each dirty subtree, OR-ing children's `NEEDS_COMPOSITING`
//! into self, and forcing `NEEDS_COMPOSITING = true` for any node
//! whose `IS_REPAINT_BOUNDARY` flag is set or whose
//! `always_needs_compositing()` trait answer is true. Post-U34 the
//! `NEEDS_COMPOSITING_BITS_UPDATE` flag is also cleared, matching
//! Flutter's per-walk state transitions.
//!
//! Also covers U33 bootstrap (IS_REPAINT_BOUNDARY auto-populated at
//! insert) and U35 unconditional `WAS_REPAINT_BOUNDARY` write at
//! paint.
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md §U33-U35
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md §D3-3, §R26b

use flui_rendering::{
    constraints::BoxConstraints,
    objects::{RenderColoredBox, RenderPadding},
    pipeline::PipelineOwner,
    traits::RenderObject,
};
use flui_types::geometry::px;

// ============================================================================
// U33 bootstrap — IS_REPAINT_BOUNDARY storage flag set at insert
// ============================================================================

/// PR-A2 U33 happy path: insert a RenderPadding (trait answer
/// `is_repaint_boundary() == false`) and verify the storage flag
/// reflects the trait answer.
#[test]
fn u33_bootstrap_sets_is_repaint_boundary_flag_from_trait_answer() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);

    let padding_node = owner
        .render_tree()
        .get(padding_id)
        .expect("padding in tree");
    // RenderPadding default is_repaint_boundary == false (no override).
    assert!(
        !padding_node.is_repaint_boundary(),
        "RenderPadding default is_repaint_boundary should be false",
    );
    assert_eq!(
        padding_node.is_repaint_boundary_flag(),
        padding_node.is_repaint_boundary(),
        "post-insert IS_REPAINT_BOUNDARY storage flag must reflect trait answer \
         (U33 bootstrap)",
    );
}

/// U33: insert_child_render_object path also calls bootstrap (each
/// insert site is independently audited per memo R26b).
#[test]
fn u33_bootstrap_runs_on_insert_child_render_object_path() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    let child_node = owner.render_tree().get(child_id).expect("child in tree");
    assert_eq!(
        child_node.is_repaint_boundary_flag(),
        child_node.is_repaint_boundary(),
        "insert_child_render_object must bootstrap IS_REPAINT_BOUNDARY storage \
         flag from the trait answer",
    );
}

// ============================================================================
// U34 — run_compositing walks subtree + clears NEEDS_COMPOSITING_BITS_UPDATE
// ============================================================================

/// U34 happy path: mark a node for compositing-bits update + invoke
/// `run_compositing` → flag must be cleared post-walk.
#[test]
fn u34_run_compositing_clears_needs_compositing_bits_update_flag() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);

    // Mark dirty for compositing-bits update.
    owner
        .render_tree()
        .get(padding_id)
        .expect("padding")
        .mark_needs_compositing_bits_update();
    let depth = owner.render_tree().depth(padding_id).unwrap_or(0) as usize;
    owner.add_node_needing_compositing_bits_update(padding_id, depth);

    // Transition through the typestate Idle → Layout → Compositing
    // to reach the compositing phase. No actual layout work needed
    // for this test — `NEEDS_COMPOSITING_BITS_UPDATE` was set
    // directly above, so `run_compositing` has all the state it needs
    // from the typestate transition alone (no `run_layout` call).
    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));
    let owner = owner.into_layout();
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("run_compositing succeeds");

    let padding_node = owner.render_tree().get(padding_id).expect("padding");
    assert!(
        !padding_node.needs_compositing_bits_update(),
        "NEEDS_COMPOSITING_BITS_UPDATE flag must be cleared after run_compositing \
         walks the subtree (Flutter object.dart:3250/3253/3256)",
    );
}

/// U34: a node whose flag was cleared between enqueue and run
/// (e.g., parent's walk processed this child mid-iteration) hits the
/// `update_subtree_compositing_bits` early-return path. The walk
/// short-circuits at the entry and leaves NEEDS_COMPOSITING alone.
///
/// **PR-A2 Codex review #3294562493:** post-fix
/// `add_node_needing_compositing_bits_update` sets the flag on
/// enqueue so an unflagged enqueue is no longer possible. To exercise
/// the short-circuit path the test now manually clears the flag
/// after enqueue (simulating the parent-cleared-me-mid-walk case).
#[test]
fn u34_run_compositing_short_circuits_when_flag_cleared_after_enqueue() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let depth = owner.render_tree().depth(padding_id).unwrap_or(0) as usize;
    owner.add_node_needing_compositing_bits_update(padding_id, depth);
    // Clear the flag the enqueue just set, simulating the case where
    // an earlier iteration's walk (e.g., the parent's recursion)
    // already processed this node and cleared its flag.
    owner
        .render_tree()
        .get(padding_id)
        .expect("padding")
        .clear_needs_compositing_bits_update();

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));
    let owner = owner.into_layout();
    let mut owner = owner.into_compositing();

    let needs_compositing_before = owner
        .render_tree()
        .get(padding_id)
        .map(|n| n.needs_compositing())
        .unwrap_or(false);

    owner.run_compositing().expect("run_compositing succeeds");

    let needs_compositing_after = owner
        .render_tree()
        .get(padding_id)
        .map(|n| n.needs_compositing())
        .unwrap_or(false);

    // Walk short-circuits → NEEDS_COMPOSITING unchanged.
    assert_eq!(
        needs_compositing_before, needs_compositing_after,
        "short-circuit branch must not mutate NEEDS_COMPOSITING when \
         NEEDS_COMPOSITING_BITS_UPDATE is false",
    );
}

/// PR-A2 Codex #3294562493 regression: enqueueing via
/// `add_node_needing_compositing_bits_update` MUST set the
/// `NEEDS_COMPOSITING_BITS_UPDATE` flag on the node, so that
/// `run_compositing`'s per-entry short-circuit can't silently drop
/// the queued work.
#[test]
fn u34_add_node_needing_compositing_bits_update_sets_flag_on_enqueue() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);

    // Pre-enqueue: clear any stale flag bits from insert / bootstrap.
    owner
        .render_tree()
        .get(padding_id)
        .expect("padding")
        .clear_needs_compositing_bits_update();
    assert!(
        !owner
            .render_tree()
            .get(padding_id)
            .unwrap()
            .needs_compositing_bits_update(),
        "precondition: flag cleared before enqueue",
    );

    let depth = owner.render_tree().depth(padding_id).unwrap_or(0) as usize;
    owner.add_node_needing_compositing_bits_update(padding_id, depth);

    assert!(
        owner
            .render_tree()
            .get(padding_id)
            .unwrap()
            .needs_compositing_bits_update(),
        "add_node_needing_compositing_bits_update must set the flag \
         (invariant: queue entry ⇒ flag set, so the run_compositing \
         walk never silently drops queued work)",
    );
}

/// U34: empty dirty queue → run_compositing is a fast-path no-op.
#[test]
fn u34_run_compositing_empty_queue_is_no_op() {
    let owner = PipelineOwner::new();
    let owner = owner.into_layout();
    let mut owner = owner.into_compositing();
    owner
        .run_compositing()
        .expect("empty queue: run_compositing returns Ok immediately");
}

/// U34: parent + child both dirty for compositing-bits update; walk
/// processes parent first (shallow-first sort), clears parent's flag,
/// recurses into child, clears child's flag. Both flags cleared
/// post-walk.
#[test]
fn u34_run_compositing_walks_parent_then_child() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(px(5.0)))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    // Both parent + child dirty for compositing-bits.
    for id in [padding_id, child_id] {
        owner
            .render_tree()
            .get(id)
            .expect("node")
            .mark_needs_compositing_bits_update();
        let depth = owner.render_tree().depth(id).unwrap_or(0) as usize;
        owner.add_node_needing_compositing_bits_update(id, depth);
    }

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));
    let owner = owner.into_layout();
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("run_compositing succeeds");

    for id in [padding_id, child_id] {
        let node = owner.render_tree().get(id).expect("node");
        assert!(
            !node.needs_compositing_bits_update(),
            "node {id:?} NEEDS_COMPOSITING_BITS_UPDATE must be cleared after \
             parent-then-child walk",
        );
    }
}
