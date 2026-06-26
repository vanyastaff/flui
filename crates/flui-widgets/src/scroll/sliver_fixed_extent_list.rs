//! [`SliverFixedExtentList`] — a sliver that lays out box children one after
//! another, each given the same fixed main-axis extent.

use std::fmt;

use flui_objects::RenderSliverFixedExtentList;
use flui_rendering::protocol::SliverProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// A sliver that places its box children sequentially along the scroll axis,
/// each occupying the same `item_extent` — cheaper to lay out than measuring
/// every child, the backbone of a fixed-row-height [`ListView`](crate::ListView).
///
/// Flutter parity: `widgets/sliver.dart` `SliverFixedExtentList` over
/// `RenderSliverFixedExtentList`. Lives inside a [`Viewport`](crate::Viewport).
///
/// Generic over `C: ViewSeq` of box child views.
#[derive(Clone)]
pub struct SliverFixedExtentList<C = Vec<BoxedView>> {
    item_extent: f32,
    children: C,
}

impl<C> SliverFixedExtentList<C> {
    /// A fixed-extent sliver list: every child gets `item_extent` on the scroll
    /// axis.
    pub fn new(item_extent: f32, children: C) -> Self {
        Self {
            item_extent,
            children,
        }
    }
}

impl<C: ViewSeq> fmt::Debug for SliverFixedExtentList<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverFixedExtentList")
            .field("item_extent", &self.item_extent)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for SliverFixedExtentList<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFixedExtentList;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverFixedExtentList::new(self.item_extent)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_item_extent(self.item_extent);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(SliverFixedExtentList);
