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

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderFlex, RenderOpacity, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{TreeNode, box_node, inspect::first_opacity_alpha, tree, update_render_object},
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
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
pub(crate) fn a_retained_frame_matches_what_a_full_repaint_produces() {
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
pub(crate) fn the_content_of_a_clean_boundary_is_not_repainted() {
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

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> flui_rendering::RenderResult<Size> {
            Ok(ctx.constrain(Size::new(10.0, 10.0)))
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

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
    ) -> flui_rendering::RenderResult<Size> {
        Ok(ctx.constrain(Size::new(10.0, 10.0)))
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
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
pub(crate) fn an_alpha_change_updates_the_layer_without_repainting_the_subtree() {
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
pub(crate) fn a_failed_pass_does_not_downgrade_a_real_repaint_to_an_update() {
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
        ) -> flui_rendering::RenderResult<Size> {
            Ok(ctx.constrain(Size::new(10.0, 10.0)))
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
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
