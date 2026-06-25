//! Device-pixel-ratio contract: the framework paints LOGICAL pixels,
//! the paint root's transform maps them to the physical surface.
//!
//! Before this, the whole pipeline ran in physical pixels with an
//! implicit DPR of 1.0 — on a 1.6× display the "200 logical" red box
//! occupied 200 PHYSICAL pixels (visually 125 logical) in the corner
//! of the window, and pointer hits drifted by the scale factor.

use flui_layer::Layer;
use flui_objects::RenderColoredBox;
use flui_painting::DisplayListCore;
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use flui_types::{Point, Rect, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

#[test]
fn paint_root_carries_the_dpr_scale_and_ops_stay_logical() {
    let mut owner = PipelineOwner::new();
    owner.set_device_pixel_ratio(2.0);

    let root = owner.insert(Box::new(RenderColoredBox::red(40.0, 40.0)) as BoxedRenderObject);
    owner.set_root_id(Some(root));
    // LOGICAL constraints (a 100×100-logical window at DPR 2 has a
    // 200×200-physical surface).
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint");

    let tree = owner.take_layer_tree().expect("layer tree");
    let root_id = tree.root().expect("root layer");
    let root_node = tree.get(root_id).expect("root node");

    // The root layer is the ONE place logical meets physical.
    let Layer::Transform(transform) = root_node.layer() else {
        panic!(
            "at DPR != 1 the paint root must be a TransformLayer carrying \
             the scale; got {:?}",
            root_node.layer(),
        );
    };
    let (sx, sy) = {
        let m = transform.transform();
        (m[0], m[5])
    };
    assert!(
        (sx - 2.0).abs() < f32::EPSILON && (sy - 2.0).abs() < f32::EPSILON,
        "root transform must scale by the DPR (got sx={sx}, sy={sy})",
    );

    // Picture ops stay LOGICAL — the scale lives on the layer, not in
    // every command.
    let picture_id = root_node.children()[0];
    let Layer::Picture(picture) = tree.get(picture_id).expect("picture node").layer() else {
        panic!("expected the merged picture under the root transform");
    };
    assert_eq!(
        picture.picture().bounds(),
        Rect::from_origin_size(Point::ZERO, Size::new(px(100.0), px(100.0))),
        "draw commands must remain in logical pixels",
    );
}

#[test]
fn dpr_one_keeps_the_offset_root() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(Box::new(RenderColoredBox::red(40.0, 40.0)) as BoxedRenderObject);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint");

    let tree = owner.take_layer_tree().expect("layer tree");
    let root_node = tree.get(tree.root().expect("root")).expect("node");
    assert!(
        matches!(root_node.layer(), Layer::Offset(_)),
        "DPR 1.0 must not pay for an identity transform layer",
    );
}
