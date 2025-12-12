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

// Re-export base traits
pub use crate::traits::{RenderObject, RenderObjectExt};

// Re-export box traits
pub use crate::traits::{
    BoxHitTestResult, MultiChildRenderBox, PaintingContext, RenderAligningShiftedBox, RenderBox,
    RenderProxyBox, RenderShiftedBox, SingleChildRenderBox, TextBaseline,
};

// Re-export sliver traits
pub use crate::traits::{
    RenderProxySliver, RenderSliver, RenderSliverMultiBoxAdaptor, RenderSliverSingleBoxAdapter,
    SliverHitTestResult, SliverPaintingContext, Transform,
};

// Re-export common types from flui_types
pub use flui_types::{Alignment, Offset};
