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
    pipeline::{Idle, PipelineOwner},
    testing::{SceneHost, TreeNode, box_node, tree, update_render_object},
};

fn mount(root: TreeNode) -> (PipelineOwner<Idle>, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    (owner, registry)
}

fn frame(scene: &mut SceneHost, owner: PipelineOwner<Idle>) -> PipelineOwner<Idle> {
    let (owner, result) = scene.run_frame(owner);
    result.expect("frame").expect("the frame produces a patch");
    owner
}

/// The content `boundary` is published with in the scene's current version.
fn content(
    scene: &SceneHost,
    owner: &PipelineOwner<Idle>,
    boundary: RenderId,
) -> Arc<BoundaryContent> {
    scene
        .content_of(owner, boundary)
        .expect("the boundary is published")
}

fn leaf() -> TreeNode {
    box_node(RenderColoredBox::red(10.0, 10.0))
}

/// Root boundary → row → [boundary "a" → leaf, boundary "b" → leaf].
fn two_boundaries() -> (PipelineOwner<Idle>, RenderId, RenderId, RenderId) {
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
    update_render_object::<RenderOpacity, _>(&mut owner, opacity, |o| o.set_opacity(0.25));
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
    let (owner, _root, a, b) = two_boundaries();
    let surface = (800, 600);

    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    assert_eq!(
        scene.owner_mut().publication().damage,
        DamageRegion::Full(FullReason::FirstFrame)
    );

    // Each later patch damages its own operations: the rect of a 10x10 boundary at its node's
    // placement, under a root boundary and a frame node that stand at the origin.
    let expected = |scene: &SceneHost, owner: &PipelineOwner<Idle>, boundary: RenderId| {
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

    owner.mark_needs_paint(b);
    let mut owner = frame(&mut scene, owner);
    assert_eq!(
        scene.owner_mut().patch_damage(),
        expected(&scene, &owner, b)
    );

    // The next frame damages only `a`.
    owner.mark_needs_paint(a);
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

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Leaf>) {
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
    let (owner, result) = scene.run_frame(owner);
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
    let (owner, result) = scene.run_frame(owner);
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

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Single>) {
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
    let placement = |scene: &SceneHost, owner: &PipelineOwner<Idle>| {
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
        owner.set_device_pixel_ratio(ratio);
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
struct PaintsChildWhen(Arc<std::sync::atomic::AtomicBool>);

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

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, flui_foundation::Single>) {
        if self.0.load(std::sync::atomic::Ordering::Relaxed) {
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
    let shows = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (owner, registry) = mount(
        box_node(RenderRepaintBoundary::new()).child(
            box_node(PaintsChildWhen(Arc::clone(&shows)))
                .label("parent")
                .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
        ),
    );
    let parent = registry.get("parent").expect("labelled");
    let child = registry.get("child").expect("labelled");
    let presents =
        |owner: &PipelineOwner<Idle>| owner.presented_nodes().iter().any(|node| node.id == child);
    let mut scene = SceneHost::new();
    let mut owner = frame(&mut scene, owner);
    assert!(!presents(&owner), "precondition: the child is left out");

    shows.store(true, std::sync::atomic::Ordering::Relaxed);
    owner.mark_needs_paint(parent);
    owner = frame(&mut scene, owner);
    assert!(presents(&owner), "the child painted again is presented");
}
