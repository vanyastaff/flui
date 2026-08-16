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
