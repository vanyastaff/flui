//! [`FocusableSliver`] — keyboard focus over one lazy sliver of rows inside
//! any viewport.

use std::fmt;
use std::sync::Arc;

use flui_foundation::geometry::Axis;
use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::scroll::list_focus::{FocusableList, ListFocusConfig, ListItemFocus};

/// A lazy sliver of rows made one Tab stop, with the arrow keys moving focus
/// over its rows, as [`ListView::focusable`](crate::ListView::focusable)
/// does for a whole list.
///
/// Put it among a [`CustomScrollView`](crate::CustomScrollView)'s slivers
/// (or a [`Viewport`](crate::Viewport)'s); each focusable sliver is its own
/// stop. Tab enters on the row it last focused, or on the first visible row.
/// Along the scroll axis the arrows move to the neighbouring row, Page
/// Up/Down by a page and Home/End to the first and last row; over a
/// [`SliverGrid`](crate::SliverGrid) they move by a row of tiles, the
/// cross-axis arrows move along the row, and a column past the end of a
/// short last row lands on its last tile. At the sliver's edge an arrow
/// stays on the row; inside another focusable list it goes to that list. A
/// row not built yet is scrolled into the band through the enclosing
/// [`Scrollable`](crate::Scrollable) and focused when it mounts. The focused
/// row stays built and announced while the pointer scrolls it away.
///
/// A row's focus goes to its first focusable descendant, so the row's button
/// activates and shows its ring as on a click.
#[derive(Clone, StatelessView)]
pub struct FocusableSliver {
    /// The sliver with each row inside its focus marker.
    sliver: BoxedView,
    item_count: usize,
    grid: bool,
    empty_label: Option<Arc<str>>,
}

impl FocusableSliver {
    /// Make `sliver`'s rows focusable from the keyboard.
    pub fn new(sliver: impl IntoFocusableSliver) -> Self {
        let parts = sliver.into_parts();
        Self {
            sliver: parts.sliver,
            item_count: parts.item_count,
            grid: parts.grid,
            empty_label: None,
        }
    }

    /// The accessible name of the sliver while it has no rows. An empty
    /// focusable sliver keeps focus on itself and announces this name.
    #[must_use]
    pub fn empty_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.empty_label = Some(label.into());
        self
    }
}

impl fmt::Debug for FocusableSliver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusableSliver")
            .field("item_count", &self.item_count)
            .field("grid", &self.grid)
            .field("empty_label", &self.empty_label)
            .finish_non_exhaustive()
    }
}

impl StatelessView for FocusableSliver {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // The axis and direction come from the sliver's own layout, and an
        // unbuilt row is reached through the enclosing reveal scope, so the
        // configured axis and position are not read.
        let config = ListFocusConfig {
            item_count: self.item_count,
            axis: Axis::Vertical,
            reversed: false,
            position: None,
            extent_estimate: 0.0,
            empty_list_label: self.empty_label.clone(),
            sliver: true,
            grid: self.grid,
        };
        FocusableList::new(config, self.sliver.clone()).boxed()
    }
}

/// What [`FocusableSliver`] takes from a sliver.
#[derive(Debug)]
pub struct FocusableSliverParts {
    sliver: BoxedView,
    item_count: usize,
    grid: bool,
}

/// A lazy sliver [`FocusableSliver`] can make focusable: [`SliverList`],
/// [`SliverGrid`] and [`SliverFixedExtentList`].
///
/// [`SliverList`]: crate::SliverList
/// [`SliverGrid`]: crate::SliverGrid
/// [`SliverFixedExtentList`]: crate::SliverFixedExtentList
pub trait IntoFocusableSliver: sealed::Sealed {
    /// The sliver with its rows inside focus markers, its row count and
    /// whether its rows are grid tiles.
    #[doc(hidden)]
    fn into_parts(self) -> FocusableSliverParts;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::scroll::SliverList {}
    impl Sealed for crate::scroll::SliverGrid {}
    impl Sealed for crate::scroll::SliverFixedExtentList {}
}

/// Each built row inside its focus marker, which carries the row's key.
fn marked(index: usize, row: BoxedView) -> BoxedView {
    BoxedView::new(ListItemFocus::new(index, row, false))
}

impl IntoFocusableSliver for crate::scroll::SliverList {
    fn into_parts(self) -> FocusableSliverParts {
        FocusableSliverParts {
            item_count: self.resolved_item_count(),
            sliver: self.map_children(marked).boxed(),
            grid: false,
        }
    }
}

impl IntoFocusableSliver for crate::scroll::SliverGrid {
    fn into_parts(self) -> FocusableSliverParts {
        FocusableSliverParts {
            item_count: self.resolved_item_count(),
            sliver: self.map_children(marked).boxed(),
            grid: true,
        }
    }
}

impl IntoFocusableSliver for crate::scroll::SliverFixedExtentList {
    fn into_parts(self) -> FocusableSliverParts {
        let adaptor = self.adaptor();
        FocusableSliverParts {
            item_count: adaptor.resolved_item_count(),
            sliver: adaptor.map_children(marked).boxed(),
            grid: false,
        }
    }
}
