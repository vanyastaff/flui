//! [`ListView`] — a scrollable list of fixed-height items.

use std::fmt;

use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView};

use crate::scroll::{SliverFixedExtentList, Viewport};

/// A scrollable list that lays out its children sequentially along
/// `scroll_direction`, each at a fixed `item_extent` — the common, efficient
/// list case.
///
/// Flutter parity: `widgets/scroll_view.dart` `ListView` (the
/// `ListView(itemExtent: …)` constructor). Composes a
/// [`Viewport`] over a
/// [`SliverFixedExtentList`]. A first cut requires
/// a fixed `item_extent`; variable-height and lazily-built lists arrive with the
/// lazy sliver list. `offset` is programmatic for now.
#[derive(Clone, StatelessView)]
pub struct ListView {
    scroll_direction: Axis,
    item_extent: f32,
    offset: f32,
    children: Vec<BoxedView>,
}

impl ListView {
    /// A vertical list whose rows are each `item_extent` tall.
    pub fn new(item_extent: f32, children: impl ViewSeq) -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            item_extent,
            offset: 0.0,
            children: children.into_boxed_vec(),
        }
    }

    /// Set the scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn scroll_direction(mut self, scroll_direction: Axis) -> Self {
        self.scroll_direction = scroll_direction;
        self
    }

    /// Set the programmatic scroll offset in logical pixels.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
}

impl fmt::Debug for ListView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListView")
            .field("scroll_direction", &self.scroll_direction)
            .field("item_extent", &self.item_extent)
            .field("offset", &self.offset)
            .field("children", &self.children.len())
            .finish()
    }
}

impl StatelessView for ListView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };
        let list = SliverFixedExtentList::new(self.item_extent, self.children.clone());
        Viewport::new((list,))
            .axis_direction(axis_direction)
            .offset(self.offset)
    }
}
