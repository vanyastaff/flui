//! [`Viewport`] — a box that lays out a sequence of *sliver* children along a
//! scroll axis, showing a window into them at a scroll offset.

use std::fmt;

use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
use flui_rendering::protocol::BoxProtocol;
use flui_types::layout::AxisDirection;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// A box render-object widget that drives a sequence of **sliver** children
/// (e.g. [`SliverToBoxAdapter`](crate::SliverToBoxAdapter)) along a scroll axis,
/// clipping them to its own bounds at a scroll offset.
///
/// Flutter parity: `widgets/viewport.dart` `Viewport` over `RenderViewport`.
/// The viewport sizes to its (bounded) incoming constraints — place it under a
/// bounded main-axis constraint, not directly inside an unbounded `Column`.
/// `offset` is a programmatic scroll position in logical pixels (interactive
/// drag-to-scroll arrives with the `Scrollable`/`ScrollController` layer).
///
/// Generic over `C: ViewSeq` of sliver child views.
#[derive(Clone)]
pub struct Viewport<C = Vec<BoxedView>> {
    axis_direction: AxisDirection,
    offset: f32,
    children: C,
}

impl<C> Viewport<C> {
    /// A vertical viewport (scrolls top-to-bottom) over `children`.
    pub fn new(children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            offset: 0.0,
            children,
        }
    }

    /// Set the scroll axis direction (default [`AxisDirection::TopToBottom`]).
    #[must_use]
    pub fn axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    /// Set the programmatic scroll offset in logical pixels.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    fn build_render_object(&self) -> RenderViewport {
        let mut render_object = RenderViewport::new(self.axis_direction);
        render_object.offset_mut().set_pixels(self.offset);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for Viewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Viewport")
            .field("axis_direction", &self.axis_direction)
            .field("offset", &self.offset)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Viewport<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderViewport;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        // Push the axis through on rebuild (reconciliation reuses the render
        // object), not just the scroll offset — otherwise a vertical↔horizontal
        // change keeps the stale axis from construction.
        render_object.set_axis_direction(self.axis_direction);
        render_object.offset_mut().set_pixels(self.offset);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Viewport);

/// A box render-object widget that shrink-wraps a sequence of **sliver**
/// children in the scroll axis.
///
/// Flutter parity: `widgets/viewport.dart` `ShrinkWrappingViewport` over
/// `RenderShrinkWrappingViewport`. It expands in the cross axis but takes its
/// main-axis size from the accumulated sliver content, constrained by its
/// parent.
#[derive(Clone)]
pub struct ShrinkWrappingViewport<C = Vec<BoxedView>> {
    axis_direction: AxisDirection,
    offset: f32,
    children: C,
}

impl<C> ShrinkWrappingViewport<C> {
    /// A vertical shrink-wrapping viewport over `children`.
    pub fn new(children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            offset: 0.0,
            children,
        }
    }

    /// Set the scroll axis direction (default [`AxisDirection::TopToBottom`]).
    #[must_use]
    pub fn axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    /// Set the programmatic scroll offset in logical pixels.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    fn build_render_object(&self) -> RenderShrinkWrappingViewport {
        let mut render_object = RenderShrinkWrappingViewport::new(self.axis_direction);
        render_object.offset_mut().set_pixels(self.offset);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for ShrinkWrappingViewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShrinkWrappingViewport")
            .field("axis_direction", &self.axis_direction)
            .field("offset", &self.offset)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for ShrinkWrappingViewport<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderShrinkWrappingViewport;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        // Reconciliation reuses the render object across rebuilds, so a
        // vertical↔horizontal axis change on the widget must be pushed through
        // (not just the scroll offset) — otherwise layout keeps the stale axis
        // from construction.
        render_object.set_axis_direction(self.axis_direction);
        render_object.offset_mut().set_pixels(self.offset);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ShrinkWrappingViewport);
