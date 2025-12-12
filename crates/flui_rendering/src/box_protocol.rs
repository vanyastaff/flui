//! BoxProtocol implementation

use crate::constraints::BoxConstraints;
use crate::geometry::Size;
use crate::parent_data::BoxParentData;
use crate::protocol::{BoxProtocol, Protocol};
use crate::traits::RenderBox;

/// Implementation of Protocol trait for BoxProtocol
impl Protocol for BoxProtocol {
    type Object = dyn RenderBox;
    type Constraints = BoxConstraints;
    type ParentData = BoxParentData;
    type Geometry = Size;

    fn default_geometry() -> Self::Geometry {
        Size::ZERO
    }

    fn name() -> &'static str {
        "box"
    }
}
