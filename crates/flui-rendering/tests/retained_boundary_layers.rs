//! A clean repaint boundary reuses its previous frame's layers.
//!
//! `run_paint` used to descend the whole tree every pass and rebuild every
//! layer, which `benches/paint.rs` measured: eight dirty boundaries cost the
//! same as one. Now a boundary absent from the paint queue is grafted from
//! `retained_boundaries` instead of repainted.
//!
//! That is sound only because the queue is exact — `run_layout` marks every
//! boundary layout touched and `mark_needs_paint` marks the rest — so the two
//! properties worth pinning are that reuse HAPPENS and that it produces the
//! same tree painting would have.

use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::{
    RenderClipRRect, RenderColoredBox, RenderFlex, RenderOpacity, RenderPadding,
    RenderRepaintBoundary, RenderTransform,
};
use flui_painting::styling::{BorderRadius, BorderRadiusExt};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{
        TreeNode, box_node,
        inspect::{clip_rrects, first_opacity_alpha, first_transform_matrix},
        tree, update_render_object,
    },
};

/// Root flex row → N boundaries, each wrapping a coloured leaf.
///
/// The root is a `RenderFlex`, not a `RenderColoredBox` (which paints no
/// children, so the walk would stop there) and not a `RenderPadding` (which
/// takes ONE child, so every boundary after the first would be unpainted and
/// the test degenerate).
fn spec(n: usize) -> TreeNode {
    let mut root = box_node(RenderFlex::row());
    for i in 0..n {
        // The opacity is not decoration: it makes each boundary's output a
        // NESTED layer rather than one flat picture. Without it a graft that
        // reparented everything onto the boundary's root would produce an
        // identical fingerprint and the equivalence test would have no teeth.
        let content =
            box_node(RenderOpacity::new(0.5)).child(box_node(RenderColoredBox::red(10.0, 10.0)));
        let mut boundary = box_node(RenderRepaintBoundary::new()).child(content);
        if i == 0 {
            boundary = boundary.label("first");
        }
        root = root.child(boundary);
    }
    root
}

fn mount(n: usize) -> (PipelineOwner<flui_rendering::pipeline::Idle>, TreeIds) {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(&mut owner, spec(n));
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let first = registry.get("first").expect("first boundary is labelled");
    (owner, TreeIds { first })
}

struct TreeIds {
    first: flui_foundation::RenderId,
}

/// Fingerprint of a layer tree: each node's layer KIND and child count, in a
/// deterministic walk.
///
/// Layer ids differ between frames (each frame builds a fresh slab), so the
/// comparison has to be structural. Child counts alone catch a graft that
/// dropped, duplicated or reparented a node — but they are blind to one that
/// preserves the shape while emitting the wrong layer variant, so the kind is
/// part of the fingerprint too. What it still does NOT cover is layer
/// PROPERTIES (an alpha, an offset) or picture contents; tests that care about
/// those assert them directly, and pixel equivalence belongs to the GPU
/// readback suite.
fn fingerprint(t: &flui_layer::LayerTree) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    let mut stack = vec![t.root()];
    while let Some(id) = stack.pop() {
        // Not `unwrap_or(&[])`: the walk only ever visits ids the tree just
        // handed out, so a `None` here means the graft minted a dangling
        // reference — the oracle must fail on that, not treat it as a leaf.
        let children = t
            .children(id)
            .expect("every id in this walk came from the tree itself");
        let kind = t
            .get(id)
            .map_or("<missing>", |node| node.layer().kind_name());
        out.push((kind, children.len()));
        for &child in children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

/// A second frame with one dirty boundary produces the same layer tree a
/// full repaint would.
///
/// The comparison is against a SECOND owner that never retains anything,
/// because it paints its first frame — so any divergence is the graft's, not
/// the scenario's.
#[test]
fn a_retained_frame_matches_what_a_full_repaint_produces() {
    const N: usize = 8;

    let (owner, ids) = mount(N);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // Dirty exactly one boundary; the other seven must be reused.
    owner.mark_needs_paint(ids.first);
    let (owner, result) = owner.run_frame();
    let retained_tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    // Reference: a fresh owner painting the same tree from scratch.
    let (reference_owner, _) = mount(N);
    let (_, result) = reference_owner.run_frame();
    let full_tree = result
        .expect("reference frame")
        .expect("reference frame produces a layer tree");

    assert_eq!(
        fingerprint(&retained_tree),
        fingerprint(&full_tree),
        "a frame that grafted seven clean boundaries must produce the same \
         layer structure as one that painted all eight"
    );
    assert_eq!(
        retained_tree.len(),
        full_tree.len(),
        "and the same number of layers"
    );
}

/// Reuse actually happens: a clean boundary's content is not repainted.
///
/// The oracle is a leaf that counts its own `paint` calls. `RenderRepaintBoundary`
/// has a `paint_count` field that looks made for this, but nothing in the
/// pipeline ever calls `increment_paint_count` — it is a dead diagnostic, and a
/// test reading it would always see zero.
#[test]
fn the_content_of_a_clean_boundary_is_not_repainted() {
    use flui_foundation::Leaf;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    /// A leaf that records how many times it was painted.
    #[derive(Debug)]
    struct CountingLeaf(Arc<AtomicUsize>);

    impl flui_foundation::Diagnosticable for CountingLeaf {}

    impl RenderBox for CountingLeaf {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constrain(Size::new(10.0, 10.0))
        }

        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }
    }

    let dirty_paints = Arc::new(AtomicUsize::new(0));
    let clean_paints = Arc::new(AtomicUsize::new(0));

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("dirty")
                    .child(box_node(CountingLeaf(Arc::clone(&dirty_paints)))),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(CountingLeaf(Arc::clone(&clean_paints)))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let dirty_id = registry.get("dirty").expect("dirty boundary is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    let clean_after_first = clean_paints.load(Ordering::Relaxed);
    let dirty_after_first = dirty_paints.load(Ordering::Relaxed);
    assert!(
        clean_after_first >= 1 && dirty_after_first >= 1,
        "precondition: the first frame paints both leaves; got clean={clean_after_first} \
         dirty={dirty_after_first}"
    );

    // Two more frames, each with only the first boundary dirty.
    for _ in 0..2 {
        owner.mark_needs_paint(dirty_id);
        let (next, result) = owner.run_frame();
        result.expect("frame");
        owner = next;
    }

    assert_eq!(
        clean_paints.load(Ordering::Relaxed),
        clean_after_first,
        "the clean boundary's content must not repaint — its layers are grafted"
    );
    assert!(
        dirty_paints.load(Ordering::Relaxed) > dirty_after_first,
        "precondition: the dirtied boundary keeps repainting, so the assertion \
         above is about retention and not about paint being skipped wholesale"
    );
}

/// A dirty boundary nested inside a clean one still repaints.
///
/// `mark_needs_paint` stops at the nearest established boundary, so
/// invalidating something inside an INNER boundary queues only that inner
/// boundary — the outer one is clean. Grafting the outer boundary replays its
/// cached subtree, which contains the inner boundary's OLD layers, and the
/// inner boundary is never descended into. Its updated pixels never reach the
/// screen, and the residue scan clears its dirty flag so the next frame does
/// not retry either.
#[test]
fn a_dirty_boundary_nested_in_a_clean_one_still_repaints() {
    use flui_foundation::Leaf;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Debug)]
    struct CountingLeaf(Arc<AtomicUsize>);

    impl flui_foundation::Diagnosticable for CountingLeaf {}

    impl RenderBox for CountingLeaf {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constrain(Size::new(10.0, 10.0))
        }

        fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }
    }

    let inner_paints = Arc::new(AtomicUsize::new(0));

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            // Outer boundary, never dirtied.
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderOpacity::new(0.5)).child(
                    // Inner boundary, the one that changes.
                    box_node(RenderRepaintBoundary::new())
                        .label("inner")
                        .child(box_node(CountingLeaf(Arc::clone(&inner_paints)))),
                ),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let inner_id = registry.get("inner").expect("inner boundary is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    let after_first = inner_paints.load(Ordering::Relaxed);
    assert!(
        after_first >= 1,
        "precondition: the first frame paints the inner leaf; got {after_first}"
    );

    // Dirty the INNER boundary only. The outer stays clean.
    owner.mark_needs_paint(inner_id);
    let (owner, result) = owner.run_frame();
    result.expect("second frame");

    assert!(
        inner_paints.load(Ordering::Relaxed) > after_first,
        "a dirty boundary must repaint even when its enclosing boundary is \
         reused; got {} paints, unchanged from frame 1 — the outer graft \
         replayed the inner boundary's stale layers and never descended",
        inner_paints.load(Ordering::Relaxed)
    );
    drop(owner);
}

// ---------------------------------------------------------------------------
// The control for `paint/opacity_tick` (issue #536)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Composited-layer updates: rebuild a node's own layers, replay the rest
// ---------------------------------------------------------------------------

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A leaf that records how many times it was painted.
///
/// `RenderRepaintBoundary::paint_count` looks made for this, but nothing in
/// the pipeline calls `increment_paint_count` — it is a dead diagnostic, and a
/// test reading it always sees zero.
#[derive(Debug)]
struct PaintCounter(Arc<AtomicUsize>);

impl flui_foundation::Diagnosticable for PaintCounter {}

impl flui_rendering::traits::RenderBox for PaintCounter {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        ctx.constrain(Size::new(10.0, 10.0))
    }

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Leaf>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

/// Root row → [boundary → opacity → counting leaf, boundary → counting leaf].
///
/// The opacity sits INSIDE a repaint boundary rather than being one: that is
/// the whole design under test — a node's own effect layers are addressable
/// within the enclosing boundary's retained capture, so nothing has to be
/// promoted to a boundary to get an update-only commit.
///
/// The second branch exists so a later frame can be forced to paint without
/// touching the first, which is the only way to observe what the STORED
/// capture holds.
fn mount_opacity_under_boundary(
    opacity: f64,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Arc<AtomicUsize>,
) {
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderOpacity::new(opacity))
                        .label("opacity")
                        .child(box_node(PaintCounter(Arc::clone(&painted)))),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("sibling")
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, opacity_id, sibling, painted)
}

/// Applies a new opacity through the same seam a widget rebuild uses:
/// the setter reports an impact, the owner applies it.
fn set_opacity(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    value: f64,
) {
    update_render_object::<RenderOpacity, _>(owner, id, |o| o.set_opacity(value));
}

/// An alpha change updates the emitted layer without repainting the subtree.
///
/// Both halves are asserted and neither alone is evidence: the paint count
/// alone passes if nothing painted at all, and the alpha alone passes on a
/// full repaint.
#[test]
fn an_alpha_change_updates_the_layer_without_repainting_the_subtree() {
    let (owner, opacity_id, _sibling, painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_opacity(&mut owner, opacity_id, 0.25);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the subtree under the opacity must NOT repaint for an alpha-only change",
    );
    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "and the emitted OpacityLayer must carry the new alpha",
    );
}

/// A repaint requested in the same frame wins over a layer update.
///
/// Flutter: `markNeedsCompositedLayerUpdate` returns early when `_needsPaint`,
/// and `flushPaint` dispatches on `_needsPaint` first. A repaint is always a
/// valid way to serve an update, so the alpha must be right either way — the
/// observable difference is that the subtree repaints.
#[test]
fn a_repaint_in_the_same_frame_wins_over_a_layer_update() {
    let (owner, opacity_id, _sibling, painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);

    set_opacity(&mut owner, opacity_id, 0.25);
    owner.mark_needs_paint(opacity_id);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > before,
        "an explicit paint mark must repaint the subtree, not take the cheap arm",
    );
    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "and the repaint must still produce the new alpha",
    );
}

/// An alpha leaving the layered range drops the layer entirely, which a patch
/// cannot express — so paint is the fallback.
///
/// It does NOT reach the paint phase's structure guard, despite the shape:
/// `set_opacity(0.5 -> 1.0)` flips `needs_compositing`, so the impact carries
/// `COMPOSITING_BITS` (which contains `PAINT`), the update mark refuses itself,
/// and `layer_patches_for` is never called. What this pins is the IMPACT
/// ALGEBRA — that a shape change reports a repaint at the setter. The guard
/// itself is pinned by `an_effect_layer_shape_change_falls_back_to_a_repaint`'s
/// count arm.
#[test]
fn a_structural_alpha_change_falls_back_to_a_repaint() {
    let (owner, opacity_id, _sibling, painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);

    set_opacity(&mut owner, opacity_id, 1.0);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > before,
        "losing the opacity layer is a structural change and must repaint",
    );
    assert_eq!(
        first_opacity_alpha(&tree),
        None,
        "a fully opaque node emits no OpacityLayer",
    );
}

/// Number of `PictureLayer`s in a tree.
fn picture_count(t: &flui_layer::LayerTree) -> usize {
    fn walk(t: &flui_layer::LayerTree, id: flui_foundation::LayerId) -> usize {
        let Some(node) = t.get(id) else { return 0 };
        let self_count = usize::from(matches!(node.layer(), flui_layer::Layer::Picture(_)));
        self_count + node.children().iter().map(|&c| walk(t, c)).sum::<usize>()
    }
    walk(t, t.root())
}

/// A real repaint queued alongside an update is not downgraded by a failed pass.
///
/// A boundary can be queued for BOTH: some node's layer property moved, and
/// something it paints changed. The walk clears `needs_paint` as it goes, so
/// after a poisoned pass the retry sees a boundary that no longer needs paint
/// while both queue entries survive — and would reclassify it as update-only,
/// graft the pre-error capture, and patch just the effect layer. The content
/// change that required the repaint would stay stale.
///
/// Note this is a regression the update path can introduce and the paint path
/// alone cannot: without an update queued, the still-queued boundary is in
/// `dirty_set` and refuses the graft outright.
#[test]
fn a_failed_pass_does_not_downgrade_a_real_repaint_to_an_update() {
    use std::sync::atomic::AtomicBool;

    #[derive(Debug)]
    struct PoisonOnDemand(Arc<AtomicBool>);

    impl flui_foundation::Diagnosticable for PoisonOnDemand {}

    impl flui_rendering::traits::RenderBox for PoisonOnDemand {
        type Arity = flui_foundation::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_foundation::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(10.0, 10.0))
        }

        fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Leaf>) {
            assert!(
                !self.0.load(Ordering::Relaxed),
                "PoisonOnDemand: armed, poisoning this paint pass on purpose",
            );
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_foundation::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }
    }

    let armed = Arc::new(AtomicBool::new(false));
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderOpacity::new(0.5))
                        .label("opacity")
                        .child(box_node(PaintCounter(Arc::clone(&painted))).label("content")),
                ),
            )
            .child(box_node(PoisonOnDemand(Arc::clone(&armed)))),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    let content = registry.get("content").expect("content is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // Both reasons at once: a layer property moved AND painted content changed.
    set_opacity(&mut owner, opacity_id, 0.25);
    owner.mark_needs_paint(content);
    armed.store(true, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    assert!(
        result.is_err(),
        "precondition: the armed leaf poisons this pass"
    );
    // Counted AFTER the failure, not before it: the poisoned pass repaints the
    // boundary before reaching the sibling that poisons it, so a count taken
    // before the failure is already incremented and cannot say anything about
    // what the retry did.
    let after_error = painted.load(Ordering::Relaxed);

    armed.store(false, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    result.expect("the retry paints cleanly");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > after_error,
        "the retry must REPAINT the boundary, not graft it and patch only the layer: \
         the content change that required the repaint would be lost",
    );
}

/// An animated opacity crossing alpha 0 adds and removes its subtree's content.
///
/// The static setter and the animation tick are separate seams — the setter
/// reports an impact, the tick sends through `RenderInvalidationHandle` — and
/// the tick's guard was untested: a mutation run removed it and every test
/// stayed green. That is the same defect the static path had, on the path an
/// `AnimatedOpacity` widget actually uses, so it is the one that would have
/// reached users.
///
/// At both alpha 255 and alpha 0 no `OpacityLayer` is emitted and the layered
/// predicate does not change, so the tick looks like a pure property change.
/// Only `skip_paint()` moves, and no patch can express it.
#[test]
fn an_animated_opacity_crossing_zero_adds_and_removes_its_content() {
    use flui_animation::{Animation, AnimationController, ProxyAnimation};
    use flui_objects::RenderAnimatedOpacity;
    use flui_scheduler::UpdateScheduler;
    use std::time::Duration;

    let controller = AnimationController::new(Duration::from_millis(100), &UpdateScheduler::new());
    controller.set_value(1.0);
    let parent: Arc<dyn Animation<f64>> = Arc::new(controller.clone());
    let proxy = ProxyAnimation::new(parent);

    let mut owner = PipelineOwner::new();
    let (root_id, _registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderAnimatedOpacity::new(proxy, false))
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));

    let (mut owner, result) = owner.run_frame();
    let opaque = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    let opaque_pictures = picture_count(&opaque);
    assert!(
        opaque_pictures > 0,
        "precondition: the subtree is visible while fully opaque",
    );

    // Tick to fully transparent. The listener fires on the render object, so
    // this is the widget-driven path end to end.
    controller.set_value(0.0);
    owner.drain_pending_dirty();
    let (mut owner, result) = owner.run_frame();
    let transparent = result
        .expect("transparent frame")
        .expect("transparent frame produces a layer tree");
    assert!(
        picture_count(&transparent) < opaque_pictures,
        "a tick to alpha 0 must drop the subtree's content from the frame; got \
         {} pictures vs {opaque_pictures} while opaque",
        picture_count(&transparent),
    );

    // …and back, which replays the capture taken while nothing was painted.
    controller.set_value(1.0);
    owner.drain_pending_dirty();
    let (owner, result) = owner.run_frame();
    let restored = result
        .expect("restored frame")
        .expect("restored frame produces a layer tree");
    drop(owner);
    assert_eq!(
        picture_count(&restored),
        opaque_pictures,
        "and a tick back must bring it in again",
    );
}

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderTransform
// ---------------------------------------------------------------------------
//
// `RenderTransform`'s `paint_effects` reports its matrix through the same
// `transform` field `own_effect_layers`/`layer_patches_for` already
// generically serve for `opacity`, so a non-translation matrix change is
// addressable the same way an alpha change is — see
// `crates/flui-rendering/ARCHITECTURE.md`'s "A composited-layer update
// patches the enclosing capture" section for the design this mirrors.

/// Mirrors `mount_opacity_under_boundary` for `RenderTransform`: root row →
/// [boundary → transform → counting leaf, boundary → leaf].
///
/// `matrix` must be non-translation for the transform to own an effect layer
/// at all — see `RenderTransform::paint_effects`'s fork.
fn mount_transform_under_boundary(
    matrix: Matrix4,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Arc<AtomicUsize>,
) {
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderTransform::new(matrix))
                        .label("transform")
                        .child(box_node(PaintCounter(Arc::clone(&painted))).label("child")),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("sibling")
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let transform_id = registry.get("transform").expect("transform is labelled");
    let child_id = registry.get("child").expect("child is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, transform_id, child_id, sibling, painted)
}

/// Applies a new matrix through the same seam a widget rebuild uses: the
/// setter reports an impact, the owner applies it.
fn set_transform(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    matrix: Matrix4,
) {
    update_render_object::<RenderTransform, _>(owner, id, |t| t.set_transform(matrix));
}

/// A matrix change updates the emitted layer without repainting the subtree.
///
/// Mirrors `an_alpha_change_updates_the_layer_without_repainting_the_subtree`.
/// Both halves are asserted and neither alone is evidence: the paint count
/// alone passes if nothing painted at all, and the matrix alone passes on a
/// full repaint.
#[test]
fn a_transform_change_updates_the_layer_without_repainting_the_subtree() {
    let (owner, transform_id, _child, _sibling, painted) =
        mount_transform_under_boundary(Matrix4::scaling(2.0, 2.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_transform(&mut owner, transform_id, Matrix4::scaling(3.0, 3.0, 1.0));
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the subtree under the transform must NOT repaint for a matrix-only change",
    );
    assert_eq!(
        first_transform_matrix(&tree),
        Some(Matrix4::scaling(3.0, 3.0, 1.0)),
        "and the emitted TransformLayer must carry the new matrix",
    );
}

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderRotatedBox
// ---------------------------------------------------------------------------
//
// `RenderRotatedBox`'s `paint_effects` reports its matrix through the same
// value `RenderTransform` above does — see
// `crates/flui-rendering/ARCHITECTURE.md`'s "And `RenderRotatedBox`" entry
// for the accounting this mirrors, including why the captured origin stays
// valid for a parity-preserving turn change specifically.

/// A SIZED, non-square counting leaf that also draws. The size is the
/// load-bearing part: this file's rotated-box tests need a non-square
/// preferred size so an axis swap is observable (`Size::new(height, width) !=
/// Size::new(width, height)` only when the two differ), which the plain
/// `PaintCounter` above — a fixed 10×10 square — cannot give them. Drawing is
/// secondary: it costs nothing extra and, unlike `PaintCounter`, lets this
/// fixture also serve a picture-count oracle if a future test here wants one
/// (`PaintCounter` counts a call but emits no draw commands, so a
/// picture-count check over it is blind — see `mount_drawing_opacity` for the
/// same reasoning on the opacity side).
#[derive(Debug)]
struct DrawingPaintCounter {
    size: Size,
    count: Arc<AtomicUsize>,
}

impl flui_foundation::Diagnosticable for DrawingPaintCounter {}

impl flui_rendering::traits::RenderBox for DrawingPaintCounter {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        ctx.constrain(self.size)
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Leaf>) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let rect = flui_foundation::geometry::Rect::from_origin_size(
            flui_foundation::geometry::Point::ZERO,
            ctx.size(),
        );
        ctx.canvas().draw_rect(
            rect,
            &flui_painting::Paint::fill(flui_painting::styling::Color::RED),
        );
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderClipRRect
// ---------------------------------------------------------------------------
//
// `RenderClip<S>::paint_effects` reports its clip through the same `clip`
// field `own_effect_layers`/`layer_patches_for` already generically serve
// for `opacity`/`transform`, so a border-radius-only change is addressable
// the same way an alpha or matrix change is.

/// Mirrors `mount_rotated_box_under_boundary` for `RenderClipRRect`: root
/// row → [boundary → padding → clip(rrect) → drawing counting leaf,
/// boundary → leaf].
///
/// The `RenderPadding` gives the clip a non-zero origin INSIDE the boundary
/// — a zero origin would let a wrong captured-origin translate pass
/// unnoticed. The leaf is a non-square `DrawingPaintCounter` so the fixture
/// also has teeth against a transposed size.
fn mount_clip_rrect_under_boundary(
    radius: f64,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Arc<AtomicUsize>,
) {
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderPadding::all(12.0)).child(
                        box_node(
                            RenderClipRRect::anti_alias()
                                .with_border_radius(BorderRadius::circular(radius)),
                        )
                        .label("clip")
                        .child(box_node(DrawingPaintCounter {
                            size: Size::new(30.0, 20.0),
                            count: Arc::clone(&painted),
                        })),
                    ),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("sibling")
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let clip_id = registry.get("clip").expect("clip is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, clip_id, sibling, painted)
}

/// Applies a new border radius through the same seam a widget rebuild uses:
/// the setter reports an impact, the owner applies it.
fn set_border_radius(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    radius: f64,
) {
    update_render_object::<RenderClipRRect, _>(owner, id, |c| {
        c.set_border_radius(Some(BorderRadius::circular(radius)))
    });
}

/// A border-radius change updates the emitted `ClipRRectLayer` without
/// repainting the subtree.
///
/// Both halves are asserted and neither alone is evidence: the paint count
/// alone passes if nothing painted at all, and the rrect alone passes on a
/// full repaint. Compared against TWO fresh reference owners — one at the
/// new radius, one at the old — so both "equals the new value" and "differs
/// from the old value" are real evidence, not merely restated preconditions.
#[test]
fn a_border_radius_change_updates_the_clip_layer_without_repainting_the_subtree() {
    let (owner, clip_id, _sibling, painted) = mount_clip_rrect_under_boundary(8.0);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_border_radius(&mut owner, clip_id, 2.0);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the subtree under the clip must NOT repaint for a border-radius-only change",
    );
    let rrects = clip_rrects(&tree);
    assert_eq!(
        rrects.len(),
        1,
        "precondition: exactly one ClipRRectLayer must be present: {rrects:?}",
    );

    let (radius2_reference, _, _, _) = mount_clip_rrect_under_boundary(2.0);
    let (_, result) = radius2_reference.run_frame();
    let radius2_rrects = clip_rrects(
        &result
            .expect("radius-2 reference frame")
            .expect("radius-2 reference frame produces a layer tree"),
    );
    assert_eq!(
        rrects[0], radius2_rrects[0],
        "the patched ClipRRectLayer must equal what a fresh owner mounted \
         directly at radius 2 produces",
    );

    let (radius8_reference, _, _, _) = mount_clip_rrect_under_boundary(8.0);
    let (_, result) = radius8_reference.run_frame();
    let radius8_rrects = clip_rrects(
        &result
            .expect("radius-8 reference frame")
            .expect("radius-8 reference frame produces a layer tree"),
    );
    assert_ne!(
        rrects[0], radius8_rrects[0],
        "and the new radius must differ from the old — otherwise the patch \
         could be a no-op that happened to pass",
    );
}

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderClipRect / RenderClipOval — the STORED
// clip_shape branch of `clip_descriptor`
// ---------------------------------------------------------------------------
//
// The two tests above exercise `clip_descriptor`'s data-only
// `rrect_border_radius` branch; these exercise the OTHER stored-shape
// branch, `clip_shape` — set through `RenderClipRect`/`RenderClipOval`'s
// `set_clip_shape` rather than `RenderClipRRect`'s `set_border_radius`, both
// of which precede the owner-lane `path_clip_target` branch tested
// separately below.

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderFlow — a structural clip, refused as one
// ---------------------------------------------------------------------------
//
// `RenderFlow` is ITSELF a repaint boundary (`is_repaint_boundary` is
// unconditional — see the type's own doc), so its own effect layers live in
// its OWN retained capture rather than an ancestor's. `set_clip_behavior`
// reports a structural `PAINT | SEMANTICS`, never `COMPOSITED_LAYER_UPDATE`
// (its clip is gated on `Clip::None`, so a behavior change can add or remove
// the layer entirely — see `RenderFlow::set_clip_behavior`'s own doc). This
// test drives the update-only path directly anyway, bypassing the setter's
// own (correct) classification, to pin that `layer_patches_for` refuses a
// wrongly-classified request against a REAL producer, repaints correctly,
// and clears the flag the refused patch never touched — the same property
// `a_childless_rotated_box_layer_update_falls_back_to_a_repaint_and_clears_the_flag`
// above pins, but for a node that is its OWN boundary rather than one nested
// inside a separate `RenderRepaintBoundary`.

// ---------------------------------------------------------------------------
// Composited-layer updates: two RenderClipPath siblings, one boundary
// ---------------------------------------------------------------------------
//
// `layer_patches_for` walks the retained capture's `effect_slots` — a `Vec`
// in pre-order CAPTURE position, never a hash map — so two independent clip
// requesters under the same boundary resolve in the same order a paint would
// visit them, deterministically across runs. Running this three times with
// fresh owners (and a fresh lane) is what makes "deterministically" a real
// claim rather than one lucky hash iteration order.

// ---------------------------------------------------------------------------
// Composited-layer updates: RenderClipPath — the layer CONTENT, not just order
// ---------------------------------------------------------------------------
//
// `two_path_clips_under_one_boundary_resolve_in_paint_order` above pins
// resolution ORDER across two siblings; it never inspects what either
// `ClipPathLayer` actually contains. This test is the path family's
// counterpart to `a_clip_shape_change_updates_the_clip_layer_without_repainting_the_subtree`
// and `a_border_radius_change_updates_the_clip_layer_without_repainting_the_subtree`
// above: it changes ONE `RenderClipPath`'s target and checks the emitted
// path itself.

// ---------------------------------------------------------------------------
// Composited-layer updates over the Sliver protocol
// ---------------------------------------------------------------------------
//
// Everything above drives the update-only path through `RenderBox`. These two
// are the first to drive it through a sliver at all.
//
// What that is worth, stated exactly rather than as "the machinery is
// protocol-agnostic": `RenderNode::paint_effects`
// (`crates/flui-rendering/src/storage/node.rs`) resolves its `size` argument
// per protocol but calls the render object's `paint_effects(size)` the same
// way in either arm, so the `opacity` field it returns carries no protocol
// split for a test to find. What these DO pin is that a sliver's
// `RenderSliver::is_repaint_boundary` is honoured by the layer-update walk —
// without it `mark_needs_composited_layer_update` reaches the viewport (the
// paint root, whose parent is `None`) and degrades to a plain repaint, which
// is precisely how both tests fail against a setter that still reports
// `PAINT`.
//
// The one genuine protocol divergence is the `size` argument
// `RenderNode::paint_effects` resolves — `geometry()` for a box,
// `absolute_paint_size()` for a sliver. `RenderSliverOpacity`'s `paint_effects`
// never sets the `transform` field, so neither test below reaches that
// branch; it stays unexercised for slivers and is not what these cover.
//
// NAMED GAP — the one hazard the Sliver protocol adds that these do not cover:
// a sliver whose `geometry.visible` is false is cut off by the paint walk's
// visibility gate BEFORE `own_effect_layers` runs, so it records no effect
// slot. An alpha change on such a node reports a layer update with no slot to
// land in, and correctness then rests entirely on `layer_patches_for`'s
// target-has-no-slot guard, which is pinned only by the Box-protocol test
// `an_effect_layer_shape_change_falls_back_to_a_repaint`. Reading both call
// sites says the behaviour is right today; it has no oracle of its own, and a
// fixture would need a sliver scrolled out of the viewport.
//
// These two mirror `an_alpha_change_updates_the_layer_without_repainting_the_subtree`
// and `a_layer_update_is_written_back_into_the_retained_capture` above, with a
// `RenderViewport` hosting a minimal sliver repaint boundary in place of
// `RenderRepaintBoundary` — flui-objects ships no dedicated sliver
// repaint-boundary render object, only the `RenderSliver::is_repaint_boundary`
// override point the pipeline reads.
