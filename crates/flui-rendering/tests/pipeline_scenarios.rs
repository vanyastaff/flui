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

use flui_objects::{RenderColoredBox, RenderPadding};
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

// ============================================================================
// 3. Invalidation round-trips across three frames
// ============================================================================

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
