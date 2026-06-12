//! Minimal leaf sliver used as a trait-method probe in unit tests.

use flui_foundation::Diagnosticable;
use flui_tree::Leaf;

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

const fn empty_sliver_constraints() -> SliverConstraints {
    use flui_types::layout::AxisDirection;

    use crate::constraints::GrowthDirection;
    use crate::view::ScrollDirection;

    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 0.0,
        cross_axis_extent: 0.0,
        cross_axis_direction: AxisDirection::LeftToRight,
        viewport_main_axis_extent: 0.0,
        remaining_cache_extent: 0.0,
        cache_origin: 0.0,
    }
}

/// Leaf sliver with zero geometry, used only to satisfy trait child references.
#[derive(Debug, Default)]
pub struct NoopSliver;

impl Diagnosticable for NoopSliver {}
impl PaintEffectsCapability for NoopSliver {}
impl SemanticsCapability for NoopSliver {}
impl HotReloadCapability for NoopSliver {}

impl RenderSliver for NoopSliver {
    type Arity = Leaf;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Self::Arity, Self::ParentData>) {
        let _ = ctx;
    }

    fn geometry(&self) -> &SliverGeometry {
        static ZERO: SliverGeometry = SliverGeometry::ZERO;
        &ZERO
    }

    fn constraints(&self) -> &SliverConstraints {
        static DEFAULT: SliverConstraints = empty_sliver_constraints();
        &DEFAULT
    }

    fn set_geometry(&mut self, _: SliverGeometry) {}

    fn hit_test(&self, _: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool {
        false
    }
}
