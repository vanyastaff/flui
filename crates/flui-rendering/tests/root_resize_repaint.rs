//! Root resize → relayout → repaint round-trip.
//!
//! Pins the two halves of the resize contract that the colored-box
//! e2e exposed as broken:
//!
//! 1. the binding-set `root_constraints` are AUTHORITATIVE for the
//!    root — a stale cached-constraints hit must not win on resize;
//! 2. `RenderViewAdapter` sizes from the INCOMING constraints, not
//!    from its mount-time `ViewConfiguration` snapshot.
//!
//! Failure mode being prevented: stretch the window and the newly
//! exposed area stays unpainted forever.

use flui_layer::Layer;
use flui_painting::DisplayListCore;
use flui_rendering::{
    constraints::BoxConstraints,
    objects::RenderColoredBox,
    pipeline::PipelineOwner,
    view::{RenderView, RenderViewAdapter, ViewConfiguration},
};
use flui_types::{Size, geometry::px};

fn run_frame_sizes(
    owner: flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Idle>,
) -> (
    Size,
    Size,
    flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Idle>,
) {
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint");

    let tree = owner
        .take_layer_tree()
        .expect("paint must produce a layer tree");
    // DFS to the first picture; its bounds reflect what was painted.
    fn picture_size(tree: &flui_layer::LayerTree, id: flui_foundation::LayerId) -> Option<Size> {
        let node = tree.get(id)?;
        if let Layer::Picture(p) = node.layer() {
            let b = p.picture().bounds();
            return Some(Size::new(b.width(), b.height()));
        }
        node.children().iter().find_map(|&c| picture_size(tree, c))
    }
    let painted = picture_size(&tree, tree.root().expect("root layer")).expect("picture layer");

    let root_id = owner.root_id().expect("root id set");
    let root_geometry = owner
        .render_tree()
        .get(root_id)
        .and_then(|n| n.geometry_box())
        .expect("root geometry computed");

    (root_geometry, painted, owner.into_idle())
}

#[test]
fn resize_relays_out_and_repaints_at_the_new_size() {
    let mut owner = PipelineOwner::new();

    // Real production root: RenderViewAdapter bootstrapped the way
    // RootRenderElement::mount does it.
    let mut render_view = RenderView::new();
    render_view.set_configuration(ViewConfiguration::from_size(
        Size::new(px(100.0), px(100.0)),
        1.0,
    ));
    render_view.prepare_initial_frame_without_owner();
    let root_id = owner.insert(Box::new(RenderViewAdapter::new(render_view))
        as Box<
            dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
        >);
    owner
        .insert_child_render_object(root_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored child insert");
    owner.set_root_id(Some(root_id));

    // Frame 1 at 100×100.
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    let (geometry, painted, mut owner) = run_frame_sizes(owner);
    assert_eq!(
        geometry,
        Size::new(px(100.0), px(100.0)),
        "frame 1: root sizes from the incoming constraints",
    );
    assert_eq!(
        painted,
        Size::new(px(100.0), px(100.0)),
        "frame 1: the child fills the tight root constraints",
    );

    // Resize to 300×200 — set_root_constraints marks the root dirty.
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(200.0)))));
    let (geometry, painted, _owner) = run_frame_sizes(owner);
    assert_eq!(
        geometry,
        Size::new(px(300.0), px(200.0)),
        "resize: root_constraints are authoritative — a stale \
         cached-constraints hit must not relayout at the old size",
    );
    assert_eq!(
        painted,
        Size::new(px(300.0), px(200.0)),
        "resize: the repaint covers the NEW size (the mount-time \
         ViewConfiguration snapshot must not cap the painted area)",
    );
}

/// Unbounded root constraints are a binding bug, not a layout input —
/// the adapter must surface a typed error instead of letting
/// `constraints.biggest()` poison `view.size` (and every downstream
/// geometry) with INF in release builds.
#[test]
fn unbounded_root_constraints_surface_a_typed_error() {
    let mut owner = PipelineOwner::new();

    let mut render_view = RenderView::new();
    render_view.set_configuration(ViewConfiguration::from_size(
        Size::new(px(100.0), px(100.0)),
        1.0,
    ));
    render_view.prepare_initial_frame_without_owner();
    let root_id = owner.insert(Box::new(RenderViewAdapter::new(render_view))
        as Box<
            dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
        >);
    owner
        .insert_child_render_object(root_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored child insert");
    owner.set_root_id(Some(root_id));

    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(f32::INFINITY),
        px(0.0),
        px(f32::INFINITY),
    )));

    let mut owner = owner.into_layout();
    let err = owner
        .run_layout()
        .expect_err("an unbounded root must fail layout with a diagnosable error");
    assert!(
        matches!(
            err,
            flui_rendering::error::RenderError::UnboundedConstraint { .. }
        ),
        "expected UnboundedConstraint, got {err:?}",
    );
}
