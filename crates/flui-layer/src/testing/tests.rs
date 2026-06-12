//! Self-tests for the layer-tree harness.

use flui_painting::{Canvas, Paint};
use flui_types::{geometry::px, styling::Color, Offset, Rect};

use crate::{
    testing::{inspect, layer, LayerTester},
    CanvasLayer, OffsetLayer, PictureLayer,
};

fn picture_0_0_40_40() -> PictureLayer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
        &Paint::fill(Color::RED),
    );
    PictureLayer::new(canvas.finish())
}

#[test]
fn structure_label_and_kind() {
    let probe = LayerTester::mount(
        layer(OffsetLayer::new(Offset::new(px(5.0), px(5.0))))
            .child(layer(CanvasLayer::new()).label("canvas")),
    );

    assert_eq!(probe.structure(), vec!["Offset", "Canvas"]);
    assert_eq!(probe.kind(probe.id("canvas")), "Canvas");
    assert_eq!(probe.kind(probe.root()), "Offset");
}

#[test]
fn structure_with_depth_nests() {
    let probe = LayerTester::mount(
        layer(OffsetLayer::new(Offset::ZERO))
            .child(layer(OffsetLayer::new(Offset::ZERO)).child(layer(CanvasLayer::new()))),
    );

    assert_eq!(
        probe.structure_with_depth(),
        vec![(0, "Offset"), (1, "Offset"), (2, "Canvas")],
    );
}

#[test]
fn first_picture_bounds_reads_record_time_bounds() {
    let probe =
        LayerTester::mount(layer(OffsetLayer::new(Offset::ZERO)).child(layer(picture_0_0_40_40())));

    assert_eq!(
        probe.first_picture_bounds(),
        Some(Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0))),
    );
}

#[test]
fn diagnostics_dump_names_each_layer_and_carries_properties() {
    let probe = LayerTester::mount(
        layer(OffsetLayer::new(Offset::new(px(5.0), px(5.0)))).child(layer(picture_0_0_40_40())),
    );

    let tree = probe.diagnostics().expect("diagnostics tree");
    assert_eq!(tree.name(), Some("Offset"), "root names the Offset layer");
    let picture = tree
        .find_child("Picture")
        .expect("the offset layer has a picture child");
    // The Picture self-describes its record-time bounds.
    assert!(
        picture.get_property("bounds").is_some(),
        "picture must carry a bounds property",
    );
}

#[test]
fn inspect_free_fns_match_probe() {
    let probe =
        LayerTester::mount(layer(OffsetLayer::new(Offset::ZERO)).child(layer(CanvasLayer::new())));
    assert_eq!(inspect::structure(probe.tree()), probe.structure());
    assert!(inspect::diagnostics_tree(probe.tree()).is_some());
}
