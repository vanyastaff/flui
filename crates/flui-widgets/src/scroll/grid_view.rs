//! [`GridView`] — a scrollable 2-D grid of eagerly or lazily-built children.

use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use flui_rendering::delegates::{
    SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent,
};
use flui_rendering::view::ScrollPosition;
use flui_types::layout::Axis;
use flui_view::element::StaticChildren;
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::{ShrinkWrappingViewport, SliverChildBuilderDelegate, SliverGrid, Viewport};

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
///   [`SliverGrid`] (element-owned, request-strategy).
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
/// A horizontal `scroll_direction` resolves its `AxisDirection` from the
/// ambient [`Directionality`](crate::Directionality) (`RightToLeft` under an
/// RTL ancestor), matching `ScrollView.getDirection`
/// (`widgets/scroll_view.dart`); the vertical axis never consults it.
/// `GridView` has no `reverse` flag yet.
///
/// Flutter parity: `widgets/scroll_view.dart` `GridView.count`,
/// `GridView.extent`, and `GridView.builder`.
#[derive(Clone, StatelessView)]
pub struct GridView {
    scroll_direction: Axis,
    offset_source: OffsetSource,
    shrink_wrap: bool,
    grid_delegate: Arc<dyn SliverGridDelegate>,
    /// Children for the static variants ([`GridView::count`],
    /// [`GridView::extent`]). Empty in the lazy variant.
    /// The static children as handed in — a shared delegate, so a `GridView`
    /// cloned or rebuilt from the same value compares as unchanged and its
    /// resident children are not rebuilt (ADR-0053).
    children: Rc<StaticChildren>,
    /// `children` with the per-item repaint boundary applied, created on
    /// first use so it too keeps its identity across builds of one value.
    children_in_boundaries: std::cell::OnceCell<Rc<StaticChildren>>,
    /// Builder delegate for the lazy variant.  `None` in the static variants.
    builder_source: Option<SliverChildBuilderDelegate>,
    /// Wrap each tile in a `RepaintBoundary` (default `true`).
    ///
    /// Flutter parity: `addRepaintBoundaries` on the delegates `GridView`
    /// builds, which defaults to `true`.
    add_repaint_boundaries: bool,
}

impl GridView {
    /// A grid with a fixed number of tiles in the cross axis.
    ///
    /// All children are given upfront.  Each row contains exactly
    /// `cross_axis_count` tiles of equal cross-axis extent. Only the
    /// viewport-visible (plus cache margin) window is actually materialized
    /// — the same [`SliverGrid`] request strategy [`GridView::builder`] uses
    /// (ADR-0053).
    ///
    /// Flutter parity: `GridView.count`.
    pub fn count(cross_axis_count: usize, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(cross_axis_count);
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Pixels(0.0),
            shrink_wrap: false,
            grid_delegate: Arc::new(delegate),
            children: StaticChildren::new(children.into_boxed_vec()),
            children_in_boundaries: std::cell::OnceCell::new(),
            builder_source: None,
            add_repaint_boundaries: true,
        }
    }

    /// A grid whose tiles are at most `max_cross_axis_extent` wide (or tall,
    /// when scrolling horizontally).
    ///
    /// The number of columns is computed so every tile fits within the
    /// `max_cross_axis_extent` limit; tiles are stretched to fill the grid's
    /// cross-axis evenly. Only the viewport-visible (plus cache margin)
    /// window is actually materialized (ADR-0053), as for
    /// [`GridView::count`].
    ///
    /// Flutter parity: `GridView.extent`.
    pub fn extent(max_cross_axis_extent: f32, children: impl ViewSeq) -> Self {
        let delegate = SliverGridDelegateWithMaxCrossAxisExtent::new(max_cross_axis_extent);
        Self {
            scroll_direction: Axis::Vertical,
            offset_source: OffsetSource::Pixels(0.0),
            shrink_wrap: false,
            grid_delegate: Arc::new(delegate),
            children: StaticChildren::new(children.into_boxed_vec()),
            children_in_boundaries: std::cell::OnceCell::new(),
            builder_source: None,
            add_repaint_boundaries: true,
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
            children: StaticChildren::new(Vec::new()),
            children_in_boundaries: std::cell::OnceCell::new(),
            builder_source: Some(SliverChildBuilderDelegate::new(item_count, builder)),
            add_repaint_boundaries: true,
        }
    }

    /// Wrap each tile in a `RepaintBoundary` (default `true`).
    ///
    /// Flutter parity: the `addRepaintBoundaries` knob its delegates carry,
    /// defaulting to `true` for the reason its doc gives — children in a
    /// scrolling container "do not need to be repainted as the list scrolls".
    /// Pass `false` when a tile is cheaper to repaint than to composite.
    #[must_use]
    pub fn repaint_boundaries(mut self, add: bool) -> Self {
        self.add_repaint_boundaries = add;
        self
    }

    /// Map an item's key to its current index in the data source — see
    /// [`ListView::find_index_by_key`](crate::ListView::find_index_by_key).
    /// Only meaningful for [`GridView::builder`].
    #[must_use]
    pub fn find_index_by_key(
        mut self,
        find: impl Fn(&dyn flui_foundation::ViewKey) -> Option<usize> + 'static,
    ) -> Self {
        if let Some(delegate) = &mut self.builder_source {
            delegate.find_index_by_key = Some(Rc::new(find));
        }
        self
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

    /// Inject a shared [`ScrollPosition`] as the composed [`Viewport`]'s (or
    /// [`ShrinkWrappingViewport`]'s, under [`GridView::shrink_wrap`]) offset
    /// — see [`Viewport::position`] for the full contract, including under
    /// shrink-wrap: [`ShrinkWrappingViewport::position`] mirrors it exactly,
    /// so `RenderShrinkWrappingViewport`'s committed content extents flush
    /// back into `position` the same way `RenderViewport`'s do. Mutually
    /// exclusive with [`GridView::offset`] — whichever is called last wins.
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
                .field("children", &self.children.len())
                .field("add_repaint_boundaries", &self.add_repaint_boundaries)
                .field(
                    "children_in_boundaries",
                    &self.children_in_boundaries.get().map(|c| c.len()),
                );
        }
        s.finish()
    }
}

impl StatelessView for GridView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // `reverse` isn't modeled on `GridView` yet, so it's always `false`
        // here — the resolution still consults ambient `Directionality` for
        // a horizontal `scroll_direction`, matching `ScrollView.getDirection`
        // (`widgets/scroll_view.dart`).
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, self.scroll_direction, false);

        let sliver: BoxedView = if let Some(ref delegate) = self.builder_source {
            // Lazy variant: wire SliverGrid (element-owned request strategy).
            let builder = if self.add_repaint_boundaries {
                super::sliver_list::wrap_builder_in_repaint_boundaries(&delegate.builder)
            } else {
                Rc::clone(&delegate.builder)
            };
            {
                let sliver = SliverGrid::new(
                    Arc::clone(&self.grid_delegate),
                    delegate.item_count,
                    builder,
                );
                match &delegate.find_index_by_key {
                    Some(find) => {
                        let find = Rc::clone(find);
                        sliver.find_index_by_key(move |key| find(key))
                    }
                    None => sliver,
                }
            }
            .boxed()
        } else {
            // Static children go through the same lazy path as a builder:
            // only the window is built, keyed children are found by the
            // delegate's key map, everything outside the window is evicted.
            let children = if self.add_repaint_boundaries {
                Rc::clone(self.children_in_boundaries.get_or_init(|| {
                    StaticChildren::mapped(
                        self.children.children().to_vec(),
                        super::sliver_list::wrap_in_repaint_boundary,
                    )
                }))
            } else {
                Rc::clone(&self.children)
            };
            SliverGrid::over(Arc::clone(&self.grid_delegate), &children).boxed()
        };

        if self.shrink_wrap {
            let viewport = ShrinkWrappingViewport::new((sliver,)).axis_direction(axis_direction);
            match &self.offset_source {
                OffsetSource::Pixels(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `ListView`'s `position_overrides_a_prior_offset_and_offset_overrides_a_prior_position`
    /// (`crates/flui-widgets/src/scroll/list_view.rs`) — `GridView` shares the
    /// same `OffsetSource` last-call-wins mechanism, but had no pinned test of
    /// its own.
    #[test]
    fn position_overrides_a_prior_offset_and_offset_overrides_a_prior_position() {
        use flui_rendering::view::ScrollPosition;

        let position = ScrollPosition::new(30.0);
        let position_debug = format!(
            "{:?}",
            GridView::count(2, Vec::<BoxedView>::new())
                .offset(12.5)
                .position(position)
        );
        assert!(
            position_debug.contains("offset_source: Position(")
                && !position_debug.contains("Pixels("),
            "the last call (.position) must win over an earlier .offset call, got: \
             {position_debug}",
        );

        let offset_debug = format!(
            "{:?}",
            GridView::count(2, Vec::<BoxedView>::new())
                .position(ScrollPosition::new(30.0))
                .offset(12.5)
        );
        assert!(
            offset_debug.contains("offset_source: Pixels(12.5)"),
            "the last call (.offset) must win over an earlier .position call, got: \
             {offset_debug}",
        );
    }
}
