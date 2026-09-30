//! Runtime validation for `SliverGeometry` layout results.

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    error::RenderError,
    parent_data::SliverParentData,
    pipeline::PipelineOwner,
    traits::RenderSliver,
    view::ScrollDirection,
};

use crate::common::BoxedSliverObject;

fn sliver_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 100.0,
        cross_axis_extent: 300.0,
        viewport_main_axis_extent: 100.0,
        remaining_cache_extent: 120.0,
        cache_origin: -20.0,
    }
}

/// Violates a PIPELINE-SAFETY rule (negative paint extent) — the class
/// `validate_layout_output` still rejects. The softer content-contract
/// rules (layout > paint, paint > max_paint) commit with a warning instead
/// — see `sliver_content_contract_violation_commits_and_stays_clean`.
fn invalid_negative_paint_geometry() -> SliverGeometry {
    SliverGeometry {
        scroll_extent: 100.0,
        paint_extent: -10.0,
        layout_extent: 0.0,
        max_paint_extent: 100.0,
        hit_test_extent: 0.0,
        cache_extent: 0.0,
        visible: true,
        ..SliverGeometry::ZERO
    }
}

fn assert_invalid_geometry(err: RenderError, expected_reason: &'static str) {
    match err {
        RenderError::InvalidGeometry {
            render_object: _,
            reason,
        } => assert_eq!(reason, expected_reason),
        other => panic!("expected InvalidGeometry, got {other:?}"),
    }
}

#[derive(Debug)]
struct BadGeometrySliver {
    /// The deliberately-invalid geometry this test double reports.
    geometry: SliverGeometry,
}

impl BadGeometrySliver {
    fn new(geometry: SliverGeometry) -> Self {
        Self { geometry }
    }
}

impl Diagnosticable for BadGeometrySliver {}

impl RenderSliver for BadGeometrySliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.geometry
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }
}

pub(crate) fn sliver_leaf_layout_rejects_invalid_geometry_before_state_commit() {
    let text = flui_rendering::TextContextHandle::standalone();
    let mut owner = PipelineOwner::new(text.clone());
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver(
            Box::new(BadGeometrySliver::new(invalid_negative_paint_geometry()))
                as BoxedSliverObject,
        );

    let entry = owner
        .render_tree_mut()
        .get_mut(sliver_id)
        .and_then(|node| node.as_sliver_mut())
        .expect("sliver entry");
    let err = entry
        .layout_leaf_only(sliver_constraints(), text.source())
        .expect_err("invalid sliver geometry must fail layout");

    assert_invalid_geometry(err, "paint_extent is negative");
    assert!(
        entry.state().geometry().is_none(),
        "invalid geometry must not be committed to RenderState"
    );
    assert!(
        entry.needs_layout(),
        "failed sliver layout must stay dirty for retry"
    );
}
