//! Cloneable parent-data presets for the render-object test harness.
//!
//! Production layout creates per-walk [`ErasedChildState`] slots with
//! `parent_data: None` and lazily inserts `ParentData::default()` on the
//! first mutable access. Widget configuration that lives on the child's
//! parent data (stack positioning, flex factors, future animation parent
//! slots) is normally applied by the element layer before layout; headless
//! harness tests mount render objects directly, so [`ParentDataSeed`] bridges
//! that gap by cloning a typed preset into the pipeline before each layout
//! walk.
//!
//! Attach a seed to a [`TreeNode`](super::tree::TreeNode) via
//! [`TreeNode::with_parent_data_seed`](super::tree::TreeNode::with_parent_data_seed)
//! (or the `with_stack_parent_data` / `with_flex_parent_data` helpers).

use crate::parent_data::{
    BoxParentData, FlexParentData, ParentData, SliverMultiBoxAdaptorParentData,
    SliverPhysicalParentData, StackParentData,
};

/// A harness-side clone of the parent metadata a widget would normally
/// write onto its child before layout.
#[derive(Debug, Clone)]
pub enum ParentDataSeed {
    /// [`StackParentData`] for `RenderStack` (see `flui_objects::RenderStack`) children.
    Stack(StackParentData),
    /// [`FlexParentData`] for `RenderFlex` (see `flui_objects::RenderFlex`) children.
    Flex(FlexParentData),
    /// Default box offset slot (rarely needed — most parents use
    /// [`StackParentData`] / [`FlexParentData`] instead).
    Box(BoxParentData),
    /// [`SliverPhysicalParentData`] for single-child sliver adapters.
    SliverPhysical(SliverPhysicalParentData),
    /// [`SliverMultiBoxAdaptorParentData`] for [`RenderSliverList`] /
    /// [`RenderSliverListLazy`] children — stamps the logical index so the
    /// virtualizer band walk can find the child in `logical_to_slot`.
    ///
    /// [`RenderSliverList`]: crate::traits::RenderSliver
    SliverMultiBoxAdaptor(SliverMultiBoxAdaptorParentData),
}

impl ParentDataSeed {
    /// Materializes the seed into the erased slot the layout walk expects.
    #[must_use]
    pub fn to_box(&self) -> Box<dyn ParentData> {
        match self {
            Self::Stack(data) => Box::new(data.clone()),
            Self::Flex(data) => Box::new(data.clone()),
            Self::Box(data) => Box::new(data.clone()),
            Self::SliverPhysical(data) => Box::new(data.clone()),
            Self::SliverMultiBoxAdaptor(data) => Box::new(data.clone()),
        }
    }
}
