//! Sliver-protocol accessibility annotations without a box adapter.

use flui_foundation::Single;
use flui_rendering::{
    RenderUpdateImpact,
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    semantics::SemanticsConfiguration,
    traits::RenderSliver,
};

/// Describes a sliver subtree to accessibility while preserving its geometry.
/// Boundary, merge, reading-order and sibling-barrier policies use the same
/// configuration and assembly rules as box annotations.
#[derive(Debug, Clone, Default)]
pub struct RenderSliverSemanticsAnnotations {
    configuration: SemanticsConfiguration,
    exclude_descendants: bool,
}

impl RenderSliverSemanticsAnnotations {
    /// Creates an annotation from an authored configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            exclude_descendants: false,
        }
    }

    /// Replaces the description, invalidating accessibility independently of layout.
    pub fn set_configuration(
        &mut self,
        configuration: SemanticsConfiguration,
    ) -> RenderUpdateImpact {
        if self.configuration == configuration {
            return RenderUpdateImpact::NONE;
        }
        self.configuration = configuration;
        RenderUpdateImpact::SEMANTICS
    }

    /// Changes whether descendants contribute accessible content.
    pub fn set_exclude_descendants(&mut self, exclude: bool) -> RenderUpdateImpact {
        if self.exclude_descendants == exclude {
            return RenderUpdateImpact::NONE;
        }
        self.exclude_descendants = exclude;
        RenderUpdateImpact::SEMANTICS
    }
}

impl flui_foundation::inspect::Inspect for RenderSliverSemanticsAnnotations {
    fn kind(&self) -> &'static str {
        "RenderSliverSemanticsAnnotations"
    }

    fn inspect(&self, _out: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderSliver for RenderSliverSemanticsAnnotations {
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
        ctx.layout_child(0, constraints)
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        *config = self.configuration.clone();
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.exclude_descendants
    }
}
