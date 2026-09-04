//! [`Viewport`] — a box that lays out a sequence of *sliver* children along a
//! scroll axis, showing a window into them at a scroll offset.

use std::fmt;

use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
use flui_rendering::protocol::BoxProtocol;
use flui_rendering::view::{CacheExtentStyle, ScrollPosition, SliverPaintOrder};
use flui_types::layout::{Axis, AxisDirection};
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Where a [`Viewport`] or [`ShrinkWrappingViewport`]'s render object gets
/// its scroll offset from. Shared by both widgets — they're two `RenderBox`
/// front ends over the same `ViewportOffset` injection mechanics, just with
/// different `RenderObject`s (`RenderViewport` vs
/// `RenderShrinkWrappingViewport`) underneath.
///
/// - `Pixels`: the widget owns a private `ScrollPosition` and pushes this
///   value into it on every rebuild — today's programmatic-offset behavior,
///   with no external subscriber.
/// - `Position`: an external `ScrollPosition` (typically a
///   `ScrollController`'s) is injected directly. Gestures write it, and the
///   render object's committed content extents (`RenderViewport` or
///   `RenderShrinkWrappingViewport::perform_layout`) flush back into it — the
///   content-dimension feedback loop.
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
    cache_extent: Option<(f32, CacheExtentStyle)>,
    paint_order: SliverPaintOrder,
    anchor: f32,
    center: Option<usize>,
    children: C,
}

impl<C> Viewport<C> {
    /// A vertical viewport (scrolls top-to-bottom) over `children`.
    pub fn new(children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            offset_source: OffsetSource::Pixels(0.0),
            cache_extent: None,
            paint_order: SliverPaintOrder::FirstIsTop,
            anchor: 0.0,
            center: None,
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

    /// Set how far beyond the visible viewport to keep slivers laid out and
    /// painted (`RenderViewport::set_cache_extent`'s passthrough — the
    /// render object has always supported this; this widget just lacked the
    /// builder). `None` (the default) keeps the render object's own default.
    #[must_use]
    pub fn cache_extent(mut self, cache_extent: f32, style: CacheExtentStyle) -> Self {
        self.cache_extent = Some((cache_extent, style));
        self
    }

    /// Set the sliver paint order (default [`SliverPaintOrder::FirstIsTop`]).
    /// Hit testing uses the opposite order — see
    /// [`RenderViewport::set_paint_order`](flui_objects::RenderViewport::set_paint_order).
    #[must_use]
    pub fn paint_order(mut self, paint_order: SliverPaintOrder) -> Self {
        self.paint_order = paint_order;
        self
    }

    /// Set where the zero-scroll line sits along the main axis, as a
    /// fraction of the viewport's extent from the leading edge (default
    /// `0.0`). Flutter's `Viewport.anchor`; see
    /// [`RenderViewport::set_anchor`](flui_objects::RenderViewport::set_anchor)
    /// for the formulas this drives.
    #[must_use]
    pub fn anchor(mut self, anchor: f32) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the index of the first forward child (Flutter's `Viewport.center`,
    /// index-based here — a key-based `center` is a follow-up). `None` (the
    /// default) means every child grows forward from the leading edge; see
    /// [`RenderViewport::set_center`](flui_objects::RenderViewport::set_center)
    /// for the full contract, including why an out-of-range index is invalid.
    #[must_use]
    pub fn center(mut self, center: Option<usize>) -> Self {
        self.center = center;
        self
    }

    fn build_render_object(&self) -> RenderViewport<ScrollPosition> {
        let cross_axis_direction = default_cross_axis_direction(self.axis_direction);
        let position = match &self.offset_source {
            OffsetSource::Pixels(pixels) => ScrollPosition::new(*pixels),
            OffsetSource::Position(position) => position.clone(),
        };
        let mut render_object =
            RenderViewport::with_offset(self.axis_direction, cross_axis_direction, position);
        if let Some((extent, style)) = self.cache_extent {
            // The returned impact is deliberately dropped. This runs before the
            // render object joins a tree, so there is nothing to invalidate —
            // and a caller may legitimately pass the render object's own
            // default (250.0 logical pixels, `Pixel` style), for which
            // `set_cache_extent` correctly reports `NONE`. Asserting `LAYOUT`
            // here made that call panic in every debug and test build.
            let _ = render_object.set_cache_extent(extent, style);
        }
        // Same rationale: a caller may pass the render object's own defaults
        // (`FirstIsTop`, `0.0`, `None`), for which each setter correctly
        // reports `NONE` before the node has even joined a tree.
        let _ = render_object.set_paint_order(self.paint_order);
        let _ = render_object.set_anchor(self.anchor);
        let _ = render_object.set_center(self.center);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for Viewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Viewport")
            .field("axis_direction", &self.axis_direction)
            .field("offset_source", &self.offset_source)
            .field("cache_extent", &self.cache_extent)
            .field("paint_order", &self.paint_order)
            .field("anchor", &self.anchor)
            .field("center", &self.center)
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
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE; // Push the axis through on rebuild (reconciliation reuses the render
        // object), not just the scroll offset — otherwise a vertical↔horizontal
        // change keeps the stale axis from construction.
        impact |= render_object.set_axis_direction(self.axis_direction);
        if let Some((extent, style)) = self.cache_extent {
            impact |= render_object.set_cache_extent(extent, style);
        }
        impact |= render_object.set_paint_order(self.paint_order);
        impact |= render_object.set_anchor(self.anchor);
        impact |= render_object.set_center(self.center);
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
                    impact |= render_object.set_offset(ScrollPosition::new(*pixels));
                }
            }
            OffsetSource::Position(position) => {
                // Swap in the injected position only on an actual identity
                // change. Never push pixels here: the shared position is
                // written directly by gestures/`ScrollController`, so
                // pushing a rebuild-time value would stomp live drag state.
                if !render_object.offset().ptr_eq(position) {
                    impact |= render_object.set_offset(position.clone());
                }
            }
        }
        impact
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
///
/// Mirrors [`Viewport`]'s `Pixels`-vs-`Position` `offset_source` mechanics —
/// see [`ShrinkWrappingViewport::position`] for the injection contract.
#[derive(Clone)]
pub struct ShrinkWrappingViewport<C = Vec<BoxedView>> {
    axis_direction: AxisDirection,
    offset_source: OffsetSource,
    paint_order: SliverPaintOrder,
    children: C,
}

impl<C> ShrinkWrappingViewport<C> {
    /// A vertical shrink-wrapping viewport over `children`.
    pub fn new(children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            offset_source: OffsetSource::Pixels(0.0),
            paint_order: SliverPaintOrder::FirstIsTop,
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
    /// exclusive with [`ShrinkWrappingViewport::position`] — whichever is
    /// called last wins.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        self.offset_source = OffsetSource::Pixels(offset);
        self
    }

    /// Inject a shared [`ScrollPosition`] as the render object's offset.
    ///
    /// Position mode: the render object's offset IS `position` — a gesture
    /// handler or `ScrollController` writing to the same `ScrollPosition` is
    /// observed directly (no push from this widget), and
    /// `RenderShrinkWrappingViewport::perform_layout`'s committed content
    /// extents flush back into it. Mutually exclusive with
    /// [`ShrinkWrappingViewport::offset`] — whichever is called last wins.
    #[must_use]
    pub fn position(mut self, position: ScrollPosition) -> Self {
        self.offset_source = OffsetSource::Position(position);
        self
    }

    /// Set the sliver paint order (default [`SliverPaintOrder::FirstIsTop`]).
    /// Hit testing uses the opposite order — see
    /// [`RenderShrinkWrappingViewport::set_paint_order`](flui_objects::RenderShrinkWrappingViewport::set_paint_order).
    #[must_use]
    pub fn paint_order(mut self, paint_order: SliverPaintOrder) -> Self {
        self.paint_order = paint_order;
        self
    }

    fn build_render_object(&self) -> RenderShrinkWrappingViewport<ScrollPosition> {
        let cross_axis_direction = default_cross_axis_direction(self.axis_direction);
        let position = match &self.offset_source {
            OffsetSource::Pixels(pixels) => ScrollPosition::new(*pixels),
            OffsetSource::Position(position) => position.clone(),
        };
        let mut render_object = RenderShrinkWrappingViewport::with_offset(
            self.axis_direction,
            cross_axis_direction,
            position,
        );
        // The setter runs; its returned impact is what is dropped. This is
        // before the node joins a tree, so there is nothing to invalidate —
        // and a caller may pass the render object's own default
        // (`FirstIsTop`), for which the setter correctly reports `NONE`.
        let _ = render_object.set_paint_order(self.paint_order);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for ShrinkWrappingViewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShrinkWrappingViewport")
            .field("axis_direction", &self.axis_direction)
            .field("offset_source", &self.offset_source)
            .field("paint_order", &self.paint_order)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for ShrinkWrappingViewport<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderShrinkWrappingViewport<ScrollPosition>;

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
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE; // Reconciliation reuses the render object across rebuilds, so a
        // vertical↔horizontal axis change on the widget must be pushed through
        // (not just the scroll offset) — otherwise layout keeps the stale axis
        // from construction.
        impact |= render_object.set_axis_direction(self.axis_direction);
        impact |= render_object.set_paint_order(self.paint_order);
        match &self.offset_source {
            OffsetSource::Pixels(pixels) => {
                // See `Viewport::update_render_object`'s matching arm for the
                // full rationale: only push into the installed offset when it
                // is still privately (uniquely) held, otherwise a mode switch
                // away from a prior Position-mode build would stomp the
                // foreign, externally shared `ScrollPosition`.
                if render_object.offset().is_uniquely_held() {
                    render_object.offset().set_pixels(*pixels);
                } else {
                    impact |= render_object.set_offset(ScrollPosition::new(*pixels));
                }
            }
            OffsetSource::Position(position) => {
                // Swap in the injected position only on an actual identity
                // change — see `Viewport::update_render_object`'s matching arm.
                if !render_object.offset().ptr_eq(position) {
                    impact |= render_object.set_offset(position.clone());
                }
            }
        }
        impact
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ShrinkWrappingViewport);

#[cfg(test)]
mod tests {
    use super::*;

    /// A caller may be explicit about the value the render object already
    /// defaults to. `set_cache_extent` correctly reports `NONE` for that, and
    /// `build_render_object` must not treat "no change" as a contract
    /// violation — an earlier `debug_assert_eq!(.., LAYOUT)` here panicked on
    /// this exact call in every debug and test build.
    #[test]
    fn an_explicit_cache_extent_equal_to_the_default_builds_without_panicking() {
        let viewport = Viewport::new(()).cache_extent(250.0, CacheExtentStyle::Pixel);
        let _render_object = viewport.build_render_object();
    }
}
