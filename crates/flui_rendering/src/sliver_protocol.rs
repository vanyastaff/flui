//! SliverProtocol implementation

use crate::constraints::SliverConstraints;
use crate::geometry::SliverGeometry;
use crate::parent_data::SliverParentData;
use crate::protocol::{Protocol, SliverProtocol};
use crate::traits::RenderSliver;

/// Implementation of Protocol trait for SliverProtocol
impl Protocol for SliverProtocol {
    type Object = dyn RenderSliver;
    type Constraints = SliverConstraints;
    type ParentData = SliverParentData;
    type Geometry = SliverGeometry;

    fn default_geometry() -> Self::Geometry {
        SliverGeometry::zero()
    }

    fn name() -> &'static str {
        "sliver"
    }
}
