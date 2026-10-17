//! The headless harness can report what a frame actually **composited**, not
//! just what it built.
//!
//! Layers are produced by *paint*, so they answer questions the render tree
//! structurally cannot: whether a widget forced a clip, a transform, or an
//! opacity layer into the output, and how many. Upstream's widget tests lean on
//! this constantly (`tester.layers`, and the `getLayers()` container-chain walk
//! `fitted_box_test.dart` defines locally).
//!
//! The pipeline always produced the tree — `HeadlessBinding::pump_frame` simply
//! dropped it on the floor, because headlessly there is no compositor to hand it
//! to. These tests pin that it is kept and reachable.

use crate::common::{lay_out, tight};
use flui_foundation::geometry::Matrix4;
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, SizedBox, Transform};

/// The structural form: [`LaidOut::layer_tree`] exposes parent/child shape, not
/// just a flat list.
///
/// Upstream's `fitted_box_test.dart` needs exactly this — its local
/// `getLayers()` walks a single-child *container chain* from the root and
/// asserts `firstChild == lastChild` at every step, which a flattened list
/// cannot express. This pins that the shape is reachable, so that helper is
/// portable when the features it exercises land.
pub(crate) fn the_composited_tree_exposes_parent_child_shape_not_just_a_flat_list() {
    let mut laid = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0))
            .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        tight(200.0, 200.0),
    );
    laid.pump();

    let tree = laid.layer_tree().expect("a pumped frame composites a tree");
    let root = tree.root();
    assert!(
        tree.len() > 1,
        "this tree composites more than the root alone; got {} layer(s)",
        tree.len()
    );
    assert_eq!(
        tree.parent(root),
        None,
        "the root layer must have no parent"
    );

    let children = tree
        .children(root)
        .expect("the root of a multi-layer tree has children");
    assert!(
        !children.is_empty(),
        "the root must actually parent the layers below it — a flat list of the \
         right length would not prove the tree is linked"
    );
    for &child in children {
        assert_eq!(
            tree.parent(child),
            Some(root),
            "every child's parent link must point back at the root"
        );
    }
}

/// `ClipRSuperellipse` composites a superellipse clip of its laid-out box,
/// and a new border radius reaches that layer on update.
pub(crate) fn a_superellipse_clip_composites_and_updates_its_radius() {
    use flui_foundation::geometry::{RSuperellipse, Radius, Rect};
    use flui_painting::styling::BorderRadius;
    use flui_widgets::ClipRSuperellipse;

    let clip = |radius: f64| {
        let corner = Radius::circular(radius);
        ClipRSuperellipse::new()
            .border_radius(BorderRadius {
                top_left: corner,
                top_right: corner,
                bottom_right: corner,
                bottom_left: corner,
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
    };
    let shapes = |laid: &crate::common::LaidOut| {
        laid.layer_tree()
            .expect("a pumped frame composites a tree")
            .iter()
            .filter_map(|(_, node)| match node.layer() {
                flui_rendering::layer::Layer::ClipSuperellipse(layer) => {
                    Some(*layer.clip_superellipse())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let bounds = Rect::from_xywh(0.0, 0.0, 60.0, 40.0);
    let mut laid = lay_out(clip(12.0), tight(60.0, 40.0));
    laid.pump();
    assert_eq!(
        shapes(&laid),
        [RSuperellipse::from_rect_circular(bounds, 12.0)]
    );

    laid.pump_widget(clip(4.0));
    assert_eq!(
        shapes(&laid),
        [RSuperellipse::from_rect_circular(bounds, 4.0)]
    );
}

/// Grouped filters under one `BackdropGroup` share its identity, which stays
/// the same across a rebuild; an ungrouped filter, or a grouped one with no
/// group above it, samples the live backdrop.
pub(crate) fn backdrop_filters_in_one_group_share_a_stable_identity() {
    use flui_painting::paint::ImageFilter;
    use flui_view::ViewExt;
    use flui_widgets::{BackdropFilter, BackdropGroup, Column, SizedBox};

    let pane = |grouped: bool| {
        let filter = BackdropFilter::new(ImageFilter::blur(4.0));
        let filter = if grouped { filter.grouped() } else { filter };
        filter.child(SizedBox::new(60.0, 10.0))
    };
    let scene = |blur: f64| {
        Column::new(vec![
            BackdropGroup::new(Column::new(vec![
                pane(true).boxed(),
                pane(true).boxed(),
                pane(false).boxed(),
                BackdropFilter::new(ImageFilter::blur(blur))
                    .grouped()
                    .child(SizedBox::new(60.0, 10.0))
                    .boxed(),
            ]))
            .boxed(),
            pane(true).boxed(),
        ])
    };
    let groups = |laid: &crate::common::LaidOut| {
        laid.layer_tree()
            .expect("a pumped frame composites a tree")
            .iter()
            .filter_map(|(_, node)| match node.layer() {
                flui_rendering::layer::Layer::BackdropFilter(layer) => Some(layer.group().cloned()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let mut laid = lay_out(scene(4.0), tight(60.0, 100.0));
    laid.pump();
    let first = groups(&laid);
    let shared = first[0].clone().expect("a grouped filter joins its group");
    assert_eq!(
        first,
        [
            Some(shared.clone()),
            Some(shared.clone()),
            None,
            Some(shared),
            None
        ]
    );

    laid.pump_widget(scene(6.0));
    assert_eq!(
        groups(&laid),
        first,
        "rebuilding keeps the group's identity"
    );
}
