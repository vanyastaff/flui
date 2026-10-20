//! A layer snapshot reads each layer's own fields.

use flui_foundation::geometry::{RRect, RSuperellipse, Rect};
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
