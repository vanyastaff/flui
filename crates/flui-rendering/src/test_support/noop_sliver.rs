//! Minimal leaf sliver used as a trait-method probe in unit tests.

use flui_tree::Leaf;

use crate::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// Leaf sliver with zero geometry, used only to satisfy trait child references.
#[derive(Debug, Default)]
pub struct NoopSliver;

impl flui_foundation::Diagnosticable for NoopSliver {}

impl RenderSliver for NoopSliver {
    type Arity = Leaf;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> SliverGeometry {
        SliverGeometry::ZERO
    }

    fn hit_test(&self, _: &mut SliverHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool {
        false
    }
}
