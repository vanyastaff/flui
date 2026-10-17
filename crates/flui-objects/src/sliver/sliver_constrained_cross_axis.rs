//! Cross-axis sizing without converting a sliver to a box.

use flui_foundation::{Single, geometry::Offset};
use flui_rendering::{
    RenderUpdateImpact,
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

/// A cross-axis limit must be finite and nonnegative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("sliver cross-axis extent must be finite and nonnegative")]
pub struct InvalidSliverCrossAxisExtent;

fn validate_extent(extent: f64) -> Result<f64, InvalidSliverCrossAxisExtent> {
    if extent.is_finite() && extent >= 0.0 {
        Ok(extent.max(0.0))
    } else {
        Err(InvalidSliverCrossAxisExtent)
    }
}

/// Limits the child's cross axis and publishes the actual claimed extent.
///
/// The child's local origin is unchanged. The enclosing viewport or track
/// determines its physical cross-axis placement. Main-axis geometry,
/// including pinned obstruction and scroll corrections, remains authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSliverConstrainedCrossAxis {
    max_extent: f64,
}

impl RenderSliverConstrainedCrossAxis {
    /// The validated maximum cross-axis extent.
    #[must_use]
    pub fn max_extent(&self) -> f64 {
        self.max_extent
    }

    /// Creates a finite cross-axis limit, including a zero-width track.
    ///
    /// # Errors
    /// Refuses negative or nonfinite limits before changing layout state.
    pub fn new(max_extent: f64) -> Result<Self, InvalidSliverCrossAxisExtent> {
        Ok(Self {
            max_extent: validate_extent(max_extent)?,
        })
    }

    /// Replaces the limit and requests layout when it changes.
    ///
    /// # Errors
    /// Invalid limits leave the previous configuration in force.
    pub fn set_max_extent(
        &mut self,
        max_extent: f64,
    ) -> Result<RenderUpdateImpact, InvalidSliverCrossAxisExtent> {
        let max_extent = validate_extent(max_extent)?;
        if self.max_extent == max_extent {
            return Ok(RenderUpdateImpact::NONE);
        }
        self.max_extent = max_extent;
        Ok(RenderUpdateImpact::LAYOUT)
    }
}

impl flui_foundation::Diagnosticable for RenderSliverConstrainedCrossAxis {}

impl RenderSliver for RenderSliverConstrainedCrossAxis {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();
        if ctx.child_count() == 0 {
            return Ok(SliverGeometry::ZERO);
        }
        let extent = constraints.cross_axis_extent.min(self.max_extent);
        let mut child_constraints = constraints;
        child_constraints.cross_axis_extent = extent;
        let mut geometry = ctx.layout_child(0, child_constraints)?;
        if geometry.scroll_offset_correction.is_some() {
            return Ok(geometry);
        }
        geometry.cross_axis_extent = Some(extent);
        ctx.position_child(0, Offset::ZERO);
        Ok(geometry)
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}
