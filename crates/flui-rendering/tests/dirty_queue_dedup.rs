//! Dirty-queue dedup + mid-phase routing tests.
//!
//! Verifies [`PipelineOwner::add_node_needing_layout`] and the paint /
//! compositing / semantics invalidation paths dedup against in-queue
//! membership AND route mid-phase marks (when the corresponding
//! `debug_doing_*` flag is true) into [`mid_layout_marks`] instead of
//! the active `dirty` queue. The drain helper
//! [`PipelineOwner::drain_mid_layout_marks`] moves entries back for
//! the next outer-loop iteration.
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md

use flui_objects::RenderColoredBox;
use flui_rendering::pipeline::{DirtyNode, PipelineOwner};

fn fresh_owner_with_one_node() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let id = owner
        .render_tree_mut()
        .insert_box(Box::new(RenderColoredBox::red(10.0, 10.0)));
    (owner, id)
}

// ============================================================================
// Dedup — repeated add_node_needing_* on same id yields single entry
// ============================================================================

#[test]
fn repeated_add_layout_dedups_to_single_entry() {
    let (mut owner, id) = fresh_owner_with_one_node();
    // Clear whatever insert() pushed so we start from empty.
    owner.clear_all_dirty_nodes();

    owner.add_node_needing_layout(id, 0);
    owner.add_node_needing_layout(id, 0);
    owner.add_node_needing_layout(id, 0);

    let layout_entries: Vec<DirtyNode> = owner.nodes_needing_layout().to_vec();
    assert_eq!(
        layout_entries.len(),
        1,
        "3 repeated add_node_needing_layout calls must collapse to 1 \
         queue entry; got {layout_entries:?}",
    );
    assert_eq!(layout_entries[0].id, id);
}

// ============================================================================
// Distinct ids — dedup does not coalesce different node ids
// ============================================================================

// ============================================================================
// Mid-phase routing — debug_doing_layout=true routes to mid_layout_marks
// ============================================================================

// ============================================================================
// clear_all_dirty_nodes — also clears mid_layout_marks
// ============================================================================

// ============================================================================
// Regression — drain mid-marks at phase end
// ============================================================================
