//! [`SliverMainAxisGroup`] — groups multiple slivers into one, laid out
//! sequentially along the main axis.

use std::fmt;

use flui_objects::RenderSliverMainAxisGroup;
use flui_rendering::protocol::SliverProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Places multiple sliver children in a linear array along the main axis,
/// presenting them to the enclosing viewport as a single sliver.
///
/// Flutter parity: `widgets/sliver.dart` `SliverMainAxisGroup` over
/// `RenderSliverMainAxisGroup`. A pinned persistent header inside the group
/// pins to the GROUP's bounds, not the viewport's — scrolling past the group
/// pushes the header out with it.
///
/// Generic over `C: ViewSeq` of sliver child views, so both the static tuple
/// path and the dynamic `Vec<BoxedView>` path work.
#[derive(Clone, Default)]
pub struct SliverMainAxisGroup<C = Vec<BoxedView>> {
    slivers: C,
}

impl<C> SliverMainAxisGroup<C> {
    /// A group of the given sliver children, in main-axis order.
    #[must_use]
    pub fn new(slivers: C) -> Self {
        Self { slivers }
    }
}

impl<C: ViewSeq> fmt::Debug for SliverMainAxisGroup<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverMainAxisGroup")
            .field("slivers", &self.slivers.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for SliverMainAxisGroup<C>
where
    C: ViewSeq + Clone + 'static,
{
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

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.slivers.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(SliverMainAxisGroup);
