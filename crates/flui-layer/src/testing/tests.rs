//! Self-tests for the layer-tree harness.

use flui_painting::{Canvas, Paint};
use flui_types::{Offset, Rect, geometry::px, styling::Color};

use crate::{
    CanvasLayer, OffsetLayer, PictureLayer,
    testing::{LayerTester, inspect, layer},
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

// ── Task 5: Layer::to_diagnostics_node typed props + PictureLayer children ───

/// PictureLayer node has name "Picture", a typed `bounds` property, and one
/// child node per `DrawCommand` in the picture.
///
/// The child node name is `"DrawCommand"` — `DrawCommand` is an enum and the
/// default `Diagnosticable::to_diagnostics_node` strips the type name to its
/// last segment.  The typed `rect` property on the child identifies which
/// variant it represents.
#[test]
fn picture_layer_node_has_command_children() {
    use flui_foundation::{Diagnosticable, DiagnosticsProperty, DiagnosticsValue};
    use flui_painting::Paint;
    use flui_types::styling::Color;

    let mut canvas = flui_painting::Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
        &Paint::fill(Color::RED),
    );
    let picture = PictureLayer::new(canvas.finish());
    let layer = crate::Layer::Picture(Box::new(picture));

    let node = layer.to_diagnostics_node();

    assert_eq!(node.name(), Some("Picture"), "name must be Picture");

    // Must carry a typed bounds property.
    let bounds_prop = node
        .find_property("bounds")
        .expect("Picture node must have a bounds property");
    assert!(
        matches!(bounds_prop.value_typed(), DiagnosticsValue::Rect { .. }),
        "bounds property must be DiagnosticsValue::Rect, got {:?}",
        bounds_prop.value_typed()
    );

    // Must carry one child node per draw command.
    let children = node.children();
    assert_eq!(children.len(), 1, "one DrawRect command → one child node");
    // DrawCommand uses per-variant node names (FIX 1); a DrawRect is named
    // "DrawRect", not the generic enum type name "DrawCommand".
    assert_eq!(
        children[0].name(),
        Some("DrawRect"),
        "child node name must be the per-variant name DrawRect"
    );
    assert!(
        children[0].find_property("rect").is_some(),
        "DrawRect child node must carry a rect property"
    );
    assert!(
        matches!(
            children[0]
                .find_property("rect")
                .map(DiagnosticsProperty::value_typed),
            Some(DiagnosticsValue::Rect { .. })
        ),
        "DrawRect child node rect property must be a typed Rect"
    );
}

/// ClipRectLayer node has name "ClipRect", a typed Rect `rect` property,
/// and a `clip` property carrying the behavior string.
#[test]
fn clip_rect_layer_node_has_typed_rect_and_clip() {
    use flui_foundation::{Diagnosticable, DiagnosticsValue};
    use flui_types::painting::Clip;

    let clip_rect = Rect::from_xywh(px(5.0), px(10.0), px(100.0), px(80.0));
    let layer = crate::Layer::ClipRect(crate::ClipRectLayer::new(clip_rect, Clip::HardEdge));

    let node = layer.to_diagnostics_node();

    assert_eq!(node.name(), Some("ClipRect"), "name must be ClipRect");

    // rect property must be a typed Rect.
    let rect_prop = node
        .find_property("rect")
        .expect("ClipRect node must have a rect property");
    assert!(
        matches!(rect_prop.value_typed(), DiagnosticsValue::Rect { .. }),
        "rect property must be DiagnosticsValue::Rect, got {:?}",
        rect_prop.value_typed()
    );

    // clip property must be present.
    assert!(
        node.find_property("clip").is_some(),
        "ClipRect node must have a clip property"
    );
}

/// OffsetLayer node has name "Offset" and typed Float `dx` / `dy` properties.
#[test]
fn offset_layer_node_has_dx_dy() {
    use flui_foundation::{Diagnosticable, DiagnosticsValue};

    let layer = crate::Layer::Offset(crate::OffsetLayer::from_xy(12.0, 34.0));
    let node = layer.to_diagnostics_node();

    assert_eq!(node.name(), Some("Offset"), "name must be Offset");

    let dx = node
        .find_property("dx")
        .expect("Offset node must have a dx property");
    assert!(
        matches!(dx.value_typed(), DiagnosticsValue::Float(_)),
        "dx must be DiagnosticsValue::Float, got {:?}",
        dx.value_typed()
    );

    let dy = node
        .find_property("dy")
        .expect("Offset node must have a dy property");
    assert!(
        matches!(dy.value_typed(), DiagnosticsValue::Float(_)),
        "dy must be DiagnosticsValue::Float, got {:?}",
        dy.value_typed()
    );
}
