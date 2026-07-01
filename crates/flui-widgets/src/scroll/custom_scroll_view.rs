//! [`CustomScrollView`] — a viewport over an arbitrary sequence of sliver
//! children.

use std::fmt;

use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::scroll::{ShrinkWrappingViewport, Viewport};

/// A scrollable area whose scroll body is composed from an arbitrary list of
/// **sliver** widgets.
///
/// `CustomScrollView` is the most general scroll-view widget: rather than
/// wrapping a single fixed sliver (as [`ListView`] and [`GridView`] do), it
/// composes a [`Viewport`] over a caller-supplied sequence of sliver children.
/// Use it to combine heterogeneous sliver families — for example, a
/// [`SliverToBoxAdapter`] header, a [`SliverFixedExtentList`] body, and a
/// [`SliverFillRemaining`] footer.
///
/// `offset` is a programmatic scroll position in logical pixels.
/// Gesture-driven scrolling is provided by [`Scrollable`] + a
/// [`ScrollController`].
/// Set [`CustomScrollView::shrink_wrap`] when the scroll view is placed under
/// unbounded constraints in the scroll axis.
///
/// Flutter parity: `widgets/scroll_view.dart` `CustomScrollView`.
///
/// [`ListView`]: crate::ListView
/// [`GridView`]: crate::GridView
/// [`SliverToBoxAdapter`]: crate::SliverToBoxAdapter
/// [`SliverFixedExtentList`]: crate::SliverFixedExtentList
/// [`SliverFillRemaining`]: crate::SliverFillRemaining
/// [`Scrollable`]: crate::Scrollable
/// [`ScrollController`]: crate::ScrollController
#[derive(Clone, StatelessView)]
pub struct CustomScrollView {
    scroll_direction: Axis,
    offset: f32,
    shrink_wrap: bool,
    slivers: Vec<BoxedView>,
}

impl CustomScrollView {
    /// A vertical custom scroll view over `slivers`.
    pub fn new(slivers: impl ViewSeq) -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            offset: 0.0,
            shrink_wrap: false,
            slivers: slivers.into_boxed_vec(),
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

    /// Whether the scroll view should size itself to its sliver contents in the
    /// scroll axis.
    ///
    /// Defaults to `false`, matching Flutter. Use `true` when the parent gives
    /// unbounded main-axis constraints.
    #[must_use]
    pub fn shrink_wrap(mut self, shrink_wrap: bool) -> Self {
        self.shrink_wrap = shrink_wrap;
        self
    }
}

impl fmt::Debug for CustomScrollView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomScrollView")
            .field("scroll_direction", &self.scroll_direction)
            .field("offset", &self.offset)
            .field("shrink_wrap", &self.shrink_wrap)
            .field("sliver_count", &self.slivers.len())
            .finish()
    }
}

impl StatelessView for CustomScrollView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };
        if self.shrink_wrap {
            ShrinkWrappingViewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .offset(self.offset)
                .boxed()
        } else {
            Viewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .offset(self.offset)
                .boxed()
        }
    }
}
