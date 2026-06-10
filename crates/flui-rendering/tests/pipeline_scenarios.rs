//! Render-object foundation under realistic multi-frame scenarios.
//!
//! Each scenario drives the REAL `run_frame` orchestration (layout →
//! compositing → paint) the bindings use in production, then asserts
//! on observable outputs: committed `RenderState` offsets, layer-tree
//! structure, picture bounds, hit paths, and dirty-queue hygiene.
//!
//! Scenarios:
//! 1. deep nesting — offsets accumulate through a 50-deep padding
//!    chain and a single merged picture comes out;
//! 2. mixed tree — flex + padding + transform + clip in one frame:
//!    structure, bounds, and per-region hits;
//! 3. invalidation round-trips — paint-only frame, then a
//!    layout-changing frame, across three frames;
//! 4. idle stability — frames after a clean one produce NO output
//!    (no spurious repaints from leftover dirty state);
//! 5. removal churn — remove + reinsert under one parent: stale ids
//!    miss, queues stay clean, the new subtree paints;
//! 6. repaint-boundary subtree — the boundary's OffsetLayer split
//!    survives re-frames and carries the laid-out offset.

use flui_layer::{Layer, LayerTree};
use flui_painting::DisplayListCore;
use flui_rendering::{
    constraints::BoxConstraints,
    hit_testing::HitTestResult,
    objects::{
        RenderColoredBox, RenderFlex, RenderPadding, RenderRepaintBoundary, RenderTransform,
    },
    pipeline::PipelineOwner,
};
use flui_types::{EdgeInsets, Offset, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

fn frame(owner: PipelineOwner) -> (PipelineOwner, Option<LayerTree>) {
    let (owner, result) = owner.run_frame();
    (owner, result.expect("frame must not error"))
}

fn structure(tree: &LayerTree) -> Vec<&'static str> {
    fn walk(tree: &LayerTree, id: flui_foundation::LayerId, out: &mut Vec<&'static str>) {
        let node = tree.get(id).expect("live layer id");
        out.push(match node.layer() {
            Layer::Offset(_) => "Offset",
            Layer::Picture(_) => "Picture",
            Layer::ClipRect(_) => "ClipRect",
            Layer::Transform(_) => "Transform",
            _ => "Other",
        });
        for &c in node.children() {
            walk(tree, c, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = tree.root() {
        walk(tree, root, &mut out);
    }
    out
}

fn first_picture_bounds(tree: &LayerTree) -> flui_types::Rect {
    fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> Option<flui_types::Rect> {
        let node = tree.get(id)?;
        if let Layer::Picture(p) = node.layer() {
            return Some(p.picture().bounds());
        }
        node.children().iter().find_map(|&c| find(tree, c))
    }
    find(tree, tree.root().expect("root")).expect("picture present")
}

fn state_offset(owner: &PipelineOwner, id: flui_foundation::RenderId) -> Offset {
    owner
        .render_tree()
        .get(id)
        .and_then(|n| n.as_box())
        .map(|e| e.state().offset())
        .expect("node state")
}

fn hits(owner: &PipelineOwner, x: f32, y: f32) -> Vec<flui_foundation::RenderId> {
    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(x), px(y)), &mut result);
    result.path().iter().map(|e| e.target).collect()
}

// ============================================================================
// 1. Deep nesting: 50 paddings of 1px each around a 10×10 box
// ============================================================================

#[test]
fn deep_padding_chain_accumulates_offsets_and_merges_one_picture() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(Box::new(RenderPadding::all(1.0)) as BoxedRenderObject);
    let mut parent = root;
    for _ in 0..49 {
        parent = owner
            .insert_child_render_object(parent, Box::new(RenderPadding::all(1.0)))
            .expect("padding link");
    }
    let leaf = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("leaf");

    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(500.0),
        px(0.0),
        px(500.0),
    )));

    let (owner, tree) = frame(owner);
    let tree = tree.expect("first frame paints");

    // Every padding contributes (1,1) to ITS child — each link's
    // committed RenderState offset is exactly (1,1)...
    assert_eq!(state_offset(&owner, leaf), Offset::new(px(1.0), px(1.0)));
    // ...and the picture's ABSOLUTE bounds carry the full 50-deep
    // accumulation: the leaf draws at (50,50)..(60,60).
    assert_eq!(
        first_picture_bounds(&tree),
        flui_types::Rect::from_ltrb(px(50.0), px(50.0), px(60.0), px(60.0)),
        "accumulated origins must be baked through the whole chain",
    );
    assert_eq!(
        structure(&tree),
        vec!["Offset", "Picture"],
        "51 inline nodes still merge into ONE picture",
    );

    // Hit straight through all 50 levels, leaf-first path of 51 ids.
    let path = hits(&owner, 55.0, 55.0);
    assert_eq!(path.len(), 51);
    assert_eq!(path[0], leaf, "leaf-first");
    assert_eq!(path[50], root, "root last");
    assert!(
        hits(&owner, 5.0, 5.0).is_empty(),
        "the border region (inside paddings, outside the box) misses",
    );
}

// ============================================================================
// 2. Mixed tree in one frame
// ============================================================================

#[test]
fn mixed_flex_padding_transform_clip_frame() {
    let mut owner = PipelineOwner::new();
    // row[ padding(5){red 40}, transform(scale2){blue 20}, clip{green 40} ]
    let row = owner.insert(Box::new(RenderFlex::row()) as BoxedRenderObject);
    let pad = owner
        .insert_child_render_object(row, Box::new(RenderPadding::all(5.0)))
        .expect("pad");
    let red = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("red");
    let scaler = owner
        .insert_child_render_object(row, Box::new(RenderTransform::scale(2.0, 2.0)))
        .expect("scaler");
    let blue = owner
        .insert_child_render_object(scaler, Box::new(RenderColoredBox::blue(20.0, 20.0)))
        .expect("blue");
    let clip = owner
        .insert_child_render_object(
            row,
            Box::new(flui_rendering::objects::RenderClipRect::hard_edge()),
        )
        .expect("clip");
    let green = owner
        .insert_child_render_object(clip, Box::new(RenderColoredBox::green(40.0, 40.0)))
        .expect("green");

    owner.set_root_id(Some(row));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        px(100.0),
    )));

    let (owner, tree) = frame(owner);
    let tree = tree.expect("frame paints");

    // Row children sit at main-axis offsets 0 / 50 / 70
    // (padding 40+10 wide, transform reports child size 20, clip 40).
    assert_eq!(state_offset(&owner, pad), Offset::new(px(0.0), px(0.0)));
    assert_eq!(state_offset(&owner, scaler), Offset::new(px(50.0), px(0.0)));
    assert_eq!(state_offset(&owner, clip), Offset::new(px(70.0), px(0.0)));

    // Layer splits happen exactly where semantics demand them: the
    // transform's effect hook wraps ITS subtree in a TransformLayer,
    // the clip scope brackets green — everything else merges.
    assert_eq!(
        structure(&tree),
        vec![
            "Offset",
            "Picture",
            "Transform",
            "Picture",
            "ClipRect",
            "Picture",
        ],
        "inline draws merge; Transform and ClipRect split the stream",
    );

    // Region hits. The scaled blue's VISUAL extent is 50..90 (its
    // 20-wide geometry under scale 2) and overlaps the clip at 70..110
    // — in the overlap the LATER sibling (green) wins, topmost-first.
    assert_eq!(hits(&owner, 20.0, 20.0).first().copied(), Some(red));
    // (60,10): inverse-mapped blue-local (5,5), left of green's start.
    assert_eq!(hits(&owner, 60.0, 10.0).first().copied(), Some(blue));
    // (80,20): inside BOTH blue's visual extent and green — z-order
    // gives the later sibling the hit.
    assert_eq!(hits(&owner, 80.0, 20.0).first().copied(), Some(green));
    assert_eq!(hits(&owner, 100.0, 20.0).first().copied(), Some(green));
}

// ============================================================================
// 3. Invalidation round-trips across three frames
// ============================================================================

#[test]
fn paint_only_then_layout_invalidations_round_trip() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Frame 1: full.
    let (mut owner, tree) = frame(owner);
    assert!(tree.is_some());

    // Frame 2: paint-only invalidation — no layout change, fresh tree.
    owner.add_node_needing_paint(child, 1);
    let (mut owner, tree2) = frame(owner);
    let tree2 = tree2.expect("paint-only frame repaints");
    assert_eq!(
        first_picture_bounds(&tree2),
        flui_types::Rect::from_ltrb(px(5.0), px(5.0), px(45.0), px(45.0)),
        "geometry unchanged on a paint-only frame",
    );

    // Frame 3: layout invalidation — padding grows, offsets move.
    {
        let tree_mut = owner.render_tree_mut();
        let node = tree_mut.get_mut(pad).expect("pad");
        let entry = node.as_box_mut().expect("box");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderPadding>()
            .expect("padding")
            .set_padding(EdgeInsets::all(px(20.0)));
    }
    owner.mark_needs_layout(pad);
    let (owner, tree3) = frame(owner);
    let tree3 = tree3.expect("layout frame repaints");
    assert_eq!(state_offset(&owner, child), Offset::new(px(20.0), px(20.0)));
    assert_eq!(
        first_picture_bounds(&tree3),
        flui_types::Rect::from_ltrb(px(20.0), px(20.0), px(60.0), px(60.0)),
        "relayout must repaint at the NEW offsets",
    );
}

// ============================================================================
// 4. Idle stability: clean frames produce no output
// ============================================================================

#[test]
fn clean_frames_after_first_produce_no_layer_tree() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(Box::new(RenderColoredBox::red(40.0, 40.0)) as BoxedRenderObject);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));

    let (mut owner, first) = frame(owner);
    assert!(first.is_some(), "frame 1 paints");

    for n in 2..=5 {
        let (next_owner, tree) = frame(owner);
        owner = next_owner;
        assert!(
            tree.is_none(),
            "frame {n} has no dirty work and must produce no output — \
             leftover dirty state here means an idle app burns frames",
        );
    }
    assert!(!owner.has_dirty_nodes());
}

// ============================================================================
// 5. Removal churn under one parent
// ============================================================================

#[test]
fn remove_and_reinsert_child_keeps_pipeline_clean() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let first_child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("first child");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let (mut owner, _) = frame(owner);

    // Remove the child, reinsert a different one (slot reuse).
    let removed = owner.remove_render_object(first_child);
    assert_eq!(removed, 1);
    let second_child = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::blue(60.0, 60.0)))
        .expect("second child");
    owner.mark_needs_layout(pad);

    let (owner, tree) = frame(owner);
    let tree = tree.expect("churn frame paints");

    assert!(
        owner.render_tree().get(first_child).is_none(),
        "the stale id must not resolve to the reused slot (ABA guard)",
    );
    assert_eq!(
        state_offset(&owner, second_child),
        Offset::new(px(5.0), px(5.0))
    );
    assert_eq!(
        first_picture_bounds(&tree),
        flui_types::Rect::from_ltrb(px(5.0), px(5.0), px(65.0), px(65.0)),
        "the NEW child paints at the padded origin",
    );
    assert_eq!(
        hits(&owner, 30.0, 30.0).first().copied(),
        Some(second_child),
        "hits route to the new child, never the stale id",
    );
    assert!(!owner.has_dirty_nodes(), "queues drain fully after churn");
}

// ============================================================================
// 6. Repaint-boundary split survives re-frames
// ============================================================================

#[test]
fn repaint_boundary_split_survives_relayout_frames() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let boundary = owner
        .insert_child_render_object(pad, Box::new(RenderRepaintBoundary::new()))
        .expect("boundary");
    let leaf = owner
        .insert_child_render_object(boundary, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf");
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let (mut owner, tree1) = frame(owner);
    let tree1 = tree1.expect("frame 1");
    assert_eq!(
        structure(&tree1),
        vec!["Offset", "Offset", "Picture"],
        "the boundary subtree splits under its own OffsetLayer",
    );

    // Move the boundary by growing the padding; re-frame.
    {
        let tree_mut = owner.render_tree_mut();
        let entry = tree_mut
            .get_mut(pad)
            .expect("pad")
            .as_box_mut()
            .expect("box");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderPadding>()
            .expect("padding")
            .set_padding(EdgeInsets::all(px(30.0)));
    }
    owner.mark_needs_layout(pad);
    let (owner, tree2) = frame(owner);
    let tree2 = tree2.expect("frame 2");

    assert_eq!(structure(&tree2), vec!["Offset", "Offset", "Picture"]);
    // The boundary's OffsetLayer carries the NEW accumulated offset;
    // the picture inside stays rebased at zero.
    let root_id = tree2.root().expect("root");
    let boundary_layer_id = tree2.get(root_id).expect("root node").children()[0];
    let boundary_node = tree2.get(boundary_layer_id).expect("boundary node");
    let Layer::Offset(offset_layer) = boundary_node.layer() else {
        panic!("boundary layer must be an OffsetLayer");
    };
    assert_eq!(
        offset_layer.offset(),
        Offset::new(px(30.0), px(30.0)),
        "an offset-only move shows up as the layer's offset",
    );
    assert_eq!(
        first_picture_bounds(&tree2),
        flui_types::Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
        "boundary-subtree coordinates stay rebased to zero",
    );
    assert_eq!(state_offset(&owner, leaf), Offset::new(px(0.0), px(0.0)));
}

// ============================================================================
// 7. Edge cases: zero sizes and empty containers
// ============================================================================

#[test]
fn zero_size_children_and_empty_containers_survive_the_pipeline() {
    let mut owner = PipelineOwner::new();
    let row = owner.insert(Box::new(RenderFlex::row()) as BoxedRenderObject);
    // A zero-size child, an empty nested row, and a normal child.
    let zero = owner
        .insert_child_render_object(row, Box::new(RenderColoredBox::red(0.0, 0.0)))
        .expect("zero child");
    let empty_row = owner
        .insert_child_render_object(row, Box::new(RenderFlex::row()))
        .expect("empty row");
    let normal = owner
        .insert_child_render_object(row, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("normal child");

    owner.set_root_id(Some(row));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(100.0),
    )));

    let (owner, tree) = frame(owner);
    let tree = tree.expect("frame paints despite degenerate children");

    // Zero-size and empty contribute nothing to main extent: the
    // normal child sits at x = 0 + 0 + 0 = 0.
    assert_eq!(state_offset(&owner, zero), Offset::new(px(0.0), px(0.0)));
    assert_eq!(
        state_offset(&owner, empty_row),
        Offset::new(px(0.0), px(0.0))
    );
    assert_eq!(state_offset(&owner, normal), Offset::new(px(0.0), px(0.0)));

    // Only the normal child draws; degenerate nodes add no commands.
    assert_eq!(
        first_picture_bounds(&tree),
        flui_types::Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
    );
    // A zero-area child never claims a hit.
    assert_eq!(hits(&owner, 10.0, 10.0).first().copied(), Some(normal));
    assert!(!owner.has_dirty_nodes());
}

// ============================================================================
// 8. Churn stress: 20 remove+reinsert cycles with frames between
// ============================================================================

#[test]
fn repeated_churn_cycles_stay_clean_and_generations_protect_every_round() {
    let mut owner = PipelineOwner::new();
    let pad = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    owner.set_root_id(Some(pad));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut stale_ids = Vec::new();
    let mut current = owner
        .insert_child_render_object(pad, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("initial child");

    for round in 0..20u32 {
        let (next_owner, tree) = frame(owner);
        owner = next_owner;
        assert!(tree.is_some(), "round {round}: churn frame must paint");

        assert_eq!(owner.remove_render_object(current), 1, "round {round}");
        stale_ids.push(current);
        let side = 10.0 + round as f32;
        current = owner
            .insert_child_render_object(pad, Box::new(RenderColoredBox::blue(side, side)))
            .expect("reinserted child");
        owner.mark_needs_layout(pad);
    }

    let (owner, tree) = frame(owner);
    assert!(tree.is_some(), "final frame paints");
    assert!(!owner.has_dirty_nodes(), "no residue after 20 churn rounds");

    // EVERY historical id must stay dead — slot reuse never
    // resurrects an old handle, no matter how many generations passed.
    for (i, stale) in stale_ids.iter().enumerate() {
        assert!(
            owner.render_tree().get(*stale).is_none(),
            "stale id from round {i} must not resolve",
        );
    }
    assert!(owner.render_tree().get(current).is_some());
    assert_eq!(hits(&owner, 20.0, 20.0).first().copied(), Some(current));
}
