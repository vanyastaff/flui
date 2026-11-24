//! The paint pass republishes a repaint boundary's content only when its
//! pixels change (ADR-0304 §8).
//!
//! A boundary absent from the paint queue keeps the content its scene node
//! holds, and the patch names it with no operation; a repaint or a layer
//! update publishes a new content at the same node. The scene owner's damage
//! is the footprint of those operations, so it trusts exactly this.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_layer::{BoundaryContent, DamageRect, DamageRegion, FullReason, Region};
use flui_objects::{
    RenderColoredBox, RenderFlex, RenderFollowerLayer, RenderOpacity, RenderRepaintBoundary,
};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{SceneHost, TreeNode, box_node, tree, update_render_object},
};

fn mount(root: TreeNode) -> (PipelineOwner, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    (owner, registry)
}

fn frame(scene: &mut SceneHost, mut owner: PipelineOwner) -> PipelineOwner {
    let result = scene.run_frame(&mut owner);
    result.expect("frame").expect("the frame produces a patch");
    owner
}

/// The content `boundary` is published with in the scene's current version.
fn content(scene: &SceneHost, owner: &PipelineOwner, boundary: RenderId) -> Arc<BoundaryContent> {
    scene
        .content_of(owner, boundary)
        .expect("the boundary is published")
}

fn leaf() -> TreeNode {
    box_node(RenderColoredBox::red(10.0, 10.0))
}

/// Root boundary → row → [boundary "a" → leaf, boundary "b" → leaf].
fn two_boundaries() -> (PipelineOwner, RenderId, RenderId, RenderId) {
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(RenderFlex::row())
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("a")
                        .child(leaf()),
                )
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("b")
                        .child(leaf()),
                ),
        ),
    );
    let get = |name| registry.get(name).expect("labelled");
    (owner, get("root"), get("a"), get("b"))
}

#[test]
fn a_clean_boundary_keeps_its_content() {
    let (owner, _root, a, b) = two_boundaries();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = content(&scene, &owner, b);
    owner.mark_needs_paint(a);
    let owner = frame(&mut scene, owner);
    assert!(
        Arc::ptr_eq(&before, &content(&scene, &owner, b)),
        "a clean boundary keeps the content its node holds"
    );
}

/// A frame that repaints one boundary patches that one node: the root that
/// slots it and the sibling beside it emit nothing.
#[test]
fn a_clean_boundary_emits_no_operation() {
    let (owner, _root, a, _b) = two_boundaries();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = owner.counters();
    owner.mark_needs_paint(a);
    let owner = frame(&mut scene, owner);
    assert_eq!(
        owner.counters().since(before).scene_ops,
        1,
        "one Publish of the repainted boundary, nothing for the clean ones"
    );
}

#[test]
fn a_repainted_boundary_republishes() {
    let (owner, _root, a, b) = two_boundaries();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let first = content(&scene, &owner, a);
    owner.mark_needs_paint(a);
    let mut owner = frame(&mut scene, owner);
    let second = content(&scene, &owner, a);
    assert!(!Arc::ptr_eq(&first, &second));

    // And the new content is what the node now holds.
    owner.mark_needs_paint(b);
    let owner = frame(&mut scene, owner);
    assert!(Arc::ptr_eq(&second, &content(&scene, &owner, a)));
}

/// A boundary nested in an outer one repaints alone: the outer boundary's
/// content still slots the nested node, so it is not republished.
#[test]
fn an_outer_boundary_keeps_its_content_when_a_nested_one_repaints() {
    let (owner, registry) = mount(
        box_node(RenderFlex::row()).child(
            box_node(RenderRepaintBoundary::new()).label("outer").child(
                box_node(RenderFlex::row()).child(leaf()).child(
                    box_node(RenderRepaintBoundary::new())
                        .label("inner")
                        .child(leaf()),
                ),
            ),
        ),
    );
    let outer = registry.get("outer").expect("labelled");
    let inner = registry.get("inner").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let (outer_before, inner_before) = (
        content(&scene, &owner, outer),
        content(&scene, &owner, inner),
    );
    owner.mark_needs_paint(inner);
    let owner = frame(&mut scene, owner);

    assert!(
        Arc::ptr_eq(&outer_before, &content(&scene, &owner, outer)),
        "the outer boundary's content is the same"
    );
    assert!(
        !Arc::ptr_eq(&inner_before, &content(&scene, &owner, inner)),
        "the nested boundary repainted"
    );
}

#[test]
fn a_layer_update_republishes() {
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("boundary")
                    .child(
                        box_node(RenderOpacity::new(0.5))
                            .label("opacity")
                            .child(leaf()),
                    ),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("sibling")
                    .child(leaf()),
            ),
    );
    let boundary = registry.get("boundary").expect("labelled");
    let opacity = registry.get("opacity").expect("labelled");
    let sibling = registry.get("sibling").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let first = content(&scene, &owner, boundary);
    update_render_object::<RenderOpacity>(&mut owner, opacity, |o| o.set_opacity(0.25));
    let mut owner = frame(&mut scene, owner);
    let second = content(&scene, &owner, boundary);
    assert!(
        !Arc::ptr_eq(&first, &second),
        "a patched content changes pixels, so it is published again"
    );

    // The patched content is the record's: a later frame for an unrelated
    // reason keeps it, not the pre-patch one.
    owner.mark_needs_paint(sibling);
    let owner = frame(&mut scene, owner);
    assert!(Arc::ptr_eq(&second, &content(&scene, &owner, boundary)));
}

#[test]
fn root_keeps_its_content_when_only_a_nested_boundary_is_dirty() {
    let (owner, root, a, _b) = two_boundaries();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let first = content(&scene, &owner, root);
    owner.mark_needs_paint(a);
    let mut owner = frame(&mut scene, owner);
    let second = content(&scene, &owner, root);
    assert!(Arc::ptr_eq(&first, &second));

    // A paint change the root itself absorbs republishes it.
    owner.mark_needs_paint(root);
    let owner = frame(&mut scene, owner);
    assert!(!Arc::ptr_eq(&second, &content(&scene, &owner, root)));
}

/// A boundary that holds a follower is retained like any other: the version
/// resolves the follower against its leader, so nothing about it forces a
/// repaint.
#[test]
fn a_boundary_holding_a_follower_keeps_its_content() {
    let link = flui_layer::LayerLink::new();
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("following")
                    .child(box_node(RenderFollowerLayer::new(link)).child(leaf())),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("other")
                    .child(leaf()),
            ),
    );
    let following = registry.get("following").expect("labelled");
    let other = registry.get("other").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    assert_eq!(
        owner.boundary_record_count(),
        2,
        "both boundaries are retained"
    );
    let first = content(&scene, &owner, following);
    owner.mark_needs_paint(other);
    let owner = frame(&mut scene, owner);
    assert!(Arc::ptr_eq(&first, &content(&scene, &owner, following)));
}

/// End to end: two pipeline frames damage exactly the boundary that changed.
/// A producer that republished clean boundaries would damage them too.
#[test]
fn pipeline_frames_diff_to_the_changed_boundary_rect() {
    // `two_boundaries` with its leaves labelled: a boundary marked with
    // nothing changed keeps its chunk, so each frame recolours a leaf.
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(RenderFlex::row())
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("a")
                        .child(leaf().label("a_leaf")),
                )
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("b")
                        .child(leaf().label("b_leaf")),
                ),
        ),
    );
    let get = |name| registry.get(name).expect("labelled");
    let (a, b) = (get("a"), get("b"));
    let recolour = |owner: &mut PipelineOwner, leaf: RenderId| {
        update_render_object::<RenderColoredBox>(owner, leaf, |leaf| {
            leaf.set_color([0.0, 0.0, 1.0, 1.0])
        });
    };
    let surface = (800, 600);

    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    assert_eq!(
        scene.owner_mut().publication().damage,
        DamageRegion::Full(FullReason::FirstFrame)
    );

    // Each later patch damages its own operations: the rect of a 10x10 boundary at its node's
    // placement, under a root boundary and a frame node that stand at the origin.
    let expected = |scene: &SceneHost, owner: &PipelineOwner, boundary: RenderId| {
        let node = owner.boundary_scene_node(boundary).expect("published");
        let origin = scene
            .version()
            .node(node)
            .expect("live node")
            .placement()
            .offset();
        DamageRegion::Region(Region::new(
            DamageRect::try_covering(
                flui_foundation::geometry::Rect::from_xywh(origin.dx, origin.dy, 10.0, 10.0),
                surface,
            )
            .expect("finite")
            .expect("on the surface"),
        ))
    };

    recolour(&mut owner, get("b_leaf"));
    let mut owner = frame(&mut scene, owner);
    assert_eq!(
        scene.owner_mut().patch_damage(),
        expected(&scene, &owner, b)
    );

    // The next frame damages only `a`.
    recolour(&mut owner, get("a_leaf"));
    let owner = frame(&mut scene, owner);
    assert_eq!(
        scene.owner_mut().patch_damage(),
        expected(&scene, &owner, a)
    );
}

/// A leaf whose paint panics while it is armed.
#[derive(Debug)]
struct PanicsWhenArmed(Arc<std::sync::atomic::AtomicBool>);

impl flui_foundation::inspect::Inspect for PanicsWhenArmed {
    fn kind(&self) -> &'static str {
        "PanicsWhenArmed"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PanicsWhenArmed {
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

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Leaf>) {
        assert!(
            !self.0.load(std::sync::atomic::Ordering::Relaxed),
            "PanicsWhenArmed: armed, failing this paint on purpose",
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

/// A paint that panics applies nothing: the scene keeps the version it held, every boundary keeps
/// its content, and the next frame paints what was owed into that version (ADR-0304 §10).
#[test]
fn a_paint_panic_applies_no_patch_and_keeps_the_presented_version() {
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("failing")
                    .child(box_node(PanicsWhenArmed(Arc::clone(&armed))).label("leaf")),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("other")
                    .child(leaf()),
            ),
    );
    let failing = registry.get("failing").expect("labelled");
    let leaf_id = registry.get("leaf").expect("labelled");
    let other = registry.get("other").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let presented = scene.version().id();
    let (failing_before, other_before) = (
        content(&scene, &owner, failing),
        content(&scene, &owner, other),
    );

    armed.store(true, std::sync::atomic::Ordering::Relaxed);
    owner.mark_needs_paint(leaf_id);
    owner.mark_needs_paint(other);
    let result = scene.run_frame(&mut owner);
    assert!(
        result.is_err(),
        "precondition: the armed leaf fails the paint"
    );
    assert_eq!(scene.version().id(), presented, "no patch was applied");
    assert!(Arc::ptr_eq(
        &failing_before,
        &content(&scene, &owner, failing)
    ));
    assert!(Arc::ptr_eq(&other_before, &content(&scene, &owner, other)));

    armed.store(false, std::sync::atomic::Ordering::Relaxed);
    let result = scene.run_frame(&mut owner);
    result.expect("the retry paints").expect("a patch");
    assert_ne!(scene.version().id(), presented);
    assert!(
        !Arc::ptr_eq(&other_before, &content(&scene, &owner, other)),
        "the retry paints the boundary the failed pass owed"
    );
}

/// A parent that paints its one child at the `dx` it holds, as the bits of an `f64`.
#[derive(Debug)]
struct PaintsChildAt(Arc<std::sync::atomic::AtomicU64>);

impl flui_foundation::inspect::Inspect for PaintsChildAt {
    fn kind(&self) -> &'static str {
        "PaintsChildAt"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PaintsChildAt {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints.loosen())?;
        Ok(constraints.biggest())
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Single>) {
        let dx = f64::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed));
        ctx.paint_child_at(flui_foundation::geometry::Offset::new(dx, 0.0));
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

/// A boundary placed at a non-finite offset is not published at a stand-in: it keeps the
/// placement its node holds, and a later frame at the same offset finds nothing to move
/// (ADR-0054).
#[test]
fn a_boundary_placed_at_a_non_finite_offset_keeps_its_last_placement() {
    let dx = Arc::new(std::sync::atomic::AtomicU64::new(5.0_f64.to_bits()));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(PaintsChildAt(Arc::clone(&dx)))
                .label("parent")
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("placed")
                        .child(leaf()),
                ),
        ),
    );
    let parent = registry.get("parent").expect("labelled");
    let placed = registry.get("placed").expect("labelled");
    let placement = |scene: &SceneHost, owner: &PipelineOwner| {
        let node = owner.boundary_scene_node(placed).expect("published");
        scene
            .version()
            .node(node)
            .expect("live node")
            .placement()
            .offset()
    };
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let first = placement(&scene, &owner);
    assert!(first.dx > 0.0, "precondition: placed at a finite offset");

    for bad in [f64::NAN, f64::INFINITY] {
        dx.store(bad.to_bits(), std::sync::atomic::Ordering::Relaxed);
        owner.mark_needs_paint(parent);
        owner = frame(&mut scene, owner);
        assert_eq!(
            placement(&scene, &owner),
            first,
            "{bad} keeps the last placement"
        );

        let before = owner.counters();
        owner.mark_needs_paint(parent);
        owner = frame(&mut scene, owner);
        assert_eq!(
            owner.counters().since(before).scene_ops,
            1,
            "{bad}: one Publish of the repainted root, no Place of the boundary"
        );
    }
}

/// A new pixel ratio repaints the scene into new nodes and retires the old
/// ones: the scene holds as many nodes after each change as after the first
/// frame.
///
/// Red-check: drop the records without keeping their nodes for retirement
/// (each change leaves a whole old scene in the table).
#[test]
fn a_pixel_ratio_change_retires_the_nodes_it_replaces() {
    let (owner, _root, _a, _b) = two_boundaries();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let first = scene.version().len();
    for ratio in [2.0, 1.5, 3.0] {
        owner.set_device_pixel_ratio(
            flui_foundation::geometry::DevicePixelRatio::try_new(ratio).expect("a ratio in range"),
        );
        owner = frame(&mut scene, owner);
        assert_eq!(
            scene.version().len(),
            first,
            "the scene after the change to {ratio} holds no old node"
        );
    }
}

/// A parent that paints its one child only while the flag it holds is set.
#[derive(Debug)]
struct PaintsChildWhen(std::rc::Rc<std::cell::Cell<bool>>);

impl flui_foundation::inspect::Inspect for PaintsChildWhen {
    fn kind(&self) -> &'static str {
        "PaintsChildWhen"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PaintsChildWhen {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints.loosen())?;
        Ok(constraints.biggest())
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Single>) {
        if self.0.get() {
            ctx.paint_child();
        }
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Single,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

/// A parent that left its child out and then paints it again presents the
/// child: the order the earlier frame kept for it is dropped, though a parent
/// painting every child in place keeps no order of its own.
///
/// Red-check: queue nothing for a plain order (the child stays left out).
#[test]
fn a_parent_painting_its_child_again_drops_the_order_that_left_it_out() {
    let shows = std::rc::Rc::new(std::cell::Cell::new(false));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(PaintsChildWhen(std::rc::Rc::clone(&shows)))
                .label("parent")
                .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
        ),
    );
    let parent = registry.get("parent").expect("labelled");
    let child = registry.get("child").expect("labelled");
    let presents =
        |owner: &PipelineOwner| owner.presented_nodes().iter().any(|node| node.id == child);
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    assert!(!presents(&owner), "precondition: the child is left out");

    shows.set(true);
    owner.mark_needs_paint(parent);
    owner = frame(&mut scene, owner);
    assert!(presents(&owner), "the child painted again is presented");
}

/// The version the one chunk `owner` painted into `content` was recorded as.
fn chunk_version(content: &BoundaryContent, owner: RenderId) -> flui_layer::ContentVersion {
    let mut versions = content.entries().iter().filter_map(|entry| match entry {
        flui_layer::Entry::Chunk { chunk, .. } if chunk.owner() == owner => Some(chunk.version()),
        _ => None,
    });
    let version = versions.next().expect("the render object painted a chunk");
    assert!(
        versions.next().is_none(),
        "the render object painted one chunk"
    );
    version
}

/// Root boundary → row → leaves "a", "b" and "c", each 10×10, painted into
/// the root's content.
fn three_leaves() -> (PipelineOwner, RenderId, [RenderId; 3]) {
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(RenderFlex::row())
                .child(leaf().label("a"))
                .child(leaf().label("b"))
                .child(leaf().label("c")),
        ),
    );
    let get = |name| registry.get(name).expect("labelled");
    (owner, get("root"), [get("a"), get("b"), get("c")])
}

/// A boundary repainted for one of its render objects keeps the recordings
/// of the chunks whose render objects owe no paint work, so the raster side
/// replays them, and records the changed one again (ADR-0304 §2).
///
/// Red-check: mint a version for every chunk sealed.
#[test]
fn a_repaint_keeps_the_chunks_of_the_render_objects_that_owe_no_paint() {
    let (owner, root, [a, b, c]) = three_leaves();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = content(&scene, &owner, root);

    update_render_object::<RenderColoredBox>(&mut owner, b, |leaf| {
        leaf.set_color([0.0, 0.0, 1.0, 1.0])
    });
    let owner = frame(&mut scene, owner);
    let after = content(&scene, &owner, root);

    assert!(
        !Arc::ptr_eq(&before, &after),
        "precondition: the boundary repainted"
    );
    for clean in [a, c] {
        assert_eq!(
            chunk_version(&before, clean),
            chunk_version(&after, clean),
            "a render object that owes no paint keeps its chunk's version"
        );
    }
    assert_ne!(
        chunk_version(&before, b),
        chunk_version(&after, b),
        "the recoloured render object's chunk is a new recording"
    );
    // "b" is the second 10×10 leaf of a row at the frame's origin.
    let surface = (800, 600);
    assert_eq!(
        scene.owner_mut().patch_damage(),
        DamageRegion::Region(Region::new(
            DamageRect::try_covering(
                flui_foundation::geometry::Rect::from_xywh(10.0, 0.0, 10.0, 10.0),
                surface,
            )
            .expect("finite")
            .expect("on the surface"),
        )),
        "only the recoloured chunk is damaged"
    );
}

/// A render object that laid out, or that owes no paint but moved in its
/// boundary, records its chunk again: layout leaves it unmarked, and the
/// parent that moved it laid out (ADR-0304 §2).
///
/// Red-check: keep an unmarked render object's chunk whether it or its
/// parent laid out.
#[test]
fn a_chunk_that_moved_in_its_boundary_is_a_new_recording() {
    let (owner, root, [a, b, c]) = three_leaves();
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = content(&scene, &owner, root);

    // "a" widens, so the row moves "b" and "c" without laying them out.
    update_render_object::<RenderColoredBox>(&mut owner, a, |leaf| {
        leaf.set_preferred_size(Size::new(20.0, 10.0))
    });
    let owner = frame(&mut scene, owner);
    let after = content(&scene, &owner, root);

    for moved in [a, b, c] {
        assert_ne!(
            chunk_version(&before, moved),
            chunk_version(&after, moved),
            "a chunk recorded at another origin is a new recording"
        );
    }
}

/// A render object its parent paints at another offset, with no layout,
/// records its chunk again: the parent was marked for its own paint, which
/// placed the child (ADR-0304 §2).
///
/// Red-check: let only a parent's layout move its children.
#[test]
fn a_chunk_its_parent_paints_elsewhere_is_a_new_recording() {
    let dx = Arc::new(std::sync::atomic::AtomicU64::new(0.0_f64.to_bits()));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(PaintsChildAt(Arc::clone(&dx)))
                .label("parent")
                .child(leaf().label("child")),
        ),
    );
    let root = registry.get("root").expect("labelled");
    let parent = registry.get("parent").expect("labelled");
    let child = registry.get("child").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = chunk_version(&content(&scene, &owner, root), child);

    dx.store(20.0_f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
    owner.mark_needs_paint(parent);
    let owner = frame(&mut scene, owner);
    assert_ne!(
        chunk_version(&content(&scene, &owner, root), child),
        before,
        "a chunk painted at another origin is a new recording"
    );
}

/// A leaf that draws a box at the origin of its paint anchor.
#[derive(Debug)]
struct DrawsAtItsAnchor;

impl flui_foundation::inspect::Inspect for DrawsAtItsAnchor {
    fn kind(&self) -> &'static str {
        "DrawsAtItsAnchor"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for DrawsAtItsAnchor {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(ctx.constrain(Size::new(10.0, 10.0)))
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Leaf>) {
        let at = ctx
            .paint_anchor()
            .map_or(flui_foundation::geometry::Point::ZERO, |anchor| {
                flui_foundation::geometry::Point::new(-anchor.offset.dx, -anchor.offset.dy)
            });
        let rect = flui_foundation::geometry::Rect::from_origin_size(at, ctx.size());
        ctx.canvas().draw_rect(
            rect,
            &flui_painting::paint::Paint::fill(flui_painting::styling::Color::from_rgba_f32_array(
                [1.0, 0.0, 0.0, 1.0],
            )),
        );
    }
}

/// A parent that paints its one child at its paint anchor's origin.
#[derive(Debug)]
struct PlacesChildAtItsAnchor;

impl flui_foundation::inspect::Inspect for PlacesChildAtItsAnchor {
    fn kind(&self) -> &'static str {
        "PlacesChildAtItsAnchor"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PlacesChildAtItsAnchor {
    type Arity = flui_foundation::Single;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let size = ctx.layout_child(0, BoxConstraints::tight(Size::new(10.0, 10.0)))?;
        Ok(ctx.constrain(size))
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Single>) {
        let at = ctx
            .paint_anchor()
            .map_or(flui_foundation::geometry::Offset::ZERO, |anchor| {
                -anchor.offset
            });
        ctx.paint_child_at(at);
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        false
    }
}

/// A render object that read its paint anchor records its chunk again when
/// its boundary repaints, and so does the child of one that placed the
/// child from it: the anchor lies outside the boundary, and nothing in it
/// marks a move of the boundary against the anchor (ADR-0304 §2).
///
/// Red-check: keep the chunk of a paint that read its anchor, or of a child
/// its parent placed from its anchor.
#[test]
fn a_chunk_that_read_its_paint_anchor_is_a_new_recording() {
    let dx = Arc::new(std::sync::atomic::AtomicU64::new(0.0_f64.to_bits()));
    let (owner, registry) = mount(
        box_node(flui_objects::RenderPaintAnchor::new()).child(
            box_node(PaintsChildAt(Arc::clone(&dx)))
                .label("mover")
                .child(
                    box_node(RenderRepaintBoundary::new())
                        .label("boundary")
                        .child(
                            box_node(RenderFlex::row())
                                .child(box_node(DrawsAtItsAnchor).label("reader"))
                                .child(
                                    box_node(PlacesChildAtItsAnchor).child(leaf().label("placed")),
                                )
                                .child(leaf().label("sibling")),
                        ),
                ),
        ),
    );
    let get = |name| registry.get(name).expect("labelled");
    let (mover, boundary) = (get("mover"), get("boundary"));
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = content(&scene, &owner, boundary);

    // The boundary moves against the anchor, and repaints for its sibling.
    dx.store(20.0_f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
    owner.mark_needs_paint(mover);
    update_render_object::<RenderColoredBox>(&mut owner, get("sibling"), |leaf| {
        leaf.set_color([0.0, 0.0, 1.0, 1.0])
    });
    let owner = frame(&mut scene, owner);
    let after = content(&scene, &owner, boundary);
    for (name, why) in [
        (
            "reader",
            "a chunk drawn from the moved anchor is a new recording",
        ),
        (
            "placed",
            "a chunk placed from the moved anchor is a new recording",
        ),
    ] {
        assert_ne!(
            chunk_version(&after, get(name)),
            chunk_version(&before, get(name)),
            "{why}"
        );
    }
}

/// A paint pass that fails after painting a changed render object clears its
/// flag and commits nothing, so the retry finds it unmarked beside a record
/// of its old output: the retry keeps no recording on the strength of a flag,
/// and the changed chunk is a new one.
///
/// Red-check: trust the flags on the pass after one that failed.
#[test]
fn the_pass_after_a_failed_one_keeps_no_chunk() {
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(RenderFlex::row())
                .child(leaf().label("a"))
                .child(box_node(PanicsWhenArmed(Arc::clone(&armed))).label("panics")),
        ),
    );
    let root = registry.get("root").expect("labelled");
    let a = registry.get("a").expect("labelled");
    let panics = registry.get("panics").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = chunk_version(&content(&scene, &owner, root), a);

    update_render_object::<RenderColoredBox>(&mut owner, a, |leaf| {
        leaf.set_color([0.0, 0.0, 1.0, 1.0])
    });
    armed.store(true, std::sync::atomic::Ordering::Relaxed);
    owner.mark_needs_paint(panics);
    let failed = scene.run_frame(&mut owner);
    assert!(
        failed.is_err(),
        "precondition: the pass fails after painting \"a\""
    );

    armed.store(false, std::sync::atomic::Ordering::Relaxed);
    owner = frame(&mut scene, owner);
    assert_ne!(
        chunk_version(&content(&scene, &owner, root), a),
        before,
        "the recoloured chunk is a new recording"
    );
}

/// A parent that paints its children, in the order and at the offsets it
/// holds, and lays each out loosely.
#[derive(Debug)]
struct PaintsChildrenAt(Vec<(usize, flui_foundation::geometry::Offset)>);

impl flui_foundation::inspect::Inspect for PaintsChildrenAt {
    fn kind(&self) -> &'static str {
        "PaintsChildrenAt"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PaintsChildrenAt {
    type Arity = flui_foundation::Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        for index in 0..ctx.child_count() {
            ctx.layout_child(index, constraints.loosen())?;
        }
        Ok(constraints.biggest())
    }

    fn paint(
        &self,
        ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Variable>,
    ) {
        for &(index, offset) in &self.0 {
            ctx.paint_child_at(index, offset);
        }
    }
}

/// A render object its parent paints twice with nothing between leaves a
/// chunk for each paint, so a chunk the first paint recorded is the one the
/// next frame's first paint records, whatever comes to lie between them.
///
/// Red-check: let a run of another paint of the owner join the open chunk.
#[test]
fn a_chunk_holds_one_paint_of_its_render_object() {
    let color = std::rc::Rc::new(std::cell::Cell::new([0.0, 0.0, 1.0, 1.0]));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(PaintsChildrenAt(vec![
                (0, flui_foundation::geometry::Offset::ZERO),
                (1, flui_foundation::geometry::Offset::ZERO),
                (0, flui_foundation::geometry::Offset::new(20.0, 0.0)),
            ]))
            .child(leaf().label("twice"))
            .child(
                box_node(DrawsEverySecondPaint {
                    color: std::rc::Rc::clone(&color),
                    paints: std::cell::Cell::new(0),
                })
                .label("between"),
            ),
        ),
    );
    let root = registry.get("root").expect("labelled");
    let twice = registry.get("twice").expect("labelled");
    let between = registry.get("between").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);

    // "between" drew nothing on its first paint and draws on its second.
    owner.mark_needs_paint(between);
    let owner = frame(&mut scene, owner);
    let content = content(&scene, &owner, root);
    let drawn: usize = content
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            flui_layer::Entry::Chunk { chunk, .. } if chunk.owner() == twice => {
                Some(chunk.commands().len())
            }
            _ => None,
        })
        .sum();
    assert_eq!(drawn, 2, "the content draws \"twice\" once for each paint");
}

/// A leaf that draws a box of the colour it holds on every second paint,
/// and nothing on the others.
#[derive(Debug)]
struct DrawsEverySecondPaint {
    color: std::rc::Rc<std::cell::Cell<[f32; 4]>>,
    paints: std::cell::Cell<u32>,
}

impl flui_foundation::inspect::Inspect for DrawsEverySecondPaint {
    fn kind(&self) -> &'static str {
        "DrawsEverySecondPaint"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for DrawsEverySecondPaint {
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

    fn paint(&self, ctx: &mut flui_rendering::context::PaintContext<'_, flui_foundation::Leaf>) {
        let paints = self.paints.get() + 1;
        self.paints.set(paints);
        if paints.is_multiple_of(2) {
            let rect = flui_foundation::geometry::Rect::from_origin_size(
                flui_foundation::geometry::Point::ZERO,
                ctx.size(),
            );
            let color = flui_painting::styling::Color::from_rgba_f32_array(self.color.get());
            ctx.canvas()
                .draw_rect(rect, &flui_painting::paint::Paint::fill(color));
        }
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

/// A render object that owed paint and is painted twice in one content
/// is not placed as before on its second paint, whose marks the first one
/// cleared: its chunk, sealed by the second paint, is a new recording.
///
/// Red-check: read whether a second paint of the render object is placed as
/// before from its parent alone.
#[test]
fn a_render_object_painted_twice_owes_paint_on_both_paints() {
    let color = std::rc::Rc::new(std::cell::Cell::new([1.0, 0.0, 0.0, 1.0]));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).label("root").child(
            box_node(PaintsChildrenAt(vec![
                (0, flui_foundation::geometry::Offset::ZERO),
                (0, flui_foundation::geometry::Offset::new(20.0, 0.0)),
            ]))
            .child(
                box_node(DrawsEverySecondPaint {
                    color: std::rc::Rc::clone(&color),
                    paints: std::cell::Cell::new(0),
                })
                .label("child"),
            ),
        ),
    );
    let root = registry.get("root").expect("labelled");
    let child = registry.get("child").expect("labelled");
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    let before = chunk_version(&content(&scene, &owner, root), child);

    color.set([0.0, 0.0, 1.0, 1.0]);
    owner.mark_needs_paint(child);
    let owner = frame(&mut scene, owner);
    assert_ne!(
        chunk_version(&content(&scene, &owner, root), child),
        before,
        "the recoloured chunk is a new recording"
    );
}
