//! [`ListView`] — a scrollable list of fixed-height items (static or lazy-built).

use std::fmt;
use std::rc::Rc;

use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::scroll::{
    ShrinkWrappingViewport, SliverChildBuilderDelegate, SliverFixedExtentList, SliverList, Viewport,
};

/// A scrollable list that lays out its children sequentially along
/// `scroll_direction`.
///
/// Two construction modes:
///
/// - **Static** ([`ListView::new`]): all children provided up front as a
///   `ViewSeq`, each at a fixed `item_extent`. Backed by
///   [`SliverFixedExtentList`] — cheap to lay out.
///
/// - **Lazy** ([`ListView::builder`]): children built on demand from a
///   closure, only for the viewport-visible + cache band. Backed by
///   [`SliverList`] (variable-height, element-owned). Wired into both
///   `HeadlessBinding::pump_frame` and the production `AppBinding::draw_frame`,
///   where the child-manager wiring and its test coverage converge.
///
///   **First-frame settling (Flutter divergence):** lazy children are built
///   *after* the frame's paint, so the first frame a viewport band appears it
///   paints blank; content lands on the next frame (~16 ms @ 60 fps). The
///   settling frame is automatically scheduled because layout marks the sliver
///   dirty. This is a deliberate divergence from Flutter, which builds lazy
///   children during the same-frame layout pass. See [`SliverChildBuilderDelegate`]
///   for the full rationale.
///
/// Both modes compose a [`Viewport`] over their respective sliver, or a
/// [`ShrinkWrappingViewport`] when [`ListView::shrink_wrap`] is enabled.
/// `offset` is a programmatic scroll position in logical pixels.
///
/// Flutter parity: `widgets/scroll_view.dart` `ListView` and
/// `ListView.builder`.
#[derive(Clone, StatelessView)]
pub struct ListView {
    scroll_direction: Axis,
    /// Per-item extent for the static variant ([`ListView::new`]).
    item_extent: f32,
    /// Per-item extent estimate for the lazy variant ([`ListView::builder`]).
    /// Seeds the virtualizer until real measurements arrive.
    item_extent_estimate: f32,
    offset: f32,
    shrink_wrap: bool,
    /// Children for the static variant. Empty in the lazy variant.
    children: Vec<BoxedView>,
    /// Builder delegate for the lazy variant. `None` in the static variant.
    builder_source: Option<SliverChildBuilderDelegate>,
}

impl ListView {
    /// A vertical list whose rows are each `item_extent` tall.
    ///
    /// All children are given upfront and laid out at the fixed `item_extent`.
    /// Prefer this constructor when the item count and content are known ahead
    /// of time; use [`ListView::builder`] for large or dynamically-generated
    /// lists.
    pub fn new(item_extent: f32, children: impl ViewSeq) -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            item_extent,
            item_extent_estimate: item_extent,
            offset: 0.0,
            shrink_wrap: false,
            children: children.into_boxed_vec(),
            builder_source: None,
        }
    }

    /// A vertical list that lazily builds its items from `builder`.
    ///
    /// Only the children currently visible in the viewport (plus a cache
    /// margin) are built; items that scroll out of the cache window are
    /// disposed. `item_extent_estimate` seeds the virtualizer until real
    /// measurements arrive from laid-out children.
    ///
    /// The `builder` closure receives a logical index and returns the item
    /// view, or `None` when the index is at or past the end of the data
    /// source.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent_estimate` is not finite and positive.
    pub fn builder<F>(item_count: usize, item_extent_estimate: f32, builder: F) -> Self
    where
        F: Fn(usize) -> Option<BoxedView> + 'static,
    {
        Self {
            scroll_direction: Axis::Vertical,
            item_extent: item_extent_estimate,
            item_extent_estimate,
            offset: 0.0,
            shrink_wrap: false,
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
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// Whether the list should size itself to its sliver contents in the scroll
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

impl fmt::Debug for ListView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ListView");
        s.field("scroll_direction", &self.scroll_direction)
            .field("offset", &self.offset)
            .field("shrink_wrap", &self.shrink_wrap);
        if self.builder_source.is_some() {
            s.field("item_extent_estimate", &self.item_extent_estimate);
            s.field("builder_source", &self.builder_source);
        } else {
            s.field("item_extent", &self.item_extent)
                .field("children", &self.children.len());
        }
        s.finish()
    }
}

impl StatelessView for ListView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };
        // Both arms produce `Viewport<(BoxedView,)>` by boxing the sliver so
        // the opaque return type is the same concrete type in both branches.
        //
        // `SliverList::new` is used directly (not `SliverList::builder`) because
        // `SliverList` is now defined in `flui-view` and the `builder` method
        // that accepted a `SliverChildBuilderDelegate` lived only on the old
        // widgets-side wrapper. The element's `view_type_id()` now returns
        // `TypeId::of::<SliverList>()`, fixing BLOCKER 1 (element identity).
        let sliver: BoxedView = if let Some(ref delegate) = self.builder_source {
            SliverList::new(
                delegate.item_count,
                self.item_extent_estimate,
                Rc::clone(&delegate.builder),
            )
            .boxed()
        } else {
            SliverFixedExtentList::new(self.item_extent, self.children.clone()).boxed()
        };
        if self.shrink_wrap {
            ShrinkWrappingViewport::new((sliver,))
                .axis_direction(axis_direction)
                .offset(self.offset)
                .boxed()
        } else {
            Viewport::new((sliver,))
                .axis_direction(axis_direction)
                .offset(self.offset)
                .boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SizedBox;

    #[test]
    fn new_defaults_to_vertical_zero_offset_not_shrink_wrapped() {
        let debug = format!("{:?}", ListView::new(50.0, Vec::<BoxedView>::new()));
        assert!(
            debug.contains("scroll_direction: Vertical")
                && debug.contains("offset: 0.0")
                && debug.contains("shrink_wrap: false")
                && debug.contains("item_extent: 50.0"),
            "Debug output must reflect the static constructor's defaults, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_children_count_for_the_static_variant() {
        let debug = format!(
            "{:?}",
            ListView::new(50.0, vec![SizedBox::shrink().boxed()])
        );
        assert!(
            debug.contains("children: 1"),
            "static ListView's Debug output must report the child count, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_item_extent_estimate_and_builder_source_for_the_lazy_variant() {
        let debug = format!(
            "{:?}",
            ListView::builder(3, 60.0, |index| (index < 3)
                .then(|| SizedBox::shrink().boxed()))
        );
        assert!(
            debug.contains("item_extent_estimate: 60.0") && debug.contains("builder_source:"),
            "lazy ListView's Debug output must report the estimate and builder \
             source instead of a static child count, got: {debug}",
        );
        assert!(
            !debug.contains("children:"),
            "lazy ListView's Debug output must not report a static children \
             count, got: {debug}",
        );
    }

    #[test]
    fn builder_methods_override_scroll_direction_offset_and_shrink_wrap() {
        let debug = format!(
            "{:?}",
            ListView::new(50.0, Vec::<BoxedView>::new())
                .scroll_direction(Axis::Horizontal)
                .offset(12.5)
                .shrink_wrap(true)
        );
        assert!(
            debug.contains("scroll_direction: Horizontal")
                && debug.contains("offset: 12.5")
                && debug.contains("shrink_wrap: true"),
            "Debug output must reflect the overridden builder values, got: {debug}",
        );
    }
}
