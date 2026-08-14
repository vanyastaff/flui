//! [`SliverMainAxisGroup`] — groups multiple slivers into one, laid out
//! sequentially along the main axis.

use flui_objects::RenderSliverMainAxisGroup;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{BoxedView, RenderView, View, impl_render_view};

/// Places multiple sliver children in a linear array along the main axis,
/// presenting them to the enclosing viewport as a single sliver.
///
/// Flutter parity: `widgets/sliver.dart` `SliverMainAxisGroup` over
/// `RenderSliverMainAxisGroup`. A pinned persistent header inside the group
/// pins to the GROUP's bounds, not the viewport's — scrolling past the group
/// pushes the header out with it.
#[derive(Clone, Debug, Default)]
pub struct SliverMainAxisGroup {
    slivers: Vec<BoxedView>,
}

impl SliverMainAxisGroup {
    /// A group of the given sliver children, in main-axis order.
    #[must_use]
    pub fn new(slivers: Vec<BoxedView>) -> Self {
        Self { slivers }
    }
}

impl RenderView for SliverMainAxisGroup {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverMainAxisGroup;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverMainAxisGroup::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The render object carries no configuration; children reconcile
        // through the element tree and re-lay through the normal dirty walk.
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        !self.slivers.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for sliver in &self.slivers {
            visitor(sliver);
        }
    }
}

impl_render_view!(SliverMainAxisGroup);
