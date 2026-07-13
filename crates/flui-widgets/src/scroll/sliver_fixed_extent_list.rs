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
    C: ViewSeq + Clone + 'static,
{
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFixedExtentList;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSliverFixedExtentList::new(self.item_extent)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
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

#[cfg(test)]
mod tests {
    use flui_view::RenderView;
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn debug_reports_item_extent_and_child_count() {
        let list = SliverFixedExtentList::new(
            30.0,
            vec![
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
            ],
        );

        let debug = format!("{list:?}");
        assert!(
            debug.contains("item_extent: 30.0") && debug.contains("children: 3"),
            "Debug output must include item_extent and children count, got: {debug}",
        );
    }

    #[test]
    fn has_children_reflects_an_empty_child_list() {
        let empty: SliverFixedExtentList = SliverFixedExtentList::new(30.0, Vec::new());
        assert!(!empty.has_children());

        let non_empty = SliverFixedExtentList::new(30.0, vec![SizedBox::shrink().boxed()]);
        assert!(non_empty.has_children());
    }

    #[test]
    fn update_render_object_applies_a_changed_item_extent() {
        let list = SliverFixedExtentList::new(30.0, Vec::<flui_view::BoxedView>::new());
        let mut render_object =
            list.create_render_object(&flui_view::RenderObjectContext::detached());

        let updated = SliverFixedExtentList::new(75.0, Vec::<flui_view::BoxedView>::new());
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        // No public getter on RenderSliverFixedExtentList; confirm via Debug
        // that the field actually changed rather than merely not panicking.
        let debug = format!("{render_object:?}");
        assert!(
            debug.contains("75"),
            "update_render_object must apply the new item_extent, got: {debug}",
        );
    }
}
