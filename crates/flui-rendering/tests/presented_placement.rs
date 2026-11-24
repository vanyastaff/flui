//! `PipelineOwner::presented_placement` answers one node by the walk
//! `presented_nodes` makes: the same gates, transform and clip.

use flui_foundation::geometry::{EdgeInsets, Offset, Point, Rect, Size};
use flui_objects::{
    RenderClipRect, RenderColoredBox, RenderFlex, RenderFollowerLayer, RenderLeaderLayer,
    RenderOffstage, RenderPadding, RenderRepaintBoundary, RenderSizedOverflowBox,
    RenderSliverToBoxAdapter, RenderViewport,
};
use flui_painting::paint::Clip;
use flui_rendering::{
    constraints::{AxisDirection, BoxConstraints},
    pipeline::PipelineOwner,
    testing::{SceneHost, TreeNode, box_node, sliver_node, tree},
    view::ScrollPosition,
};

fn laid_out_and_painted(root: TreeNode) -> (PipelineOwner, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let (root_id, registry) = tree::mount(&mut owner, root);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let mut scene = SceneHost::new();
    let result = scene.run_frame(&mut owner);
    result.expect("frame").expect("the frame produces a patch");
    (owner, registry)
}

fn close(a: Rect, b: Rect) -> bool {
    [
        (a.left(), b.left()),
        (a.top(), b.top()),
        (a.right(), b.right()),
        (a.bottom(), b.bottom()),
    ]
    .iter()
    .all(|(x, y)| (x - y).abs() < 1e-9)
}

/// For every node of each scene: a node `presented_nodes` reports has a
/// placement whose transform maps its box onto the reported rectangle and
/// whose clip leaves the reported visible part; a node it does not report
/// (offstage, left out of its parent's paint) has none. The scenes cover a
/// clip cutting an overflowing child, a follower placed against its leader,
/// an offstage subtree and a viewport cutting a sliver scrolled partly out.
///
/// Fails with the placement walk not asking the parent's last paint order
/// (the offstage child gets a placement); the gate and the parent-to-child
/// step are the ones `presented_nodes` applies, so a change to either moves
/// both walks and is held by the presented walk's own tests.
#[test]
fn a_nodes_placement_agrees_with_what_the_presented_walk_reports() {
    let link = flui_layer::LayerLink::new();
    let scenes: [(&str, TreeNode, &[&str]); 4] = [
        (
            "a clip cuts its overflowing child",
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderFlex::row()).child(
                    box_node(RenderPadding::new(EdgeInsets::only_left(20.0))).child(
                        box_node(RenderClipRect::new(Clip::HardEdge)).child(
                            box_node(RenderSizedOverflowBox::centered(50.0, 50.0))
                                .child(box_node(RenderColoredBox::red(120.0, 120.0)).label("cut")),
                        ),
                    ),
                ),
            ),
            &["cut"],
        ),
        (
            "a follower placed against its leader",
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderFlex::row())
                    .child(
                        box_node(RenderLeaderLayer::new(link))
                            .child(box_node(RenderColoredBox::red(40.0, 40.0))),
                    )
                    .child(
                        box_node(RenderRepaintBoundary::new()).child(
                            box_node(
                                RenderFollowerLayer::new(link).with_offset(Offset::new(30.0, 10.0)),
                            )
                            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("follower")),
                        ),
                    ),
            ),
            &["follower"],
        ),
        (
            "an offstage subtree",
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderFlex::row())
                    .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("shown"))
                    .child(
                        box_node(RenderOffstage::new(true))
                            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("hidden")),
                    ),
            ),
            &["shown"],
        ),
        (
            "a viewport cuts a sliver scrolled partly out",
            box_node(RenderRepaintBoundary::new()).child(
                box_node(RenderViewport::with_offset(
                    AxisDirection::TopToBottom,
                    AxisDirection::LeftToRight,
                    ScrollPosition::fixed(30.0),
                ))
                .child(
                    sliver_node(RenderSliverToBoxAdapter::new())
                        .child(box_node(RenderColoredBox::red(200.0, 100.0)).label("first")),
                )
                .child(
                    sliver_node(RenderSliverToBoxAdapter::new())
                        .child(box_node(RenderColoredBox::red(200.0, 100.0)).label("second")),
                ),
            ),
            &["first", "second"],
        ),
    ];
    for (case, root, shown) in scenes {
        let (owner, registry) = laid_out_and_painted(root);
        let presented = owner.presented_nodes();
        for label in shown {
            let id = registry.get(label).expect("labelled");
            assert!(
                presented.iter().any(|node| node.id == id),
                "{case}: precondition, {label} is presented"
            );
        }
        for (id, node) in owner.render_tree().iter() {
            let placement = owner.presented_placement(id);
            let Some(reported) = presented.iter().find(|reported| reported.id == id) else {
                assert_eq!(
                    placement, None,
                    "{case}: a node not presented has no placement"
                );
                continue;
            };
            let placement = placement.unwrap_or_else(|| panic!("{case}: {id:?} has a placement"));
            let size = node.laid_out_size().expect("a presented node is laid out");
            let drawn = placement
                .transform
                .transform_rect(&Rect::from_origin_size(Point::ZERO, size));
            assert!(
                close(drawn, reported.rect),
                "{case}: {} drawn at {drawn:?}, reported at {:?}",
                reported.name,
                reported.rect
            );
            let visible = placement
                .clip
                .intersect(&reported.rect)
                .filter(|kept| !kept.is_empty());
            assert_eq!(
                visible.is_some(),
                reported.visible.is_some(),
                "{case}: {} is shown",
                reported.name
            );
            if let (Some(visible), Some(expected)) = (visible, reported.visible) {
                assert!(
                    close(visible, expected),
                    "{case}: {} shows {visible:?}, reported {expected:?}",
                    reported.name
                );
            }
        }
        if case == "an offstage subtree" {
            let hidden = registry.get("hidden").expect("labelled");
            assert_eq!(owner.presented_placement(hidden), None, "{case}: offstage");
        }
    }
}
