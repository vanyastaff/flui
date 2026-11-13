//! Typed contracts for implementing custom box and sliver render objects.
//!
//! Implement [`RenderBox`] or [`RenderSliver`] and connect it to a
//! [`crate::view::RenderView`]. Layout, painting, hit testing, semantics, and
//! invalidation use the same types as built-in widgets. Arena storage and the
//! GPU compositor remain implementation details outside this authoring module; the built-in
//! render objects are in [`objects`].

pub use flui_foundation::{Arity, AtLeast, Exact, Leaf, Never, Optional, Range, Single, Variable};
pub use flui_rendering::constraints::{
    BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry,
};
pub use flui_rendering::context::{
    BoxDryBaselineContext, BoxDryLayoutContext, BoxHitTestContext, BoxIntrinsicsContext,
    BoxLayoutContext, FragmentRecorder, HitTestContext, LayoutContext, PaintAnchor, PaintContext,
    PaintFragment, SliverHitTestContext, SliverLayoutContext,
};
pub use flui_rendering::hit_testing::{
    CursorIcon, HitMetadata, HitTestBehavior, MouseTrackerAnnotation, PanZoomTarget, PointerTarget,
    ScrollTarget, SmartZoomTarget,
};
pub use flui_rendering::input_regions::InputRegionSink;
pub use flui_rendering::parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentDataMixin, FlexFit, FlexParentData,
    FlowParentData, HorizontalInsets, ListBodyParentData, ListWheelParentData,
    MultiChildLayoutParentData, ParentData, SliverCrossAxisAllocation, SliverCrossAxisParentData,
    SliverLogicalContainerParentData, SliverLogicalParentData, SliverMultiBoxAdaptorParentData,
    SliverParentData, SliverPhysicalContainerParentData, SliverPhysicalParentData, SliverSlot,
    StackParentData, TableCellParentData, TableCellVerticalAlignment, TextParentData,
    TreeSliverNodeParentData, WrapParentData,
};
pub use flui_rendering::pipeline::{
    DirtySendError, RenderInvalidationHandle, TextContextMut, TextSource,
};
pub use flui_rendering::protocol::{
    BoxChildSize, BoxHitTest, BoxLayout, BoxProtocol, ChildLayout, ChildState, MainAxisPosition,
    Protocol, ProtocolConstraints, ProtocolGeometry, ProtocolPosition, SliverChildState,
    SliverHitTest, SliverLayout, SliverProtocol, UsageByParent,
};
pub use flui_rendering::semantics::{
    NumericRange, NumericRangeError, SemanticsConfiguration, SemanticsRole,
};
pub use flui_rendering::traits::{
    HitTestOutcome, PaintClip, PaintEffects, PaintOpacity, RenderBox, RenderObject, RenderSliver,
    SemanticsClip, TextBaseline,
};
pub use flui_rendering::{PoisonPhase, RenderError, RenderResult, RenderUpdateImpact};
pub use flui_rendering::{
    forward_single_child_box_hit_test, forward_single_child_box_layout,
    forward_single_child_box_queries, forward_single_child_intrinsics,
};

/// The concrete render objects behind the built-in widgets, such as [`objects::RenderAlign`],
/// [`objects::RenderFlex`] and [`objects::RenderSliverList`], with the configuration types they
/// take.
///
/// A custom widget wraps one of these in its [`crate::view::RenderView`] the way the built-in
/// widgets do, and each built-in widget's `RenderView::RenderObject` names its object here.
pub mod objects {
    pub use flui_objects::*;
}
