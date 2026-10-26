//! A repaint boundary's capture holds its own region and a slot for each boundary nested in it.
//!
//! A clean boundary is replayed from its capture whatever is nested in it, and each nested
//! boundary is decided on its own at its slot: grafted when clean, repainted when queued or
//! without a capture. The oracle for "the retained output is the painted output" is a fresh
//! owner painting the same tree from scratch.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::geometry::Size;
use flui_foundation::{Leaf, RenderId};
use flui_layer::{Layer, LayerTree};
use flui_objects::{
    RenderColoredBox, RenderFlex, RenderLeaderLayer, RenderOpacity, RenderPaintAnchor,
    RenderRepaintBoundary,
};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, PaintAnchor, PaintCx},
    parent_data::BoxParentData,
    pipeline::{Idle, PipelineOwner},
    testing::{TreeNode, box_node, tree, update_render_object},
    traits::RenderBox,
};

fn mount(root: TreeNode) -> (PipelineOwner<Idle>, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    (owner, registry)
}

fn frame(owner: PipelineOwner<Idle>) -> (PipelineOwner<Idle>, LayerTree) {
    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("frame")
        .expect("the frame produces a layer tree");
    (owner, tree)
}

fn leaf() -> TreeNode {
    box_node(RenderColoredBox::red(10.0, 10.0))
}

/// What a layer is, for comparing two trees: its kind, with the properties that decide pixels
/// for the kinds the scenarios use.
fn describe(layer: &Layer) -> String {
    match layer {
        Layer::Picture(picture) => format!("Picture:{}", picture.picture().commands().len()),
        Layer::Offset(_) | Layer::Opacity(_) => format!("{layer:?}"),
        other => other.kind_name().to_owned(),
    }
}

/// The layer tree as a preorder list of `(depth, description)`: the structure, the order of
/// siblings and the properties in one comparable value.
fn fingerprint(t: &LayerTree) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(t.root(), 0usize)];
    while let Some((id, depth)) = stack.pop() {
        let node = t.get(id).expect("every id in this walk came from the tree");
        out.push((depth, describe(node.layer())));
        for &child in t
            .children(id)
            .expect("a live id has a child list")
            .iter()
            .rev()
        {
            stack.push((child, depth + 1));
        }
    }
    out
}

/// A leaf that counts its paints and keeps the paint anchor it last saw.
#[derive(Debug, Clone)]
struct PaintTally {
    paints: Arc<AtomicUsize>,
    anchor: Arc<Mutex<Option<PaintAnchor>>>,
}

impl PaintTally {
    fn new() -> Self {
        Self {
            paints: Arc::new(AtomicUsize::new(0)),
            anchor: Arc::new(Mutex::new(None)),
        }
    }

    fn paints(&self) -> usize {
        self.paints.load(Ordering::Relaxed)
    }

    fn anchor(&self) -> Option<PaintAnchor> {
        *self.anchor.lock().expect("no panic holds the lock")
    }
}

impl flui_foundation::Diagnosticable for PaintTally {}

impl RenderBox for PaintTally {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(ctx.constrain(Size::new(10.0, 10.0)))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        self.paints.fetch_add(1, Ordering::Relaxed);
        *self.anchor.lock().expect("no panic holds the lock") = ctx.paint_anchor();
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        false
    }
}

/// Root row → boundary "outer" → row → [tally, boundary "inner" → tally, tally]; a sibling
/// boundary "other" next to "outer" gives a frame something unrelated to repaint.
struct Nested {
    owner: PipelineOwner<Idle>,
    outer: RenderId,
    inner: RenderId,
    other: RenderId,
    outer_tally: PaintTally,
    inner_tally: PaintTally,
}

fn nested() -> Nested {
    let (outer_tally, inner_tally) = (PaintTally::new(), PaintTally::new());
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).label("outer").child(
                    box_node(RenderFlex::row())
                        .child(leaf())
                        .child(
                            box_node(RenderRepaintBoundary::new())
                                .label("inner")
                                .child(box_node(inner_tally.clone())),
                        )
                        .child(leaf())
                        .child(box_node(outer_tally.clone())),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("other")
                    .child(leaf()),
            ),
    );
    let get = |name| registry.get(name).expect("labelled");
    Nested {
        owner,
        outer: get("outer"),
        inner: get("inner"),
        other: get("other"),
        outer_tally,
        inner_tally,
    }
}

/// The dirty nested boundary's own subtree is all that paints: the enclosing boundaries are
/// replayed, not re-recorded.
#[test]
fn a_nested_boundary_mark_paints_only_that_boundary() {
    let Nested {
        owner,
        inner,
        outer_tally,
        inner_tally,
        ..
    } = nested();
    let (mut owner, _) = frame(owner);
    let (outer_before, inner_before) = (outer_tally.paints(), inner_tally.paints());
    let before = owner.counters();

    owner.mark_needs_paint(inner);
    let (owner, _) = frame(owner);

    // The inner boundary is a render node of its own, with one tally under it.
    // The root row repaints every frame (the root region is not retained), then the inner
    // boundary and the tally under it.
    assert_eq!(owner.counters().since(before).nodes_painted, 3);
    assert_eq!(inner_tally.paints(), inner_before + 1);
    assert_eq!(
        outer_tally.paints(),
        outer_before,
        "the outer region was replayed"
    );
}

/// The nested boundary keeps its place between the inline runs on either side of it, and its
/// layer is minted at replay under the layer its slot named.
#[test]
fn a_replayed_slot_keeps_its_place_between_inline_runs() {
    let Nested {
        owner,
        outer,
        inner,
        ..
    } = nested();
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(inner);
    let (_, second) = frame(owner);

    let kinds_under = |t: &LayerTree| -> Vec<String> {
        let outer_layer = t
            .iter()
            .find(|(_, node)| node.render_id() == Some(outer))
            .map(|(id, _)| id)
            .expect("outer is stamped");
        t.children(outer_layer)
            .expect("outer layer")
            .iter()
            .map(|&id| describe(t.get_layer(id).expect("child of a live layer")))
            .collect()
    };
    let order = kinds_under(&second);
    assert_eq!(order.len(), 3, "run, nested boundary, run: {order:?}");
    assert!(order[0].starts_with("Picture"), "{order:?}");
    assert!(order[1].starts_with("Offset"), "{order:?}");
    assert!(order[2].starts_with("Picture"), "{order:?}");
    assert_eq!(order, kinds_under(&first));
    assert_eq!(fingerprint(&first), fingerprint(&second));
}

/// The repainted boundary carries a new token, and the boundary around it keeps its own: the
/// differ sees the nested region change and nothing else.
#[test]
fn a_repainted_nested_boundary_restamps_and_its_neighbours_keep_their_tokens() {
    let Nested {
        owner,
        outer,
        inner,
        other,
        ..
    } = nested();
    let tokens = |t: &LayerTree| {
        t.iter()
            .filter_map(|(_, node)| {
                node.boundary()
                    .map(|stamp| (stamp.render_id(), stamp.content().clone()))
            })
            .collect::<std::collections::HashMap<_, _>>()
    };
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(inner);
    let (_, second) = frame(owner);
    let (a, b) = (tokens(&first), tokens(&second));
    assert_ne!(a[&inner], b[&inner]);
    assert_eq!(a[&outer], b[&outer]);
    assert_eq!(a[&other], b[&other]);
}

/// A nested boundary with no capture of its own (it holds a leader, which is never retained)
/// repaints at its slot while the boundary around it is replayed.
#[test]
fn a_nested_boundary_without_a_capture_repaints_alone() {
    let (outer_tally, inner_tally) = (PaintTally::new(), PaintTally::new());
    let link = flui_layer::LayerLink::new();
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new()).child(
                    box_node(RenderFlex::row())
                        .child(box_node(outer_tally.clone()))
                        .child(
                            box_node(RenderRepaintBoundary::new()).child(
                                box_node(RenderLeaderLayer::new(link))
                                    .child(box_node(inner_tally.clone())),
                            ),
                        ),
                ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("other")
                    .child(leaf()),
            ),
    );
    let other = registry.get("other").expect("labelled");
    let (mut owner, first) = frame(owner);
    let (outer_before, inner_before) = (outer_tally.paints(), inner_tally.paints());

    owner.mark_needs_paint(other);
    let (_, second) = frame(owner);

    assert_eq!(
        outer_tally.paints(),
        outer_before,
        "the outer region was replayed"
    );
    assert_eq!(
        inner_tally.paints(),
        inner_before + 1,
        "the uncaptured nested boundary repainted at its slot"
    );
    assert_eq!(fingerprint(&first), fingerprint(&second));
}

/// A paint anchor inside the replayed region still governs the nested boundary that repaints.
#[test]
fn an_anchor_inside_the_replayed_region_reaches_the_repainted_boundary() {
    let tally = PaintTally::new();
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderPaintAnchor::new()).child(
                    box_node(RenderFlex::row()).child(leaf()).child(
                        box_node(RenderRepaintBoundary::new())
                            .label("inner")
                            .child(box_node(tally.clone())),
                    ),
                ),
            )
            .child(leaf()),
    );
    let inner = registry.get("inner").expect("labelled");
    let (mut owner, _) = frame(owner);
    let first_anchor = tally.anchor().expect("the tally sits under an anchor");
    assert!(
        first_anchor.offset.dx > 0.0,
        "precondition: the tally is away from the anchor's origin"
    );
    let paints = tally.paints();

    owner.mark_needs_paint(inner);
    let _ = frame(owner);

    assert_eq!(tally.paints(), paints + 1);
    assert_eq!(tally.anchor(), Some(first_anchor));
}

/// An anchor above two replayed regions is rebased at each slot on the way down.
#[test]
fn an_anchor_outside_the_region_is_rebased_at_each_slot() {
    let tally = PaintTally::new();
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderPaintAnchor::new()).child(
                    box_node(RenderFlex::row()).child(leaf()).child(
                        box_node(RenderRepaintBoundary::new()).child(
                            box_node(RenderFlex::row()).child(leaf()).child(
                                box_node(RenderRepaintBoundary::new())
                                    .label("inner")
                                    .child(box_node(tally.clone())),
                            ),
                        ),
                    ),
                ),
            )
            .child(leaf()),
    );
    let inner = registry.get("inner").expect("labelled");
    let (mut owner, _) = frame(owner);
    let first_anchor = tally.anchor().expect("the tally sits under an anchor");
    assert!(
        first_anchor.offset.dx >= 20.0,
        "precondition: two leaves sit before the tally along the way: {first_anchor:?}"
    );

    owner.mark_needs_paint(inner);
    let _ = frame(owner);

    assert_eq!(tally.anchor(), Some(first_anchor));
}

// ---------------------------------------------------------------------------
// Differential: retained frames against a fresh full repaint
// ---------------------------------------------------------------------------

/// A small deterministic generator, so a failing seed reproduces.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % bound
    }
}

struct Spec {
    root: TreeNode,
    boundaries: usize,
    opacities: usize,
}

/// A random tree of boundaries, opacities and rows over coloured leaves. `alphas[k]` is the
/// alpha of the k-th opacity, so a reference tree can be built in the state a test mutated to.
fn generate(seed: u64, alphas: &[f64]) -> Spec {
    struct Builder<'a> {
        rng: Lcg,
        boundaries: usize,
        opacities: usize,
        alphas: &'a [f64],
    }
    impl Builder<'_> {
        fn node(&mut self, depth: usize) -> TreeNode {
            if depth == 0 {
                return leaf();
            }
            match self.rng.next(4) {
                0 => {
                    let label = format!("b{}", self.boundaries);
                    self.boundaries += 1;
                    box_node(RenderRepaintBoundary::new())
                        .label(leak(label))
                        .child(self.node(depth - 1))
                }
                1 => {
                    let k = self.opacities;
                    self.opacities += 1;
                    let alpha = self.alphas.get(k).copied().unwrap_or(0.5);
                    box_node(RenderOpacity::new(alpha))
                        .label(leak(format!("o{k}")))
                        .child(self.node(depth - 1))
                }
                _ => {
                    let mut row = box_node(RenderFlex::row());
                    for _ in 0..=self.rng.next(3) {
                        row = row.child(self.node(depth - 1));
                    }
                    row
                }
            }
        }
    }
    let mut builder = Builder {
        rng: Lcg(seed),
        boundaries: 0,
        opacities: 0,
        alphas,
    };
    let mut root = box_node(RenderFlex::row());
    for _ in 0..3 {
        root = root.child(builder.node(4));
    }
    Spec {
        root,
        boundaries: builder.boundaries,
        opacities: builder.opacities,
    }
}

/// Random trees, random repaint marks and alpha updates: every retained frame is the layer tree
/// a fresh owner paints from scratch for the same state.
#[test]
fn retained_frames_match_a_full_repaint_over_random_trees_and_marks() {
    let mut checked = 0;
    for seed in 0..48u64 {
        let spec = generate(seed, &[]);
        if spec.boundaries == 0 {
            continue;
        }
        let (boundaries, opacities) = (spec.boundaries, spec.opacities);
        let (owner, registry) = mount(spec.root);
        let id = |name: String| registry.get(&name).expect("labelled");
        let mut alphas = vec![0.5; opacities];
        let mut rng = Lcg(seed ^ 0x9e37_79b9);
        let (mut owner, _) = frame(owner);

        for step in 0..6 {
            for _ in 0..=rng.next(3) {
                owner.mark_needs_paint(id(format!("b{}", rng.next(boundaries))));
            }
            if opacities > 0 && step % 2 == 1 {
                let k = rng.next(opacities);
                alphas[k] = if alphas[k] > 0.4 { 0.3 } else { 0.7 };
                update_render_object::<RenderOpacity, _>(&mut owner, id(format!("o{k}")), |o| {
                    o.set_opacity(alphas[k])
                });
            }
            let (next, retained) = frame(owner);
            owner = next;

            let (reference, _) = mount(generate(seed, &alphas).root);
            let (_, full) = frame(reference);
            assert_eq!(
                fingerprint(&retained),
                fingerprint(&full),
                "seed {seed} step {step}: the retained frame differs from a full repaint"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 100,
        "the generator must produce enough cases: {checked}"
    );
}

// A paint-anchor-free assertion that the generator produces nested boundaries at all, so the
// differential above exercises slots and not only flat boundaries.
#[test]
fn the_generator_nests_boundaries() {
    let nested_somewhere = (0..48u64).any(|seed| {
        let spec = generate(seed, &[]);
        let (owner, registry) = mount(spec.root);
        let (_, t) = frame(owner);
        let _ = registry;
        // A stamped layer below another stamped layer is a nested boundary.
        t.iter().any(|(id, node)| {
            let mut above = t.parent(id);
            let mut nested = false;
            while let Some(ancestor) = above {
                nested |= node.boundary().is_some()
                    && t.get(ancestor).is_some_and(|n| n.boundary().is_some());
                above = t.parent(ancestor);
            }
            nested
        })
    });
    assert!(nested_somewhere);
}

/// Labels are `&'static str`; a test generating a few dozen of them leaks them.
fn leak(label: String) -> &'static str {
    Box::leak(label.into_boxed_str())
}
