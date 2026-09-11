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

use flui_objects::{
    RenderClipRRect, RenderColoredBox, RenderFlex, RenderFlow, RenderOpacity, RenderPadding,
    RenderRepaintBoundary, RenderRotatedBox, RenderTransform,
};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{
        TreeNode, box_node, edit_render_object,
        inspect::{
            clip_rects, clip_rrects, first_opacity_alpha, first_transform_matrix, layer_structure,
            transform_matrices,
        },
        tree, update_render_object,
    },
};
use flui_types::{
    Matrix4, Size,
    geometry::px,
    styling::{BorderRadius, BorderRadiusExt},
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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
    let Some(root) = t.root() else {
        return out;
    };
    let mut stack = vec![root];
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
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use flui_tree::Leaf;
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
            ctx.constrain(Size::new(px(10.0), px(10.0)))
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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

/// Removing a boundary drops its retained output.
///
/// A stale entry could never be served to a different node — `RenderId` is
/// generational, so a recycled slab slot misses the map — but without eviction
/// the map grows for the owner's lifetime as boundaries come and go. The
/// sibling caches (`scheduler`, `layout_poison`) are evicted at the same point
/// for the same reason.
#[test]
fn removing_a_boundary_drops_its_retained_output() {
    let (owner, ids) = mount(3);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    assert_eq!(
        owner.retained_boundary_count(),
        3,
        "precondition: the first frame retains all three boundaries"
    );

    owner.remove_render_object(ids.first);

    assert_eq!(
        owner.retained_boundary_count(),
        2,
        "the removed boundary's retained output must be dropped with it"
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
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use flui_tree::Leaf;
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
            ctx.constrain(Size::new(px(10.0), px(10.0)))
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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

/// Every boundary's root layer carries its own `RenderId`, and no
/// other layer carries any.
///
/// This is the identity ADR-0061 says damage needs: a frame builds a fresh
/// `LayerTree` with fresh slab indices, so `LayerId` pairs nothing across a
/// frame boundary and the comparison has to key on something that survives.
///
/// The fixture uses EIGHT boundaries on purpose. A single-boundary tree pairs
/// correctly under any stamping rule at all — including stamping every layer
/// with the same id, or stamping the wrong one — so it would pass against a
/// broken implementation. With eight, the set of stamps has to be exactly the
/// set of boundary ids, and the count of stamped layers has to be eight.
///
/// Red-check: stamp in `push_layer` instead of `push_boundary_layer` — the
/// opacity and picture layers pick up ids too and the count assertion fails.
#[test]
fn every_boundary_root_layer_carries_its_own_id() {
    const N: usize = 8;

    let (owner, _) = mount(N);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("first frame")
        .expect("first frame produces a layer tree");

    let stamps = stamps_of(&tree);
    assert_eq!(
        stamps.len(),
        N,
        "exactly one layer per boundary carries a stamp, got {stamps:?}"
    );

    let boundary_ids: std::collections::BTreeSet<flui_foundation::RenderId> = owner
        .render_tree()
        .iter()
        .filter(|(_, node)| node.is_repaint_boundary())
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        stamps
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        boundary_ids,
        "the stamps must be exactly the tree's repaint-boundary ids"
    );
}

/// A ROOT that declares itself a repaint boundary is stamped too.
///
/// The sibling test above cannot see this case, and passes without it: its
/// `mount()` root is a `RenderFlex`, which is not a boundary, so the root never
/// appears on either side of its comparison. Stamping is driven by
/// `push_boundary_layer` from the PARENT's child loop, and the root has no
/// parent -- so a boundary root produced a layer tree containing a boundary
/// nothing could identify, which is precisely the shape `render_id` exists to
/// prevent.
///
/// Red-check: remove the `root_boundary` argument threaded into
/// `FragmentComposer::new` and the count is N, not N+1.
#[test]
fn a_root_that_is_itself_a_boundary_carries_a_stamp() {
    const N: usize = 3;

    // Same tree as `mount`, but wrapped so the ROOT is a repaint boundary.
    let mut owner = PipelineOwner::new();
    let (root_id, _registry) = tree::mount(
        &mut owner,
        box_node(RenderRepaintBoundary::new()).child(spec(N)),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("first frame")
        .expect("first frame produces a layer tree");

    let stamps = stamps_of(&tree);
    let boundary_ids: std::collections::BTreeSet<flui_foundation::RenderId> = owner
        .render_tree()
        .iter()
        .filter(|(_, node)| node.is_repaint_boundary())
        .map(|(id, _)| id)
        .collect();

    assert!(
        boundary_ids.contains(&root_id),
        "premise: the wrapped root must actually BE a repaint boundary, or \
         this test proves nothing"
    );
    assert_eq!(
        stamps.len(),
        N + 1,
        "N child boundaries plus the root itself must each carry a stamp, \
         got {stamps:?}"
    );
    assert_eq!(
        stamps
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        boundary_ids,
        "the stamps must be exactly the tree's repaint-boundary ids, root included"
    );
}

/// A frame that grafts seven boundaries still identifies all eight, so
/// retention does not break the comparison the stamp exists for.
///
/// These boundaries are all TOP-LEVEL, and that is why this test does not need
/// `graft` to carry anything: each one's `OffsetLayer` is pushed by the root
/// flex, which repaints every frame and re-stamps it. The capture's own carry
/// matters only for a NESTED boundary — see
/// [`a_boundary_nested_inside_a_reused_one_keeps_its_stamp`], which is the
/// fixture this one cannot substitute for.
///
/// The equality alone is a VACUOUS oracle and mutation-testing proved it:
/// with the stamp removed both frames report an empty set, which compares
/// equal. The count assertion below is what makes it discriminate — it was
/// added after the mutation run, not before it.
#[test]
fn a_retained_frame_still_identifies_every_boundary() {
    const N: usize = 8;

    let (owner, ids) = mount(N);
    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    let before = stamps_of(&first);

    // One boundary repaints; the other seven are grafted.
    owner.mark_needs_paint(ids.first);
    let (_, result) = owner.run_frame();
    let second = result
        .expect("second frame")
        .expect("second frame produces a layer tree");

    assert_eq!(
        before.len(),
        N,
        "precondition: the first frame identified every boundary — without \
         this the equality below holds vacuously when nothing is stamped"
    );
    assert_eq!(
        stamps_of(&second),
        before,
        "the same eight boundaries must be identifiable in both frames — \
         seven of them reached through a graft"
    );
}

/// Every stamp in `t`, in a deterministic walk order.
fn stamps_of(t: &flui_layer::LayerTree) -> Vec<flui_foundation::RenderId> {
    let mut out = Vec::new();
    let Some(root) = t.root() else {
        return out;
    };
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Some(node) = t.get(id)
            && let Some(render_id) = node.render_id()
        {
            out.push(render_id);
        }
        let children = t
            .children(id)
            .expect("every id in this walk came from the tree itself");
        for &child in children.iter().rev() {
            stack.push(child);
        }
    }
    out.sort_unstable();
    out
}

/// A boundary NESTED inside a reused boundary keeps its stamp.
///
/// This is the case the flat eight-boundary fixture above cannot see, and
/// getting it wrong was caught by review rather than by that fixture. A
/// top-level boundary's stamp sits on the `OffsetLayer` its parent pushes,
/// which is *above* the capture root — the parent repaints every frame and
/// re-stamps it, so the capture need carry nothing. A nested boundary's parent
/// paints INSIDE the outer capture, so its stamped `OffsetLayer` is one of the
/// captured nodes (`RetainedSubtree` flattens nested boundaries). Drop the
/// carry in `graft` and every nested boundary becomes unidentifiable the
/// moment its enclosing boundary is reused.
///
/// Red-check: remove the `render_id` propagation from `graft` — the second
/// frame reports two stamps where the first reported three.
#[test]
fn a_boundary_nested_inside_a_reused_one_keeps_its_stamp() {
    // Root flex → [dirty boundary, outer boundary → opacity → inner boundary].
    // The first is marked so a second frame runs at all; the second is clean
    // and therefore grafted, carrying the inner boundary's layers with it.
    let mut root = box_node(RenderFlex::row());
    root = root.child(
        box_node(RenderRepaintBoundary::new())
            .label("first")
            .child(box_node(RenderColoredBox::red(10.0, 10.0))),
    );
    root = root.child(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(RenderOpacity::new(0.5)).child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(10.0, 10.0))),
            ),
        ),
    );

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let first = registry.get("first").expect("first boundary is labelled");

    let (mut owner, result) = owner.run_frame();
    let before = stamps_of(
        &result
            .expect("first frame")
            .expect("first frame produces a layer tree"),
    );
    assert_eq!(
        before.len(),
        3,
        "precondition: two top-level boundaries and one nested inside the \
         second, all identifiable — got {before:?}"
    );

    owner.mark_needs_paint(first);
    let (_, result) = owner.run_frame();
    let after = stamps_of(
        &result
            .expect("second frame")
            .expect("second frame produces a layer tree"),
    );

    assert_eq!(
        after, before,
        "the nested boundary must still be identifiable after its enclosing \
         boundary was grafted rather than repainted"
    );
}

// ---------------------------------------------------------------------------
// The control for `paint/opacity_tick` (issue #536)
// ---------------------------------------------------------------------------

/// Root flex row → [boundary → opacity → row of `subtree` leaves, boundary → leaf].
///
/// Mirrors `benches/helpers.rs::build_opacity_tree_painted_once`, so the
/// benchmark's two arms have an equivalence oracle rather than only a pair of
/// timings.
fn opacity_spec(subtree: usize) -> TreeNode {
    let content = {
        let mut row = box_node(RenderFlex::row());
        for i in 0..subtree {
            let leaf = box_node(RenderColoredBox::red(10.0, 10.0));
            row = row.child(if i == 0 {
                leaf.label("opacity-leaf")
            } else {
                leaf
            });
        }
        row
    };
    box_node(RenderFlex::row())
        .child(
            box_node(RenderRepaintBoundary::new())
                .child(box_node(RenderOpacity::new(0.5)).child(content)),
        )
        .child(
            box_node(RenderRepaintBoundary::new())
                .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("sibling-leaf")),
        )
}

fn mount_opacity(
    subtree: usize,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(&mut owner, opacity_spec(subtree));
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_leaf = registry.get("opacity-leaf").expect("labelled");
    let sibling = registry.get("sibling-leaf").expect("labelled");
    (owner, opacity_leaf, sibling)
}

/// Grafting an opacity's subtree produces the same layer tree that repainting
/// it does, at every subtree size — and the grafted tree's node count does not
/// grow with the subtree.
///
/// This is the control that makes `paint/opacity_tick` mean something. That
/// benchmark reports a flat ~1.2 µs for the graft arm against a linear repaint
/// arm, and a flat number is exactly what a graft that silently dropped the
/// subtree would also report. The structural comparison rules that out.
///
/// The flatness is real and has a cause: the leaves are inline (non-boundary),
/// so `run_paint` merges their draw runs into ONE `PictureLayer` sharing an
/// `Arc<DisplayList>`. The capture is therefore a handful of nodes whatever
/// `subtree` is, and grafting clones an `Arc` rather than re-recording every
/// command. That is the structural-sharing substrate ADR-0061 named as
/// retention's prerequisite, doing the work it was added for.
#[test]
fn a_grafted_opacity_subtree_matches_a_full_repaint_at_any_size() {
    let mut grafted_counts = Vec::new();
    for &subtree in &[1_usize, 10, 100] {
        // Frame 1 warms the retention cache for both boundaries.
        let (owner, _opacity_leaf, sibling) = mount_opacity(subtree);
        let (mut owner, result) = owner.run_frame();
        result.expect("first frame");

        // Frame 2: only the SIBLING is dirty, so the opacity boundary grafts.
        owner.mark_needs_paint(sibling);
        let (owner, result) = owner.run_frame();
        let grafted = result
            .expect("second frame")
            .expect("second frame produces a layer tree");
        drop(owner);

        // A fresh owner painting its first frame retains nothing, so its tree
        // is what a full repaint produces.
        let (fresh, _, _) = mount_opacity(subtree);
        let (_, result) = fresh.run_frame();
        let repainted = result
            .expect("reference frame")
            .expect("reference frame produces a layer tree");

        assert_eq!(
            fingerprint(&grafted),
            fingerprint(&repainted),
            "grafted and repainted layer trees must match at subtree = {subtree}",
        );
        assert!(
            grafted.len() > 1,
            "a graft that produced an empty tree would also look flat in the bench",
        );
        grafted_counts.push(grafted.len());
    }

    // The layer count must NOT grow with the subtree — that is what makes the
    // graft arm O(1) for this shape. Every measurement is compared, not just
    // the ends: `[6, 7, 6]` is a real regression that a first-vs-last check
    // would wave through.
    assert!(
        grafted_counts.windows(2).all(|w| w[0] == w[1]),
        "inline leaves merge into one PictureLayer, so the layer count must be \
         identical at every subtree size; got {grafted_counts:?}",
    );
}

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
    type Arity = flui_tree::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_tree::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        ctx.constrain(Size::new(px(10.0), px(10.0)))
    }

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_tree::Leaf,
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
    opacity: f32,
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, opacity_id, sibling, painted)
}

/// Applies a new opacity through the same seam a widget rebuild uses:
/// the setter reports an impact, the owner applies it.
fn set_opacity(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    value: f32,
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

/// The update reaches the STORED capture, not just the emitted frame.
///
/// Three frames, because two cannot see this: change, observe, then dirty
/// something else so the boundary is grafted for an unrelated reason. If the
/// patch only touched the emitted tree, the capture still holds the alpha it
/// was captured with, and this third frame reverts it — permanently, because
/// nothing marks it again.
#[test]
fn a_layer_update_is_written_back_into_the_retained_capture() {
    let (owner, opacity_id, sibling, _painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_opacity(&mut owner, opacity_id, 0.25);
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");

    // Third frame: the opacity is clean; the sibling boundary forces the pass.
    owner.mark_needs_paint(sibling);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "a later frame that grafts the capture for an unrelated reason must \
         replay the UPDATED alpha, not the one it was captured with",
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

/// A pending layer update on a node whose subtree is removed does not outlive
/// it.
///
/// The capture is evicted with the subtree (`remove_subtree`), and `RenderId`
/// is generational, so a recycled slab slot cannot be served the old entry.
/// This pins the pair: after removal the boundary count drops, and the frame
/// that follows is still correct rather than replaying a capture whose node is
/// gone.
#[test]
fn removing_a_subtree_with_a_pending_layer_update_evicts_its_capture() {
    let (owner, opacity_id, _sibling, _painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let retained_before = owner.retained_boundary_count();
    assert!(
        retained_before > 0,
        "precondition: the first frame retains at least one boundary",
    );

    // Ask for an update, then delete the subtree that would have served it
    // before the frame that would have applied it ever runs.
    set_opacity(&mut owner, opacity_id, 0.25);
    let boundary = owner
        .render_tree()
        .get(opacity_id)
        .and_then(|node| node.links().parent())
        .expect("the opacity sits under a repaint boundary");
    owner.remove_render_object(boundary);

    let (owner, result) = owner.run_frame();
    result.expect("the frame after a removal must still succeed");
    assert!(
        owner.retained_boundary_count() < retained_before,
        "the removed boundary's retained output must be evicted, not left for \
         a later frame to graft",
    );
}

/// A boundary that lost and regained its retained output repaints rather than
/// serving a stale capture.
///
/// `mark_needs_composited_layer_update`'s eligibility is
/// `is_repaint_boundary_flag() && was_repaint_boundary()` — the same predicate
/// `mark_needs_paint` stops at. A boundary with no retained output yet fails
/// it, so the mark degrades to a paint mark instead of addressing a capture
/// that does not exist.
///
/// Non-discriminating by construction, and kept as a regression guard rather
/// than as evidence: mounting schedules an initial paint, so the first frame is
/// a full root descent whatever the mark did, and the alpha is read live during
/// it. Replacing the degrade arm with a bare `return` leaves this green.
#[test]
fn a_layer_update_without_retained_output_degrades_to_a_repaint() {
    let (owner, opacity_id, _sibling, painted) = mount_opacity_under_boundary(0.5);
    // No first frame: nothing has painted, so no boundary has retained output
    // and `was_repaint_boundary()` is false everywhere.
    let mut owner = owner;
    set_opacity(&mut owner, opacity_id, 0.25);

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > 0,
        "with nothing retained the update must degrade to a paint, so the \
         subtree paints",
    );
    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "and the painted result carries the new alpha",
    );
}

/// Going fully transparent actually hides the subtree.
///
/// The trap: at alpha 255 and at alpha 0 `RenderOpacity` emits no
/// `OpacityLayer` either way, and `needs_compositing()` is false at both, so
/// nothing structural appears to change. But `skip_paint()` flips, and a node
/// with no effect layer has no slot in `effect_slots` for the update arm to
/// patch — so an update-only commit would graft the old, visible capture and
/// leave the subtree on screen at opacity 0.
///
/// The oracle is the emitted picture count rather than the paint counter: the
/// question is what the frame CONTAINS, and a graft of stale content paints
/// nothing while still emitting it.
#[test]
fn going_fully_transparent_removes_the_subtree_from_the_frame() {
    let (owner, opacity_id) = mount_drawing_opacity(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // 0.5 -> 1.0 first: both endpoints of the real transition must have no
    // OpacityLayer, so the bug is about `skip_paint`, not about a layer.
    set_opacity(&mut owner, opacity_id, 1.0);
    let (mut owner, result) = owner.run_frame();
    let opaque = result
        .expect("opaque frame")
        .expect("opaque frame produces a layer tree");
    assert!(
        picture_count(&opaque) > 0,
        "precondition: the subtree is visible while opaque",
    );

    set_opacity(&mut owner, opacity_id, 0.0);
    let (owner, result) = owner.run_frame();
    let transparent = result
        .expect("transparent frame")
        .expect("transparent frame produces a layer tree");
    drop(owner);

    assert!(
        picture_count(&transparent) < picture_count(&opaque),
        "a fully transparent opacity must drop its subtree's content from the \
         frame; got {} pictures at alpha 0 vs {} at alpha 255",
        picture_count(&transparent),
        picture_count(&opaque),
    );
}

/// …and coming back from fully transparent restores it.
///
/// The mirror of the case above: at alpha 0 the node paints nothing, so the
/// boundary's capture holds no content. Serving 0 -> 255 as an update-only
/// commit would replay that empty capture and leave the subtree invisible.
#[test]
fn leaving_fully_transparent_restores_the_subtree_to_the_frame() {
    let (owner, opacity_id) = mount_drawing_opacity(0.0);
    let (mut owner, result) = owner.run_frame();
    let hidden = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    let hidden_pictures = picture_count(&hidden);

    set_opacity(&mut owner, opacity_id, 1.0);
    let (owner, result) = owner.run_frame();
    let shown = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert!(
        picture_count(&shown) > hidden_pictures,
        "leaving alpha 0 must bring the subtree's content back into the frame; \
         got {} pictures at alpha 255 vs {hidden_pictures} at alpha 0",
        picture_count(&shown),
    );
}

/// Root row → boundary → opacity → a leaf that actually DRAWS.
///
/// Deliberately not `mount_opacity_under_boundary`: that fixture's leaf is a
/// `PaintCounter`, which records a call and emits no draw commands, so it
/// produces no `PictureLayer` and a picture-count oracle over it cannot tell a
/// visible subtree from a hidden one. Content that draws is what makes the
/// assertion mean anything.
fn mount_drawing_opacity(
    opacity: f32,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderOpacity::new(opacity))
                    .label("opacity")
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    (owner, opacity_id)
}

/// Number of `PictureLayer`s in a tree.
fn picture_count(t: &flui_layer::LayerTree) -> usize {
    fn walk(t: &flui_layer::LayerTree, id: flui_foundation::LayerId) -> usize {
        let Some(node) = t.get(id) else { return 0 };
        let self_count = usize::from(matches!(node.layer(), flui_layer::Layer::Picture(_)));
        self_count + node.children().iter().map(|&c| walk(t, c)).sum::<usize>()
    }
    t.root().map_or(0, |root| walk(t, root))
}

/// An update served by grafting an OUTER boundary must not leave the inner
/// boundary's own capture stale.
///
/// The shape: outer boundary → inner boundary → opacity → content.
/// `mark_needs_composited_layer_update` walks up to the INNER boundary, since
/// that is the first one owning retained output. But the outer boundary's
/// capture flattens the inner one's layers, so if the outer is allowed to graft
/// it can serve the update from its own copy — patching that copy and clearing
/// the flag while the inner boundary's capture keeps the old value.
///
/// The third frame is where that shows: repaint the outer for an unrelated
/// reason and it descends to the inner, which is clean, and grafts the stale
/// capture — restoring the old alpha permanently.
#[test]
fn an_update_under_nested_boundaries_does_not_leave_the_inner_capture_stale() {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).label("outer").child(
                    box_node(RenderRepaintBoundary::new()).child(
                        box_node(RenderOpacity::new(0.5))
                            .label("opacity")
                            .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                    ),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let outer = registry.get("outer").expect("outer is labelled");
    let opacity_id = registry.get("opacity").expect("opacity is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_opacity(&mut owner, opacity_id, 0.25);
    let (mut owner, result) = owner.run_frame();
    let updated = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    assert_eq!(
        first_opacity_alpha(&updated).map(|a| (a * 100.0).round()),
        Some(25.0),
        "precondition: the update lands in the frame it was requested for",
    );

    // Third frame: repaint the OUTER boundary for an unrelated reason. It
    // descends to the inner boundary, which is clean, and grafts its capture.
    owner.mark_needs_paint(outer);
    let (owner, result) = owner.run_frame();
    let regrafted = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        first_opacity_alpha(&regrafted).map(|a| (a * 100.0).round()),
        Some(25.0),
        "an outer repaint that grafts the inner boundary must replay the \
         UPDATED alpha; a stale inner capture silently restores the old value",
    );
}

/// A frame that fails after patching a boundary does not swallow the update.
///
/// The patch itself lives on the composer and dies with it when a later
/// sibling's `paint_raw` poisons the pass — but the pending-update FLAG is
/// state on the render node, and clearing it during the walk would survive the
/// failure. The dirty queue is deliberately kept across a paint error for the
/// retry, and that retry would then find nothing pending, graft the old layer,
/// and stay visibly stale until something else mutated the property.
///
/// So the flags are cleared only once the frame commits.
#[test]
fn a_paint_error_after_a_patch_leaves_the_update_pending_for_the_retry() {
    for repaint_arm in [false, true] {
        poisoned_frame_keeps_the_update(repaint_arm);
    }
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
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(10.0), px(10.0)))
        }

        fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
            assert!(
                !self.0.load(Ordering::Relaxed),
                "PoisonOnDemand: armed, poisoning this paint pass on purpose",
            );
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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

/// Body of the test above, run once per arm.
///
/// Both arms clear the pending flag, in different places — the graft arm in
/// `layer_patches_for`, the repaint arm in the paint walk — and both defer it
/// to the frame's commit.
///
/// Only the GRAFT arm is pinned here. The repaint arm's retry is rescued
/// anyway, because `mark_needs_paint` withdrew the update record when it queued
/// the boundary, so the retry repaints and rebuilds from live properties;
/// clearing that arm's flag mid-walk leaves this green. It runs both orderings
/// to document the pair, not because both discriminate.
fn poisoned_frame_keeps_the_update(repaint_arm: bool) {
    use std::sync::atomic::AtomicBool;

    /// A leaf that panics in `paint` while armed.
    #[derive(Debug)]
    struct PoisonOnDemand(Arc<AtomicBool>);

    impl flui_foundation::Diagnosticable for PoisonOnDemand {}

    impl flui_rendering::traits::RenderBox for PoisonOnDemand {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(10.0), px(10.0)))
        }

        fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
            assert!(
                !self.0.load(Ordering::Relaxed),
                "PoisonOnDemand: armed, poisoning this paint pass on purpose",
            );
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }
    }

    let armed = Arc::new(AtomicBool::new(false));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderOpacity::new(0.5))
                        .label("opacity")
                        .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                ),
            )
            // Ordered AFTER the boundary, so the failure lands once the
            // boundary has already been grafted and patched this pass.
            .child(box_node(PoisonOnDemand(Arc::clone(&armed)))),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame paints cleanly");

    // Second frame: request the update, and poison the pass after it lands.
    set_opacity(&mut owner, opacity_id, 0.25);
    if repaint_arm {
        // Force the REPAINT arm. Its flag clearing lives in the paint walk
        // rather than the graft arm, and it has the same obligation: the retry
        // sees a boundary that no longer needs paint, reclassifies it as
        // update-only, and would graft the pre-error capture unpatched.
        owner.mark_needs_paint(opacity_id);
    }
    armed.store(true, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    assert!(
        result.is_err(),
        "precondition: the armed leaf must poison this pass, or the test is \
         asserting about an ordinary frame",
    );

    // Retry: nothing new is requested, exactly as a real retry would.
    armed.store(false, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("the retry paints cleanly")
        .expect("the retry produces a layer tree");
    drop(owner);

    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "the update must survive a failed frame; clearing its flag mid-walk \
         loses it for good, because the retry sees nothing pending \
         (repaint_arm = {repaint_arm})",
    );
}

/// An effect layer that APPEARS falls back to a repaint.
///
/// The structure guard compares a node's rebuilt effect layers against the ones
/// the capture holds — but a node whose layers did not exist when the capture
/// was taken has no slot at all, so a guard that only walks existing slots
/// never examines it. Grafting would then replay output that predates the
/// effect entirely, and the request would be dropped silently.
///
/// No shipped render object reaches this today: opacity reports a repaint
/// itself whenever its layer appears or disappears. This models the next one
/// that will (a transform, per the follow-up issue) and pins that the frame
/// refuses rather than trusting the caller.
#[test]
fn an_effect_layer_that_appears_falls_back_to_a_repaint() {
    /// A proxy whose transform can be switched on at runtime, reporting only a
    /// composited-layer update — deliberately the WRONG impact for a shape
    /// change, so the paint phase's own guard is what is under test.
    #[derive(Debug, Default)]
    struct AppearingTransform {
        enabled: bool,
    }

    impl flui_foundation::Diagnosticable for AppearingTransform {}

    impl flui_rendering::traits::RenderBox for AppearingTransform {
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)
            } else {
                constraints.smallest()
            }
        }

        flui_rendering::forward_single_child_box_queries!();

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }

        fn paint_effects(&self, _size: Size) -> flui_rendering::traits::PaintEffects {
            if self.enabled {
                flui_rendering::traits::PaintEffects::NONE
                    .with_transform(flui_types::Matrix4::translation(3.0, 5.0, 0.0))
            } else {
                flui_rendering::traits::PaintEffects::NONE
            }
        }
    }

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(AppearingTransform::default())
                    .label("fx")
                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let fx = registry.get("fx").expect("fx is labelled");

    let (owner, result) = owner.run_frame();
    let first = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    assert_eq!(
        transform_matrices(&first).len(),
        0,
        "precondition: no transform layer while the effect is off",
    );

    // Switch the effect on and ask for a layer-only update — the wrong impact
    // for a shape change, which is exactly what the guard must catch.
    let mut owner = owner;
    edit_render_object::<AppearingTransform, _, _>(&mut owner, fx, |object| object.enabled = true);
    owner.mark_needs_composited_layer_update(fx);

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        transform_matrices(&tree).len(),
        1,
        "an effect layer that did not exist in the capture cannot be patched \
         into it; the frame must repaint and emit the new layer",
    );
}

/// Number of `TransformLayer`s in a tree.
/// A node whose effect layers change SHAPE cannot be patched, and the frame
/// must notice by itself.
///
/// Neither case is reachable through a shipped opacity, which reports a repaint
/// itself whenever its layer appears or disappears — so the frame's own guards
/// never fired and a mutation run found them inert. These drive the path
/// directly, through a proxy that reports the WRONG impact on purpose.
///
/// - **count** (captured with one layer, now emitting two, or the reverse)
///   pins the length guard: disabling it turns these red. The clip rows are
///   the one place a `clip` field's appearance and disappearance reach that
///   guard — no shipped clip producer reports an update across such a
///   transition yet, so without them the clip arm of `own_effect_layers`
///   would be exercised only through a repaint.
/// - **kind** (captured with one layer, now emitting one of a different type)
///   pins the OUTPUT but not the mechanism. `own_effect_layers` emits a fixed
///   order, so a same-count swap patches positionally to the same tree a
///   repaint gives, and disabling the discriminant guard leaves this green.
///   That guard is defence in depth against a future third effect type, and
///   the comment at it says so rather than claiming a pin it does not have.
#[test]
fn an_effect_layer_shape_change_falls_back_to_a_repaint() {
    /// Emits any combination of an opacity, a clip and a transform layer.
    #[derive(Debug, Default, Clone, Copy)]
    struct ShapeShifter {
        alpha: bool,
        clip: bool,
        transform: bool,
    }

    /// `(Opacity, ClipRect, Transform)` layer counts a tree must show for a
    /// shape — the same order `own_effect_layers` nests them in.
    fn expected_counts(shape: ShapeShifter) -> (usize, usize, usize) {
        (
            usize::from(shape.alpha),
            usize::from(shape.clip),
            usize::from(shape.transform),
        )
    }

    fn counts(tree: &flui_layer::LayerTree) -> (usize, usize, usize) {
        let kinds = layer_structure(tree);
        let count = |kind: &str| kinds.iter().filter(|k| **k == kind).count();
        (count("Opacity"), count("ClipRect"), count("Transform"))
    }

    impl flui_foundation::Diagnosticable for ShapeShifter {}

    impl flui_rendering::traits::RenderBox for ShapeShifter {
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)
            } else {
                constraints.smallest()
            }
        }

        flui_rendering::forward_single_child_box_queries!();

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }

        fn paint_effects(&self, size: Size) -> flui_rendering::traits::PaintEffects {
            let mut effects = flui_rendering::traits::PaintEffects::NONE;
            if self.alpha {
                effects = effects.with_opacity(flui_rendering::traits::PaintOpacity::new(128));
            }
            if self.clip {
                effects = effects.with_clip(flui_rendering::traits::PaintClip::Rect {
                    rect: flui_types::Rect::from_origin_size(flui_types::Point::ZERO, size),
                    behavior: flui_types::painting::Clip::HardEdge,
                });
            }
            if self.transform {
                effects = effects.with_transform(flui_types::Matrix4::translation(3.0, 5.0, 0.0));
            }
            effects
        }
    }

    // `start` is the shape the capture is taken with; `then` is what the
    // second frame switches to under an update-only mark, and what a correct
    // repaint must therefore emit.
    let alpha = ShapeShifter {
        alpha: true,
        ..ShapeShifter::default()
    };
    for (label, start, then) in [
        (
            "count: 1 -> 2 layers (transform appears)",
            alpha,
            ShapeShifter {
                transform: true,
                ..alpha
            },
        ),
        (
            "kind: opacity -> transform",
            alpha,
            ShapeShifter {
                alpha: false,
                transform: true,
                ..alpha
            },
        ),
        (
            "count: 1 -> 2 layers (clip appears)",
            alpha,
            ShapeShifter {
                clip: true,
                ..alpha
            },
        ),
        (
            "count: 2 -> 1 layers (clip disappears)",
            ShapeShifter {
                clip: true,
                ..alpha
            },
            alpha,
        ),
    ] {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(start)
                        .label("fx")
                        .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let fx = registry.get("fx").expect("fx is labelled");

        let (mut owner, result) = owner.run_frame();
        let first = result
            .expect("first frame")
            .expect("first frame produces a layer tree");
        assert_eq!(
            counts(&first),
            expected_counts(start),
            "{label}: precondition — captured with the starting shape",
        );

        edit_render_object::<ShapeShifter, _, _>(&mut owner, fx, |object| *object = then);
        owner.mark_needs_composited_layer_update(fx);

        let (owner, result) = owner.run_frame();
        let second = result
            .expect("second frame")
            .expect("second frame produces a layer tree");
        drop(owner);

        assert_eq!(
            counts(&second),
            expected_counts(then),
            "{label}: a shape change cannot be patched into the capture, so the \
             frame must repaint and emit the new shape",
        );
    }
}

/// The mirror of the case above: the REPAINT is queued first.
///
/// Round-seven's fix withdraws the update classification when
/// `mark_needs_paint` selects a boundary. That covers update-then-paint. This
/// is paint-then-update, where the update mark arrives at a boundary that is
/// already dirty and would add the weaker classification on top — with the same
/// consequence after a failed pass: the retry sees `needs_paint` cleared by the
/// walk, reclassifies the boundary as update-only, grafts the pre-error capture
/// and patches just the effect layer, losing the content change.
///
/// The two requesters must be SIBLINGS. `mark_needs_paint` flags every node on
/// its way up, so an update target on that path would refuse itself for the
/// ordinary reason and the boundary would never gain the second record.
#[test]
fn a_repaint_queued_before_an_update_keeps_its_precedence_across_a_failure() {
    use std::sync::atomic::AtomicBool;

    #[derive(Debug)]
    struct PoisonOnDemand(Arc<AtomicBool>);

    impl flui_foundation::Diagnosticable for PoisonOnDemand {}

    impl flui_rendering::traits::RenderBox for PoisonOnDemand {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(10.0), px(10.0)))
        }

        fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
            assert!(
                !self.0.load(Ordering::Relaxed),
                "PoisonOnDemand: armed, poisoning this paint pass on purpose",
            );
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
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
                    box_node(RenderFlex::row())
                        .child(
                            box_node(RenderOpacity::new(0.5))
                                .label("opacity")
                                .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                        )
                        // Sibling of the opacity, not its descendant.
                        .child(box_node(PaintCounter(Arc::clone(&painted))).label("content")),
                ),
            )
            .child(box_node(PoisonOnDemand(Arc::clone(&armed)))),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    let content = registry.get("content").expect("content is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // REPAINT first, update second — the opposite order to the test above.
    owner.mark_needs_paint(content);
    set_opacity(&mut owner, opacity_id, 0.25);
    armed.store(true, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    assert!(
        result.is_err(),
        "precondition: the armed leaf poisons this pass"
    );
    let after_error = painted.load(Ordering::Relaxed);

    armed.store(false, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    result.expect("the retry paints cleanly");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > after_error,
        "a repaint queued before an update must keep its precedence across a \
         failed pass; downgrading the retry to update-only loses the content",
    );
}

/// A boundary queued for an update that the frame never reaches loses its
/// capture, so the next frame that reaches it repaints.
///
/// The residue scan cannot cover this: a layer update leaves the boundary
/// WITHOUT `needs_paint`, so the scan's `if render_node.needs_paint()` guard
/// skips it entirely. For a node that still owns a slot that is harmless —
/// the next graft patches it from live properties — but a node whose effect
/// layer only just APPEARED has no slot at all, so nothing can serve it and
/// its flag blocks any further mark.
///
/// The fixture suppresses the boundary through an ancestor's `skip_paint`
/// rather than through layout, because a boundary dropped by layout is marked
/// for paint when it is laid out again and would repaint for that reason
/// instead — hiding whether the eviction does anything.
#[test]
fn an_unreached_update_boundary_loses_its_capture() {
    for nested in [false, true] {
        unreached_update_boundary_loses_its_capture(nested);
    }
}

/// Body of the test above.
///
/// `nested` wraps the skipped boundary in ANOTHER retained boundary, which is
/// the case a flat fixture cannot see: the enclosing capture flattens the inner
/// boundary's layers, so evicting only the inner leaves the outer replaying the
/// same stale output — and once the queue clears, the graft-time
/// `nested_boundaries` check no longer sees the inner as dirty and nothing
/// stops it. Both boundaries are out of reach that frame, so neither is
/// re-captured.
fn unreached_update_boundary_loses_its_capture(nested: bool) {
    #[derive(Debug, Default)]
    struct GainsATransform {
        enabled: bool,
    }

    impl flui_foundation::Diagnosticable for GainsATransform {}

    impl flui_rendering::traits::RenderBox for GainsATransform {
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)
            } else {
                constraints.smallest()
            }
        }

        flui_rendering::forward_single_child_box_queries!();

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }

        fn paint_effects(&self, _size: Size) -> flui_rendering::traits::PaintEffects {
            if self.enabled {
                flui_rendering::traits::PaintEffects::NONE
                    .with_transform(flui_types::Matrix4::translation(3.0, 5.0, 0.0))
            } else {
                flui_rendering::traits::PaintEffects::NONE
            }
        }
    }

    // The gate sits ABOVE every boundary, so a fully transparent frame leaves
    // them all unreached and none of them re-captures.
    let inner = box_node(RenderRepaintBoundary::new()).child(
        box_node(GainsATransform::default())
            .label("fx")
            .child(box_node(RenderColoredBox::red(20.0, 20.0))),
    );
    let under_gate = if nested {
        box_node(RenderRepaintBoundary::new()).child(inner)
    } else {
        inner
    };
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderOpacity::new(0.5))
                .label("gate")
                .child(under_gate),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let gate = registry.get("gate").expect("gate is labelled");
    let fx = registry.get("fx").expect("fx is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // Second frame: the gate goes fully transparent, so the walk never reaches
    // the boundary — and the effect appears while it is out of sight.
    set_opacity(&mut owner, gate, 0.0);
    edit_render_object::<GainsATransform, _, _>(&mut owner, fx, |object| object.enabled = true);
    owner.mark_needs_composited_layer_update(fx);
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");

    // Third frame: the gate becomes visible again. The boundary below it is
    // clean and unqueued, so a surviving capture would be grafted verbatim —
    // and that capture predates the effect entirely.
    set_opacity(&mut owner, gate, 0.5);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        transform_matrices(&tree).len(),
        1,
        "the capture taken before the effect existed must not survive a frame \
         that could not serve the update; nothing else can restore the layer, \
         because the node owns no slot to patch and its flag blocks new marks \
         (nested = {nested})",
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
    let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));

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

/// A patched transform layer is rebuilt at the origin it was CAPTURED at.
///
/// A transform is conjugated by the accumulated paint origin, so the patch
/// path stores that origin in the capture and replays it. Nothing else pins
/// it: opacity layers are origin-independent (always `Offset::ZERO`), so a
/// mutation replacing the stored origin with zero leaves every other test
/// green while silently moving any patched transform.
///
/// The fixture puts the effect behind a `RenderPadding` so its origin inside
/// the boundary is non-zero — at the boundary root the origin is zero and the
/// bug would be invisible. The oracle is a second owner that PAINTS the same
/// final state, so the patch is compared against what a repaint produces
/// rather than against a matrix written out by hand.
#[test]
fn a_patched_transform_uses_the_origin_it_was_captured_at() {
    #[derive(Debug)]
    struct Shifter {
        dx: f32,
    }

    impl flui_foundation::Diagnosticable for Shifter {}

    impl flui_rendering::traits::RenderBox for Shifter {
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)
            } else {
                constraints.smallest()
            }
        }

        flui_rendering::forward_single_child_box_queries!();

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }

        fn paint_effects(&self, _size: Size) -> flui_rendering::traits::PaintEffects {
            // A SCALE, not a translation. Conjugation by the origin is
            // `T(o)·M·T(-o)`, and translations commute with translations — so
            // a translating fixture cancels the origin entirely and cannot
            // tell a right answer from a wrong one. A scale does not commute,
            // which is what makes the captured origin observable.
            flui_rendering::traits::PaintEffects::NONE
                .with_transform(flui_types::Matrix4::scaling(self.dx, self.dx, 1.0))
        }
    }

    fn mount_shifter(
        dx: f32,
    ) -> (
        PipelineOwner<flui_rendering::pipeline::Idle>,
        flui_foundation::RenderId,
    ) {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    // Non-zero origin for the effect inside the boundary.
                    box_node(flui_objects::RenderPadding::all(12.0)).child(
                        box_node(Shifter { dx })
                            .label("fx")
                            .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                    ),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let fx = registry.get("fx").expect("fx is labelled");
        (owner, fx)
    }

    // Patch path: capture at scale 2, then switch to scale 7 as a layer update.
    let (owner, fx) = mount_shifter(2.0);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    edit_render_object::<Shifter, _, _>(&mut owner, fx, |object| object.dx = 7.0);
    owner.mark_needs_composited_layer_update(fx);
    let (owner, result) = owner.run_frame();
    let patched = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    // Reference: a fresh owner that PAINTS dx = 7 from scratch.
    let (reference, _) = mount_shifter(7.0);
    let (_, result) = reference.run_frame();
    let repainted = result
        .expect("reference frame")
        .expect("reference frame produces a layer tree");

    assert_eq!(
        first_transform_matrix(&patched).expect("a transform layer must be present"),
        first_transform_matrix(&repainted).expect("a transform layer must be present"),
        "a patched transform must equal the one a repaint produces; rebuilding \
         it at the wrong origin moves the layer silently",
    );
}

/// Two opacities under ONE boundary both update in the same frame.
///
/// `layer_patches_for` walks the capture's slots and serves every flagged node
/// it finds, so a second requester in the same boundary must not displace the
/// first — and neither may repaint the subtree. A single-requester fixture
/// cannot see a patch loop that stops after one entry, or one that serves the
/// wrong node's properties into a shared index.
#[test]
fn two_opacities_under_one_boundary_both_update_without_repainting() {
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderOpacity::new(0.5)).label("outer").child(
                    box_node(RenderFlex::row())
                        .child(
                            box_node(RenderOpacity::new(0.25))
                                .label("inner")
                                .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                        )
                        .child(box_node(PaintCounter(Arc::clone(&painted)))),
                ),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let outer = registry.get("outer").expect("outer is labelled");
    let inner = registry.get("inner").expect("inner is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the subtree paints on the first frame"
    );

    set_opacity(&mut owner, outer, 0.75);
    set_opacity(&mut owner, inner, 0.125);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "neither update may repaint the shared subtree",
    );
    // Compared as the u8 the pipeline actually stores: an opacity round-trips
    // through `opacity_to_alpha`, so 0.75 comes back as 191/255 = 0.7490…, and
    // an oracle written in floats would be asserting about the rounding rather
    // than about the update.
    let mut alphas: Vec<u8> = Vec::new();
    let mut stack = vec![tree.root().expect("root")];
    while let Some(id) = stack.pop() {
        if let Some(node) = tree.get(id) {
            if let flui_layer::Layer::Opacity(o) = node.layer() {
                alphas.push((o.alpha() * 255.0).round() as u8);
            }
            stack.extend(node.children().iter().copied());
        }
    }
    alphas.sort_unstable();
    assert_eq!(
        alphas,
        vec![
            (0.125_f32 * 255.0).round() as u8,
            (0.75_f32 * 255.0).round() as u8
        ],
        "both opacity layers must carry their new alpha",
    );
}

/// The mirror of the two orderings above: the marks arrive AFTER the failure.
///
/// Rounds seven and eight both fixed mark-then-fail. This is fail-then-mark,
/// and it defeats the guard those fixes rely on. After a poisoned pass the
/// boundary sits in the paint queue with `NEEDS_PAINT` already cleared by the
/// walk — so a layer update arriving next sees a boundary that looks clean,
/// classifies it update-only, and the retry grafts the pre-failure capture with
/// only the effect layer patched. The content change that queued the repaint is
/// gone, and `clear_paint_queue` erases the evidence.
///
/// Every earlier poisoned-frame test issues its marks BEFORE the failure, where
/// `mark_needs_paint`'s own withdrawal has already run. That is one ordering
/// tested three times; this is the one that was not.
#[test]
fn a_mark_arriving_after_a_failed_pass_does_not_downgrade_the_repaint() {
    use std::sync::atomic::AtomicBool;

    #[derive(Debug)]
    struct PoisonOnDemand(Arc<AtomicBool>);

    impl flui_foundation::Diagnosticable for PoisonOnDemand {}

    impl flui_rendering::traits::RenderBox for PoisonOnDemand {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(10.0), px(10.0)))
        }

        fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
            assert!(
                !self.0.load(Ordering::Relaxed),
                "PoisonOnDemand: armed, poisoning this paint pass on purpose",
            );
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
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
                    box_node(RenderFlex::row())
                        .child(
                            box_node(RenderOpacity::new(0.5))
                                .label("opacity")
                                .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                        )
                        .child(box_node(PaintCounter(Arc::clone(&painted))).label("content")),
                ),
            )
            .child(box_node(PoisonOnDemand(Arc::clone(&armed)))),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry.get("opacity").expect("opacity is labelled");
    let content = registry.get("content").expect("content is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // A real repaint is queued, and the pass fails while serving it.
    owner.mark_needs_paint(content);
    armed.store(true, Ordering::Relaxed);
    let (mut owner, result) = owner.run_frame();
    assert!(
        result.is_err(),
        "precondition: the armed leaf poisons this pass"
    );
    let after_error = painted.load(Ordering::Relaxed);

    // ONLY NOW does the layer update arrive — an animation tick landing on the
    // frame after a failure. The boundary's `needs_paint` was cleared by the
    // failed walk, so a guard reading that flag sees a clean boundary.
    set_opacity(&mut owner, opacity_id, 0.25);

    armed.store(false, Ordering::Relaxed);
    let (owner, result) = owner.run_frame();
    result.expect("the retry paints cleanly");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > after_error,
        "the queued repaint must still be honoured; a mark arriving after the \
         failure must not downgrade it to an update and graft stale content",
    );
}

/// A boundary that LOSES boundary status drops its update classification.
///
/// The compositing walk's lost-boundary branch removes the node's stale
/// paint-queue entry and re-enqueues it through `mark_needs_paint`. That walk
/// starts at the node and goes UP to the nearest boundary — past this node,
/// which is no longer one — so `mark_needs_paint`'s own withdrawal clears the
/// ANCESTOR's update record, never this node's.
///
/// Left behind, the node sits in both classifications at once: queued for a
/// real repaint and still named as an update target. That is the state
/// `run_paint` asserts against, so without the fix this ordering aborts every
/// debug build — and in a release build it is the lost-repaint bug rounds seven
/// and eight were about.
///
/// Reaching it needs `set_repaint_boundary_flag`, because the flag is
/// insert-time configuration today and no production render object varies its
/// boundary status (issue #995). That makes this latent rather than live, and
/// it is exactly the trap #995 would spring.
#[test]
fn losing_boundary_status_withdraws_a_pending_update() {
    let (owner, opacity_id, _sibling, _painted) = mount_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    let boundary = owner
        .render_tree()
        .get(opacity_id)
        .and_then(|node| node.links().parent())
        .expect("the opacity sits under a repaint boundary");

    let retained_before = owner.retained_boundary_count();
    assert!(
        retained_before > 0,
        "precondition: the first frame retained the boundary under test",
    );

    // A descendant asks for a layer update, so the boundary is classified.
    owner.mark_needs_composited_layer_update(opacity_id);

    // …and then the boundary stops being one, which is what drives the
    // compositing walk's lost-boundary branch.
    owner
        .render_tree()
        .get(boundary)
        .expect("boundary node")
        .set_repaint_boundary_flag(false);
    owner.mark_needs_compositing_bits_update(boundary);

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("the frame after a boundary is lost must not abort")
        .expect("the frame produces a layer tree");

    // An output assertion, not just "it did not abort". The `debug_assert` in
    // `run_paint` is the sharper oracle but compiles out of a release build, so
    // without this the test would prove nothing where it matters most: the node
    // is now an ordinary non-boundary, and the frame must render its subtree
    // rather than replay retained output it no longer owns.
    assert!(
        first_opacity_alpha(&tree).is_some(),
        "the subtree must still be composited after its boundary is lost",
    );
    // Deliberately NOT asserting the capture was evicted. The compositing
    // walk reads the `IS_REPAINT_BOUNDARY` flag this test flips, while the
    // paint walk reads the live `is_repaint_boundary()` trait answer — which
    // still says "boundary", so the node re-captures on this very frame. The
    // two oracles cannot be made to agree from a test while issue #995 stands,
    // and asserting on the count here would only pin that disagreement.
    let _ = retained_before;
    drop(owner);
}

/// `nested_boundaries` must survive a graft, or a THREE-level nest goes stale.
///
/// `note_boundary` records a boundary into every open capture scope, and
/// `open_capture` wraps only the REPAINT arm. So when an outer boundary
/// repaints while a middle one is merely grafted, the walk never descends past
/// the middle — and the grandchild boundary, whose layers the graft physically
/// clones into the outer capture, is never noted in the outer's list.
///
/// The outer capture then embeds a boundary it does not name, so the graft
/// refusal that exists to stop exactly this cannot see it: the grandchild goes
/// dirty, the outer is clean and its list does not mention it, the outer is
/// reused, and the grandchild's stale layers are replayed. The evictions miss
/// it for the same reason.
///
/// Two levels always paint fresh and hide it — which is why every other nesting
/// test in this file passes either way.
#[test]
fn a_grandchild_boundary_is_still_named_after_its_parent_was_grafted() {
    let painted = Arc::new(AtomicUsize::new(0));
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).label("outer").child(
                box_node(RenderFlex::row())
                    // A direct child of `outer`, so it can be dirtied without
                    // touching either boundary below.
                    .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("direct"))
                    .child(
                        box_node(RenderRepaintBoundary::new()).label("middle").child(
                            box_node(RenderRepaintBoundary::new()).label("inner").child(
                                box_node(PaintCounter(Arc::clone(&painted))).label("target"),
                            ),
                        ),
                    ),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let direct = registry.get("direct").expect("direct is labelled");
    let target = registry.get("target").expect("target is labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // Frame 2: dirty only the OUTER boundary's own content. It repaints and
    // grafts `middle` without descending — so `inner` is never noted into the
    // outer capture, even though the graft clones its layers in.
    owner.mark_needs_paint(direct);
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");
    let before = painted.load(Ordering::Relaxed);

    // Frame 3: dirty the innermost content. `mark_needs_paint` stops at
    // `inner`, so the outer is clean — and must still refuse to graft.
    owner.mark_needs_paint(target);
    let (owner, result) = owner.run_frame();
    result.expect("third frame");
    drop(owner);

    assert!(
        painted.load(Ordering::Relaxed) > before,
        "the outer capture embeds the grandchild boundary's layers, so it must \
         name it and decline to graft while it is dirty; otherwise the \
         grandchild's stale output is replayed and nothing ever repaints it",
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
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

/// The update reaches the STORED capture, not just the emitted frame.
///
/// Mirrors `a_layer_update_is_written_back_into_the_retained_capture`. Three
/// frames, because two cannot see this: change, observe, then dirty something
/// else so the boundary is grafted for an unrelated reason. If the patch only
/// touched the emitted tree, the capture still holds the matrix it was
/// captured with, and this third frame reverts it — permanently, because
/// nothing marks it again.
///
/// Frame 2 also asserts the paint counter is flat — unlike its opacity
/// counterpart, this check is load-bearing HERE, not merely a restated
/// precondition: `RenderTransform` has an older, still-present full-repaint
/// route to the identical matrix (`FragmentOp::PushTransform`'s replay arm in
/// `paint.rs`, which `paint()` used exclusively before this change and which
/// remains reachable whenever the boundary repaints for any other reason).
/// Without pinning that frame 2 took the graft+patch path — not a repaint
/// that incidentally produces the same output — this test cannot tell "the
/// patch wrote back" from "the repaint route also happens to write back",
/// and would pass unchanged if the write-back loop in `run_paint` were
/// deleted.
#[test]
fn a_transform_layer_update_is_written_back_into_the_retained_capture() {
    let (owner, transform_id, _child, sibling, painted) =
        mount_transform_under_boundary(Matrix4::scaling(2.0, 2.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);

    set_transform(&mut owner, transform_id, Matrix4::scaling(3.0, 3.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");
    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "precondition: frame 2 must take the update-only path, not a repaint \
         — see this test's own doc for why that matters here",
    );

    // Third frame: the transform is clean; the sibling boundary forces the pass.
    owner.mark_needs_paint(sibling);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        first_transform_matrix(&tree),
        Some(Matrix4::scaling(3.0, 3.0, 1.0)),
        "a later frame that grafts the capture for an unrelated reason must \
         replay the UPDATED matrix, not the one it was captured with",
    );
}

/// `has_child` is written only in `perform_layout`, so a setter reads the
/// PREVIOUS frame's value — reachable when a transform-only setter and a
/// same-frame child removal target the SAME node.
///
/// Two independent mechanisms make the combination safe:
///
/// 1. `remove_render_object` fires `note_render_child_membership_changed` for
///    the transform (whose child just left it), applying `LAYOUT |
///    COMPOSITING_BITS | SEMANTICS`. `LAYOUT` carries `PAINT_BIT`. The
///    `mark_needs_layout` flag walk that `LAYOUT` triggers sets
///    `NEEDS_LAYOUT` on every ancestor up to the relayout boundary, so a
///    flagged node cannot take the constraints-cache short-circuit — the
///    enclosing repaint boundary re-enters layout and is recorded as having
///    laid out (`pipeline/owner/layout.rs`), which upgrades the boundary's
///    paint-queue entry from the setter's `LayerUpdate` to `Repaint` —
///    `PaintQueue::enqueue` never downgrades (`scheduler.rs`'s
///    `enqueue_paint` doc), so this holds whichever of the two marks landed
///    first.
/// 2. Even were that classification wrong, `layer_patches_for` reads
///    `paint_effects()` LIVE — after layout, which always runs before
///    paint, has already reset `has_child` to `false` — so it computes zero
///    fresh effect layers against the ONE slot the capture holds, refuses the
///    patch on the shape mismatch, and falls back to a repaint on its own.
///
/// This test does not try to distinguish which mechanism fires; it pins that
/// the OUTCOME is correct either way — no stale layer, no repainted ghost of
/// the removed child.
#[test]
fn a_same_frame_child_removal_and_transform_setter_still_repaints_correctly() {
    let (owner, transform_id, child_id, _sibling, painted) =
        mount_transform_under_boundary(Matrix4::scaling(2.0, 2.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    assert!(
        painted.load(Ordering::Relaxed) > 0,
        "precondition: the child paints on the first frame",
    );
    assert!(
        first_transform_matrix(&first).is_some(),
        "precondition: the first frame emits a TransformLayer",
    );
    let before = painted.load(Ordering::Relaxed);

    // Same frame: a matrix-only setter call (which reads the STALE
    // has_child == true and reports COMPOSITED_LAYER_UPDATE) and the child's
    // removal (which reports LAYOUT for the same node).
    set_transform(&mut owner, transform_id, Matrix4::scaling(5.0, 5.0, 1.0));
    owner.remove_render_object(child_id);

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the removed child cannot paint again",
    );
    assert_eq!(
        first_transform_matrix(&tree),
        None,
        "with the child gone, RenderTransform emits no TransformLayer at all — \
         a stale has_child read that let a patch through would either keep \
         the (now childless) layer alive or graft the removed child's \
         content back in",
    );
}

/// `layer_patches_for` rebuilds a patched transform layer at the CAPTURED
/// `EffectSlots::origin`, not a live one — correct only while that origin
/// still matches the node's live accumulated position. Nothing states that
/// invariant in code; this pins it.
///
/// Unlike opacity (whose layer is position-independent, always
/// `Offset::ZERO`), a transform layer is conjugated by the accumulated paint
/// origin, so a stale origin would silently rotate/scale the content about
/// the WRONG point. This moves a sibling's size — an ordinary
/// `mark_needs_layout` — in the SAME frame as a matrix-only
/// `COMPOSITED_LAYER_UPDATE` request, and compares against a freshly-mounted
/// second owner that paints the same final state from scratch. The matrix is
/// a SCALE, never a translation: translations commute with the origin
/// conjugation (`T(o)·T(v)·T(-o) = T(v)`) and cannot tell a right answer from
/// a wrong one, so only a non-commuting matrix makes a stale origin visible —
/// "the right layer count, the right discriminant, and the wrong matrix",
/// which no other oracle in this file would catch.
///
/// The node sits behind a `RenderPadding` so its in-boundary origin is
/// non-zero — at the boundary root the origin is zero and a stale-origin bug
/// would be invisible.
///
/// **What this pins is the INVARIANT, not this change.** As constructed the
/// scenario does not reach `layer_patches_for` at all, on either side of this
/// change, and that is the point rather than a shortfall: a same-frame layout
/// change ANYWHERE inside a boundary marks that same boundary needing paint
/// too — the `mark_needs_layout` flag walk sets `NEEDS_LAYOUT` on every
/// ancestor up to the relayout boundary, a flagged node cannot take the
/// constraints-cache short-circuit, so the enclosing repaint boundary
/// re-enters layout and is recorded (`pipeline/owner/layout.rs`) — and if the
/// relayout boundary sits below it, the dirty-root mark walks up to it too.
/// Either route enqueues `Repaint`, upgrading any `LayerUpdate` entry the
/// matrix setter queued (`PaintQueue::enqueue` never downgrades). Growing
/// the sibling therefore forces a full repaint, which computes the
/// conjugation from a LIVE origin and is trivially correct.
///
/// That structural guarantee is the *only* reason a transform patch may reuse
/// a captured origin, and nothing else in the tree states it. Decouple "the
/// boundary moved" from "the boundary is marked needing paint" — an
/// optimisation someone will eventually attempt on that `mark_needs_paint`
/// loop — and the patch path becomes reachable with a stale origin.
///
/// So this test has teeth, and they were measured rather than assumed:
/// replacing that loop's body with `let _ = id;` fails it with the exact
/// predicted signature — the layer count right, the discriminant right, and
/// the translation column reading `-192.0` where a repaint produces `-612.0`.
/// The subtree would render scaled about the wrong point, silently.
///
/// Complementary, not redundant: the patch path's origin correctness for a
/// genuine `COMPOSITED_LAYER_UPDATE` graft — no repaint, non-zero captured
/// origin — is pinned by the pre-existing
/// `a_patched_transform_uses_the_origin_it_was_captured_at` above, unmodified
/// by this change. That one proves the patch uses the captured origin
/// correctly; this one proves nothing can move the node out from under it.
#[test]
fn a_same_frame_layout_change_forces_the_repaint_a_transform_patch_relies_on() {
    #[derive(Debug)]
    struct Grower {
        width: f32,
    }

    impl flui_foundation::Diagnosticable for Grower {}

    impl flui_rendering::traits::RenderBox for Grower {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(self.width), px(20.0)))
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }
    }

    fn mount(
        width: f32,
        matrix: Matrix4,
    ) -> (
        PipelineOwner<flui_rendering::pipeline::Idle>,
        flui_foundation::RenderId,
        flui_foundation::RenderId,
    ) {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderFlex::row())
                        .child(box_node(Grower { width }).label("grower"))
                        .child(
                            box_node(RenderPadding::all(12.0)).child(
                                box_node(RenderTransform::new(matrix))
                                    .label("transform")
                                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                            ),
                        ),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let grower = registry.get("grower").expect("grower is labelled");
        let transform = registry.get("transform").expect("transform is labelled");
        (owner, grower, transform)
    }

    let (owner, grower_id, transform_id) = mount(20.0, Matrix4::scaling(2.0, 2.0, 1.0));
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    // Same frame: grow the preceding sibling (an ordinary layout change) AND
    // change the transform's matrix through a composited-layer-update
    // request.
    edit_render_object::<Grower, _, _>(&mut owner, grower_id, |object| object.width = 90.0);
    owner.mark_needs_layout(grower_id);
    set_transform(&mut owner, transform_id, Matrix4::scaling(7.0, 7.0, 1.0));

    let (owner, result) = owner.run_frame();
    let moved = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    // Reference: a fresh owner that paints the same final state — grower at
    // width 90, transform at scale 7 — from scratch.
    let (reference, _grower, _transform) = mount(90.0, Matrix4::scaling(7.0, 7.0, 1.0));
    let (_, result) = reference.run_frame();
    let repainted = result
        .expect("reference frame")
        .expect("reference frame produces a layer tree");

    assert_eq!(
        first_transform_matrix(&moved),
        first_transform_matrix(&repainted),
        "a same-frame move and matrix-only update together must still produce \
         the matrix a full repaint would; a stale captured origin would move \
         it silently",
    );
}

/// Clip counterpart of
/// `a_same_frame_layout_change_forces_the_repaint_a_transform_patch_relies_on`
/// above: the identical same-frame-layout-change-forces-a-repaint invariant,
/// pinned through a clip patch instead of a transform one.
///
/// No shipped clip producer reports an update-only `RenderUpdateImpact` yet
/// (`clip_layer`'s translate-by-origin in `pipeline/owner/paint.rs` is only
/// ever reached through a full repaint today), so `ClipShifter` below is a
/// proxy that reports `COMPOSITED_LAYER_UPDATE` on purpose — mirroring
/// `ShapeShifter` and `AppearingTransform` elsewhere in this file — to drive
/// that arm of `layer_patches_for` through the same-frame race at all.
///
/// Like the transform test, a clip rect is translated by the node's
/// accumulated paint origin at emission time (`clip_layer`, same file),
/// never conjugated the way a transform matrix is — but a stale ORIGIN is
/// just as wrong for a translation as for a conjugation: it silently shifts
/// the clip to the wrong place. Growing the preceding `Grower` sibling and
/// issuing a clip-only composited-layer-update request in the SAME frame
/// must still repaint: the layout change marks the enclosing boundary
/// needing paint too (`PaintQueue::enqueue` never downgrades a queued
/// `Repaint`), so the resulting clip rect is computed from the LIVE origin,
/// not a captured one.
#[test]
fn a_same_frame_layout_change_forces_the_repaint_a_clip_patch_relies_on() {
    #[derive(Debug)]
    struct Grower {
        width: f32,
    }

    impl flui_foundation::Diagnosticable for Grower {}

    impl flui_rendering::traits::RenderBox for Grower {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            ctx.constrain(Size::new(px(self.width), px(20.0)))
        }

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Leaf,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }
    }

    /// A clip double that reports the WRONG impact on purpose — a rect-only
    /// field mutation goes through `mark_needs_composited_layer_update`,
    /// never a repaint — since no shipped clip producer does this yet. See
    /// this test's doc above.
    #[derive(Debug)]
    struct ClipShifter {
        rect: flui_types::Rect<flui_types::Pixels>,
    }

    impl flui_foundation::Diagnosticable for ClipShifter {}

    impl flui_rendering::traits::RenderBox for ClipShifter {
        type Arity = flui_tree::Single;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> Size {
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)
            } else {
                constraints.smallest()
            }
        }

        flui_rendering::forward_single_child_box_queries!();

        fn hit_test(
            &self,
            _ctx: &mut flui_rendering::context::BoxHitTestContext<
                '_,
                flui_tree::Single,
                flui_rendering::parent_data::BoxParentData,
            >,
        ) -> bool {
            false
        }

        fn paint_effects(&self, _size: Size) -> flui_rendering::traits::PaintEffects {
            flui_rendering::traits::PaintEffects::NONE.with_clip(
                flui_rendering::traits::PaintClip::Rect {
                    rect: self.rect,
                    behavior: flui_types::painting::Clip::HardEdge,
                },
            )
        }
    }

    fn mount(
        width: f32,
        rect: flui_types::Rect<flui_types::Pixels>,
    ) -> (
        PipelineOwner<flui_rendering::pipeline::Idle>,
        flui_foundation::RenderId,
        flui_foundation::RenderId,
    ) {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderFlex::row())
                        .child(box_node(Grower { width }).label("grower"))
                        .child(
                            box_node(RenderPadding::all(12.0)).child(
                                box_node(ClipShifter { rect })
                                    .label("clip")
                                    .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                            ),
                        ),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let grower = registry.get("grower").expect("grower is labelled");
        let clip = registry.get("clip").expect("clip is labelled");
        (owner, grower, clip)
    }

    /// Applies a new clip rect through the update-only seam
    /// `mark_needs_composited_layer_update` drives directly — never through
    /// `RenderUpdateImpact`, since `ClipShifter` reports the wrong impact on
    /// purpose (see the test's doc above).
    fn set_rect(
        owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
        id: flui_foundation::RenderId,
        rect: flui_types::Rect<flui_types::Pixels>,
    ) {
        edit_render_object::<ClipShifter, _, _>(owner, id, |c| c.rect = rect);
        owner.mark_needs_composited_layer_update(id);
    }

    let rect1 = flui_types::Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0));
    let (owner, grower_id, clip_id) = mount(20.0, rect1);
    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    assert_eq!(
        clip_rects(&first).len(),
        1,
        "precondition: the first frame emits one ClipRectLayer",
    );

    // Same frame: grow the preceding sibling (an ordinary layout change) AND
    // change the clip's rect through a composited-layer-update request. The
    // new rect differs from `rect1` in BOTH dimensions and is not square, so
    // neither a transposition nor a stale size can pass this by accident.
    edit_render_object::<Grower, _, _>(&mut owner, grower_id, |object| object.width = 90.0);
    owner.mark_needs_layout(grower_id);
    let rect2 = flui_types::Rect::from_xywh(px(0.0), px(0.0), px(15.0), px(5.0));
    set_rect(&mut owner, clip_id, rect2);

    let (owner, result) = owner.run_frame();
    let moved = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    // Reference: a fresh owner that paints the same final state — grower at
    // width 90, clip rect2 — from scratch.
    let (reference, _grower, _clip) = mount(90.0, rect2);
    let (_, result) = reference.run_frame();
    let repainted = result
        .expect("reference frame")
        .expect("reference frame produces a layer tree");

    assert_eq!(
        clip_rects(&moved),
        clip_rects(&repainted),
        "a same-frame move and clip-only update together must still produce \
         the clip rect a full repaint would; a stale captured origin would \
         leave it at the old position",
    );

    // The fixture itself must have teeth: the grower's +70px width delta
    // really did shift the clip's origin, so a correct repaint (new rect at
    // the NEW origin) cannot coincide with a stale-origin patch (new rect at
    // the OLD origin) or with no update at all (old rect at the old
    // origin) by accident.
    let first_origin = clip_rects(&first)[0].min;
    assert_eq!(
        clip_rects(&moved).len(),
        1,
        "precondition: the second frame's tree must carry exactly one clip \
         rect to index into",
    );
    let moved_origin = clip_rects(&moved)[0].min;
    assert_eq!(
        moved_origin.x - first_origin.x,
        px(70.0),
        "the grower's width delta (20 -> 90) must shift the clip rect's \
         origin by the same 70px in x; first frame origin {first_origin:?}, \
         second frame origin {moved_origin:?}",
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
    type Arity = flui_tree::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_tree::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        ctx.constrain(self.size)
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let rect = flui_types::Rect::from_origin_size(flui_types::Point::ZERO, ctx.size());
        ctx.canvas()
            .draw_rect(rect, &flui_painting::Paint::fill(flui_types::Color::RED));
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_tree::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

/// Mirrors `mount_transform_under_boundary` for `RenderRotatedBox`: root row
/// → [boundary → padding → rotated box → drawing counting leaf, boundary →
/// leaf].
///
/// The `RenderPadding` gives the rotated box a non-zero origin INSIDE the
/// boundary — the `transform` field of `RenderNode::paint_effects` is
/// conjugated by the accumulated paint origin, so a stale-origin bug that a
/// zero origin cannot expose is
/// exactly the failure mode this shape is built to catch (mirrors
/// `a_patched_transform_uses_the_origin_it_was_captured_at`'s fixture, above).
fn mount_rotated_box_under_boundary(
    quarter_turns: i32,
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
                        box_node(RenderRotatedBox::new(quarter_turns))
                            .label("rotated")
                            .child(box_node(DrawingPaintCounter {
                                size: Size::new(px(30.0), px(50.0)),
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let rotated_id = registry.get("rotated").expect("rotated is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, rotated_id, sibling, painted)
}

/// Applies a new quarter-turn count through the same seam a widget rebuild
/// uses: the setter reports an impact, the owner applies it.
fn set_rotated_box_quarter_turns(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    quarter_turns: i32,
) {
    update_render_object::<RenderRotatedBox, _>(owner, id, |r| r.set_quarter_turns(quarter_turns));
}

/// A parity-preserving quarter-turn change updates the emitted TransformLayer
/// without repainting the subtree, and the write-back reaches the STORED
/// capture — the three-way oracle plus write-back frame, combined into one
/// test the way `a_transform_change_updates_the_layer_without_repainting_the_subtree`
/// and `a_transform_layer_update_is_written_back_into_the_retained_capture`
/// are two.
///
/// Frame 1: mount at turn 1. Frame 2: `set_quarter_turns(3)` — same parity,
/// different quadrant, so `COMPOSITED_LAYER_UPDATE | SEMANTICS`. Three
/// things are required together, and none alone is evidence: the paint count
/// staying FLAT (alone passes if nothing painted at all, including a stale
/// graft), the matrix matching a FRESH owner mounted directly at turn 3
/// (alone passes on a full repaint that happens to compute the same value),
/// and the matrix differing from the turn-1 matrix (alone passes on a patch
/// that is silently a no-op). Frame 3: dirty the SIBLING boundary so the root
/// repaints and the rotated box's own (clean) boundary is grafted for an
/// unrelated reason — the only way to observe what the STORED capture, not
/// just the emitted frame, holds.
#[test]
fn a_rotated_box_quarter_turn_update_patches_the_layer_and_writes_back() {
    let (owner, rotated_id, sibling, painted) = mount_rotated_box_under_boundary(1);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_rotated_box_quarter_turns(&mut owner, rotated_id, 3);
    let (mut owner, result) = owner.run_frame();
    let frame2 = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the subtree under the rotated box must NOT repaint for a \
         parity-preserving turn change",
    );

    let (turn1_reference, _, _, _) = mount_rotated_box_under_boundary(1);
    let (_, result) = turn1_reference.run_frame();
    let turn1_matrix = first_transform_matrix(
        &result
            .expect("turn-1 reference frame")
            .expect("turn-1 reference frame produces a layer tree"),
    )
    .expect("turn-1 reference emits a TransformLayer");

    let (turn3_reference, _, _, _) = mount_rotated_box_under_boundary(3);
    let (_, result) = turn3_reference.run_frame();
    let turn3_matrix = first_transform_matrix(
        &result
            .expect("turn-3 reference frame")
            .expect("turn-3 reference frame produces a layer tree"),
    )
    .expect("turn-3 reference emits a TransformLayer");

    assert_eq!(
        first_transform_matrix(&frame2),
        Some(turn3_matrix),
        "the patched TransformLayer must equal what a fresh owner mounted \
         directly at turn 3 produces",
    );
    assert_ne!(
        turn3_matrix, turn1_matrix,
        "and turn 3 must differ from turn 1 — otherwise the patch could be a \
         no-op that happened to pass",
    );

    // Third frame: the rotated box is clean; the sibling boundary forces the
    // pass, grafting the rotated box's boundary for an unrelated reason.
    owner.mark_needs_paint(sibling);
    let (owner, result) = owner.run_frame();
    let frame3 = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the third frame must still not repaint the rotated box's subtree",
    );
    assert_eq!(
        first_transform_matrix(&frame3),
        Some(turn3_matrix),
        "a later frame that grafts the capture for an unrelated reason must \
         replay the UPDATED (turn-3) matrix, not the one it was captured with",
    );
}

/// A parity change (odd → even) still relayouts and repaints — the fast
/// path's complement, and the case that would make a reverted
/// `set_quarter_turns` (unconditional `LAYOUT`) indistinguishable from the
/// real thing if this file tested only the parity-preserving case above.
#[test]
fn a_rotated_box_parity_change_relayouts_and_swaps_size() {
    let (owner, rotated_id, _sibling, painted) = mount_rotated_box_under_boundary(1);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );
    let size_before = owner
        .render_tree()
        .get(rotated_id)
        .and_then(flui_rendering::storage::RenderNode::size)
        .expect("rotated box has a committed size after the first frame");

    set_rotated_box_quarter_turns(&mut owner, rotated_id, 2);
    let (owner, result) = owner.run_frame();
    let frame2 = result
        .expect("second frame")
        .expect("second frame produces a layer tree");

    assert!(
        painted.load(Ordering::Relaxed) > before,
        "a parity change must relayout and therefore repaint the subtree — \
         unlike the parity-preserving update in \
         a_rotated_box_quarter_turn_update_patches_the_layer_and_writes_back",
    );
    let size_after = owner
        .render_tree()
        .get(rotated_id)
        .and_then(flui_rendering::storage::RenderNode::size)
        .expect("rotated box has a committed size after the second frame");
    drop(owner);

    assert_eq!(
        size_after,
        Size::new(size_before.height, size_before.width),
        "an odd-to-even parity change must swap the box's own reported size",
    );

    let (reference, _, _, _) = mount_rotated_box_under_boundary(2);
    let (_, result) = reference.run_frame();
    let turn2_matrix = first_transform_matrix(
        &result
            .expect("turn-2 reference frame")
            .expect("turn-2 reference frame produces a layer tree"),
    )
    .expect("a rotated box with a child always emits a TransformLayer");
    assert_eq!(
        first_transform_matrix(&frame2).expect("the repainted frame carries the layer"),
        turn2_matrix,
        "the repainted matrix must equal what a fresh owner mounted directly \
         at turn 2 produces",
    );
}

/// Mirrors `mount_rotated_box_under_boundary` but leaves the rotated box
/// CHILDLESS: no leaf, so `RenderRotatedBox::paint_effects`'s `transform`
/// field is `None` and the boundary's retained capture never allocates an
/// `effect_slots` entry for it — the shape `layer_patches_for`'s "target has
/// no slot in this capture" refusal (`pipeline/owner/paint.rs`) exists to
/// catch.
fn mount_childless_rotated_box_under_boundary(
    quarter_turns: i32,
) -> (
    PipelineOwner<flui_rendering::pipeline::Idle>,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderPadding::all(12.0))
                    .child(box_node(RenderRotatedBox::new(quarter_turns)).label("rotated")),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let rotated_id = registry.get("rotated").expect("rotated is labelled");
    (owner, rotated_id)
}

/// A childless rotated box's layer-update mark degrades to a repaint
/// correctly, AND the repaint actually clears the flag the refused patch
/// never touched — the property `layer_patches_for`'s own comment calls
/// "latent today" ("the next caller to get that wrong would graft stale
/// output silently instead of failing loudly"): a childless `RenderRotatedBox`
/// whose setter reports `COMPOSITED_LAYER_UPDATE` (it has no `has_child`
/// gate — see the ARCHITECTURE.md "Two edge cases" paragraph) is the first
/// real caller that hits it, because `RotatedBox::new(n)` seeds an empty
/// child by default.
///
/// Three things, and the third is the one that actually pins the refusal —
/// the first two pass just as well on a graft that silently swallowed the
/// request, since a childless box paints nothing either way:
///
/// (a) the fallback must not panic;
/// (b) the resulting frame must match a fresh owner mounted directly at the
///     new turn (structurally: same layer-kind/child-count fingerprint, since
///     neither ever emits a `TransformLayer`);
/// (c) the refusal must not strand the node's own
///     `NEEDS_COMPOSITED_LAYER_UPDATE` flag. Checked directly rather than
///     only through "frame 3 is empty" (which passes either way: nothing
///     re-populates the SCHEDULER's paint queue from a stale per-node flag on
///     its own, so an empty frame 3 is not evidence) — the flag is public API
///     (`RenderNode::needs_composited_layer_update`) precisely so a test can
///     observe it. A stuck flag matters because it self-refuses: a REAL
///     second change to the same node is silently dropped
///     (`mark_needs_composited_layer_update`'s own guard reads the flag), so
///     frame 3 then re-marks the SAME node with a further same-parity turn
///     and checks it actually reaches the boundary this time.
///
/// Red evidence: skipping `layer_patches_for`'s `contains_key` guard leaves
/// (a) and (b) green (a childless box paints nothing under either path, so
/// the empty graft and the real repaint are indistinguishable in the emitted
/// frame) and fails (c) both ways — the flag reads `true` right after frame
/// 2, and the frame-3 re-mark is swallowed, so frame 3 produces no layer
/// tree at all instead of a second (degraded-but-real) repaint.
#[test]
fn a_childless_rotated_box_layer_update_falls_back_to_a_repaint_and_clears_the_flag() {
    let (owner, rotated_id) = mount_childless_rotated_box_under_boundary(1);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");

    set_rotated_box_quarter_turns(&mut owner, rotated_id, 3);
    let (owner, result) = owner.run_frame();
    let frame2 = result
        .expect("second frame must not panic or error") // (a)
        .expect("second frame produces a layer tree");

    let (reference, _) = mount_childless_rotated_box_under_boundary(3);
    let (_, result) = reference.run_frame();
    let expected = result
        .expect("turn-3 reference frame")
        .expect("turn-3 reference frame produces a layer tree");
    assert_eq!(
        fingerprint(&frame2),
        fingerprint(&expected),
        "a childless rotated box's layer-update mark must degrade to a \
         repaint whose fingerprint matches a fresh owner mounted directly at \
         turn 3", // (b)
    );

    let still_needs_layer_update = owner
        .render_tree()
        .get(rotated_id)
        .expect("rotated box node")
        .needs_composited_layer_update();
    assert!(
        !still_needs_layer_update,
        "the repaint that served the refused patch must clear \
         NEEDS_COMPOSITED_LAYER_UPDATE on the childless node the same way it \
         clears every other node it visits — a stuck flag here means the \
         patch's refusal never actually visited it", // (c), direct
    );

    // Nothing newly dirtied: a clean frame produces no layer tree at all.
    let (mut owner, result) = owner.run_frame();
    assert!(
        result.expect("third frame must not error").is_none(),
        "with nothing dirtied, frame 3 must produce no layer tree",
    );

    // The flag's real consequence: a stuck flag self-refuses future marks
    // (`mark_needs_composited_layer_update` returns early while the flag is
    // already set), so a SECOND real same-parity change must still reach the
    // boundary rather than being silently dropped.
    set_rotated_box_quarter_turns(&mut owner, rotated_id, 1);
    let (owner, result) = owner.run_frame();
    let frame4 = result.expect("fourth frame must not error").expect(
        "a second same-parity change must still produce a frame — a \
                 stuck flag from the first refusal would have swallowed this \
                 mark and left frame 4 empty, just like a clean frame",
    ); // (c), consequence
    drop(owner);

    let (reference, _) = mount_childless_rotated_box_under_boundary(1);
    let (_, result) = reference.run_frame();
    let expected = result
        .expect("turn-1 reference frame")
        .expect("turn-1 reference frame produces a layer tree");
    assert_eq!(
        fingerprint(&frame4),
        fingerprint(&expected),
        "the second refusal must ALSO degrade to a correct repaint, matching \
         a fresh owner mounted directly at turn 1",
    );
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
    radius: f32,
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
                                .with_border_radius(BorderRadius::circular(px(radius))),
                        )
                        .label("clip")
                        .child(box_node(DrawingPaintCounter {
                            size: Size::new(px(30.0), px(20.0)),
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let clip_id = registry.get("clip").expect("clip is labelled");
    let sibling = registry.get("sibling").expect("sibling is labelled");
    (owner, clip_id, sibling, painted)
}

/// Applies a new border radius through the same seam a widget rebuild uses:
/// the setter reports an impact, the owner applies it.
fn set_border_radius(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    radius: f32,
) {
    update_render_object::<RenderClipRRect, _>(owner, id, |c| {
        c.set_border_radius(Some(BorderRadius::circular(px(radius))))
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

/// The update reaches the STORED capture, not just the emitted frame.
///
/// Three frames, because two cannot see this: change, observe, then dirty
/// something else so the boundary is grafted for an unrelated reason. If the
/// patch only touched the emitted tree, the capture still holds the radius
/// it was captured with, and this third frame reverts it — permanently,
/// because nothing marks it again.
#[test]
fn a_clip_layer_update_is_written_back_into_the_retained_capture() {
    let (owner, clip_id, sibling, painted) = mount_clip_rrect_under_boundary(8.0);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);

    set_border_radius(&mut owner, clip_id, 2.0);
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");
    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "precondition: frame 2 must take the update-only path, not a repaint \
         — a repaint would still write the right radius, but then this test \
         could not tell the patch's write-back from a repaint's",
    );

    // Third frame: the clip is clean; the sibling boundary forces the pass.
    owner.mark_needs_paint(sibling);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    let rrects = clip_rrects(&tree);
    assert_eq!(
        rrects.len(),
        1,
        "precondition: exactly one ClipRRectLayer must be present: {rrects:?}",
    );

    let (reference, _, _, _) = mount_clip_rrect_under_boundary(2.0);
    let (_, result) = reference.run_frame();
    let reference_rrects = clip_rrects(
        &result
            .expect("reference frame")
            .expect("reference frame produces a layer tree"),
    );
    assert_eq!(
        rrects[0], reference_rrects[0],
        "a later frame that grafts the capture for an unrelated reason must \
         replay the UPDATED radius, not the one it was captured with",
    );
}

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

/// A `FlowDelegate` that paints its single child untransformed. A local,
/// minimal fixture: the crate's own `#[cfg(test)]` delegates in `flow.rs`
/// are private to that module and unreachable from an integration test.
#[derive(Debug)]
struct SingleChildFlowDelegate;

impl flui_foundation::Diagnosticable for SingleChildFlowDelegate {}

impl flui_rendering::delegates::FlowDelegate for SingleChildFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        constraints
    }

    fn paint_children(&self, context: &mut flui_rendering::delegates::FlowPaintingContext<'_, '_>) {
        if context.child_count() > 0 {
            context.paint_child(0, Matrix4::IDENTITY);
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn flui_rendering::delegates::FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn flui_rendering::delegates::FlowDelegate) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn a_flow_clip_behavior_change_is_structural_and_refused() {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderFlow::new(Arc::new(SingleChildFlowDelegate)))
                .label("flow")
                .child(box_node(RenderColoredBox::red(30.0, 20.0))),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let flow_id = registry.get("flow").expect("flow is labelled");

    let (mut owner, result) = owner.run_frame();
    let first = result
        .expect("first frame")
        .expect("first frame produces a layer tree");
    assert!(
        layer_structure(&first).contains(&"ClipRect"),
        "precondition: a flow at Clip::HardEdge (the default) emits a \
         ClipRect layer: {:?}",
        layer_structure(&first),
    );

    // The setter's own classification, pinned directly rather than assumed.
    let impact = edit_render_object::<RenderFlow, _, _>(&mut owner, flow_id, |flow| {
        flow.set_clip_behavior(flui_types::painting::Clip::None)
    });
    assert_eq!(
        impact,
        flui_rendering::RenderUpdateImpact::PAINT | flui_rendering::RenderUpdateImpact::SEMANTICS,
        "RenderFlow::set_clip_behavior must report a structural repaint — its \
         clip can add or remove a layer, which a patch cannot express",
    );

    // The wrong classification, on purpose: bypass the setter's own (correct)
    // impact and drive the update-only path directly, mirroring
    // `ClipShifter`'s `set_rect` helper above.
    owner.mark_needs_composited_layer_update(flow_id);
    let (owner, result) = owner.run_frame();
    let second = result
        .expect(
            "second frame must not error — the refusal must degrade to a \
             repaint, not fail the pass",
        )
        .expect("second frame produces a layer tree");

    assert!(
        !layer_structure(&second).contains(&"ClipRect"),
        "the flow's clip is gated on Clip::None, so a real repaint at the new \
         behavior must drop the ClipRect layer entirely — a patch could not \
         have removed it: {:?}",
        layer_structure(&second),
    );
    let still_needs_layer_update = owner
        .render_tree()
        .get(flow_id)
        .expect("flow node")
        .needs_composited_layer_update();
    assert!(
        !still_needs_layer_update,
        "the repaint that served the refused patch must clear \
         NEEDS_COMPOSITED_LAYER_UPDATE — a stuck flag here means the refusal \
         never actually visited the node",
    );
}

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

/// Two `RenderClipPath` siblings under one boundary both update in the same
/// frame, and the order their targets resolve in matches paint order on
/// every run.
#[test]
fn two_path_clips_under_one_boundary_resolve_in_paint_order() {
    use std::{cell::RefCell, rc::Rc};

    use flui_interaction::{InteractionDispatchHandle, InteractionLane};
    use flui_objects::RenderClipPath;
    use flui_rendering::hit_testing::PathClipTarget;

    /// Registers a path clipper that pushes `tag` onto `sequence` when
    /// resolved, and returns its target token.
    fn register(
        handle: &InteractionDispatchHandle,
        sequence: &Rc<RefCell<Vec<char>>>,
        tag: char,
    ) -> PathClipTarget {
        let sequence = Rc::clone(sequence);
        handle
            .register_path_clipper(move |size: Size| {
                sequence.borrow_mut().push(tag);
                let mut path = flui_types::painting::Path::new();
                path.add_rect(flui_types::Rect::from_origin_size(
                    flui_types::Point::ZERO,
                    size,
                ));
                path
            })
            .expect("register path clipper")
    }

    /// Root row → boundary → row → [clip_a → counting leaf, clip_b →
    /// counting leaf], each clip's target resolving through `handle`.
    fn mount_two_clips(
        handle: &InteractionDispatchHandle,
        sequence: &Rc<RefCell<Vec<char>>>,
    ) -> (
        PipelineOwner<flui_rendering::pipeline::Idle>,
        flui_foundation::RenderId,
        flui_foundation::RenderId,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let painted_a = Arc::new(AtomicUsize::new(0));
        let painted_b = Arc::new(AtomicUsize::new(0));

        let mut clip_a = RenderClipPath::anti_alias();
        let _ = clip_a.set_path_clip_target(Some(register(handle, sequence, 'A')));
        let mut clip_b = RenderClipPath::anti_alias();
        let _ = clip_b.set_path_clip_target(Some(register(handle, sequence, 'B')));

        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderFlex::row())
                        .child(
                            box_node(clip_a)
                                .label("clip_a")
                                .child(box_node(PaintCounter(Arc::clone(&painted_a)))),
                        )
                        .child(
                            box_node(clip_b)
                                .label("clip_b")
                                .child(box_node(PaintCounter(Arc::clone(&painted_b)))),
                        ),
                ),
            ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let clip_a_id = registry.get("clip_a").expect("clip_a is labelled");
        let clip_b_id = registry.get("clip_b").expect("clip_b is labelled");
        (owner, clip_a_id, clip_b_id, painted_a, painted_b)
    }

    for run in 0..3 {
        let lane = InteractionLane::try_new().expect("interaction lane");
        let handle = lane.dispatch_handle();
        let sequence: Rc<RefCell<Vec<char>>> = Rc::new(RefCell::new(Vec::new()));

        lane.enter(|| {
            let (owner, clip_a_id, clip_b_id, painted_a, painted_b) =
                mount_two_clips(&handle, &sequence);
            let (mut owner, result) = owner.run_frame();
            result.expect("first frame");
            assert_eq!(
                sequence.borrow().clone(),
                vec!['A', 'B'],
                "run {run}: a full paint must resolve two sibling path clips \
                 in tree order",
            );
            sequence.borrow_mut().clear();
            let before_a = painted_a.load(Ordering::Relaxed);
            let before_b = painted_b.load(Ordering::Relaxed);
            assert!(
                before_a > 0 && before_b > 0,
                "run {run}: precondition: both leaves paint on the first frame",
            );

            // Same frame: a FRESH clipper (same tag, new identity) becomes
            // each node's new target — both changes report a
            // composited-layer update through the real setter seam. B is
            // marked BEFORE A here, opposite of tree order: if the patch
            // walk resolved requests in mark/append order rather than the
            // capture's pre-order position, this would flip the recorded
            // sequence to `[B, A]` and the order assertion below would still
            // catch it even though it happens to coincide with tree order
            // whenever a caller marks front-to-back.
            let new_target_b = register(&handle, &sequence, 'B');
            let new_target_a = register(&handle, &sequence, 'A');
            update_render_object::<RenderClipPath, _>(&mut owner, clip_b_id, |c| {
                c.set_path_clip_target(Some(new_target_b))
            });
            update_render_object::<RenderClipPath, _>(&mut owner, clip_a_id, |c| {
                c.set_path_clip_target(Some(new_target_a))
            });

            let (owner, result) = owner.run_frame();
            result.expect("second frame");
            drop(owner);

            assert_eq!(
                painted_a.load(Ordering::Relaxed),
                before_a,
                "run {run}: clip_a's leaf must NOT repaint — the second frame \
                 must take the patch arm, not a repaint",
            );
            assert_eq!(
                painted_b.load(Ordering::Relaxed),
                before_b,
                "run {run}: clip_b's leaf must NOT repaint either",
            );
            assert_eq!(
                sequence.borrow().clone(),
                vec!['A', 'B'],
                "run {run}: a same-frame layer update on both siblings must \
                 resolve them in the SAME order a full repaint does — the \
                 capture-ordered effect_slots (a Vec, not a hash map) is what \
                 this pins",
            );
        });
    }
}

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

use flui_objects::{RenderSliverOpacity, RenderViewport};
use flui_rendering::testing::sliver_node;
use flui_types::layout::AxisDirection;

/// The Sliver-protocol counterpart of `RenderRepaintBoundary`: declares
/// itself a repaint boundary and passes its single child through untouched.
#[derive(Debug, Default)]
struct SliverBoundary;

impl flui_foundation::Diagnosticable for SliverBoundary {}

impl flui_rendering::traits::RenderSliver for SliverBoundary {
    type Arity = flui_tree::Single;
    type ParentData = flui_rendering::parent_data::SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<
            '_,
            flui_tree::Single,
            flui_rendering::parent_data::SliverParentData,
        >,
    ) -> flui_rendering::constraints::SliverGeometry {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            flui_rendering::constraints::SliverGeometry::ZERO
        }
    }

    fn hit_test(
        &self,
        ctx: &mut flui_rendering::context::SliverHitTestContext<
            '_,
            flui_tree::Single,
            flui_rendering::parent_data::SliverParentData,
        >,
    ) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }
}

/// A sliver leaf that records how many times it was painted AND draws a
/// filled rect.
///
/// Deliberately not the box `PaintCounter` above, which records a call but
/// emits no draw commands — that shape made `mount_drawing_opacity` necessary
/// earlier in this file, because a leaf that draws nothing produces no
/// `PictureLayer` and leaves a picture-count oracle unable to tell a visible
/// subtree from a hidden one. This leaf draws so any future oracle over these
/// fixtures inherits that ability rather than the blind shape.
#[derive(Debug)]
struct SliverPaintCounter(Arc<AtomicUsize>);

impl flui_foundation::Diagnosticable for SliverPaintCounter {}

impl flui_rendering::traits::RenderSliver for SliverPaintCounter {
    type Arity = flui_tree::Leaf;
    type ParentData = flui_rendering::parent_data::SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<
            '_,
            flui_tree::Leaf,
            flui_rendering::parent_data::SliverParentData,
        >,
    ) -> flui_rendering::constraints::SliverGeometry {
        let extent = 10.0_f32.min(ctx.constraints().remaining_paint_extent);
        flui_rendering::constraints::SliverGeometry::new(extent, extent, 0.0)
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, flui_tree::Leaf>) {
        self.0.fetch_add(1, Ordering::Relaxed);
        let rect = flui_types::Rect::from_origin_size(flui_types::Point::ZERO, ctx.size());
        let color = flui_types::Color::from_rgba_f32_array([1.0, 0.0, 0.0, 1.0]);
        ctx.canvas()
            .draw_rect(rect, &flui_painting::Paint::fill(color));
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::SliverHitTestContext<
            '_,
            flui_tree::Leaf,
            flui_rendering::parent_data::SliverParentData,
        >,
    ) -> bool {
        false
    }
}

/// Root box → viewport → [boundary → sliver opacity → drawing counting leaf,
/// boundary → drawing counting leaf].
///
/// Mirrors `mount_opacity_under_boundary` exactly, over the Sliver protocol:
/// the `RenderSliverOpacity` sits INSIDE a sliver repaint boundary, not as
/// one, and a second sibling boundary exists so a later frame can be forced
/// to paint without touching the first — the only way to observe what the
/// STORED capture holds.
fn mount_sliver_opacity_under_boundary(
    opacity: f32,
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
        box_node(RenderViewport::new(AxisDirection::TopToBottom))
            .child(
                sliver_node(SliverBoundary).child(
                    sliver_node(RenderSliverOpacity::new(opacity))
                        .label("sliver-opacity")
                        .child(sliver_node(SliverPaintCounter(Arc::clone(&painted)))),
                ),
            )
            .child(
                sliver_node(SliverBoundary)
                    .label("sliver-sibling")
                    .child(sliver_node(SliverPaintCounter(Arc::new(AtomicUsize::new(
                        0,
                    ))))),
            ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
    let opacity_id = registry
        .get("sliver-opacity")
        .expect("sliver-opacity is labelled");
    let sibling = registry
        .get("sliver-sibling")
        .expect("sliver-sibling is labelled");
    (owner, opacity_id, sibling, painted)
}

/// Applies a new opacity through the setter-then-apply seam, same as
/// `set_opacity` above but against `RenderSliverOpacity`.
fn set_sliver_opacity(
    owner: &mut PipelineOwner<flui_rendering::pipeline::Idle>,
    id: flui_foundation::RenderId,
    value: f32,
) {
    update_render_object::<RenderSliverOpacity, _>(owner, id, |o| o.set_opacity(value));
}

/// Same oracle as `an_alpha_change_updates_the_layer_without_repainting_the_subtree`,
/// driven through the Sliver protocol.
#[test]
fn a_sliver_alpha_change_updates_the_layer_without_repainting_the_subtree() {
    let (owner, opacity_id, _sibling, painted) = mount_sliver_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_sliver_opacity(&mut owner, opacity_id, 0.25);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the subtree under the sliver opacity must NOT repaint for an \
         alpha-only change",
    );
    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "and the emitted OpacityLayer must carry the new alpha",
    );
}

/// Same oracle as `a_layer_update_is_written_back_into_the_retained_capture`,
/// driven through the Sliver protocol — three frames, because two cannot see
/// a missing write-back (see that test's doc comment for why), PLUS a paint
/// count check the box version does not need.
///
/// A pure value check on frame 3 cannot by itself distinguish a correct
/// update-only patch from a correct-but-unnecessary full repaint: either way
/// `retained_boundaries` ends up holding the fresh alpha, because a normal
/// repaint also stores whatever it just painted. So this also asserts the
/// leaf never repaints across frames 2-3 — the property that actually
/// depends on `set_opacity` reporting `COMPOSITED_LAYER_UPDATE` rather than
/// `PAINT` — otherwise the value assertion alone would pass unmodified
/// against the pre-fix setter, same as the box test would if `RenderOpacity`
/// still reported `PAINT` for an in-range change.
#[test]
fn a_sliver_layer_update_is_written_back_into_the_retained_capture() {
    let (owner, opacity_id, sibling, painted) = mount_sliver_opacity_under_boundary(0.5);
    let (mut owner, result) = owner.run_frame();
    result.expect("first frame");
    let before = painted.load(Ordering::Relaxed);
    assert!(
        before > 0,
        "precondition: the leaf paints on the first frame"
    );

    set_sliver_opacity(&mut owner, opacity_id, 0.25);
    let (mut owner, result) = owner.run_frame();
    result.expect("second frame");

    // Third frame: the sliver opacity is clean; the sibling boundary forces
    // the pass.
    owner.mark_needs_paint(sibling);
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("third frame")
        .expect("third frame produces a layer tree");
    drop(owner);

    assert_eq!(
        painted.load(Ordering::Relaxed),
        before,
        "the leaf under the sliver opacity must NOT repaint across the alpha \
         change (frame 2) or the unrelated sibling-forced pass (frame 3) — an \
         update-only patch touches only the enclosing capture's own effect \
         layers",
    );
    assert_eq!(
        first_opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "a later frame that grafts the capture for an unrelated reason must \
         replay the UPDATED alpha, not the one it was captured with",
    );
}

/// Patching a transform's layer produces the same layer tree a full repaint
/// does, at every subtree size — and the patched tree's node count does not
/// grow with the subtree.
///
/// This is the control that makes `paint/transform_matrix_change` mean
/// something, and it is the transform twin of
/// `a_grafted_opacity_subtree_matches_a_full_repaint_at_any_size` above. That
/// benchmark's update arm reports a flat ~1.2-1.5 µs against a repaint arm
/// that reaches 198 µs at 1000 nodes, and **a flat number is exactly what a
/// patch that silently dropped the subtree would also report**. Only the
/// structural comparison rules that out.
///
/// The tree mirrors `helpers::build_transform_tree` in the benchmark itself
/// rather than the `PaintCounter` fixture above — a control that measures a
/// different tree from the one benchmarked controls nothing, and
/// `PaintCounter` emits no draw commands, so it produces no `PictureLayer`
/// for a structural fingerprint to compare.
///
/// The matrix is a SCALE: a pure translation takes `paint`'s no-layer fast
/// path, so there would be no transform layer to patch and the test would
/// pass while measuring nothing.
///
/// Deliberately green on BOTH sides of the change that introduced it, and not
/// offered as red-green evidence for it: a full repaint also produces a
/// correct tree, which is the whole premise of comparing against one. What it
/// guards is the benchmark's interpretation. The red-green evidence for the
/// patch path itself is
/// `a_transform_change_updates_the_layer_without_repainting_the_subtree` and
/// `a_transform_layer_update_is_written_back_into_the_retained_capture`, both
/// of which fail with the paint counter at 2 against 1 without it.
#[test]
fn a_patched_transform_subtree_matches_a_full_repaint_at_any_size() {
    fn mount_sized(
        layered: bool,
        subtree: usize,
        matrix: Matrix4,
    ) -> (
        PipelineOwner<flui_rendering::pipeline::Idle>,
        flui_foundation::RenderId,
    ) {
        let content = box_node(RenderFlex::row()).children((0..subtree).map(|_| {
            let leaf = box_node(RenderColoredBox::red(1.0, 1.0));
            if layered {
                box_node(RenderRepaintBoundary::new()).child(leaf)
            } else {
                leaf
            }
        }));
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row())
                .child(
                    box_node(RenderRepaintBoundary::new()).child(
                        box_node(RenderTransform::new(matrix))
                            .label("transform")
                            .child(content),
                    ),
                )
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .child(box_node(RenderColoredBox::red(1.0, 1.0))),
                ),
        );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));
        let transform_id = registry.get("transform").expect("transform is labelled");
        (owner, transform_id)
    }

    let updated = Matrix4::scaling(3.0, 3.0, 1.0);

    for (layered, shape) in [(false, "inline"), (true, "layered")] {
        let mut patched_counts = Vec::new();

        for &subtree in &[1_usize, 10, 100] {
            // Frame 1 captures at the ORIGINAL matrix; frame 2 requests a
            // layer-only update, which is the benchmark's `update` arm exactly.
            let (owner, transform_id) =
                mount_sized(layered, subtree, Matrix4::scaling(2.0, 2.0, 1.0));
            let (mut owner, result) = owner.run_frame();
            result.expect("first frame");
            set_transform(&mut owner, transform_id, updated);
            let (owner, result) = owner.run_frame();
            let patched = result
                .expect("second frame")
                .expect("second frame produces a layer tree");
            drop(owner);

            // A fresh owner painting its first frame retains nothing, so its tree
            // is what a full repaint of the final state produces.
            let (fresh, _) = mount_sized(layered, subtree, updated);
            let (_, result) = fresh.run_frame();
            let repainted = result
                .expect("reference frame")
                .expect("reference frame produces a layer tree");

            assert_eq!(
                fingerprint(&patched),
                fingerprint(&repainted),
                "patched and repainted layer trees must match at {shape} subtree = {subtree}",
            );
            assert_eq!(
                first_transform_matrix(&patched),
                first_transform_matrix(&repainted),
                "and the patched transform layer must carry the matrix a repaint \
             would produce, conjugated by the same origin ({shape}, subtree = \
             {subtree})",
            );
            assert!(
                patched.len() > 1,
                "a patch that produced an empty tree would also look flat in the bench",
            );
            patched_counts.push(patched.len());
        }

        // The O(1) claim is an INLINE property only: it holds because non-boundary
        // leaves merge into one `PictureLayer` sharing an `Arc<DisplayList>`. In
        // the layered shape every leaf is its own boundary, so the capture — and
        // therefore the graft — grows with the subtree by design, which is exactly
        // why the benchmark's layered arm narrows to ~1.9x instead of 133x.
        // Asserting a constant count there would be asserting the feature is
        // broken. Every measurement compared, not just the ends: `[6, 7, 6]` is a
        // real regression a first-vs-last check would wave through.
        if layered {
            assert!(
                patched_counts.windows(2).all(|w| w[0] < w[1]),
                "layered leaves each carry their own boundary, so the layer count \
                 must grow with the subtree; got {patched_counts:?}",
            );
        } else {
            assert!(
                patched_counts.windows(2).all(|w| w[0] == w[1]),
                "inline leaves merge into one PictureLayer, so the layer count \
                 must be identical at every subtree size; got {patched_counts:?}",
            );
        }
    }
}
