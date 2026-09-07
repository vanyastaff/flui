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

use flui_objects::{RenderColoredBox, RenderFlex, RenderOpacity, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{TreeNode, box_node, tree},
};
use flui_types::{Size, geometry::px};

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

/// Structural fingerprint of a layer tree: the count and the per-node child
/// counts in a deterministic walk.
///
/// Layer ids differ between frames (each frame builds a fresh slab), so the
/// comparison has to be structural. This is enough to catch a graft that
/// dropped a node, duplicated one, or reparented it.
fn fingerprint(t: &flui_layer::LayerTree) -> Vec<usize> {
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
        out.push(children.len());
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
    // graft arm O(1) and the whole update-only design worth building.
    assert_eq!(
        grafted_counts.first(),
        grafted_counts.last(),
        "inline leaves merge into one PictureLayer, so the layer count is \
         independent of subtree size; got {grafted_counts:?}",
    );
}
