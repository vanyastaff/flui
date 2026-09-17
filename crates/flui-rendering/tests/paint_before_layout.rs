//! The `run_paint` frame-completeness gate (`RenderError::PaintBeforeLayout`).
//!
//! Phase ORDERING is enforced by the type system: each `run_*` method lives
//! only on its phase's impl block and the transitions are by-value
//! (`rebind_phase`), so `run_paint` cannot even be named before `run_layout`
//! returns. What the type system cannot express is COMPLETENESS — a caller
//! may legally drive the phases directly
//! (`into_layout().into_compositing().into_paint()`) and skip `run_layout`
//! with empty queues, which is exactly the manual-phase pattern the
//! benches and the direct-chain tests use. The one runtime gate paint keeps
//! is therefore completeness: entering `run_paint` with the scheduler's
//! layout queue non-empty is refused.
//!
//! The gate reads the scheduler's `needs_layout` QUEUE, not the per-node
//! `NEEDS_LAYOUT` flag: a flag-only mark (`RenderNode::mark_layout_flag`)
//! without an owner-side enqueue is a stale-geometry signal the paint walk
//! itself gates on (needs_layout nodes are skipped, see
//! `paint_dirty_flag_discipline::paint_skips_node_that_still_needs_layout`),
//! not a frame-level contract breach.

use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{
    constraints::BoxConstraints, error::RenderError, pipeline::PipelineOwner, traits::RenderObject,
};
use flui_types::geometry::px;

fn mount_two_node_tree() -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let child_id = owner
        .insert_child_render_object(root_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    (owner, child_id)
}

/// The misuse the gate exists for: layout work is scheduled (the
/// owner-side `mark_needs_layout` walk enqueued a relayout boundary) and
/// the caller drives the phases directly, skipping `run_layout`. The paint
/// phase begins with layout work pending and must refuse.
#[test]
fn run_paint_refuses_to_start_with_layout_work_pending() {
    let (owner, child_id) = mount_two_node_tree();

    let (mut owner, frame) = owner.run_frame();
    frame
        .expect("frame 1 succeeds")
        .expect("frame 1 produces a layer tree");

    owner.mark_needs_layout(child_id);
    assert!(
        !owner.nodes_needing_layout().is_empty(),
        "precondition: mark_needs_layout must leave the layout queue \
         non-empty — the gate fires on queue work, not node flags",
    );

    let mut owner = owner.into_layout().into_compositing().into_paint();
    let result = owner.run_paint();

    match result {
        Err(RenderError::PaintBeforeLayout) => {}
        other => panic!(
            "run_paint with pending layout work must be refused with \
             PaintBeforeLayout, got {other:?}"
        ),
    }
}

/// A healthy frame is unaffected: the gate must not reject a caller that
/// runs the phases in order through the orchestrator.
#[test]
fn a_healthy_frame_through_run_frame_is_unaffected() {
    let (owner, _child_id) = mount_two_node_tree();

    let (mut owner, frame) = owner.run_frame();
    let layer_tree = frame
        .expect("a healthy frame succeeds")
        .expect("a healthy frame produces a layer tree");
    assert!(
        layer_tree.root().is_some(),
        "the layer tree must be non-empty",
    );
    drop(owner.take_layer_tree());
}

/// The empty-queue direct chain stays legal: driving the phases by hand
/// with NO pending layout work (the bench pattern,
/// `benches/semantics_assembly.rs`) returns `Ok` — here with actual paint
/// work, so the walk genuinely runs rather than hitting the
/// empty-paint-queue early return.
#[test]
fn an_empty_layout_queue_direct_chain_still_paints() {
    let (owner, child_id) = mount_two_node_tree();

    let (mut owner, frame) = owner.run_frame();
    frame
        .expect("frame 1 succeeds")
        .expect("frame 1 produces a layer tree");

    owner.mark_needs_paint(child_id);
    assert!(
        owner.nodes_needing_layout().is_empty(),
        "precondition: a paint mark must not enqueue layout work",
    );

    let mut owner = owner.into_layout().into_compositing().into_paint();
    owner
        .run_paint()
        .expect("the empty-layout-queue direct chain must paint, not be refused");
}

/// Both queues non-empty: the gate must refuse BEFORE any paint work is
/// served — pinning the gate's placement above the paint walk, not merely
/// above the empty-paint-queue early return.
#[test]
fn run_paint_refuses_before_serving_any_paint_work_when_both_queues_pending() {
    let (owner, child_id) = mount_two_node_tree();

    let (mut owner, frame) = owner.run_frame();
    frame
        .expect("frame 1 succeeds")
        .expect("frame 1 produces a layer tree");

    owner.mark_needs_layout(child_id);
    owner.mark_needs_paint(child_id);
    assert!(
        !owner.nodes_needing_layout().is_empty() && !owner.nodes_needing_paint().is_empty(),
        "precondition: both queues must be non-empty",
    );

    let mut owner = owner.into_layout().into_compositing().into_paint();
    let result = owner.run_paint();

    match result {
        Err(RenderError::PaintBeforeLayout) => {}
        other => panic!("the gate must refuse before serving paint work, got {other:?}"),
    }
}
