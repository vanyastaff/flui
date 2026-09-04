//! [`CustomScrollView`] — a viewport over an arbitrary sequence of sliver
//! children.

use std::fmt;

use flui_rendering::view::SliverPaintOrder;
use flui_types::layout::Axis;
use flui_types::painting::Clip;
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
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
/// A horizontal `scroll_direction` resolves its `AxisDirection` from the
/// ambient [`Directionality`] (`RightToLeft` under an RTL ancestor), matching
/// `ScrollView.getDirection` (`widgets/scroll_view.dart`); the vertical axis
/// never consults it. `CustomScrollView` has no `reverse` flag yet, unlike
/// [`SingleChildScrollView`].
///
/// Flutter parity: `widgets/scroll_view.dart` `CustomScrollView`.
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
    offset: f32,
    shrink_wrap: bool,
    center: Option<usize>,
    anchor: f32,
    paint_order: SliverPaintOrder,
    clip_behavior: Clip,
    slivers: Vec<BoxedView>,
}

impl CustomScrollView {
    /// A vertical custom scroll view over `slivers`.
    pub fn new(slivers: impl ViewSeq) -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            offset: 0.0,
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

    /// Set the index of the first forward child (Flutter's
    /// `ScrollView.center`, index-based here — a key-based `center` is a
    /// follow-up). Has no effect when [`CustomScrollView::shrink_wrap`] is
    /// set: Flutter's `ShrinkWrappingViewport` has no `center` either. See
    /// [`Viewport::center`] for the full contract.
    #[must_use]
    pub fn center(mut self, center: Option<usize>) -> Self {
        self.center = center;
        self
    }

    /// Set where the zero-scroll line sits along the main axis, as a
    /// fraction of the viewport's extent from the leading edge (default
    /// `0.0`). Has no effect when [`CustomScrollView::shrink_wrap`] is set:
    /// Flutter's `ShrinkWrappingViewport` has no `anchor` either. See
    /// [`Viewport::anchor`] for the formulas this drives.
    #[must_use]
    pub fn anchor(mut self, anchor: f32) -> Self {
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
    /// [`CustomScrollView::shrink_wrap`] is set. Flutter's
    /// `ScrollView.clipBehavior`.
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
            .field("offset", &self.offset)
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
        // horizontal `scroll_direction`, matching `ScrollView.getDirection`
        // (`widgets/scroll_view.dart`).
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, self.scroll_direction, false);
        if self.shrink_wrap {
            ShrinkWrappingViewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .offset(self.offset)
                .paint_order(self.paint_order)
                .clip_behavior(self.clip_behavior)
                .boxed()
        } else {
            Viewport::new(self.slivers.clone())
                .axis_direction(axis_direction)
                .offset(self.offset)
                .center(self.center)
                .anchor(self.anchor)
                .paint_order(self.paint_order)
                .clip_behavior(self.clip_behavior)
                .boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_view::BoxedView;

    use super::*;

    fn empty_scroll_view() -> CustomScrollView {
        CustomScrollView::new(Vec::<BoxedView>::new())
    }

    #[test]
    fn new_defaults_to_vertical_zero_offset_not_shrink_wrapped() {
        let debug = format!("{:?}", empty_scroll_view());
        assert!(
            debug.contains("scroll_direction: Vertical")
                && debug.contains("offset: 0.0")
                && debug.contains("shrink_wrap: false"),
            "Debug output must reflect Flutter's CustomScrollView defaults, got: {debug}",
        );
    }

    #[test]
    fn builder_methods_override_scroll_direction_offset_and_shrink_wrap() {
        let debug = format!(
            "{:?}",
            empty_scroll_view()
                .scroll_direction(Axis::Horizontal)
                .offset(42.5)
                .shrink_wrap(true)
        );
        assert!(
            debug.contains("scroll_direction: Horizontal")
                && debug.contains("offset: 42.5")
                && debug.contains("shrink_wrap: true"),
            "Debug output must reflect the overridden builder values, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_the_sliver_count() {
        let root = CustomScrollView::new(vec![
            crate::SliverToBoxAdapter::new().boxed(),
            crate::SliverToBoxAdapter::new().boxed(),
        ]);
        let debug = format!("{root:?}");
        assert!(
            debug.contains("sliver_count: 2"),
            "Debug output must report the sliver child count, got: {debug}",
        );
    }
}
