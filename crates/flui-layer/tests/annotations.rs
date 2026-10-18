use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Offset, Point, Rect};
use flui_layer::{
    AnnotatedRegionLayer, ClipRectLayer, Layer, LayerTree, OffsetLayer, TransformLayer,
};
use flui_painting::paint::Clip;

fn region(rect: Rect<f64>, label: &'static str) -> Layer {
    Layer::from(AnnotatedRegionLayer::new(rect, Arc::new(label)))
}

fn tree() -> LayerTree {
    LayerTree::new(Layer::from(OffsetLayer::zero()))
}

fn labels(tree: &LayerTree, x: f64, y: f64) -> Vec<&'static str> {
    tree.find_annotations::<&'static str>(Point::new(x, y))
        .into_iter()
        .map(|label| *label)
        .collect()
}

/// Later siblings and a region's own children paint above it.
pub(crate) fn topmost_first() {
    let mut tree = tree();
    let root = tree.root();
    let below = tree.push_child(root, region(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), "below"));
    tree.push_child(
        below,
        region(Rect::from_xywh(0.0, 0.0, 10.0, 10.0), "inside below"),
    );
    tree.push_child(root, region(Rect::from_xywh(0.0, 0.0, 20.0, 20.0), "above"));
    assert_eq!(labels(&tree, 5.0, 5.0), ["above", "inside below", "below"]);
    assert_eq!(
        tree.find_annotation::<&'static str>(Point::new(5.0, 5.0))
            .as_deref(),
        Some(&"above")
    );
    assert_eq!(labels(&tree, 30.0, 30.0), ["below"]);
}

pub(crate) fn offsets_and_transforms_map_the_point() {
    let mut tree = tree();
    let root = tree.push_child(
        tree.root(),
        Layer::from(OffsetLayer::new(Offset::new(10.0, 10.0))),
    );
    tree.push_child(root, region(Rect::from_xywh(0.0, 0.0, 5.0, 5.0), "offset"));
    let scaled = tree.push_child(
        root,
        Layer::from(TransformLayer::new(Matrix4::scaling(2.0, 2.0, 1.0))),
    );
    tree.push_child(
        scaled,
        region(Rect::from_xywh(10.0, 10.0, 5.0, 5.0), "scaled"),
    );
    assert_eq!(labels(&tree, 12.0, 12.0), ["offset"]);
    assert_eq!(labels(&tree, 2.0, 2.0), [""; 0]);
    // (32, 32) is (22, 22) under the offset, (11, 11) under the scale.
    assert_eq!(labels(&tree, 32.0, 32.0), ["scaled"]);
    assert_eq!(labels(&tree, 20.0, 20.0), [""; 0]);
}

/// A clip hides what it excludes; `Clip::None` clips nothing.
pub(crate) fn clips_hide_what_they_exclude() {
    for (behavior, outside) in [(Clip::HardEdge, Vec::new()), (Clip::None, vec!["clipped"])] {
        let mut tree = tree();
        let clip = tree.push_child(
            tree.root(),
            Layer::from(ClipRectLayer::new(
                Rect::from_xywh(0.0, 0.0, 5.0, 5.0),
                behavior,
            )),
        );
        tree.push_child(
            clip,
            region(Rect::from_xywh(0.0, 0.0, 20.0, 20.0), "clipped"),
        );
        assert_eq!(labels(&tree, 2.0, 2.0), ["clipped"], "{behavior:?}");
        assert_eq!(labels(&tree, 10.0, 10.0), outside, "{behavior:?}");
    }
}

pub(crate) fn singular_transforms_hide_their_subtree() {
    let mut tree = tree();
    let flat = tree.push_child(
        tree.root(),
        Layer::from(TransformLayer::new(Matrix4::scaling(0.0, 1.0, 1.0))),
    );
    tree.push_child(
        flat,
        region(Rect::from_xywh(-100.0, -100.0, 200.0, 200.0), "flat"),
    );
    assert_eq!(labels(&tree, 0.0, 0.0), [""; 0]);
}

/// Under perspective, a region behind the camera is not painted, so the scene
/// point that its inverse division would land on does not find it.
pub(crate) fn perspective_hides_what_lies_behind_the_camera() {
    // w = 1 - x / 100: local x = 50 draws at scene x = 100, and local x = 200
    // has w = -1, behind the camera.
    let mut perspective = Matrix4::identity();
    perspective.m[3] = -0.01;
    let mut tree = tree();
    let projected = tree.push_child(tree.root(), Layer::from(TransformLayer::new(perspective)));
    tree.push_child(
        projected,
        region(Rect::from_xywh(40.0, -10.0, 20.0, 20.0), "visible"),
    );
    tree.push_child(
        projected,
        region(Rect::from_xywh(150.0, -10.0, 100.0, 20.0), "behind"),
    );
    assert_eq!(labels(&tree, 100.0, 0.0), ["visible"]);
    assert_eq!(labels(&tree, -200.0, 0.0), [""; 0]);
}

/// Another annotation type is skipped, a sized-by-parent region matches
/// wherever its parent admits the point, and a region elsewhere is missed.
pub(crate) fn other_types_and_regions_elsewhere_are_skipped() {
    let mut tree = tree();
    let root = tree.root();
    tree.push_child(
        root,
        Layer::from(AnnotatedRegionLayer::new(
            Rect::from_xywh(0.0, 0.0, 50.0, 50.0),
            Arc::new(7_u32),
        )),
    );
    tree.push_child(
        root,
        Layer::from(AnnotatedRegionLayer::sized_by_parent(Arc::new("anywhere"))),
    );
    tree.push_child(
        root,
        region(Rect::from_xywh(40.0, 40.0, 5.0, 5.0), "elsewhere"),
    );
    assert_eq!(labels(&tree, 5.0, 5.0), ["anywhere"]);
    assert_eq!(
        tree.find_annotations::<u32>(Point::new(5.0, 5.0))
            .iter()
            .map(|v| **v)
            .collect::<Vec<_>>(),
        [7]
    );
}
