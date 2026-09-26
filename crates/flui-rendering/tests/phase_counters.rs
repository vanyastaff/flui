//! What each `PipelineCounters` field counts, pinned on small real trees.
//!
//! The counters are monotonic totals; every test reads them before and after
//! a frame and asserts on the difference, which is how a frame driver uses
//! them.

use flui_objects::{RenderColoredBox, RenderFlex, RenderPadding, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{Idle, PipelineCounters, PipelineOwner},
    testing::{RenderLabelRegistry, TreeNode, box_node, tree},
};
use flui_types::{Size, geometry::px};

fn root(owner: &mut PipelineOwner<Idle>, spec: TreeNode) -> RenderLabelRegistry {
    let (root_id, registry) = tree::mount(owner, spec);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    registry
}

/// Runs one frame and returns the owner with the counters it added.
fn frame(owner: PipelineOwner<Idle>) -> (PipelineOwner<Idle>, PipelineCounters) {
    let before = owner.counters();
    let (owner, result) = owner.run_frame();
    result.expect("frame succeeds");
    let delta = owner.counters().since(before);
    (owner, delta)
}

/// padding → padding → coloured leaf, the root labelled `root`.
fn chain() -> TreeNode {
    box_node(RenderPadding::all(5.0))
        .label("root")
        .child(box_node(RenderPadding::all(5.0)).child(box_node(RenderColoredBox::red(10.0, 10.0))))
}

#[test]
fn counters_count_every_node_whose_perform_layout_ran() {
    let mut owner = PipelineOwner::new();
    let _ = root(&mut owner, chain());

    let (_, first) = frame(owner);

    assert_eq!(
        first.nodes_laid_out, 3,
        "all three nodes lay out on the first frame: {first:?}"
    );
    assert_eq!(
        first.layout_passes, 1,
        "one non-empty layout batch: {first:?}"
    );
    assert_eq!(
        first.layout_roots, 3,
        "mounting queued all three nodes; the root's walk lays out the other \
         two, whose entries are still drained (and skipped as clean): {first:?}"
    );
    assert_eq!(
        first.frames_produced, 1,
        "the frame committed a layer tree: {first:?}"
    );
    assert!(
        first.nodes_painted >= 3,
        "every node painted on the first frame: {first:?}"
    );
    assert!(first.layers_produced >= 1, "{first:?}");
    assert_eq!(first.layers_reused, 0, "nothing is retained yet: {first:?}");
}

#[test]
fn counters_skip_a_clean_child_the_short_circuit_serves() {
    let mut owner = PipelineOwner::new();
    let registry = root(&mut owner, chain());
    let root_id = registry.get("root").expect("root is labelled");
    let (mut owner, _) = frame(owner);

    // Only the root is re-marked, and it hands its child the same
    // constraints, so the child's cached geometry is served.
    owner.mark_needs_layout(root_id);
    let (_, second) = frame(owner);

    assert_eq!(
        second.nodes_laid_out, 1,
        "only the re-marked root lays out; its clean child hits the cache: {second:?}"
    );
    assert_eq!(second.layout_passes, 1, "{second:?}");
}

#[test]
fn counters_count_grafted_layers_apart_from_fresh_ones() {
    let mut owner = PipelineOwner::new();
    let registry = root(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("dirty")
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            ),
    );
    let dirty_id = registry.get("dirty").expect("dirty boundary is labelled");

    let (mut owner, first) = frame(owner);
    assert_eq!(
        first.nodes_painted, 5,
        "the first frame paints the flex, both boundaries and both leaves: {first:?}"
    );
    assert_eq!(first.layers_reused, 0, "{first:?}");

    owner.mark_needs_paint(dirty_id);
    let (_, second) = frame(owner);

    assert_eq!(
        second.nodes_painted, 3,
        "the flex, the dirty boundary and its leaf paint; the clean boundary \
         and its leaf are grafted, not painted: {second:?}"
    );
    assert!(
        second.layers_reused >= 1,
        "the clean boundary's retained layers are counted as reused: {second:?}"
    );
    assert!(second.layers_produced >= 1, "{second:?}");
    assert_eq!(
        second.nodes_laid_out, 0,
        "a paint-only frame lays nothing out: {second:?}"
    );
    assert_eq!(second.frames_produced, 1, "{second:?}");
}

#[test]
fn counters_count_a_frame_only_when_paint_commits() {
    let mut owner = PipelineOwner::new();
    let _ = root(&mut owner, chain());
    let (owner, first) = frame(owner);
    assert_eq!(first.frames_produced, 1, "{first:?}");

    let (_, idle) = frame(owner);

    assert_eq!(
        idle,
        PipelineCounters::default(),
        "an idle run_frame does no counted work and commits no frame"
    );
}
