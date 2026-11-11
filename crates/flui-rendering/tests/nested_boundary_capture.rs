//! A repaint boundary's content holds its own region and a slot for each boundary nested in it.
//!
//! A clean boundary keeps its content whatever is nested in it, and each nested boundary is
//! decided on its own: kept when clean, repainted alone when queued. The oracle for "the retained
//! output is the painted output" is a fresh owner painting the same tree from scratch.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::geometry::Size;
use flui_foundation::{Leaf, RenderId};
use flui_layer::{Layer, LayerTree, leader_origin, resolve_follower_transform};
use flui_objects::{
    RenderColoredBox, RenderFlex, RenderFollowerLayer, RenderLeaderLayer, RenderOpacity,
    RenderPaintAnchor, RenderRepaintBoundary, RenderSizedBox,
};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, PaintAnchor, PaintCx},
    parent_data::BoxParentData,
    pipeline::{Idle, PipelineOwner},
    testing::{SceneHost, TreeNode, box_node, tree, update_render_object},
    traits::RenderBox,
};

fn mount(root: TreeNode) -> (PipelineOwner<Idle>, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    (owner, registry)
}

fn frame(scene: &mut SceneHost, owner: PipelineOwner<Idle>) -> (PipelineOwner<Idle>, LayerTree) {
    let (owner, result) = scene.run_frame(owner);
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
        Layer::Offset(_) | Layer::Opacity(_) | Layer::Leader(_) => format!("{layer:?}"),
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

impl flui_foundation::inspect::Inspect for PaintTally {
    fn kind(&self) -> &'static str {
        "PaintTally"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

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
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    let (outer_before, inner_before) = (outer_tally.paints(), inner_tally.paints());
    let before = owner.counters();

    owner.mark_needs_paint(inner);
    let (owner, _) = frame(&mut scene, owner);

    // The inner boundary and the tally under it: the frame node and the outer boundary keep
    // their contents, which still slot the inner node.
    assert_eq!(owner.counters().since(before).nodes_painted, 2);
    assert_eq!(inner_tally.paints(), inner_before + 1);
    assert_eq!(
        outer_tally.paints(),
        outer_before,
        "the outer region was replayed"
    );
}

/// The nested boundary keeps its place between the inline runs on either side of it: the outer
/// content, kept, holds a run, the slot of the inner node, and a run.
#[test]
fn a_replayed_slot_keeps_its_place_between_inline_runs() {
    let Nested {
        owner,
        outer,
        inner,
        ..
    } = nested();
    let mut scene = SceneHost::new();
    let (mut owner, first) = frame(&mut scene, owner);
    let inner_node = owner.boundary_scene_node(inner).expect("published");
    owner.mark_needs_paint(inner);
    let (owner, second) = frame(&mut scene, owner);

    let content = scene.content_of(&owner, outer).expect("published");
    let order: Vec<&str> = content
        .entries()
        .iter()
        .map(|entry| match entry {
            flui_layer::Entry::Chunk { .. } => "run",
            flui_layer::Entry::Slot { node, .. } if *node == inner_node => "inner",
            flui_layer::Entry::Slot { .. } => "slot",
            flui_layer::Entry::Effect { .. } => "effect",
        })
        .collect();
    assert_eq!(order, ["run", "inner", "run"]);
    assert_eq!(fingerprint(&first), fingerprint(&second));
}

/// The repainted boundary is published with a new content, and the boundaries around and beside
/// it keep theirs: the patch names the nested node and nothing else.
#[test]
fn a_repainted_nested_boundary_republishes_and_its_neighbours_keep_their_contents() {
    let Nested {
        owner,
        outer,
        inner,
        other,
        ..
    } = nested();
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    let before = |scene: &SceneHost, owner: &PipelineOwner<Idle>, id| {
        scene.content_of(owner, id).expect("published")
    };
    let (a_inner, a_outer, a_other) = (
        before(&scene, &owner, inner),
        before(&scene, &owner, outer),
        before(&scene, &owner, other),
    );
    owner.mark_needs_paint(inner);
    let (owner, _) = frame(&mut scene, owner);
    assert!(!Arc::ptr_eq(&a_inner, &before(&scene, &owner, inner)));
    assert!(Arc::ptr_eq(&a_outer, &before(&scene, &owner, outer)));
    assert!(Arc::ptr_eq(&a_other, &before(&scene, &owner, other)));
}

/// A nested boundary holding a follower is retained like any other: a sibling's repaint
/// leaves both it and the boundary around it unpainted, and the frame matches.
#[test]
fn a_nested_boundary_holding_a_follower_is_retained() {
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
                                box_node(RenderFollowerLayer::new(link))
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
    let mut scene = SceneHost::new();
    let (mut owner, first) = frame(&mut scene, owner);
    let (outer_before, inner_before) = (outer_tally.paints(), inner_tally.paints());

    owner.mark_needs_paint(other);
    let (_, second) = frame(&mut scene, owner);

    assert_eq!(
        outer_tally.paints(),
        outer_before,
        "the outer boundary was kept"
    );
    assert_eq!(
        inner_tally.paints(),
        inner_before,
        "the follower's boundary was kept"
    );
    assert_eq!(fingerprint(&first), fingerprint(&second));
}

/// A boundary that holds a leader is retained, and its leader is resolved where the boundary now
/// stands: a sibling's relayout moves the boundary, and the follower resolves against the moved
/// leader as it does for a fresh owner painting the same tree.
#[test]
fn a_boundary_holding_a_leader_is_retained() {
    let link = flui_layer::LayerLink::new();
    let scene = |lead_width: f64, tally: &PaintTally| {
        box_node(RenderFlex::row())
            .child(box_node(RenderSizedBox::fixed(lead_width, 10.0)).label("lead"))
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("leading")
                    .child(box_node(RenderLeaderLayer::new(link)).child(box_node(tally.clone()))),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .child(box_node(RenderFollowerLayer::new(link)).child(leaf())),
            )
    };
    let follower_transform = |t: &LayerTree| {
        let (follower, _) = t
            .iter()
            .find(|(_, node)| matches!(node.layer(), Layer::Follower(_)))
            .expect("the follower is painted");
        resolve_follower_transform(t, follower)
    };

    let tally = PaintTally::new();
    let (owner, registry) = mount(scene(20.0, &tally));
    let lead = registry.get("lead").expect("labelled");
    let leading = registry.get("leading").expect("labelled");
    let mut host = SceneHost::new();
    let (mut owner, _) = frame(&mut host, owner);
    assert_eq!(
        owner.boundary_record_count(),
        2,
        "both boundaries are retained"
    );
    let paints = tally.paints();
    let content = host.content_of(&owner, leading).expect("published");

    update_render_object::<RenderSizedBox, _>(&mut owner, lead, |sized| {
        sized.set_size(Some(30.0), Some(10.0))
    });
    let (owner, second) = frame(&mut host, owner);
    assert_eq!(
        tally.paints(),
        paints,
        "the moved leading boundary was kept, not repainted"
    );
    assert!(
        Arc::ptr_eq(
            &content,
            &host.content_of(&owner, leading).expect("published")
        ),
        "the moved boundary keeps its content; only its placement changed"
    );
    assert_eq!(
        host.version().leader_origin(link).map(|origin| origin.x),
        Some(30.0),
        "the version resolves the leader where the boundary now stands"
    );

    let (fresh, _) = mount(scene(30.0, &PaintTally::new()));
    let (_, fresh) = frame(&mut SceneHost::new(), fresh);
    assert_eq!(fingerprint(&second), fingerprint(&fresh));
    let origin = leader_origin(&second, link);
    assert_eq!(origin, leader_origin(&fresh, link));
    assert_eq!(origin.expect("the leader is indexed").x, 30.0);
    assert_eq!(follower_transform(&second), follower_transform(&fresh));
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
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    let first_anchor = tally.anchor().expect("the tally sits under an anchor");
    assert!(
        first_anchor.offset.dx > 0.0,
        "precondition: the tally is away from the anchor's origin"
    );
    let paints = tally.paints();

    owner.mark_needs_paint(inner);
    let _ = frame(&mut scene, owner);

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
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    let first_anchor = tally.anchor().expect("the tally sits under an anchor");
    assert!(
        first_anchor.offset.dx >= 20.0,
        "precondition: two leaves sit before the tally along the way: {first_anchor:?}"
    );

    owner.mark_needs_paint(inner);
    let _ = frame(&mut scene, owner);

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
    root_is_boundary: bool,
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
    let mut row = box_node(RenderFlex::row());
    for _ in 0..3 {
        row = row.child(builder.node(4));
    }
    // Odd seeds put the whole tree under a root boundary, so the root's own capture is exercised.
    let root_is_boundary = seed % 2 == 1;
    let root = if root_is_boundary {
        box_node(RenderRepaintBoundary::new())
            .label("root")
            .child(row)
    } else {
        row
    };
    Spec {
        root,
        root_is_boundary,
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
        let (boundaries, opacities, root_is_boundary) =
            (spec.boundaries, spec.opacities, spec.root_is_boundary);
        let (owner, registry) = mount(spec.root);
        let id = |name: String| registry.get(&name).expect("labelled");
        let mut alphas = vec![0.5; opacities];
        let mut rng = Lcg(seed ^ 0x9e37_79b9);
        let mut scene = SceneHost::new();
        let (mut owner, _) = frame(&mut scene, owner);

        for step in 0..6 {
            if root_is_boundary && rng.next(4) == 0 {
                owner.mark_needs_paint(id("root".to_owned()));
            }
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
            let (next, retained) = frame(&mut scene, owner);
            owner = next;

            let (reference, _) = mount(generate(seed, &alphas).root);
            let (_, full) = frame(&mut SceneHost::new(), reference);
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
        let (owner, _registry) = mount(spec.root);
        let mut scene = SceneHost::new();
        let _ = frame(&mut scene, owner);
        // The scene's root slots the frame node, which slots the top-level boundaries: a node
        // deeper than those is a boundary nested in another.
        scene.version().nodes().any(|node| node.depth() >= 3)
    });
    assert!(nested_somewhere);
}

/// Labels are `&'static str`; a test generating a few dozen of them leaks them.
fn leak(label: String) -> &'static str {
    Box::leak(label.into_boxed_str())
}

/// Root boundary → row → [tally, boundary "inner" → tally]: the root is retained like any boundary.
fn rooted() -> (PipelineOwner<Idle>, RenderId, PaintTally, PaintTally) {
    let (root_tally, inner_tally) = (PaintTally::new(), PaintTally::new());
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(RenderFlex::row())
                .child(box_node(root_tally.clone()))
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("inner")
                        .child(box_node(inner_tally.clone())),
                ),
        ),
    );
    let inner = registry.get("inner").expect("labelled");
    (owner, inner, root_tally, inner_tally)
}

/// A root that is a repaint boundary is replayed while only a boundary nested in it repaints.
#[test]
fn a_root_boundary_is_replayed_when_only_a_nested_boundary_repaints() {
    let (owner, inner, root_tally, inner_tally) = rooted();
    let mut scene = SceneHost::new();
    let (mut owner, first) = frame(&mut scene, owner);
    let (root_before, inner_before) = (root_tally.paints(), inner_tally.paints());
    let before = owner.counters();

    owner.mark_needs_paint(inner);
    let (owner, second) = frame(&mut scene, owner);

    // The inner boundary and the tally under it; the root and its row are replayed.
    assert_eq!(owner.counters().since(before).nodes_painted, 2);
    assert_eq!(root_tally.paints(), root_before);
    assert_eq!(inner_tally.paints(), inner_before + 1);
    assert_eq!(fingerprint(&first), fingerprint(&second));
}

/// An effect whose only enclosing boundary is the root is patched, not repainted.
#[test]
fn a_layer_update_under_the_root_boundary_patches_the_retained_root() {
    let tally = PaintTally::new();
    let build = |alpha: f64| {
        box_node(RenderRepaintBoundary::new()).child(
            box_node(RenderOpacity::new(alpha))
                .label("opacity")
                .child(box_node(tally.clone())),
        )
    };
    let (owner, registry) = mount(build(0.5));
    let opacity = registry.get("opacity").expect("labelled");
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    let painted = tally.paints();

    update_render_object::<RenderOpacity, _>(&mut owner, opacity, |o| o.set_opacity(0.25));
    let (_, patched) = frame(&mut scene, owner);

    assert_eq!(
        tally.paints(),
        painted,
        "the root was patched, not repainted"
    );
    let (reference, _) = mount(build(0.25));
    let (_, full) = frame(&mut SceneHost::new(), reference);
    assert_eq!(fingerprint(&patched), fingerprint(&full));
}

/// Retained output was recorded for the old pixel ratio, so the next paint under another one
/// repaints everything, even when only a nested boundary asked for it.
#[test]
fn a_pixel_ratio_change_drops_the_retained_output() {
    let (owner, inner, root_tally, inner_tally) = rooted();
    let mut scene = SceneHost::new();
    let (mut owner, _) = frame(&mut scene, owner);
    owner.mark_needs_paint(inner);
    let (mut owner, _) = frame(&mut scene, owner);
    let (root_before, inner_before) = (root_tally.paints(), inner_tally.paints());

    owner.set_device_pixel_ratio(2.0);
    owner.mark_needs_paint(inner);
    let (_, _) = frame(&mut scene, owner);

    assert_eq!(root_tally.paints(), root_before + 1);
    assert_eq!(inner_tally.paints(), inner_before + 1);
}
