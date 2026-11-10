//! [`CustomScrollView`] — a scroll view over an arbitrary sequence of sliver
//! children.

use std::fmt;
use std::rc::Rc;

use flui_painting::paint::Clip;
use flui_rendering::view::{ScrollPosition, SliverPaintOrder};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::{ScrollView, ScrollViewConfig, Scrollable, ShrinkWrappingViewport, Viewport};

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
/// It scrolls itself through the [`Scrollable`] it composes; its
/// controller, axis, `reverse` and gesture policy are the [`ScrollView`]
/// builders. Set [`CustomScrollView::shrink_wrap`] when the scroll view is
/// placed under unbounded constraints in the scroll axis.
///
/// A horizontal axis resolves its `AxisDirection` from the ambient
/// [`Directionality`] (`RightToLeft` under an RTL ancestor); the vertical
/// axis never consults it. [`ScrollView::reverse`] flips whichever direction
/// that resolves to.
///
/// [`Directionality`]: crate::Directionality
/// [`ListView`]: crate::ListView
/// [`GridView`]: crate::GridView
/// [`SliverToBoxAdapter`]: crate::SliverToBoxAdapter
/// [`SliverFixedExtentList`]: crate::SliverFixedExtentList
/// [`SliverFillRemaining`]: crate::SliverFillRemaining
#[derive(Clone, StatelessView)]
pub struct CustomScrollView {
    config: ScrollViewConfig,
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
            config: ScrollViewConfig::default(),
            shrink_wrap: false,
            center: None,
            anchor: 0.0,
            paint_order: SliverPaintOrder::FirstIsTop,
            clip_behavior: Clip::HardEdge,
            slivers: slivers.into_boxed_vec(),
        }
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

impl ScrollView for CustomScrollView {
    fn scroll_config_mut(&mut self) -> &mut ScrollViewConfig {
        &mut self.config
    }
}

impl fmt::Debug for CustomScrollView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomScrollView")
            .field("config", &self.config)
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
        let axis_direction = axis_direction_from_axis_reverse_and_directionality(
            ctx,
            self.config.scroll_direction,
            self.config.reverse,
        );
        let view = self.clone();
        Scrollable::with_config(
            self.config.clone(),
            Rc::new(move |position: ScrollPosition| {
                if view.shrink_wrap {
                    ShrinkWrappingViewport::new(position, view.slivers.clone())
                        .axis_direction(axis_direction)
                        .paint_order(view.paint_order)
                        .clip_behavior(view.clip_behavior)
                        .boxed()
                } else {
                    Viewport::new(position, view.slivers.clone())
                        .axis_direction(axis_direction)
                        .center(view.center)
                        .anchor(view.anchor)
                        .paint_order(view.paint_order)
                        .clip_behavior(view.clip_behavior)
                        .boxed()
                }
            }),
        )
    }
}
