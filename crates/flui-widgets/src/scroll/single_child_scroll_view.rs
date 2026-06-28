//! [`SingleChildScrollView`] — makes a single child scrollable along one axis.

use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, Child, IntoView};

use crate::scroll::{SliverToBoxAdapter, Viewport};

/// A box that lets its single child be larger than the available space along
/// `scroll_direction`, showing a scrollable window into it.
///
/// Flutter parity: `widgets/scroll_view.dart` `SingleChildScrollView`. Composes
/// a [`Viewport`] over a
/// [`SliverToBoxAdapter`]: the child is laid out
/// unbounded on the scroll axis and the viewport clips the overflow.
///
/// `scroll_direction` defaults to [`Axis::Vertical`]. `offset` is a programmatic
/// scroll position; gesture-driven scrolling arrives with the
/// `Scrollable`/`ScrollController` layer.
#[derive(Clone, Debug, StatelessView)]
pub struct SingleChildScrollView {
    scroll_direction: Axis,
    offset: f32,
    child: Child,
}

impl Default for SingleChildScrollView {
    fn default() -> Self {
        Self {
            scroll_direction: Axis::Vertical,
            offset: 0.0,
            child: Child::empty(),
        }
    }
}

impl SingleChildScrollView {
    /// A vertical scroll view with no child yet.
    pub fn new() -> Self {
        Self::default()
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

    /// Set the scrollable child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatelessView for SingleChildScrollView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = match self.scroll_direction {
            Axis::Vertical => AxisDirection::TopToBottom,
            Axis::Horizontal => AxisDirection::LeftToRight,
        };
        let adapter = match self.child.clone().into_inner() {
            Some(boxed) => SliverToBoxAdapter::new().child(boxed),
            None => SliverToBoxAdapter::new(),
        };
        Viewport::new((adapter,))
            .axis_direction(axis_direction)
            .offset(self.offset)
    }
}
