//! [`GridView`] — a scrollable 2-D grid of eagerly-built children.

use std::fmt;
use std::sync::Arc;

use flui_rendering::delegates::{
    SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent,
};
use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::scroll::{SliverGrid, Viewport};

/// A scrollable 2-D grid that lays out its children eagerly.
///
/// Two construction modes, mirroring Flutter's named constructors:
///
/// - [`GridView::count`] — a fixed number of columns in the cross axis, driven
///   by [`SliverGridDelegateWithFixedCrossAxisCount`].
///
/// - [`GridView::extent`] — columns sized so that each is at most
///   `max_cross_axis_extent` wide, driven by
///   [`SliverGridDelegateWithMaxCrossAxisExtent`].
///
/// Both modes compose a [`Viewport`] over a [`SliverGrid`] with the selected
/// delegate. `offset` is a programmatic scroll position in logical pixels;
/// gesture-driven scrolling is provided by [`Scrollable`](crate::Scrollable).
///
/// Flutter parity: `widgets/scroll_view.dart` `GridView.count` and
/// `GridView.extent` (line 1976).
///
/// **Divergence:** `GridView.builder` (lazy construction) is deferred — it
/// requires a `RenderSliverGridLazy` render object that does not exist yet.
#[derive(Clone, StatelessView)]
pub struct GridView {
    scroll_direction: Axis,
    offset: f32,
    grid_delegate: Arc<dyn SliverGridDelegate>,
    children: Vec<BoxedView>,
}

impl GridView {
    /// A grid with a fixed number of tiles in the cross axis.
    ///
    /// All children are given upfront. Each row contains exactly
    /// `cross_axis_count` tiles of equal cross-axis extent.
    ///
    /// Flutter parity: `GridView.count` (`scroll_view.dart` `GridView.count`).
    pub fn count(cross_axis_count: usize, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(cross_axis_count);
        Self {
            scroll_direction: Axis::Vertical,
            offset: 0.0,
            grid_delegate: Arc::new(delegate),
            children: children.into_boxed_vec(),
        }
    }

    /// A grid whose tiles are at most `max_cross_axis_extent` wide (or tall,
    /// when scrolling horizontally).
    ///
    /// The number of columns is computed so every tile fits within the
    /// `max_cross_axis_extent` limit; tiles are stretched to fill the grid's
    /// cross-axis evenly.
    ///
    /// Flutter parity: `GridView.extent` (`scroll_view.dart` `GridView.extent`).
    pub fn extent(max_cross_axis_extent: f32, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithMaxCrossAxisExtent::new(max_cross_axis_extent);
        Self {
            scroll_direction: Axis::Vertical,
            offset: 0.0,
            grid_delegate: Arc::new(delegate),
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

impl fmt::Debug for GridView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GridView")
            .field("scroll_direction", &self.scroll_direction)
            .field("offset", &self.offset)
            .field("grid_delegate", &self.grid_delegate)
            .field("children", &self.children.len())
            .finish()
    }
}

impl StatelessView for GridView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };
        let sliver: BoxedView =
            SliverGrid::new(Arc::clone(&self.grid_delegate), self.children.clone()).boxed();
        Viewport::new((sliver,))
            .axis_direction(axis_direction)
            .offset(self.offset)
    }
}
