//! `RenderDecoratedBox` through the REAL pipeline: the decoration's
//! draw commands land before (Background) or after (Foreground) the
//! child's inside the merged fragment picture, and hit testing honors
//! the rounded-corner geometry.

use flui_layer::{Layer, LayerTree};
use flui_objects::{DecorationPosition, RenderColoredBox, RenderDecoratedBox};
use flui_painting::{DisplayListCore, DrawCommand};
use flui_rendering::{
    constraints::BoxConstraints, hit_testing::HitTestResult, pipeline::PipelineOwner,
};
use flui_types::{
    Offset, Size,
    geometry::px,
    styling::{BorderRadius, BorderRadiusExt, BoxDecoration, Color},
};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

fn frame_commands(owner: PipelineOwner) -> (PipelineOwner, Vec<&'static str>) {
    let (owner, result) = owner.run_frame();
    let tree = result.expect("frame must not error").expect("frame paints");
    let mut kinds = Vec::new();
    fn walk(tree: &LayerTree, id: flui_foundation::LayerId, kinds: &mut Vec<&'static str>) {
        let Some(node) = tree.get(id) else { return };
        if let Layer::Picture(picture) = node.layer() {
            for command in picture.picture().commands() {
                kinds.push(match command {
                    DrawCommand::DrawRect { .. } => "rect",
                    DrawCommand::DrawRRect { .. } => "rrect",
                    _ => "other",
                });
            }
        }
        for &child in node.children() {
            walk(tree, child, kinds);
        }
    }
    if let Some(root) = tree.root() {
        walk(&tree, root, &mut kinds);
    }
    (owner, kinds)
}

fn fixture(position: DecorationPosition) -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let decorated = owner.insert(Box::new(
        RenderDecoratedBox::new(
            BoxDecoration::with_color(Color::RED)
                .set_border_radius(Some(BorderRadius::circular(px(20.0)))),
        )
        .with_position(position),
    ) as BoxedRenderObject);
    owner
        .insert_child_render_object(decorated, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child insert");
    owner.set_root_id(Some(decorated));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    (owner, decorated)
}

#[test]
fn background_decoration_paints_before_the_child() {
    let (owner, _) = fixture(DecorationPosition::Background);
    let (_owner, kinds) = frame_commands(owner);
    assert_eq!(
        kinds,
        vec!["rrect", "rect"],
        "background decoration (rounded red) must precede the child's \
         rect in the merged fragment"
    );
}

#[test]
fn foreground_decoration_paints_after_the_child() {
    let (owner, _) = fixture(DecorationPosition::Foreground);
    let (_owner, kinds) = frame_commands(owner);
    assert_eq!(
        kinds,
        vec!["rect", "rrect"],
        "foreground decoration must follow the child's rect"
    );
}

/// A childless decorated box, sized by tight 100×100 constraints, so
/// hit-testing falls straight through to `hitTestSelf` — the decoration's own
/// shape — with no child to mask any point.
fn childless_fixture() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let decorated = owner.insert(Box::new(RenderDecoratedBox::new(
        BoxDecoration::with_color(Color::RED)
            .set_border_radius(Some(BorderRadius::circular(px(20.0)))),
    )) as BoxedRenderObject);
    owner.set_root_id(Some(decorated));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    (owner, decorated)
}

#[test]
fn hit_test_excludes_the_rounded_corner() {
    // Isolate `RenderDecoratedBox::hitTestSelf` (no child to mask the corner):
    // the decorated area is hit-opaque only within the decoration's rounded-rect
    // shape (Flutter `BoxDecoration.hitTest`), so the radius-20 corners are cut.
    let (owner, decorated) = childless_fixture();
    let (owner, _) = owner.run_frame();

    let hit_at = |x: f32, y: f32| {
        let mut result = HitTestResult::new();
        owner.hit_test(Offset::new(px(x), px(y)), &mut result);
        result.path().last().map(|entry| entry.target)
    };

    assert_eq!(
        hit_at(50.0, 50.0),
        Some(decorated),
        "the decorated area is hit-opaque at its center"
    );
    assert_eq!(
        hit_at(2.0, 2.0),
        None,
        "the top-left rounded corner lies outside the radius-20 shape"
    );
    assert_eq!(
        hit_at(98.0, 98.0),
        None,
        "the bottom-right rounded corner lies outside the radius-20 shape"
    );
}

#[test]
fn child_is_hit_in_the_rounded_corner_cutout() {
    // Flutter tests `hitTestChildren` before `hitTestSelf` (fixed 31359807): a
    // child occupying a rounded-corner cut-out is still hit even though the
    // decoration's own shape excludes that corner. The fixture's child fills the
    // box (tight constraints stretch it to 100×100), so (2,2) — outside the
    // radius-20 shape (see `hit_test_excludes_the_rounded_corner`) — is
    // nonetheless hit because the child covers it. The hit-path's root is the
    // decorated box, proving the descent reached the child first.
    let (owner, decorated) = fixture(DecorationPosition::Background);
    let (owner, _) = owner.run_frame();

    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(2.0), px(2.0)), &mut result);
    assert_eq!(
        result.path().last().map(|entry| entry.target),
        Some(decorated),
        "a child filling the rounded corner is hit even though the decoration \
         shape excludes that corner (child-first)"
    );
}
