//! [`CustomScrollView`] — a viewport over an arbitrary sequence of sliver
//! children.

use std::fmt;

use flui_foundation::geometry::Axis;
use flui_painting::paint::Clip;
use flui_rendering::view::{ScrollPosition, SliverPaintOrder};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::{ShrinkWrappingViewport, Viewport};

/// Where the composed viewport gets its scroll offset from, as in
/// [`GridView`](crate::GridView).
#[derive(Clone, Debug)]
enum OffsetSource {
    Fixed(f64),
    Position(ScrollPosition),
}

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
/// A horizontal `scroll_direction` resolves its `AxisDirection` from the
/// ambient [`Directionality`] (`RightToLeft` under an RTL ancestor); the
/// vertical axis never consults it. `CustomScrollView` has no `reverse` flag yet,
/// unlike [`SingleChildScrollView`].
///
/// [`Directionality`]: crate::Directionality
/// [`SingleChildScrollView`]: crate::SingleChildScrollView
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
    offset_source: OffsetSource,
    shrink_wrap: bool,
    center: Option<usize>,
    anchor: f64,
    paint_order: SliverPaintOrder,
    clip_behavior: Clip,
    slivers: Vec<BoxedView>,
}

impl CustomScrollView {
    /// A vertical custom scroll view over `slivers`.
    pub fn new(slivers: impl ViewSeq) -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Fixed(0.0),
            shrink_wrap: false,
            center: None,
            anchor: 0.0,
            paint_order: SliverPaintOrder::FirstIsTop,
            clip_behavior: Clip::HardEdge,
            slivers: slivers.into_boxed_vec(),
        }
    }

    /// Set the scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn scroll_direction(mut self, scroll_direction: Axis) -> Self {
        self.scroll_direction = scroll_direction;
        self
    }

    /// Set the programmatic scroll offset in logical pixels. Mutually
    /// exclusive with [`CustomScrollView::position`]: whichever is called last
    /// wins.
    #[must_use]
    pub fn offset(mut self, offset: f64) -> Self {
        self.offset_source = OffsetSource::Fixed(offset);
        self
    }

    /// Inject a shared [`ScrollPosition`] as the composed viewport's offset,
    /// as [`ListView::position`](crate::ListView::position) does: a
    /// [`Scrollable`] drives it, and the viewport flushes its committed
    /// content extents back into it. Mutually exclusive with
    /// [`CustomScrollView::offset`]: whichever is called last wins.
    #[must_use]
    pub fn position(mut self, position: ScrollPosition) -> Self {
        self.offset_source = OffsetSource::Position(position);
        self
    }

    /// Whether the scroll view should size itself to its sliver contents in the
    /// scroll axis.
    ///
    /// Defaults to `false`. Use `true` when the parent gives
    /// unbounded main-axis constraints.
    #[must_use]
    pub fn shrink_wrap(mut self, shrink_wrap: bool) -> Self {
        self.shrink_wrap = shrink_wrap;
        self
    }

    /// Set the index of the first forward child (index-based; a key-based
    /// `center` is a follow-up). Has no effect when
    /// [`CustomScrollView::shrink_wrap`] is set: the shrink-wrapping viewport
    /// has no `center`. See
    /// [`Viewport::center`] for the full contract.
    #[must_use]
    pub fn center(mut self, center: Option<usize>) -> Self {
        self.center = center;
        self
    }

    /// Set where the zero-scroll line sits along the main axis, as a
    /// fraction of the viewport's extent from the leading edge (default
    /// `0.0`). Has no effect when [`CustomScrollView::shrink_wrap`] is set:
    /// the shrink-wrapping viewport has no `anchor`. See
    /// [`Viewport::anchor`] for the formulas this drives.
    #[must_use]
    pub fn anchor(mut self, anchor: f64) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the sliver paint order (default [`SliverPaintOrder::FirstIsTop`]).
    /// Hit testing uses the opposite order; applies whether or not
    /// [`CustomScrollView::shrink_wrap`] is set.
    #[must_use]
    pub fn paint_order(mut self, paint_order: SliverPaintOrder) -> Self {
        self.paint_order = paint_order;
        self
    }

    /// Set how overflowing content is clipped (default [`Clip::HardEdge`]).
    /// [`Clip::None`] clips nothing, so a sliver may paint outside the
    /// viewport's bounds; applies whether or not
    /// [`CustomScrollView::shrink_wrap`] is set.
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }
}

impl fmt::Debug for CustomScrollView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomScrollView")
            .field("scroll_direction", &self.scroll_direction)
            .field("offset_source", &self.offset_source)
            .field("shrink_wrap", &self.shrink_wrap)
            .field("center", &self.center)
            .field("anchor", &self.anchor)
            .field("clip_behavior", &self.clip_behavior)
            .field("paint_order", &self.paint_order)
            .field("sliver_count", &self.slivers.len())
            .finish()
    }
}

impl StatelessView for CustomScrollView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // `reverse` isn't modeled on `CustomScrollView` yet (unlike
        // `SingleChildScrollView`), so it's always `false` here — the
        // resolution still consults ambient `Directionality` for a
        // horizontal `scroll_direction`.
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, self.scroll_direction, false);
        if self.shrink_wrap {
            let viewport = ShrinkWrappingViewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .paint_order(self.paint_order)
                .clip_behavior(self.clip_behavior);
            match &self.offset_source {
                OffsetSource::Fixed(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
            .boxed()
        } else {
            let viewport = Viewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .center(self.center)
                .anchor(self.anchor)
                .paint_order(self.paint_order)
                .clip_behavior(self.clip_behavior);
            match &self.offset_source {
                OffsetSource::Fixed(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
            .boxed()
        }
    }
}
