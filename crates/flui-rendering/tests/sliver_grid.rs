//! `RenderSliverGrid` — oracle-derived golden tests.
//!
//! All expected values are derived from Flutter's `RenderSliverGrid.performLayout`
//! (`.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart:594-728`)
//! and verified independently in the plan comments.
//!
//! Primary scenario: vertical grid, `SliverGridDelegateWithFixedCrossAxisCount(2)`,
//! no spacing, aspect 1.0, `cross_axis_extent=200` → tiles are 100×100.
//! 8 children, `scroll_offset=100`, `remaining_paint_extent=200`,
//! `remaining_cache_extent=200`, `cache_origin=0`.
//!
//! In-band window: `[scroll_offset + cache_origin, scroll_offset + cache_origin +
//! remaining_cache_extent)` = `[100, 300)`.
//! - `first = get_min(100) = floor(100/100)*2 = 2`
//! - `last  = min(get_max(300), 7) = min(ceil(300/100)*2−1, 7) = min(5,7) = 5`
//!
//! Children 2..=5 are laid out; 0,1,6,7 are out of band.
//! Paint offsets (vertical forward, scroll_offset=100):
//!   child 2 → scroll_offset=100, cross=0   → Offset(0,   0)
//!   child 3 → scroll_offset=100, cross=100 → Offset(100, 0)
//!   child 4 → scroll_offset=200, cross=0   → Offset(0,   100)
//!   child 5 → scroll_offset=200, cross=100 → Offset(100, 100)
//!
//! SliverGeometry: scroll_extent=400, paint_extent=200, layout_extent=200,
//! max_paint_extent=400, cache_extent=200, hit_test_extent=200,
//! has_visual_overflow=true.

use std::sync::Arc;

use flui_foundation::Leaf;
use flui_foundation::geometry::{Rect, Size};
use flui_objects::RenderSliverGrid;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints},
    context::{BoxHitTestContext, BoxLayoutContext},
    delegates::SliverGridDelegateWithFixedCrossAxisCount,
    parent_data::{BoxParentData, SliverMultiBoxAdaptorParentData},
    pipeline::PipelineOwner,
    testing::sliver as sliver_presets,
    traits::RenderBox,
};

use crate::common::{BoxedRenderObject, BoxedSliverObject, sliver_geometry};

// ── helpers ──────────────────────────────────────────────────────────────────

/// A minimal hittable Box child that sizes to `desired` and accepts all hits
/// within its own bounds.
#[derive(Debug)]
struct FixedHitBox {
    desired: Size,
}

impl FixedHitBox {
    fn new(width: f64, height: f64) -> Self {
        Self {
            desired: Size::new(width, height),
        }
    }
}

impl flui_foundation::Diagnosticable for FixedHitBox {}

impl RenderBox for FixedHitBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        ctx.constraints().constrain(self.desired)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        ctx.is_within_bounds(Rect::from_origin_size(
            flui_foundation::geometry::Point::ZERO,
            ctx.own_size(),
        ))
    }
}

/// A Box root that drives one sliver child with fixed `SliverConstraints`.
#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
}

impl flui_foundation::Diagnosticable for SliverHost {}

impl RenderBox for SliverHost {
    type Arity = flui_foundation::Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, flui_foundation::Variable, Self::ParentData>,
    ) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<'_, flui_foundation::Variable, Self::ParentData>,
    ) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

/// Builds a pipeline tree, runs layout, and returns the laid-out owner plus IDs.
///
/// Returns `(owner, root_id, grid_sliver_id, child_ids)`.
fn build_grid_tree(
    constraints: SliverConstraints,
    delegate: Arc<dyn flui_rendering::delegates::SliverGridDelegate>,
    child_count: usize,
) -> (
    PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Vec<flui_foundation::RenderId>,
) {
    let mut owner = PipelineOwner::new();

    let root_id = owner.insert(Box::new(SliverHost { constraints }) as BoxedRenderObject);
    let grid_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverGrid::new(delegate, child_count)) as BoxedSliverObject,
        )
        .expect("grid sliver inserted");

    let mut child_ids = Vec::with_capacity(child_count);
    for index in 0..child_count {
        let child_id = owner
            .render_tree_mut()
            .insert_box_child(
                grid_id,
                Box::new(FixedHitBox::new(1000.0, 1000.0)) as BoxedRenderObject,
            )
            .expect("box child inserted");
        // The logical index the element tree stamps at adoption (mirrors
        // `crates/flui-rendering/tests/sliver_fixed_extent_list.rs`'s
        // `fixed_extent_tree`): the request-strategy grid reconciles resident
        // children by reading this back before its own layout pass, so it
        // must already be correct, not merely default-initialized.
        owner
            .render_tree_mut()
            .get_mut(child_id)
            .expect("just inserted")
            .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(index)));
        child_ids.push(child_id);
    }

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(200.0, 200.0))));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");

    (owner, root_id, grid_id, child_ids)
}

// ── oracle golden test ────────────────────────────────────────────────────────

/// Primary oracle scenario.
///
/// vertical grid, FixedCrossAxisCount(2), no spacing, aspect 1.0,
/// cross_axis_extent=200 → tiles 100×100.
/// 8 children, scroll_offset=100, remaining_paint_extent=200,
/// remaining_cache_extent=200, cache_origin=0.
fn primary_constraints() -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(100.0)
        .remaining_paint_extent(200.0)
        .cross_axis_extent(200.0)
        .viewport_main_axis_extent(200.0)
        .remaining_cache_extent(200.0)
        .build()
}

fn two_column_delegate() -> Arc<dyn flui_rendering::delegates::SliverGridDelegate> {
    Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2))
}

#[test]
fn sliver_grid_golden_geometry() {
    // Oracle: scroll_extent=400, paint_extent=200, layout_extent=200,
    // max_paint_extent=400, cache_extent=200, has_visual_overflow=true.
    let (owner, _root, grid, _children) =
        build_grid_tree(primary_constraints(), two_column_delegate(), 8);

    let geom = sliver_geometry(&owner, grid);

    assert_eq!(
        geom.scroll_extent, 400.0,
        "8 children / 2 cols = 4 rows × 100px stride = 400px total",
    );
    assert_eq!(
        geom.paint_extent, 200.0,
        "rows 1+2 within the 200px remaining_paint_extent",
    );
    assert_eq!(
        geom.layout_extent, 200.0,
        "layout_extent matches paint_extent for a basic grid",
    );
    assert_eq!(
        geom.max_paint_extent, 400.0,
        "max_paint_extent equals scroll_extent",
    );
    assert_eq!(
        geom.cache_extent, 200.0,
        "cache_extent matches remaining_cache_extent (no cache origin offset)",
    );
    assert!(
        geom.has_visual_overflow,
        "scroll_extent(400) > paint_extent(200) → has_visual_overflow must be true",
    );
}

// ── horizontal axis ───────────────────────────────────────────────────────────

// ── RTL mirror ───────────────────────────────────────────────────────────────

// ── cross-axis spacing ────────────────────────────────────────────────────────

// ── should_relayout on delegate swap ─────────────────────────────────────────

// ── empty grid ───────────────────────────────────────────────────────────────
