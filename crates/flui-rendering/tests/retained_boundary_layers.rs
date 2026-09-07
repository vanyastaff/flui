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
    let impact = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("opacity node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<RenderOpacity>()
        .expect("RenderOpacity")
        .set_opacity(value);
    owner.apply_render_update_impact(id, impact);
}

/// The alpha of the only `OpacityLayer` in a tree, or `None` when there is none.
fn opacity_alpha(tree: &flui_layer::LayerTree) -> Option<f32> {
    fn find(tree: &flui_layer::LayerTree, id: flui_foundation::LayerId) -> Option<f32> {
        let node = tree.get(id)?;
        if let flui_layer::Layer::Opacity(o) = node.layer() {
            return Some(o.alpha());
        }
        node.children().iter().find_map(|&c| find(tree, c))
    }
    find(tree, tree.root()?)
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
        opacity_alpha(&tree).map(|a| (a * 100.0).round()),
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
        opacity_alpha(&tree).map(|a| (a * 100.0).round()),
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
        opacity_alpha(&tree).map(|a| (a * 100.0).round()),
        Some(25.0),
        "and the repaint must still produce the new alpha",
    );
}

/// An alpha leaving the layered range drops the layer entirely, which a patch
/// cannot express — so paint is the fallback.
///
/// This is the structure guard. `paint_alpha()` returns `None` at alpha 255, so
/// the captured shape no longer applies and replaying it would keep a layer the
/// node no longer emits.
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
        opacity_alpha(&tree),
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
        opacity_alpha(&tree).map(|a| (a * 100.0).round()),
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
        opacity_alpha(&updated).map(|a| (a * 100.0).round()),
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
        opacity_alpha(&regrafted).map(|a| (a * 100.0).round()),
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

/// Body of the test above, run once per arm.
///
/// Both arms clear the pending flag, in different places — the graft arm in
/// `layer_patches_for`, the repaint arm in the paint walk — and both must defer
/// it to the frame's commit.
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
        opacity_alpha(&tree).map(|a| (a * 100.0).round()),
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

        fn paint_transform(&self, _size: Size) -> Option<flui_types::Matrix4> {
            self.enabled
                .then(|| flui_types::Matrix4::translation(3.0, 5.0, 0.0))
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
        transform_count(&first),
        0,
        "precondition: no transform layer while the effect is off",
    );

    // Switch the effect on and ask for a layer-only update — the wrong impact
    // for a shape change, which is exactly what the guard must catch.
    let mut owner = owner;
    owner
        .render_tree_mut()
        .get_mut(fx)
        .expect("fx node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<AppearingTransform>()
        .expect("AppearingTransform")
        .enabled = true;
    owner.mark_needs_composited_layer_update(fx);

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("second frame")
        .expect("second frame produces a layer tree");
    drop(owner);

    assert_eq!(
        transform_count(&tree),
        1,
        "an effect layer that did not exist in the capture cannot be patched \
         into it; the frame must repaint and emit the new layer",
    );
}

/// Number of `TransformLayer`s in a tree.
fn transform_count(t: &flui_layer::LayerTree) -> usize {
    fn walk(t: &flui_layer::LayerTree, id: flui_foundation::LayerId) -> usize {
        let Some(node) = t.get(id) else { return 0 };
        usize::from(matches!(node.layer(), flui_layer::Layer::Transform(_)))
            + node.children().iter().map(|&c| walk(t, c)).sum::<usize>()
    }
    t.root().map_or(0, |root| walk(t, root))
}

/// A node whose effect layers change SHAPE cannot be patched, and the frame
/// must notice by itself.
///
/// Neither case is reachable through a shipped opacity, which reports a repaint
/// itself whenever its layer appears or disappears — so the frame's own guards
/// never fired and a mutation run found them inert. These drive the path
/// directly, through a proxy that reports the WRONG impact on purpose.
///
/// - **count** (captured with one layer, now emitting two) pins the length
///   guard: disabling it turns this red.
/// - **kind** (captured with one layer, now emitting one of a different type)
///   pins the OUTPUT but not the mechanism. `own_effect_layers` emits a fixed
///   order, so a same-count swap patches positionally to the same tree a
///   repaint gives, and disabling the discriminant guard leaves this green.
///   That guard is defence in depth against a future third effect type, and
///   the comment at it says so rather than claiming a pin it does not have.
#[test]
fn an_effect_layer_shape_change_falls_back_to_a_repaint() {
    /// Emits an opacity layer, a transform layer, or both.
    #[derive(Debug, Default)]
    struct ShapeShifter {
        alpha: bool,
        transform: bool,
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

        fn paint_alpha(&self) -> Option<u8> {
            self.alpha.then_some(128)
        }

        fn paint_transform(&self, _size: Size) -> Option<flui_types::Matrix4> {
            self.transform
                .then(|| flui_types::Matrix4::translation(3.0, 5.0, 0.0))
        }
    }

    // `gains` says what the second frame switches on; `expect_transform` is
    // what a correct repaint must then emit.
    for (label, gains_transform, keeps_alpha) in [
        ("count: 1 -> 2 layers", true, true),
        ("kind: opacity -> transform", true, false),
    ] {
        let mut owner = PipelineOwner::new();
        let (root_id, registry) = tree::mount(
            &mut owner,
            box_node(RenderFlex::row()).child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(ShapeShifter {
                        alpha: true,
                        transform: false,
                    })
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
            (opacity_alpha(&first).is_some(), transform_count(&first)),
            (true, 0),
            "{label}: precondition — captured with exactly one opacity layer",
        );

        {
            let object = owner
                .render_tree_mut()
                .get_mut(fx)
                .expect("fx node")
                .as_box_mut()
                .expect("box entry")
                .render_object_mut()
                .as_any_mut()
                .downcast_mut::<ShapeShifter>()
                .expect("ShapeShifter");
            object.transform = gains_transform;
            object.alpha = keeps_alpha;
        }
        owner.mark_needs_composited_layer_update(fx);

        let (owner, result) = owner.run_frame();
        let second = result
            .expect("second frame")
            .expect("second frame produces a layer tree");
        drop(owner);

        assert_eq!(
            (opacity_alpha(&second).is_some(), transform_count(&second)),
            (keeps_alpha, usize::from(gains_transform)),
            "{label}: a shape change cannot be patched into the capture, so the \
             frame must repaint and emit the new shape",
        );
    }
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

        fn paint_transform(&self, _size: Size) -> Option<flui_types::Matrix4> {
            self.enabled
                .then(|| flui_types::Matrix4::translation(3.0, 5.0, 0.0))
        }
    }

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderFlex::row()).child(
            box_node(RenderOpacity::new(0.5)).label("gate").child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(GainsATransform::default())
                        .label("fx")
                        .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                ),
            ),
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
    owner
        .render_tree_mut()
        .get_mut(fx)
        .expect("fx node")
        .as_box_mut()
        .expect("box entry")
        .render_object_mut()
        .as_any_mut()
        .downcast_mut::<GainsATransform>()
        .expect("GainsATransform")
        .enabled = true;
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
        transform_count(&tree),
        1,
        "the capture taken before the effect existed must not survive a frame \
         that could not serve the update; nothing else can restore the layer, \
         because the node owns no slot to patch and its flag blocks new marks",
    );
}
