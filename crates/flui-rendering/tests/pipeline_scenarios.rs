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

use flui_foundation::geometry::{EdgeInsets, Matrix4, Offset, Point, Rect, Size};
use flui_layer::{Layer, LayerTree};
use flui_objects::{RenderClipRect, RenderColoredBox, RenderFlex, RenderPadding, RenderTransform};
use flui_rendering::{
    constraints::BoxConstraints,
    testing::{Probe, RenderTester, box_node},
};

/// Loose `0..=hi x 0..=hi` constraints (children settle at natural size).
fn loose(width: f64, height: f64) -> BoxConstraints {
    BoxConstraints::new(0.0, width, 0.0, height)
}

// ============================================================================
// 1. Deep nesting: 50 paddings of 1px each around a 10×10 box
// ============================================================================

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
    assert_eq!(run.offset(pad), Offset::new(0.0, 0.0));
    assert_eq!(run.offset(scaler), Offset::new(50.0, 0.0));
    assert_eq!(run.offset(clip), Offset::new(70.0, 0.0));

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
    let owner = run.owner();
    let scaler_node = owner.render_tree().get(scaler).expect("scaler node");
    // The laid-out size (from RenderState) resolves the alignment origin, so
    // feed it in rather than reading a cached object field.
    //
    // Read through `apply_paint_transform` rather than `paint_effects().transform`:
    // `paint_effects().transform` is `None` for a pure translation, which `paint`
    // applies as a plain child offset instead of a layer, so it is not a total
    // accessor for the effective matrix. `apply_paint_transform` is unconditional
    // and, with a zero child offset, yields exactly that matrix — the one this
    // test then expects to find conjugated in the composited layer.
    //
    // Note this test's `structure()` assertion above is the workspace's only
    // oracle for `RenderTransform` emitting exactly ONE transform layer. Its
    // `paint` must not re-open a `with_transform` scope around the layer the
    // pipeline already pushes from `paint_effects`; nothing in
    // `flui-objects`' own unit tests would catch that, but this list would.
    let scaler_size = scaler_node
        .size()
        .unwrap_or(flui_foundation::geometry::Size::ZERO);
    let local = {
        let mut m = Matrix4::IDENTITY;
        scaler_node.box_render_object().apply_paint_transform(
            0,
            flui_foundation::geometry::Offset::ZERO,
            scaler_size,
            &mut m,
        );
        m
    };
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
        walk(tree, tree.root(), &mut out);
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
        walk(tree, tree.root(), &mut out);
        out
    };
    assert_eq!(
        clip_rects,
        vec![Rect::from_origin_size(
            Point::new(70.0, 0.0),
            Size::new(40.0, 40.0),
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
    run.owner_mut().mark_needs_paint(child);
    let report = run.pump();
    assert!(report.painted, "paint-only frame repaints");
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(5.0, 5.0, 45.0, 45.0)),
        "geometry unchanged on a paint-only frame",
    );

    // Frame 3: layout invalidation — padding grows, offsets move.
    run.update::<RenderPadding>(pad, |padding| {
        assert_eq!(
            padding.set_padding(EdgeInsets::all(20.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
    });
    let report = run.pump();
    assert!(report.painted, "layout frame repaints");
    assert_eq!(run.offset(child), Offset::new(20.0, 20.0));
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(20.0, 20.0, 60.0, 60.0)),
        "relayout must repaint at the NEW offsets",
    );
}

// ============================================================================
// 4. Idle stability: clean frames produce no output
// ============================================================================

// ============================================================================
// 5. Removal churn under one parent
// ============================================================================

// ============================================================================
// 6. Repaint-boundary split survives re-frames
// ============================================================================

// ============================================================================
// 7. Edge cases: zero sizes and empty containers
// ============================================================================

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
        let side = 10.0 + round as f64;
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

/// Removing a non-leaf directly must dispose the whole subtree, not just the
/// removed node's own slot.
///
/// Regression this guards: an earlier remove path freed only the child's
/// slot and orphaned every descendant in the slab (leak) while leaving their
/// dirty entries behind. The cascade dispose frees the subtree and evicts
/// its dirty entries; the parent reflows clean.
#[test]
fn removing_a_non_leaf_directly_disposes_the_subtree_without_leaking() {
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

    run.owner_mut().remove_render_object(branch);
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
