//! Prelude module with common imports

// Re-export core types
pub use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// Re-export constraints
pub use crate::constraints::{
    Axis, AxisDirection, BoxConstraints, GrowthDirection, ScrollDirection, SliverConstraints,
};

// Re-export geometry
pub use crate::geometry::{Size, SliverGeometry};

// Re-export parent data
pub use crate::parent_data::{BoxParentData, ParentData, SliverParentData};

// Re-export containers
pub use crate::containers::{
    Aligning, AligningBox, AligningSliver, BoxChild, BoxChildren, Children, Proxy, ProxyBox,
    Shifted, ShiftedBox, ShiftedSliver, Single, SliverChild, SliverChildren, SliverProxy,
};

// Re-export traits
pub use crate::traits::{
    BoxHitTestResult, PaintingContext, RenderBox, RenderObject, RenderObjectExt, RenderSliver,
    SliverHitTestResult, SliverPaintingContext, TextBaseline, Transform,
};

// Re-export common types from flui_types
pub use flui_types::{Alignment, Offset};
