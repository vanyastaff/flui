//! Unit tests for `RenderState<P>` storage primitives.
//!
//! The propagation tests + `MockTree` helper that previously lived here were
//! deleted during the flui-rendering Phase 1 zombie cleanup
//! (`docs/plans/2026-05-20-005-refactor-flui-rendering-zombie-cleanup-plan.md`)
//! because the `RenderState::mark_needs_*` methods they exercised were
//! unreachable in production. Production dirty marking goes through
//! `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked
//! from `flui-view` and `flui-hot-reload`. Coverage of the real production
//! path is tracked separately under Mythos audit Step 4 item 13.

use std::mem::size_of;

use flui_types::{Offset, geometry::px};

use super::*;
use crate::protocol::{BoxProtocol, SliverProtocol};

// ========================================================================
// Static memory-footprint assertions
// ========================================================================
//
// These tests guard the data-oriented design budgets documented in
// `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 9.
// If a future change blows up the per-node size, these tests fail
// loudly rather than the regression sneaking in unobserved.

#[test]
fn render_state_box_fits_budget() {
    // RenderState<BoxProtocol> = AtomicRenderFlags(4) + Option<Size>
    // + Option<BoxConstraints> + AtomicOffset(8) + layout_cache
    // + parent_data + PhantomData(0).
    // Documented estimate: 44-60 bytes for the core fields. Cap at 128
    // to leave room for future fields without forcing a re-budget on every commit.
    let actual = size_of::<RenderState<BoxProtocol>>();
    assert!(
        actual <= 128,
        "RenderState<BoxProtocol> grew beyond budget: {actual} bytes (cap 128)"
    );
}

#[test]
fn render_state_sliver_fits_budget() {
    let actual = size_of::<RenderState<SliverProtocol>>();
    assert!(
        actual <= 192,
        "RenderState<SliverProtocol> grew beyond budget: {actual} bytes (cap 192)"
    );
}

#[test]
fn test_geometry_set_is_idempotent() {
    // `set_geometry` previously panicked on second invocation
    // (OnceCell-backed). Re-layout now overwrites cleanly mirroring
    // Flutter `_size = size` straight assignment.
    let mut state = BoxRenderState::new();
    let size1 = flui_types::Size::new(px(100.0), px(50.0));
    let size2 = flui_types::Size::new(px(200.0), px(100.0));

    // First set establishes geometry.
    state.set_geometry(size1);
    assert_eq!(state.geometry(), Some(size1));

    // Second set overwrites with no panic — re-layout safe.
    state.set_geometry(size2);
    assert_eq!(state.geometry(), Some(size2));

    // Clear resets to None.
    state.clear_geometry();
    assert_eq!(state.geometry(), None);
}

#[test]
fn test_atomic_offset() {
    let state = BoxRenderState::new();
    let offset = Offset::new(px(10.0), px(20.0));

    state.set_offset(offset);
    assert_eq!(state.offset(), offset);

    // Can update multiple times
    let offset2 = Offset::new(px(30.0), px(40.0));
    state.set_offset(offset2);
    assert_eq!(state.offset(), offset2);
}

#[test]
fn test_boundary_flags() {
    let state = BoxRenderState::new();

    assert!(!state.is_relayout_boundary());
    assert!(!state.is_repaint_boundary());

    state.set_relayout_boundary(true);
    assert!(state.is_relayout_boundary());

    state.set_repaint_boundary(true);
    assert!(state.is_repaint_boundary());

    state.set_relayout_boundary(false);
    assert!(!state.is_relayout_boundary());
    assert!(state.is_repaint_boundary());
}

/// Regression test: a non-root node with loose (non-tight) constraints and
/// `sized_by_parent = false` must NOT default to a relayout boundary.
/// Previously the relayout-boundary bootstrap passed
/// `parent_uses_size = false` which made `!parent_uses_size = true`,
/// flipping every Box node into a boundary and silently blocking
/// `PipelineOwner::mark_needs_layout` propagation at the leaf.
#[test]
fn compute_relayout_boundary_non_tight_non_root_is_not_boundary_by_default() {
    use crate::constraints::BoxConstraints;
    use flui_types::Size;

    let mut state = BoxRenderState::new();
    // Loose constraints (not tight) — typical layout-from-parent case.
    let loose = BoxConstraints::loose(Size::new(px(200.0), px(100.0)));
    state.set_constraints(loose);

    // Bootstrap as if running under the relayout-boundary bootstrap with
    // the BoxProtocol override: parent_uses_size=true (conservative),
    // sized_by_parent=false, has_parent=true (non-root).
    state.compute_relayout_boundary(true, false, true);

    assert!(
        !state.is_relayout_boundary(),
        "non-tight, non-root, non-sized-by-parent node MUST default to non-boundary so mark_needs_layout propagates up to a real boundary",
    );
}

/// Companion test: tight constraints still mark as boundary (Flutter parity:
/// when parent forces a single valid size, the node can re-layout in
/// isolation).
#[test]
fn compute_relayout_boundary_tight_constraints_is_boundary() {
    use crate::constraints::BoxConstraints;
    use flui_types::Size;

    let mut state = BoxRenderState::new();
    let tight = BoxConstraints::tight(Size::new(px(50.0), px(50.0)));
    state.set_constraints(tight);

    state.compute_relayout_boundary(true, false, true);

    assert!(
        state.is_relayout_boundary(),
        "tight constraints mean parent forces a single valid size — node is a boundary",
    );
}

/// Companion test: root (no parent) is always a boundary regardless of
/// other signals — propagation cannot escape the root.
#[test]
fn compute_relayout_boundary_root_is_always_boundary() {
    let state = BoxRenderState::new();
    state.compute_relayout_boundary(true, false, /* has_parent = */ false);
    assert!(
        state.is_relayout_boundary(),
        "root must always be a boundary"
    );
}

// ========================================================================
// Construction, parent data, and layout-cache accessors
// ========================================================================

#[test]
fn with_flags_seeds_exactly_the_given_flags() {
    // `new()` always seeds NEEDS_LAYOUT | NEEDS_PAINT; `with_flags` must be
    // able to bypass that default for tests/special init (e.g. a state that
    // starts already laid out).
    let clean = BoxRenderState::with_flags(RenderFlags::empty());
    assert!(!clean.needs_layout());
    assert!(!clean.needs_paint());

    let dirty = BoxRenderState::with_flags(RenderFlags::NEEDS_LAYOUT);
    assert!(dirty.needs_layout());
    assert!(!dirty.needs_paint());
}

#[test]
fn default_matches_new() {
    let default_state = BoxRenderState::default();
    let new_state = BoxRenderState::new();

    assert_eq!(default_state.needs_layout(), new_state.needs_layout());
    assert_eq!(default_state.needs_paint(), new_state.needs_paint());
    assert_eq!(default_state.geometry(), new_state.geometry());
    assert_eq!(default_state.offset(), new_state.offset());
    assert!(default_state.parent_data().is_none());
}

#[test]
fn parent_data_round_trips_through_typed_and_erased_accessors() {
    use crate::parent_data::BoxParentData;

    let mut state = BoxRenderState::new();
    assert!(state.parent_data().is_none());
    assert!(state.parent_data_as::<BoxParentData>().is_none());

    let offset = Offset::new(px(3.0), px(4.0));
    state.set_parent_data(Box::new(BoxParentData::new(offset)));

    // Type-erased read.
    assert!(state.parent_data().is_some());

    // Typed downcast read.
    let typed = state
        .parent_data_as::<BoxParentData>()
        .expect("parent data was just set to BoxParentData");
    assert_eq!(typed.offset, offset);

    // Typed mutation round-trips.
    state
        .parent_data_as_mut::<BoxParentData>()
        .expect("parent data is BoxParentData")
        .offset = Offset::new(px(7.0), px(8.0));
    assert_eq!(
        state.parent_data_as::<BoxParentData>().unwrap().offset,
        Offset::new(px(7.0), px(8.0))
    );

    // Erased mutation reaches the same storage as the typed accessors.
    state
        .parent_data_mut()
        .expect("parent data is set")
        .downcast_mut::<BoxParentData>()
        .expect("still BoxParentData")
        .offset = Offset::new(px(1.0), px(2.0));
    assert_eq!(
        state.parent_data_as::<BoxParentData>().unwrap().offset,
        Offset::new(px(1.0), px(2.0))
    );

    // Replacing with a new value overwrites rather than accumulating state.
    state.set_parent_data(Box::new(BoxParentData::zero()));
    assert_eq!(
        state.parent_data_as::<BoxParentData>().unwrap().offset,
        Offset::ZERO
    );
}

#[test]
fn parent_data_as_returns_none_for_mismatched_type() {
    use crate::parent_data::{BoxParentData, SliverLogicalParentData};

    let mut state = BoxRenderState::new();
    state.set_parent_data(Box::new(BoxParentData::zero()));

    // Downcasting to an unrelated concrete ParentData type must fail cleanly,
    // not panic — this is the guard the parent-side typed read path relies on.
    assert!(state.parent_data_as::<SliverLogicalParentData>().is_none());
    assert!(
        state
            .parent_data_as_mut::<SliverLogicalParentData>()
            .is_none()
    );
}

#[test]
fn layout_cache_insert_peek_and_clear_round_trip() {
    let mut state = BoxRenderState::new();

    assert!(
        state
            .layout_cache()
            .peek_intrinsic(IntrinsicDimension::MinWidth, 100.0)
            .is_none()
    );

    state
        .layout_cache_mut()
        .insert_intrinsic(IntrinsicDimension::MinWidth, 100.0, 42.0);
    assert_eq!(
        state
            .layout_cache()
            .peek_intrinsic(IntrinsicDimension::MinWidth, 100.0),
        Some(42.0)
    );

    // A miss (different extent) must not spuriously hit the memoized entry.
    assert!(
        state
            .layout_cache()
            .peek_intrinsic(IntrinsicDimension::MinWidth, 200.0)
            .is_none()
    );

    // Clearing a populated cache reports that something WAS cached (the
    // pipeline uses this to decide whether to escalate invalidation past a
    // relayout boundary — box.dart:2840).
    assert!(state.clear_layout_cache());
    assert!(
        state
            .layout_cache()
            .peek_intrinsic(IntrinsicDimension::MinWidth, 100.0)
            .is_none()
    );

    // Clearing an already-empty cache reports nothing was cached.
    assert!(!state.clear_layout_cache());
}

#[test]
fn clone_preserves_geometry_constraints_offset_and_parent_data_but_resets_layout_cache() {
    use crate::constraints::BoxConstraints;
    use crate::parent_data::BoxParentData;
    use flui_types::Size;

    let mut state = BoxRenderState::new();
    let size = Size::new(px(30.0), px(40.0));
    let constraints = BoxConstraints::tight(size);
    let offset = Offset::new(px(5.0), px(6.0));

    state.set_geometry(size);
    state.set_constraints(constraints);
    state.set_offset(offset);
    state.set_parent_data(Box::new(BoxParentData::new(offset)));
    state
        .layout_cache_mut()
        .insert_intrinsic(IntrinsicDimension::MinWidth, 100.0, 1.0);

    let cloned = state.clone();

    assert_eq!(cloned.geometry(), Some(size));
    assert_eq!(cloned.constraints(), Some(&constraints));
    assert_eq!(cloned.offset(), offset);
    assert_eq!(
        cloned.parent_data_as::<BoxParentData>().unwrap().offset,
        offset
    );

    // Memoized layout-cache results are node-local; a clone must start cold
    // rather than sharing (or duplicating) the source node's cached entries.
    assert!(
        cloned
            .layout_cache()
            .peek_intrinsic(IntrinsicDimension::MinWidth, 100.0)
            .is_none()
    );
}

// ========================================================================
// Box- and Sliver-protocol geometry convenience methods
// ========================================================================

#[test]
fn box_size_and_has_size_use_zero_fallback_before_layout() {
    let mut state = BoxRenderState::new();

    // Before the first layout, `size()` must fall back to ZERO rather than
    // panicking, and `has_size` must not falsely match ZERO.
    assert_eq!(state.size(), flui_types::Size::ZERO);
    assert!(!state.has_size(flui_types::Size::ZERO));

    let size = flui_types::Size::new(px(64.0), px(32.0));
    state.set_size(size);
    assert_eq!(state.size(), size);
    assert!(state.has_size(size));
    assert!(!state.has_size(flui_types::Size::new(px(1.0), px(1.0))));
}

#[test]
fn sliver_extent_accessors_use_zero_fallback_before_layout() {
    let state = SliverRenderState::new();

    assert_eq!(state.scroll_extent(), 0.0);
    assert_eq!(state.paint_extent(), 0.0);
    assert_eq!(state.layout_extent(), 0.0);
    assert_eq!(state.max_paint_extent(), 0.0);
}

#[test]
fn set_sliver_geometry_populates_all_extent_accessors() {
    use crate::constraints::SliverGeometry;

    let mut state = SliverRenderState::new();
    state.set_sliver_geometry(SliverGeometry {
        scroll_extent: 1000.0,
        paint_extent: 400.0,
        layout_extent: 350.0,
        max_paint_extent: 500.0,
        ..SliverGeometry::ZERO
    });

    assert_eq!(state.scroll_extent(), 1000.0);
    assert_eq!(state.paint_extent(), 400.0);
    assert_eq!(state.layout_extent(), 350.0);
    assert_eq!(state.max_paint_extent(), 500.0);
}

#[test]
fn absolute_paint_size_is_zero_before_layout() {
    let state = SliverRenderState::new();
    assert_eq!(state.absolute_paint_size(), flui_types::Size::ZERO);
}

#[test]
fn absolute_paint_size_maps_main_axis_to_height_for_vertical_scroll() {
    use crate::constraints::{SliverConstraints, SliverGeometry};
    use flui_types::prelude::AxisDirection;

    let mut state = SliverRenderState::new();
    state.set_sliver_geometry(SliverGeometry {
        paint_extent: 80.0,
        ..SliverGeometry::ZERO
    });
    state.set_constraints(SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_extent: 120.0,
        ..SliverConstraints::default()
    });

    // Vertical scroll: main axis (paint_extent) is height, cross axis is width.
    assert_eq!(
        state.absolute_paint_size(),
        flui_types::Size::new(px(120.0), px(80.0))
    );
}

#[test]
fn absolute_paint_size_maps_main_axis_to_width_for_horizontal_scroll() {
    use crate::constraints::{SliverConstraints, SliverGeometry};
    use flui_types::prelude::AxisDirection;

    let mut state = SliverRenderState::new();
    state.set_sliver_geometry(SliverGeometry {
        paint_extent: 80.0,
        ..SliverGeometry::ZERO
    });
    state.set_constraints(SliverConstraints {
        axis_direction: AxisDirection::LeftToRight,
        cross_axis_extent: 120.0,
        ..SliverConstraints::default()
    });

    // Horizontal scroll: main axis (paint_extent) is width, cross axis is height.
    assert_eq!(
        state.absolute_paint_size(),
        flui_types::Size::new(px(80.0), px(120.0))
    );
}
