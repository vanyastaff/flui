//! What a node's render state keeps of its last layout, and what a slot of
//! the render slab costs.

use flui_foundation::CreationSite;
use flui_foundation::geometry::Size;
use flui_rendering::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};
use flui_rendering::storage::{BoxRenderState, RenderNode, SliverRenderState};

/// Every render node occupies one slot this size, whatever its protocol, so
/// the bound is what each control pays per render node before its object's
/// own allocation. A debug build keeps each node's creation site.
#[test]
fn render_node_slot_stays_within_its_budget() {
    let site = if CreationSite::KEPT { 8 } else { 0 };
    assert!(
        size_of::<RenderNode>() <= 216 + site,
        "RenderNode grew to {} bytes",
        size_of::<RenderNode>()
    );
}

#[test]
fn sliver_state_keeps_geometry_and_constraints_apart() {
    let mut state = SliverRenderState::new();
    assert!(state.geometry().is_none());
    assert!(state.constraints().is_none());

    // Clearing a record nothing was written to leaves it empty.
    state.clear_geometry();
    state.clear_constraints();
    assert!(state.geometry().is_none());
    assert!(state.constraints().is_none());

    let constraints = SliverConstraints {
        remaining_paint_extent: 300.0,
        ..SliverConstraints::default()
    };
    state.set_constraints(constraints);
    assert_eq!(state.constraints(), Some(&constraints));
    assert!(state.geometry().is_none());

    let geometry = SliverGeometry {
        scroll_extent: 500.0,
        paint_extent: 300.0,
        layout_extent: 300.0,
        max_paint_extent: 500.0,
        visible: true,
        ..SliverGeometry::default()
    };
    state.set_geometry(geometry);
    assert_eq!(state.geometry(), Some(geometry));

    let copy = state.clone();
    assert_eq!(copy.geometry(), Some(geometry));
    assert_eq!(copy.constraints(), Some(&constraints));

    state.clear_geometry();
    assert!(state.geometry().is_none());
    assert_eq!(state.constraints(), Some(&constraints));

    state.clear_constraints();
    assert!(state.constraints().is_none());
    state.set_geometry(geometry);
    assert_eq!(state.geometry(), Some(geometry));
    assert!(state.constraints().is_none());
}

#[test]
fn box_state_keeps_size_and_constraints_apart() {
    let mut state = BoxRenderState::new();
    assert!(state.geometry().is_none());
    assert!(state.constraints().is_none());

    // A zero size and zero constraints are values, not absence.
    state.set_constraints(BoxConstraints::ZERO);
    assert_eq!(state.constraints(), Some(&BoxConstraints::ZERO));
    assert!(state.geometry().is_none());
    state.set_geometry(Size::ZERO);
    assert_eq!(state.geometry(), Some(Size::ZERO));

    let constraints = BoxConstraints::tight(Size::new(40.0, 20.0));
    state.set_constraints(constraints);
    state.set_geometry(Size::new(40.0, 20.0));
    let copy = state.clone();
    assert_eq!(copy.geometry(), Some(Size::new(40.0, 20.0)));
    assert_eq!(copy.constraints(), Some(&constraints));

    state.clear_constraints();
    assert!(state.constraints().is_none());
    assert_eq!(state.geometry(), Some(Size::new(40.0, 20.0)));
    state.clear_geometry();
    assert!(state.geometry().is_none());
}
