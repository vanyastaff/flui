//! [`Viewport`] — a box that lays out a sequence of *sliver* children along a
//! scroll axis, showing a window into them at a scroll offset.

use std::fmt;

use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
use flui_rendering::protocol::BoxProtocol;
use flui_rendering::view::ScrollPosition;
use flui_types::layout::{Axis, AxisDirection};
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Where a [`Viewport`]'s render object gets its scroll offset from.
///
/// - `Pixels`: the widget owns a private `ScrollPosition` and pushes this
///   value into it on every rebuild — today's programmatic-offset behavior,
///   with no external subscriber.
/// - `Position`: an external `ScrollPosition` (typically a
///   `ScrollController`'s) is injected directly. Gestures write it, and
///   `RenderViewport::perform_layout`'s committed content extents flush back
///   into it — the content-dimension feedback loop.
#[derive(Clone, Debug)]
enum OffsetSource {
    Pixels(f32),
    Position(ScrollPosition),
}

/// The default cross-axis direction for a given scroll `axis_direction` —
/// horizontal scroll axes lay their cross axis top-to-bottom, vertical axes
/// lay theirs left-to-right. Mirrors `RenderViewport::new`'s own derivation
/// (`flui_objects::sliver::viewport`'s private `default_cross_axis_direction`);
/// duplicated here because `Viewport<ScrollPosition>` has no `::new`
/// convenience constructor to inherit it from — only the 3-arg `with_offset`
/// injection constructor, which takes both directions explicitly.
fn default_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction.axis() {
        Axis::Horizontal => AxisDirection::TopToBottom,
        Axis::Vertical => AxisDirection::LeftToRight,
    }
}

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
    offset_source: OffsetSource,
    children: C,
}

impl<C> Viewport<C> {
    /// A vertical viewport (scrolls top-to-bottom) over `children`.
    pub fn new(children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            offset_source: OffsetSource::Pixels(0.0),
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
    ///
    /// Pixels mode: the render object's offset is a private `ScrollPosition`
    /// this widget owns and pushes `offset` into on every rebuild. Mutually
    /// exclusive with [`Viewport::position`] — whichever is called last wins.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset_source = OffsetSource::Pixels(offset);
        self
    }

    /// Inject a shared [`ScrollPosition`] as the render object's offset.
    ///
    /// Position mode: the render object's offset IS `position` — a
    /// gesture handler or `ScrollController` writing to the same
    /// `ScrollPosition` is observed directly (no push from this widget), and
    /// `RenderViewport::perform_layout`'s committed content extents flush
    /// back into it. Mutually exclusive with [`Viewport::offset`] —
    /// whichever is called last wins.
    #[must_use]
    pub fn position(mut self, position: ScrollPosition) -> Self {
        self.offset_source = OffsetSource::Position(position);
        self
    }

    fn build_render_object(&self) -> RenderViewport<ScrollPosition> {
        let cross_axis_direction = default_cross_axis_direction(self.axis_direction);
        let position = match &self.offset_source {
            OffsetSource::Pixels(pixels) => ScrollPosition::new(*pixels),
            OffsetSource::Position(position) => position.clone(),
        };
        RenderViewport::with_offset(self.axis_direction, cross_axis_direction, position)
    }
}

impl<C: ViewSeq> fmt::Debug for Viewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Viewport")
            .field("axis_direction", &self.axis_direction)
            .field("offset_source", &self.offset_source)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Viewport<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderViewport<ScrollPosition>;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        // Push the axis through on rebuild (reconciliation reuses the render
        // object), not just the scroll offset — otherwise a vertical↔horizontal
        // change keeps the stale axis from construction.
        render_object.set_axis_direction(self.axis_direction);
        match &self.offset_source {
            OffsetSource::Pixels(pixels) => {
                // Compat with today's behavior: push the new value into the
                // widget-owned position every rebuild — UNLESS the position
                // currently installed is a foreign one left over from a
                // PRIOR Position-mode build (a mode switch on the same
                // render object; `update_render_object` only sees this
                // rebuild's config, not the previous one's, so identity is
                // the only signal available). Pushing into a foreign,
                // externally shared `ScrollPosition` would stomp whatever
                // the controller/gesture side holds. `is_uniquely_held`
                // distinguishes the two: a private, widget-owned position
                // has no other clone alive; an injected one is always
                // shared with at least the controller that owns it.
                if render_object.offset().is_uniquely_held() {
                    render_object.offset().set_pixels(*pixels);
                } else {
                    render_object.set_offset(ScrollPosition::new(*pixels));
                }
            }
            OffsetSource::Position(position) => {
                // Swap in the injected position only on an actual identity
                // change. Never push pixels here: the shared position is
                // written directly by gestures/`ScrollController`, so
                // pushing a rebuild-time value would stomp live drag state.
                if !render_object.offset().ptr_eq(position) {
                    render_object.set_offset(position.clone());
                }
            }
        }
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
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderShrinkWrappingViewport;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
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
