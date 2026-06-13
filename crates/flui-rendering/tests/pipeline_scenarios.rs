//! Render-object foundation under realistic multi-frame scenarios.
//!
//! Each scenario drives the REAL frame pipeline (layout → compositing →
//! paint) the bindings use in production, then asserts on observable
//! outputs: committed offsets, layer-tree structure, picture bounds, hit
//! paths, dirty-queue hygiene, and — via the `Diagnosticable`-backed
//! diagnostics — the render objects' self-described configuration.
//!
//! These scenarios are expressed with the `flui_rendering::testing` harness
//! (`RenderTester` / `FrameRun` / `Probe`): declarative tree specs, symmetric
//! `run_frame`, `update` + `pump` for multi-frame mutation, and structured
//! property queries. The advanced layer-internal checks (transform matrices,
//! clip shapes, offset layers) still walk the produced `LayerTree` directly.
//!
//! Scenarios:
//! 1. deep nesting — offsets accumulate through a 50-deep padding chain and
//!    a single merged picture comes out;
//! 2. mixed tree — flex + padding + transform + clip in one frame;
//! 3. invalidation round-trips — paint-only then layout-changing frames;
//! 4. idle stability — frames after a clean one produce NO output;
//! 5. removal churn — remove + reinsert under one parent;
//! 6. repaint-boundary subtree — the boundary's OffsetLayer split survives
//!    re-frames;
//! 7. edge cases — zero sizes and empty containers;
//! 8. churn stress — 20 remove+reinsert cycles.

use flui_layer::{Layer, LayerTree};
use flui_rendering::{
    constraints::BoxConstraints,
    objects::{
        RenderClipRect, RenderColoredBox, RenderFlex, RenderPadding, RenderRepaintBoundary,
        RenderTransform,
    },
    testing::{Probe, RenderTester, box_node},
};
use flui_types::{EdgeInsets, Matrix4, Offset, Point, Rect, Size, geometry::px};

/// Loose `0..=hi x 0..=hi` constraints (children settle at natural size).
fn loose(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(width), px(0.0), px(height))
}

// ============================================================================
// 1. Deep nesting: 50 paddings of 1px each around a 10×10 box
// ============================================================================

#[test]
fn deep_padding_chain_accumulates_offsets_and_merges_one_picture() {
    // Build leaf-first, then wrap in 50 padding(1px) layers.
    let mut spec = box_node(RenderColoredBox::red(10.0, 10.0)).label("leaf");
    for _ in 0..50 {
        spec = box_node(RenderPadding::all(1.0)).child(spec);
    }
    let run = RenderTester::mount(spec)
        .with_constraints(loose(500.0, 500.0))
        .run_frame();

    let leaf = run.id("leaf");
    let root = run.root();

    // Every padding contributes (1,1) to ITS child — the leaf's committed
    // offset relative to its parent is exactly (1,1)...
    assert_eq!(run.offset(leaf), Offset::new(px(1.0), px(1.0)));
    // ...and the picture's ABSOLUTE bounds carry the full 50-deep
    // accumulation: the leaf draws at (50,50)..(60,60).
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(50.0), px(50.0), px(60.0), px(60.0))),
        "accumulated origins must be baked through the whole chain",
    );
    assert_eq!(
        run.structure(),
        vec!["Offset", "Picture"],
        "51 inline nodes still merge into ONE picture",
    );

    // Hit straight through all 50 levels, leaf-first path of 51 ids.
    let path = run.hit(55.0, 55.0);
    assert_eq!(path.len(), 51);
    assert_eq!(path[0], leaf, "leaf-first");
    assert_eq!(path[50], root, "root last");
    assert!(
        run.hit(5.0, 5.0).is_empty(),
        "the border region (inside paddings, outside the box) misses",
    );

    // The leaf self-describes its color through the full pipeline.
    assert_eq!(
        run.property(leaf, "color").as_deref(),
        Some("[1.0, 0.0, 0.0, 1.0]"),
    );
}

// ============================================================================
// 2. Mixed tree in one frame
// ============================================================================

#[test]
fn mixed_flex_padding_transform_clip_frame() {
    // row[ padding(5){red 40}, transform(scale2){blue 20}, clip{green 40} ]
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderPadding::all(5.0))
                    .label("pad")
                    .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("red")),
            )
            .child(
                box_node(RenderTransform::scale(2.0, 2.0))
                    .label("scaler")
                    .child(box_node(RenderColoredBox::blue(20.0, 20.0)).label("blue")),
            )
            .child(
                box_node(RenderClipRect::hard_edge())
                    .label("clip")
                    .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("green")),
            ),
    )
    .with_constraints(loose(300.0, 100.0))
    .run_frame();

    let pad = run.id("pad");
    let red = run.id("red");
    let scaler = run.id("scaler");
    let blue = run.id("blue");
    let clip = run.id("clip");
    let green = run.id("green");

    // Row children sit at main-axis offsets 0 / 50 / 70.
    assert_eq!(run.offset(pad), Offset::new(px(0.0), px(0.0)));
    assert_eq!(run.offset(scaler), Offset::new(px(50.0), px(0.0)));
    assert_eq!(run.offset(clip), Offset::new(px(70.0), px(0.0)));

    // Layer splits happen exactly where semantics demand them.
    assert_eq!(
        run.structure(),
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

    // Effect/clip layers anchor at the NODE origin (Flutter pushTransform:
    // T(o)·M·T(−o); pushClipRect: clipRect.shift(offset)).
    let tree = run.layer_tree().expect("frame paints");
    let local = run
        .owner()
        .render_tree()
        .get(scaler)
        .expect("scaler node")
        .box_render_object()
        .paint_transform()
        .expect("scale(2,2) reports a paint transform");
    let expected =
        Matrix4::translation(50.0, 0.0, 0.0) * local * Matrix4::translation(-50.0, 0.0, 0.0);
    assert_ne!(
        expected, local,
        "sanity: at a non-zero origin the conjugation must differ from \
         the raw local matrix",
    );
    let transform_matrices: Vec<Matrix4> = {
        fn walk(tree: &LayerTree, id: flui_foundation::LayerId, out: &mut Vec<Matrix4>) {
            let node = tree.get(id).expect("live layer id");
            if let Layer::Transform(t) = node.layer() {
                out.push(*t.transform());
            }
            for &c in node.children() {
                walk(tree, c, out);
            }
        }
        let mut out = Vec::new();
        walk(tree, tree.root().expect("root"), &mut out);
        out
    };
    assert_eq!(
        transform_matrices,
        vec![expected],
        "the scaler's TransformLayer must carry the origin-conjugated matrix",
    );
    let clip_rects: Vec<Rect> = {
        fn walk(tree: &LayerTree, id: flui_foundation::LayerId, out: &mut Vec<Rect>) {
            let node = tree.get(id).expect("live layer id");
            if let Layer::ClipRect(c) = node.layer() {
                out.push(c.clip_rect());
            }
            for &child in node.children() {
                walk(tree, child, out);
            }
        }
        let mut out = Vec::new();
        walk(tree, tree.root().expect("root"), &mut out);
        out
    };
    assert_eq!(
        clip_rects,
        vec![Rect::from_origin_size(
            Point::new(px(70.0), px(0.0)),
            Size::new(px(40.0), px(40.0)),
        )],
        "the clip shape must be shifted by the node origin",
    );

    // Region hits. Scaled blue's visual extent is 50..90 and overlaps the
    // clip at 70..110 — in the overlap the later sibling (green) wins.
    assert_eq!(run.hit(20.0, 20.0).first().copied(), Some(red));
    assert_eq!(run.hit(60.0, 10.0).first().copied(), Some(blue));
    assert_eq!(run.hit(80.0, 20.0).first().copied(), Some(green));
    assert_eq!(run.hit(100.0, 20.0).first().copied(), Some(green));

    // Self-description: the scaler reports its (local) transform and the
    // padding its insets — config the geometry assertions don't cover.
    assert!(
        run.property(scaler, "transform").is_some(),
        "the transform node self-describes its matrix",
    );
    assert!(
        run.property(pad, "padding").is_some(),
        "the padding node self-describes its insets",
    );
    assert_eq!(
        run.descendant_property("RenderFlex", "direction")
            .as_deref(),
        Some("Horizontal"),
    );
    assert!(
        run.descendant_property("RenderPadding", "padding")
            .is_some(),
        "padding self-describes its insets",
    );
}

// ============================================================================
// 3. Invalidation round-trips across three frames
// ============================================================================

#[test]
fn paint_only_then_layout_invalidations_round_trip() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let pad = run.root();
    let child = run.id("child");
    assert!(run.painted(), "frame 1 paints");

    // Frame 2: paint-only invalidation — no layout change, fresh tree.
    run.owner_mut().add_node_needing_paint(child, 1);
    let report = run.pump();
    assert!(report.painted, "paint-only frame repaints");
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(5.0), px(5.0), px(45.0), px(45.0))),
        "geometry unchanged on a paint-only frame",
    );

    // Frame 3: layout invalidation — padding grows, offsets move.
    run.update::<RenderPadding>(pad, |padding| {
        padding.set_padding(EdgeInsets::all(px(20.0)));
    });
    let report = run.pump();
    assert!(report.painted, "layout frame repaints");
    assert_eq!(run.offset(child), Offset::new(px(20.0), px(20.0)));
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(20.0), px(20.0), px(60.0), px(60.0))),
        "relayout must repaint at the NEW offsets",
    );
}

// ============================================================================
// 4. Idle stability: clean frames produce no output
// ============================================================================

#[test]
fn clean_frames_after_first_produce_no_layer_tree() {
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();
    assert!(run.painted(), "frame 1 paints");

    for n in 2..=5 {
        let report = run.pump();
        assert!(
            !report.painted,
            "frame {n} has no dirty work and must produce no output — \
             leftover dirty state here means an idle app burns frames",
        );
    }
    assert!(run.is_clean());
}

// ============================================================================
// 5. Removal churn under one parent
// ============================================================================

#[test]
fn remove_and_reinsert_child_keeps_pipeline_clean() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("first")),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let pad = run.root();
    let first_child = run.id("first");

    // Remove the child, reinsert a different one (slot reuse).
    assert_eq!(run.owner_mut().remove_render_object(first_child), 1);
    let second_child = run
        .owner_mut()
        .insert_child_render_object(pad, Box::new(RenderColoredBox::blue(60.0, 60.0)))
        .expect("second child");
    run.owner_mut().mark_needs_layout(pad);
    let report = run.pump();
    assert!(report.painted, "churn frame paints");

    assert!(
        run.owner().render_tree().get(first_child).is_none(),
        "the stale id must not resolve to the reused slot (ABA guard)",
    );
    assert_eq!(run.offset(second_child), Offset::new(px(5.0), px(5.0)));
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(5.0), px(5.0), px(65.0), px(65.0))),
        "the NEW child paints at the padded origin",
    );
    assert_eq!(
        run.hit(30.0, 30.0).first().copied(),
        Some(second_child),
        "hits route to the new child, never the stale id",
    );
    assert!(run.is_clean(), "queues drain fully after churn");
}

// ============================================================================
// 6. Repaint-boundary split survives re-frames
// ============================================================================

#[test]
fn repaint_boundary_split_survives_relayout_frames() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0)).child(
            box_node(RenderRepaintBoundary::new())
                .label("boundary")
                .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("leaf")),
        ),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let pad = run.root();
    let leaf = run.id("leaf");
    assert_eq!(
        run.structure(),
        vec!["Offset", "Offset", "Picture"],
        "the boundary subtree splits under its own OffsetLayer",
    );

    // Move the boundary by growing the padding; re-frame.
    run.update::<RenderPadding>(pad, |padding| {
        padding.set_padding(EdgeInsets::all(px(30.0)));
    });
    run.pump();

    assert_eq!(run.structure(), vec!["Offset", "Offset", "Picture"]);
    // The boundary's OffsetLayer carries the NEW accumulated offset; the
    // picture inside stays rebased at zero.
    let tree = run.layer_tree().expect("frame 2");
    let root_id = tree.root().expect("root");
    let boundary_layer_id = tree.get(root_id).expect("root node").children()[0];
    let boundary_node = tree.get(boundary_layer_id).expect("boundary node");
    let Layer::Offset(offset_layer) = boundary_node.layer() else {
        panic!("boundary layer must be an OffsetLayer");
    };
    assert_eq!(
        offset_layer.offset(),
        Offset::new(px(30.0), px(30.0)),
        "an offset-only move shows up as the layer's offset",
    );
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0))),
        "boundary-subtree coordinates stay rebased to zero",
    );
    assert_eq!(run.offset(leaf), Offset::new(px(0.0), px(0.0)));
}

// ============================================================================
// 7. Edge cases: zero sizes and empty containers
// ============================================================================

#[test]
fn zero_size_children_and_empty_containers_survive_the_pipeline() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(0.0, 0.0)).label("zero"))
            .child(box_node(RenderFlex::row()).label("empty_row"))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("normal")),
    )
    .with_constraints(loose(200.0, 100.0))
    .run_frame();

    let zero = run.id("zero");
    let empty_row = run.id("empty_row");
    let normal = run.id("normal");

    // Zero-size and empty contribute nothing to the main extent.
    assert_eq!(run.offset(zero), Offset::new(px(0.0), px(0.0)));
    assert_eq!(run.offset(empty_row), Offset::new(px(0.0), px(0.0)));
    assert_eq!(run.offset(normal), Offset::new(px(0.0), px(0.0)));

    // Only the normal child draws; degenerate nodes add no commands.
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0))),
    );
    // A zero-area child never claims a hit.
    assert_eq!(run.hit(10.0, 10.0).first().copied(), Some(normal));
    assert!(run.is_clean());
}

// ============================================================================
// 8. Churn stress: 20 remove+reinsert cycles with frames between
// ============================================================================

#[test]
fn repeated_churn_cycles_stay_clean_and_generations_protect_every_round() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("initial")),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let pad = run.root();
    let mut current = run.id("initial");
    assert!(run.painted(), "initial frame paints");

    let mut stale_ids = Vec::new();
    for round in 0..20u32 {
        assert_eq!(
            run.owner_mut().remove_render_object(current),
            1,
            "round {round}"
        );
        stale_ids.push(current);
        let side = 10.0 + round as f32;
        current = run
            .owner_mut()
            .insert_child_render_object(pad, Box::new(RenderColoredBox::blue(side, side)))
            .expect("reinserted child");
        run.owner_mut().mark_needs_layout(pad);
        let report = run.pump();
        assert!(report.painted, "round {round}: churn frame must paint");
    }

    assert!(run.is_clean(), "no residue after 20 churn rounds");

    // EVERY historical id must stay dead — slot reuse never resurrects an
    // old handle, no matter how many generations passed.
    for (i, stale) in stale_ids.iter().enumerate() {
        assert!(
            run.owner().render_tree().get(*stale).is_none(),
            "stale id from round {i} must not resolve",
        );
    }
    assert!(run.owner().render_tree().get(current).is_some());
    assert_eq!(run.hit(20.0, 20.0).first().copied(), Some(current));
}

// ====================================================================
// Deferred mutations integration tests
// ====================================================================

#[test]
fn deferred_remove_during_layout_removes_child_after_pass() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();

    let root = run.root();
    let child = run.id("child");
    assert!(run.owner().render_tree().get(child).is_some());

    // Enqueue deferred remove during "layout" (simulated)
    run.owner_mut().defer_remove(root, child);
    assert_eq!(run.owner().deferred_mutation_count(), 1);

    // Pump triggers layout → drain → apply
    run.owner_mut().mark_needs_layout(root);
    run.pump();

    // Child should be removed
    assert!(
        run.owner().render_tree().get(child).is_none(),
        "deferred remove should have removed the child after layout pass"
    );
    assert_eq!(run.owner().deferred_mutation_count(), 0);
}

#[test]
fn deferred_update_on_nonexistent_target_is_silent_noop() {
    // Update targeting a non-existent RenderId should not panic
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_constraints(loose(200.0, 200.0))
        .run_frame();

    let fake_id = flui_foundation::RenderId::new(9999);
    run.owner_mut().defer_update(
        fake_id,
        Box::new(|_obj: &mut dyn std::any::Any| {
            panic!("should not be called for non-existent target");
        }),
    );

    let root = run.root();
    run.owner_mut().mark_needs_layout(root);
    run.pump(); // should not panic
}

#[test]
fn deferred_mutations_preserved_across_drain() {
    // After drain, new mutations can be enqueued
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_constraints(loose(200.0, 200.0))
        .run_frame();

    let root = run.root();
    let fake = flui_foundation::RenderId::new(9999);

    // First batch
    run.owner_mut().defer_remove(root, fake);
    assert_eq!(run.owner().deferred_mutation_count(), 1);

    // Drain via pump
    run.owner_mut().mark_needs_layout(root);
    run.pump();
    assert_eq!(run.owner().deferred_mutation_count(), 0);

    // Second batch — reuse is fine
    run.owner_mut().defer_remove(root, fake);
    assert_eq!(run.owner().deferred_mutation_count(), 1);
}

/// A deferred `Insert` must schedule the new child for layout AND paint.
///
/// Regression: the apply path previously inserted the node but never
/// enqueued it, so it carried `NEEDS_LAYOUT` while being absent from every
/// dirty queue — laid out never, painted never (an invisible child forever).
/// The queue drains after the layout pass, so the child appears in the tree
/// this frame and settles (lays out + paints) on the next.
#[test]
fn deferred_insert_box_schedules_layout_and_paint_for_new_child() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("first")),
    )
    .with_constraints(loose(300.0, 100.0))
    .run_frame();
    let root = run.root();
    let before = run
        .owner()
        .render_tree()
        .get(root)
        .expect("root node")
        .children()
        .to_vec();
    assert_eq!(before.len(), 1);

    // Enqueue an append and run a frame: the drain inserts + schedules.
    run.owner_mut()
        .defer_insert_box(root, Box::new(RenderColoredBox::blue(40.0, 40.0)), None);
    run.owner_mut().mark_needs_layout(root);
    run.pump();

    let after = run
        .owner()
        .render_tree()
        .get(root)
        .expect("root node")
        .children()
        .to_vec();
    assert_eq!(
        after.len(),
        2,
        "deferred insert appended a child to the row"
    );
    let new_child = *after
        .iter()
        .find(|id| !before.contains(id))
        .expect("the appended child has a fresh id");

    // The child was scheduled, not orphaned: a settle frame lays it out into
    // the row's second slot and drains every dirty queue. Before the fix the
    // child was never enqueued, so it stayed at the origin and unpainted.
    run.pump();
    assert!(
        run.owner().render_tree().get(new_child).is_some(),
        "inserted child is still live after settling",
    );
    assert_eq!(
        run.offset(new_child),
        Offset::new(px(40.0), px(0.0)),
        "the new child flows into the row after the existing 40px child",
    );
    assert!(run.is_clean(), "queues drain fully once the insert settles");
}

/// A deferred `Insert` with an explicit index lands at that position, not
/// appended. Regression: the apply path discarded `index` and always pushed.
#[test]
fn deferred_insert_box_honors_requested_index() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("a"))
            .child(box_node(RenderColoredBox::green(10.0, 10.0)).label("b")),
    )
    .with_constraints(loose(300.0, 100.0))
    .run_frame();
    let root = run.root();
    let a = run.id("a");
    let b = run.id("b");

    // Insert between the two existing children.
    run.owner_mut()
        .defer_insert_box(root, Box::new(RenderColoredBox::blue(10.0, 10.0)), Some(1));
    run.owner_mut().mark_needs_layout(root);
    run.pump();

    let children = run
        .owner()
        .render_tree()
        .get(root)
        .expect("root node")
        .children()
        .to_vec();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], a, "first sibling stays first");
    assert_eq!(children[2], b, "second sibling shifts right");
    assert!(
        children[1] != a && children[1] != b,
        "the inserted child occupies the middle slot, not the tail",
    );
}

/// A deferred `Remove` of a non-leaf must dispose the whole subtree.
///
/// Regression: the apply path used `remove_shallow`, which freed only the
/// child's slot and orphaned every descendant in the slab (leak) while
/// leaving their dirty entries behind. The cascade dispose frees the subtree
/// and evicts its dirty entries; the parent reflows clean.
#[test]
fn deferred_remove_non_leaf_disposes_subtree_without_leaking() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row()).child(
            box_node(RenderPadding::all(5.0))
                .label("branch")
                .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("leaf")),
        ),
    )
    .with_constraints(loose(200.0, 200.0))
    .run_frame();
    let root = run.root();
    let branch = run.id("branch");
    let leaf = run.id("leaf");
    assert!(run.owner().render_tree().get(branch).is_some());
    assert!(run.owner().render_tree().get(leaf).is_some());

    run.owner_mut().defer_remove(root, branch);
    run.owner_mut().mark_needs_layout(root);
    run.pump();

    assert!(
        run.owner().render_tree().get(branch).is_none(),
        "the removed branch is gone",
    );
    assert!(
        run.owner().render_tree().get(leaf).is_none(),
        "its descendant is freed, not orphaned in the slab",
    );

    // The parent was re-dirtied by the removal; a settle frame drains it.
    run.pump();
    assert!(
        run.is_clean(),
        "no stale dirty entries survive the disposed subtree",
    );
}
