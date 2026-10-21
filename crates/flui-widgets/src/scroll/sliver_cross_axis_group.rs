//! Side-by-side slivers and their typed track policies.

use flui_objects::RenderSliverCrossAxisGroup;
use flui_rendering::{
    parent_data::{SliverCrossAxisAllocation, SliverCrossAxisParentData},
    protocol::SliverProtocol,
};
use flui_view::{BoxedView, IntoView, ParentDataView, View, ViewExt, seq::ViewSeq};
use std::{fmt, num::NonZeroU32};

/// Presents multiple slivers as parallel tracks in one scrolling coordinate.
/// Ordinary children split the cross axis equally; [`SliverCrossAxisItem`]
/// changes a child's share or lets it claim an explicit content extent.
#[derive(Clone, Default)]
pub struct SliverCrossAxisGroup<C = Vec<BoxedView>> {
    slivers: C,
}

impl<C> SliverCrossAxisGroup<C> {
    /// Creates a group in declarative cross-axis order.
    #[must_use]
    pub fn new(slivers: C) -> Self {
        Self { slivers }
    }
}

impl<C: ViewSeq> fmt::Debug for SliverCrossAxisGroup<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverCrossAxisGroup")
            .field("slivers", &self.slivers.len())
            .finish()
    }
}

impl<C: ViewSeq + Clone + 'static> flui_view::RenderView for SliverCrossAxisGroup<C> {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverCrossAxisGroup;
    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverCrossAxisGroup::new()
    }
    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
    fn has_children(&self) -> bool {
        !self.slivers.is_empty()
    }
    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.slivers.for_each(|_, child| visitor(child));
    }
}
crate::__private::generic_render_view_element!(SliverCrossAxisGroup);

/// Gives a sliver child an explicit cross-axis track policy.
/// The render parent's parent-data type rejects placement under other layouts.
#[derive(Clone, Debug)]
pub struct SliverCrossAxisItem {
    allocation: SliverCrossAxisAllocation,
    child: BoxedView,
}

impl SliverCrossAxisItem {
    /// Shares remaining cross-axis space with the given positive weight.
    #[must_use]
    pub fn flex(weight: NonZeroU32, child: impl IntoView) -> Self {
        Self {
            allocation: SliverCrossAxisAllocation::Flex(weight),
            child: child.into_view().boxed(),
        }
    }
    /// Claims the child's reported extent before flexible tracks are allocated.
    /// Use a [`super::SliverConstrainedCrossAxis`] child or another sliver that
    /// publishes `SliverGeometry::cross_axis_extent`.
    #[must_use]
    pub fn content(child: impl IntoView) -> Self {
        Self {
            allocation: SliverCrossAxisAllocation::Content,
            child: child.into_view().boxed(),
        }
    }
}

impl ParentDataView for SliverCrossAxisItem {
    type ParentData = SliverCrossAxisParentData;
    fn child(&self) -> &dyn View {
        &self.child
    }
    fn create_parent_data(&self) -> Self::ParentData {
        SliverCrossAxisParentData {
            allocation: self.allocation,
        }
    }
    fn apply_parent_data(&self, data: &mut Self::ParentData) -> flui_rendering::RenderUpdateImpact {
        if data.allocation == self.allocation {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        data.allocation = self.allocation;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
    fn debug_type_name(&self) -> &'static str {
        "SliverCrossAxisItem"
    }
    fn typical_ancestor_description(&self) -> &'static str {
        "SliverCrossAxisGroup"
    }
}
flui_view::impl_parent_data_view!(SliverCrossAxisItem);
