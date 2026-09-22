//! Typed contracts for implementing custom box and sliver render objects.
//!
//! Implement [`RenderBox`] or [`RenderSliver`] and connect it to a
//! [`crate::view::RenderView`]. Layout, painting, hit testing, semantics, and
//! invalidation use the same types as built-in widgets. Arena storage and the
//! GPU compositor remain implementation details outside this authoring module.

pub use flui_rendering::RenderUpdateImpact;
pub use flui_rendering::constraints::{BoxConstraints, SliverConstraints, SliverGeometry};
pub use flui_rendering::context::{
    BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    PaintCx, SliverHitTestContext, SliverLayoutContext,
};
pub use flui_rendering::hit_testing::{
    CursorIcon, HitTestBehavior, MouseTrackerAnnotation, PanZoomTarget, PointerTarget, ScrollTarget,
};
pub use flui_rendering::parent_data::{
    BoxParentData, ContainerBoxParentData, FlexFit, FlexParentData, FlowParentData,
    ListBodyParentData, ListWheelParentData, MultiChildLayoutParentData, ParentData,
    SliverLogicalContainerParentData, SliverLogicalParentData, SliverMultiBoxAdaptorParentData,
    SliverParentData, SliverPhysicalContainerParentData, SliverPhysicalParentData, SliverSlot,
    StackParentData, TableCellParentData, TableCellVerticalAlignment, TextParentData,
    TreeSliverNodeParentData, WrapParentData,
};
pub use flui_rendering::pipeline::RenderInvalidationHandle;
pub use flui_rendering::protocol::{BoxProtocol, Protocol, SliverProtocol, UsageByParent};
pub use flui_rendering::semantics::{SemanticsConfiguration, SemanticsProperties, SemanticsRole};
pub use flui_rendering::traits::{
    HitTestOutcome, PaintClip, PaintEffects, PaintOpacity, RenderBox, RenderObject, RenderSliver,
    TextBaseline,
};
pub use flui_rendering::{
    forward_single_child_box_hit_test, forward_single_child_box_layout,
    forward_single_child_box_queries, forward_single_child_intrinsics,
};
pub use flui_tree::{Arity, AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable};
