//! `run_layout` wiring to `layout_dirty_root`.
//!
//! Verifies the rewrite: `PipelineOwner::run_layout` now calls
//! `layout_dirty_root` per dirty entry (using cached / root
//! constraints from `cached_or_root_constraints`) instead of the
//! legacy `layout_node_with_children` no-op recursion. The result
//! is that `run_layout` actually computes geometries — previously
//! it walked the tree but invoked no per-node layout (audit-confirmed
//! no-op stub before this rewrite).
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner, traits::RenderObject};

// ============================================================================
// run_layout actually lays out via layout_dirty_root + root_constraints
// ============================================================================

/// Happy path: `run_layout` on a freshly-inserted
/// Padding → ColoredBox tree (no cached state) uses
/// `root_constraints` to drive the first layout pass.
///
/// Before the `run_layout` rewrite: it walked the dirty queue + recursed via
/// `layout_node_with_children` which never invoked
/// `perform_layout_raw` on anyone — geometries stayed at default
/// (`Size::ZERO`). Test would have asserted `None` geometry.
///
/// After the rewrite: `run_layout` calls `layout_dirty_root` per dirty entry,
/// sourcing constraints from `root_constraints`. ColoredBox lays out
/// to its preferred size; Padding wraps it.
#[test]
fn run_layout_uses_root_constraints_to_drive_first_frame() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let _colored = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored child insert");

    owner.set_root_id(Some(padding_id));
    // Bind root constraints: 0..200 × 0..200 loose.
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

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
        Some(Size::new(50.0, 50.0)),
        "post-run_layout Padding(5) wrapping ColoredBox(40×40) must \
         have geometry 50×50 — verifies run_layout actually invokes \
         per-node layout via layout_dirty_root (before the rewrite this \
         was None / Size::ZERO)",
    );
    assert!(
        !padding_node.needs_layout(),
        "padding NEEDS_LAYOUT must be cleared after run_layout",
    );
}

// ============================================================================
// Frame 2 — cached constraints supersede root_constraints
// ============================================================================

// ============================================================================
// No constraints + non-root id — skip with warning, no Err
// ============================================================================

// ============================================================================
// root_constraints get/set round-trip
// ============================================================================

// ============================================================================
// root_constraints setter review fixes
// ============================================================================
