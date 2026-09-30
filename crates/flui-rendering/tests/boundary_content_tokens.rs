//! The paint pass certifies each repaint boundary's content token.
//!
//! A boundary keeps the token it committed while it is absent from the paint
//! queue (a layer update counts as presence) and still has a retained capture;
//! everything else mints. The damage differ in `flui-layer` trusts exactly
//! this: an unchanged token means the boundary's own pixels did not change.

use std::collections::HashMap;

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_layer::{ContentToken, DamageRect, DamageRegion, Layer, LayerDiffer, LayerTree, Scene};
use flui_objects::{
    RenderColoredBox, RenderFlex, RenderLeaderLayer, RenderOpacity, RenderRepaintBoundary,
};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{Idle, PipelineOwner},
    testing::{TreeNode, box_node, tree, update_render_object},
};

/// Every stamp's token, by boundary.
fn tokens(t: &LayerTree) -> HashMap<RenderId, ContentToken> {
    t.iter()
        .filter_map(|(_, node)| {
            node.boundary()
                .map(|stamp| (stamp.render_id(), stamp.content().clone()))
        })
        .collect()
}

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
fn a_grafted_boundary_keeps_its_token() {
    let (owner, _root, a, b) = two_boundaries();
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(a);
    let (_, second) = frame(owner);
    let (first, second) = (tokens(&first), tokens(&second));
    assert_eq!(
        first[&b], second[&b],
        "a clean boundary served from its capture keeps the committed token"
    );
}

#[test]
fn a_repainted_boundary_mints() {
    let (owner, _root, a, b) = two_boundaries();
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(a);
    let (mut owner, second) = frame(owner);
    assert_ne!(tokens(&first)[&a], tokens(&second)[&a]);

    // And the new token is what the capture now vouches for.
    owner.mark_needs_paint(b);
    let (_, third) = frame(owner);
    assert_eq!(tokens(&second)[&a], tokens(&third)[&a]);
}

/// The reason tokens exist: an outer boundary re-descended only because a
/// boundary nested in it is dirty re-records its inline pictures, so their
/// `Arc`s change although its content did not. A pointer comparison of
/// pictures would report it changed on every such frame.
#[test]
fn an_outer_boundary_redescended_for_a_nested_repaint_keeps_its_token() {
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
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(inner);
    let (_, second) = frame(owner);

    let (t1, t2) = (tokens(&first), tokens(&second));
    assert_eq!(
        t1[&outer], t2[&outer],
        "the outer boundary's content is the same"
    );
    assert_ne!(t1[&inner], t2[&inner], "the nested boundary repainted");

    let outer_pictures = |t: &LayerTree| -> Vec<*const flui_painting::DrawCommand> {
        let outer_layer = t
            .iter()
            .find(|(_, node)| node.render_id() == Some(outer))
            .map(|(id, _)| id)
            .expect("outer is stamped");
        t.children(outer_layer)
            .expect("outer layer")
            .iter()
            .filter_map(|&id| match t.get_layer(id) {
                Some(Layer::Picture(picture)) => Some(picture.picture().commands().as_ptr()),
                _ => None,
            })
            .collect()
    };
    let (p1, p2) = (outer_pictures(&first), outer_pictures(&second));
    assert!(
        !p1.is_empty(),
        "precondition: the outer boundary paints a picture of its own"
    );
    assert!(
        p1.iter().zip(&p2).all(|(a, b)| a != b),
        "the outer pictures were re-recorded, so picture identity cannot be the test"
    );
}

#[test]
fn a_layer_update_mints() {
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
    let (mut owner, first) = frame(owner);
    update_render_object::<RenderOpacity, _>(&mut owner, opacity, |o| o.set_opacity(0.25));
    let (mut owner, second) = frame(owner);
    assert_ne!(
        tokens(&first)[&boundary],
        tokens(&second)[&boundary],
        "a patched graft changes pixels, so the token must change"
    );

    // The minted token was written back into the capture: a later graft for
    // an unrelated reason serves it, not the pre-patch one.
    owner.mark_needs_paint(sibling);
    let (_, third) = frame(owner);
    assert_eq!(tokens(&second)[&boundary], tokens(&third)[&boundary]);
}

#[test]
fn root_keeps_its_token_when_only_a_nested_boundary_is_dirty() {
    let (owner, root, a, _b) = two_boundaries();
    let (mut owner, first) = frame(owner);
    owner.mark_needs_paint(a);
    let (mut owner, second) = frame(owner);
    assert_eq!(tokens(&first)[&root], tokens(&second)[&root]);

    // A paint change the root itself absorbs mints.
    owner.mark_needs_paint(root);
    let (_, third) = frame(owner);
    assert_ne!(tokens(&second)[&root], tokens(&third)[&root]);
}

/// A boundary whose capture was refused (it holds a leader, whose follower
/// correlation a capture cannot carry) has nothing to vouch for its content,
/// so it mints on every frame even while clean.
#[test]
fn an_evicted_capture_mints() {
    let link = flui_layer::LayerLink::new();
    let (owner, registry) = mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("leading")
                    .child(box_node(RenderLeaderLayer::new(link)).child(leaf())),
            )
            .child(
                box_node(RenderRepaintBoundary::new())
                    .label("other")
                    .child(leaf()),
            ),
    );
    let leading = registry.get("leading").expect("labelled");
    let other = registry.get("other").expect("labelled");
    let (mut owner, first) = frame(owner);
    assert_eq!(
        owner.retained_boundary_count(),
        1,
        "precondition: only the leaderless boundary is retained"
    );
    owner.mark_needs_paint(other);
    let (_, second) = frame(owner);
    assert_ne!(tokens(&first)[&leading], tokens(&second)[&leading]);
}

/// End to end: two pipeline frames through the differ damage exactly the
/// boundary that changed. On a tree whose paint pass certifies nothing, the
/// second frame would be `Full`.
#[test]
fn pipeline_frames_diff_to_the_changed_boundary_rect() {
    let (owner, _root, a, b) = two_boundaries();
    let surface = (200, 200);
    let mut differ = LayerDiffer::default();

    let (mut owner, first) = frame(owner);
    assert_eq!(differ.diff(&Scene::new(first), surface), DamageRegion::Full);

    // The damage rect of a 10x10 boundary at its stamped layer's offset.
    let expected = |t: &LayerTree, boundary: RenderId| {
        let origin = t
            .iter()
            .find(|(_, node)| node.render_id() == Some(boundary))
            .and_then(|(_, node)| match node.layer() {
                Layer::Offset(offset) => Some(offset.offset()),
                _ => None,
            })
            .expect("a boundary's stamped layer is an OffsetLayer");
        DamageRegion::Partial(
            DamageRect::covering(
                flui_foundation::geometry::Rect::from_xywh(origin.dx, origin.dy, 10.0, 10.0),
                surface,
            )
            .expect("on the surface"),
        )
    };

    owner.mark_needs_paint(b);
    let (mut owner, second) = frame(owner);
    let want = expected(&second, b);
    assert_eq!(differ.diff(&Scene::new(second), surface), want);

    // The next frame damages only `a`: `b` keeps the token its new capture
    // committed.
    owner.mark_needs_paint(a);
    let (_, third) = frame(owner);
    let want = expected(&third, a);
    assert_eq!(differ.diff(&Scene::new(third), surface), want);
}
