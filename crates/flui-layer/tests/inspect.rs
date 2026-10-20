//! A layer snapshot reads each layer's own fields.

use flui_foundation::geometry::{Point, RRect, RSuperellipse, Rect};
use flui_foundation::inspect::{SnapshotQuery, Variant, snapshot};
use flui_layer::{
    ClipPathLayer, ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer, Layer, LayerTree,
};
use flui_painting::paint::{Clip, Path};

/// Builds a clip layer of one kind around `rect`.
type MakeClip = fn(Rect, Clip) -> Layer;

/// Every clip layer reports how it rasterises its edge, so two clips of one shape that differ
/// only in quality read apart.
///
/// Fails when a clip variant reports its shape alone.
#[test]
fn clip_layers_report_their_clip_behavior() {
    let rect = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let clips: [(&str, MakeClip); 4] = [
        ("ClipRect", |rect, clip| {
            ClipRectLayer::new(rect, clip).into()
        }),
        ("ClipRRect", |rect, clip| {
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 4.0), clip).into()
        }),
        ("ClipPath", |rect, clip| {
            ClipPathLayer::new(Path::rectangle(rect), clip).into()
        }),
        ("ClipSuperellipse", |rect, clip| {
            ClipSuperellipseLayer::new(RSuperellipse::from_rect_circular(rect, 4.0), clip).into()
        }),
    ];
    for (kind, layer) in clips {
        for (clip, name) in [
            (Clip::HardEdge, "HardEdge"),
            (Clip::AntiAliasWithSaveLayer, "AntiAliasWithSaveLayer"),
        ] {
            let tree = LayerTree::new(layer(rect, clip));
            let root = snapshot(&tree, SnapshotQuery::default()).expect("a tree has a root");
            assert_eq!(
                root.get::<Variant<'_>>("clip_behavior"),
                Ok(Variant { ty: "Clip", name }),
                "{kind}"
            );
        }
    }
}

/// A non-rectangular clip reports its exact shape: two clips with one bounding rectangle and one
/// edge quality but different shapes snapshot apart.
///
/// Fails when a rounded-rectangle, path or superellipse clip reports only its bounds.
#[test]
fn clip_layers_of_one_bounds_and_different_shapes_read_apart() {
    let rect = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let pairs: [(&str, Layer, Layer); 3] = [
        (
            "ClipRRect",
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 4.0), Clip::AntiAlias).into(),
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 8.0), Clip::AntiAlias).into(),
        ),
        (
            "ClipPath",
            ClipPathLayer::new(Path::rectangle(rect), Clip::AntiAlias).into(),
            ClipPathLayer::new(diamond(), Clip::AntiAlias).into(),
        ),
        (
            "ClipSuperellipse",
            ClipSuperellipseLayer::new(
                RSuperellipse::from_rect_circular(rect, 4.0),
                Clip::AntiAlias,
            )
            .into(),
            ClipSuperellipseLayer::new(
                RSuperellipse::from_rect_circular(rect, 8.0),
                Clip::AntiAlias,
            )
            .into(),
        ),
    ];
    for (kind, first, second) in pairs {
        let first = snapshot(&LayerTree::new(first), SnapshotQuery::default()).expect("a root");
        let second = snapshot(&LayerTree::new(second), SnapshotQuery::default()).expect("a root");
        assert_eq!(first.rect, second.rect, "{kind}: one bounding rectangle");
        assert_ne!(first, second, "{kind}");
    }
}

/// A diamond touching every edge of the 40×20 rectangle, so its bounds are that rectangle.
fn diamond() -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(20.0, 0.0));
    path.line_to(Point::new(40.0, 10.0));
    path.line_to(Point::new(20.0, 20.0));
    path.line_to(Point::new(0.0, 10.0));
    path.close();
    path
}
