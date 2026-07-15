//! [`GridView`] — a scrollable 2-D grid of eagerly or lazily-built children.

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_rendering::delegates::{
    SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent,
};
use flui_rendering::view::{ScrollPosition, ViewportOffset};
use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::scroll::{
    ShrinkWrappingViewport, SliverChildBuilderDelegate, SliverGrid, SliverGridLazy, Viewport,
};

/// Where the composed [`Viewport`] gets its scroll offset from — mirrors
/// [`Viewport`]'s own `OffsetSource`, since this widget is a thin
/// pixels-or-position passthrough onto it. Not shared with `Viewport`'s
/// private enum of the same name; duplicated per composing widget, matching
/// [`crate::SingleChildScrollView`]'s template.
#[derive(Clone, Debug)]
enum OffsetSource {
    Pixels(f32),
    Position(ScrollPosition),
}

/// A scrollable 2-D grid.
///
/// Three construction modes, mirroring Flutter's named constructors:
///
/// - [`GridView::count`] — a fixed number of columns in the cross axis, driven
///   by [`SliverGridDelegateWithFixedCrossAxisCount`].
///
/// - [`GridView::extent`] — columns sized so that each is at most
///   `max_cross_axis_extent` wide (or tall when scrolling horizontally), driven
///   by [`SliverGridDelegateWithMaxCrossAxisExtent`].
///
/// - [`GridView::builder`] — lazily builds tiles from a closure; only the
///   children currently visible in the viewport plus a cache margin are built.
///   Tiles that scroll out of the cache window are disposed.  Backed by
///   [`SliverGridLazy`] (element-owned, request-strategy).
///
///   **First-frame settling (Flutter divergence):** lazy children are built
///   *after* the frame's paint, so the first frame a viewport band appears it
///   paints blank; content lands on the next frame (~16 ms @ 60 fps).  See
///   [`SliverChildBuilderDelegate`] for the full rationale.
///
/// `offset` is a programmatic scroll position in logical pixels;
/// gesture-driven scrolling is provided by [`Scrollable`](crate::Scrollable).
/// Set [`GridView::shrink_wrap`] when the grid is placed under unbounded
/// constraints in the scroll axis.
///
/// Flutter parity: `widgets/scroll_view.dart` `GridView.count`,
/// `GridView.extent`, and `GridView.builder`.
#[derive(Clone, StatelessView)]
pub struct GridView {
    scroll_direction: Axis,
    offset_source: OffsetSource,
    shrink_wrap: bool,
    grid_delegate: Arc<dyn SliverGridDelegate>,
    /// Children for the eager variants.  Empty in the lazy variant.
    children: Vec<BoxedView>,
    /// Builder delegate for the lazy variant.  `None` in the eager variants.
    builder_source: Option<SliverChildBuilderDelegate>,
}

impl GridView {
    /// A grid with a fixed number of tiles in the cross axis.
    ///
    /// All children are given upfront.  Each row contains exactly
    /// `cross_axis_count` tiles of equal cross-axis extent.
    ///
    /// Flutter parity: `GridView.count`.
    pub fn count(cross_axis_count: usize, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(cross_axis_count);
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Pixels(0.0),
            shrink_wrap: false,
            grid_delegate: Arc::new(delegate),
            children: children.into_boxed_vec(),
            builder_source: None,
        }
    }

    /// A grid whose tiles are at most `max_cross_axis_extent` wide (or tall,
    /// when scrolling horizontally).
    ///
    /// The number of columns is computed so every tile fits within the
    /// `max_cross_axis_extent` limit; tiles are stretched to fill the grid's
    /// cross-axis evenly.
    ///
    /// Flutter parity: `GridView.extent`.
    pub fn extent(max_cross_axis_extent: f32, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithMaxCrossAxisExtent::new(max_cross_axis_extent);
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Pixels(0.0),
            shrink_wrap: false,
            grid_delegate: Arc::new(delegate),
            children: children.into_boxed_vec(),
            builder_source: None,
        }
    }

    /// A grid that lazily builds its tiles from `builder`.
    ///
    /// Only the children currently visible in the viewport (plus a cache
    /// margin) are built; tiles that scroll out of the cache window are
    /// disposed.  The `builder` closure receives a logical index and returns
    /// the tile view, or `None` when the index is at or past the end of the
    /// data source.
    ///
    /// Flutter parity: `GridView.builder`.
    pub fn builder<F>(
        grid_delegate: Arc<dyn SliverGridDelegate>,
        item_count: usize,
        builder: F,
    ) -> Self
    where
        F: Fn(usize) -> Option<BoxedView> + 'static,
    {
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Pixels(0.0),
            shrink_wrap: false,
            grid_delegate,
            children: Vec::new(),
            builder_source: Some(SliverChildBuilderDelegate::new(item_count, builder)),
        }
    }

    /// Set the scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn scroll_direction(mut self, scroll_direction: Axis) -> Self {
        self.scroll_direction = scroll_direction;
        self
    }

    /// Set the programmatic scroll offset in logical pixels.
    ///
    /// Pixels mode: the composed [`Viewport`] (or [`ShrinkWrappingViewport`]
    /// under [`GridView::shrink_wrap`]) owns a private `ScrollPosition` and
    /// this value is pushed into it on every rebuild. Mutually exclusive with
    /// [`GridView::position`] — whichever is called last wins.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset_source = OffsetSource::Pixels(offset);
        self
    }

    /// Inject a shared [`ScrollPosition`] as the composed [`Viewport`]'s
    /// offset — see [`Viewport::position`] for the full contract. Mutually
    /// exclusive with [`GridView::offset`] — whichever is called last wins.
    ///
    /// **Shrink-wrap limitation:** under [`GridView::shrink_wrap`],
    /// [`ShrinkWrappingViewport`] has no `.position(...)` passthrough (a
    /// separate, currently-open remainder — see `docs/ROADMAP.md`
    /// Business.1). A shrink-wrapped grid still lays out at `position`'s
    /// current pixel value (snapshotted once per rebuild), but does not join
    /// the live content-dimension feedback loop: gesture/controller writes
    /// after that snapshot are not reflected until the next rebuild, and
    /// `RenderShrinkWrappingViewport`'s committed extents never flush back
    /// into `position`.
    #[must_use]
    pub fn position(mut self, position: ScrollPosition) -> Self {
        self.offset_source = OffsetSource::Position(position);
        self
    }

    /// Whether the grid should size itself to its sliver contents in the scroll
    /// axis.
    ///
    /// Defaults to `false`, matching Flutter. Use `true` when the parent gives
    /// unbounded main-axis constraints.
    #[must_use]
    pub fn shrink_wrap(mut self, shrink_wrap: bool) -> Self {
        self.shrink_wrap = shrink_wrap;
        self
    }
}

impl fmt::Debug for GridView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("GridView");
        s.field("scroll_direction", &self.scroll_direction)
            .field("offset_source", &self.offset_source)
            .field("shrink_wrap", &self.shrink_wrap);
        if self.builder_source.is_some() {
            s.field("builder_source", &self.builder_source);
        } else {
            s.field("grid_delegate", &self.grid_delegate)
                .field("children", &self.children.len());
        }
        s.finish()
    }
}

impl StatelessView for GridView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };

        let sliver: BoxedView = if let Some(ref delegate) = self.builder_source {
            // Lazy variant: wire SliverGridLazy (element-owned request strategy).
            SliverGridLazy::new(
                Arc::clone(&self.grid_delegate),
                delegate.item_count,
                Rc::clone(&delegate.builder),
            )
            .boxed()
        } else {
            // Eager variant: all children pre-attached.
            SliverGrid::new(Arc::clone(&self.grid_delegate), self.children.clone()).boxed()
        };

        if self.shrink_wrap {
            // ShrinkWrappingViewport has no `.position(...)` passthrough (see
            // `GridView::position`'s doc for the honest limitation) — snapshot
            // the current pixel value from either offset source and push it
            // as a one-time constant.
            let offset = match &self.offset_source {
                OffsetSource::Pixels(pixels) => *pixels,
                OffsetSource::Position(position) => position.pixels(),
            };
            ShrinkWrappingViewport::new((sliver,))
                .axis_direction(axis_direction)
                .offset(offset)
                .boxed()
        } else {
            let viewport = Viewport::new((sliver,)).axis_direction(axis_direction);
            match &self.offset_source {
                OffsetSource::Pixels(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
            .boxed()
        }
    }
}
