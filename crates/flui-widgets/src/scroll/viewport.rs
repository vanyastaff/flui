//! [`Viewport`] — a box that lays out a sequence of *sliver* children along a
//! scroll axis, showing a window into them at a scroll offset.

use std::fmt;

use flui_foundation::geometry::Axis;
use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
use flui_painting::paint::Clip;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_rendering::view::{
    CacheExtentStyle, ScrollPosition, SliverPaintOrder, ViewportOffset as _,
};
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::__private::generic_render_view_element;
use crate::localization::{Directionality, resolve_horizontal_axis_direction};

/// The cross-axis direction for a scroll `axis_direction`. A horizontal scroll
/// axis lays its cross axis top-to-bottom and never calls `text_direction`, so
/// it registers no `Directionality` dependency; a vertical one lays its
/// horizontal cross axis from the reading-start edge, the rule
/// `resolve_horizontal_axis_direction` holds for every horizontal axis.
fn cross_axis_direction(
    axis_direction: AxisDirection,
    text_direction: impl FnOnce() -> TextDirection,
) -> AxisDirection {
    match axis_direction.axis() {
        Axis::Horizontal => AxisDirection::TopToBottom,
        Axis::Vertical => resolve_horizontal_axis_direction(text_direction(), false),
    }
}

/// [`cross_axis_direction`] against the ambient [`Directionality`].
fn resolve_cross_axis_direction(
    axis_direction: AxisDirection,
    ctx: &flui_view::RenderObjectContext<'_>,
) -> AxisDirection {
    cross_axis_direction(axis_direction, || Directionality::of_render_object(ctx))
}

/// A box render-object widget that drives a sequence of **sliver** children
/// (e.g. [`SliverToBoxAdapter`](crate::SliverToBoxAdapter)) along a scroll axis,
/// clipping them to its own bounds at a scroll offset.
///
/// Backed by `RenderViewport`. The viewport sizes to its (bounded) incoming constraints — place it under a
/// bounded main-axis constraint, not directly inside an unbounded `Column`.
///
/// A viewport is passive: it shows its slivers at the offset of the
/// [`ScrollPosition`] it is given and never moves it. The standard scroll
/// views ([`CustomScrollView`](crate::CustomScrollView),
/// [`ListView`](crate::ListView), …) compose one under a
/// [`Scrollable`](crate::Scrollable) that does; a viewport of your own goes
/// in [`Scrollable::new`](crate::Scrollable::new)'s closure.
///
/// Generic over `C: ViewSeq` of sliver child views.
#[derive(Clone)]
pub struct Viewport<C = Vec<BoxedView>> {
    axis_direction: AxisDirection,
    position: ScrollPosition,
    cache_extent: Option<(f64, CacheExtentStyle)>,
    paint_order: SliverPaintOrder,
    anchor: f64,
    center: Option<usize>,
    clip_behavior: Clip,
    children: C,
}

impl<C> Viewport<C> {
    /// A vertical viewport (scrolls top-to-bottom) over `children`, at the
    /// offset of `position`.
    ///
    /// The render object's offset IS `position`: whatever writes it (a
    /// [`Scrollable`](crate::Scrollable)'s gestures, a
    /// [`ScrollController`](crate::ScrollController)) is observed directly,
    /// with no push from this widget, and `RenderViewport::perform_layout`'s
    /// committed content extents flush back into it. A rebuild with another
    /// position (by identity) installs it; one that never moves is
    /// [`ScrollPosition::fixed`], kept across builds by whoever holds it.
    pub fn new(position: ScrollPosition, children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            position,
            cache_extent: None,
            paint_order: SliverPaintOrder::FirstIsTop,
            anchor: 0.0,
            center: None,
            clip_behavior: Clip::HardEdge,
            children,
        }
    }

    /// Set the scroll axis direction (default [`AxisDirection::TopToBottom`]).
    #[must_use]
    pub fn axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    /// Set how far beyond the visible viewport to keep slivers laid out and
    /// painted (`RenderViewport::set_cache_extent`'s passthrough — the
    /// render object has always supported this; this widget just lacked the
    /// builder). `None` (the default) keeps the render object's own default.
    #[must_use]
    pub fn cache_extent(mut self, cache_extent: f64, style: CacheExtentStyle) -> Self {
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
    /// `0.0`). See
    /// [`RenderViewport::set_anchor`](flui_objects::RenderViewport::set_anchor)
    /// for the formulas this drives.
    #[must_use]
    pub fn anchor(mut self, anchor: f64) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the index of the first forward child (index-based; a key-based
    /// `center` is a follow-up). `None` (the
    /// default) means every child grows forward from the leading edge; see
    /// [`RenderViewport::set_center`](flui_objects::RenderViewport::set_center)
    /// for the full contract, including why an out-of-range index is invalid.
    #[must_use]
    pub fn center(mut self, center: Option<usize>) -> Self {
        self.center = center;
        self
    }

    /// Set how overflowing content is clipped (default [`Clip::HardEdge`]).
    /// [`Clip::None`] clips nothing, so a child may paint outside the
    /// viewport's bounds.
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    fn build_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
    ) -> RenderViewport<ScrollPosition> {
        let cross_axis_direction = resolve_cross_axis_direction(self.axis_direction, ctx);
        let mut render_object = RenderViewport::with_offset(
            self.axis_direction,
            cross_axis_direction,
            self.position.clone(),
        );
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
        let _ = render_object.set_clip_behavior(self.clip_behavior);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for Viewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Viewport")
            .field("axis_direction", &self.axis_direction)
            .field("position", &self.position.pixels())
            .field("cache_extent", &self.cache_extent)
            .field("paint_order", &self.paint_order)
            .field("anchor", &self.anchor)
            .field("center", &self.center)
            .field("clip_behavior", &self.clip_behavior)
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

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.build_render_object(ctx)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE; // Push the axis through on rebuild (reconciliation reuses the render
        // object), not just the scroll offset — otherwise a vertical↔horizontal
        // change keeps the stale axis from construction.
        impact |= render_object.set_axis_direction(self.axis_direction);
        // The cross axis is direction-aware: re-resolve it (a vertical viewport
        // flips its horizontal cross axis under RTL), after `set_axis_direction`
        // laid down the LTR default.
        impact |= render_object
            .set_cross_axis_direction(resolve_cross_axis_direction(self.axis_direction, ctx));
        if let Some((extent, style)) = self.cache_extent {
            impact |= render_object.set_cache_extent(extent, style);
        }
        impact |= render_object.set_paint_order(self.paint_order);
        impact |= render_object.set_anchor(self.anchor);
        impact |= render_object.set_center(self.center);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        // Swap in the position only on an actual identity change. Never push
        // pixels here: the shared position is written directly by its writer,
        // so pushing a rebuild-time value would stomp live drag state.
        if !render_object.offset().ptr_eq(&self.position) {
            impact |= render_object.set_offset(self.position.clone());
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
/// Backed by `RenderShrinkWrappingViewport`. It expands in the cross axis but takes its
/// main-axis size from the accumulated sliver content, constrained by its
/// parent.
///
/// Passive like [`Viewport`]: it shows its slivers at the offset of the
/// position it is given.
#[derive(Clone)]
pub struct ShrinkWrappingViewport<C = Vec<BoxedView>> {
    axis_direction: AxisDirection,
    position: ScrollPosition,
    paint_order: SliverPaintOrder,
    clip_behavior: Clip,
    children: C,
}

impl<C> ShrinkWrappingViewport<C> {
    /// A vertical shrink-wrapping viewport over `children`, at the offset of
    /// `position` — the contract of [`Viewport::new`].
    pub fn new(position: ScrollPosition, children: C) -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            position,
            paint_order: SliverPaintOrder::FirstIsTop,
            clip_behavior: Clip::HardEdge,
            children,
        }
    }

    /// Set the scroll axis direction (default [`AxisDirection::TopToBottom`]).
    #[must_use]
    pub fn axis_direction(mut self, axis_direction: AxisDirection) -> Self {
        self.axis_direction = axis_direction;
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

    /// Set how overflowing content is clipped (default [`Clip::HardEdge`]).
    /// [`Clip::None`] clips nothing.
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    fn build_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
    ) -> RenderShrinkWrappingViewport<ScrollPosition> {
        let cross_axis_direction = resolve_cross_axis_direction(self.axis_direction, ctx);
        let mut render_object = RenderShrinkWrappingViewport::with_offset(
            self.axis_direction,
            cross_axis_direction,
            self.position.clone(),
        );
        // The setter runs; its returned impact is what is dropped. This is
        // before the node joins a tree, so there is nothing to invalidate —
        // and a caller may pass the render object's own default
        // (`FirstIsTop`), for which the setter correctly reports `NONE`.
        let _ = render_object.set_paint_order(self.paint_order);
        let _ = render_object.set_clip_behavior(self.clip_behavior);
        render_object
    }
}

impl<C: ViewSeq> fmt::Debug for ShrinkWrappingViewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShrinkWrappingViewport")
            .field("axis_direction", &self.axis_direction)
            .field("position", &self.position.pixels())
            .field("paint_order", &self.paint_order)
            .field("clip_behavior", &self.clip_behavior)
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

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.build_render_object(ctx)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE; // Reconciliation reuses the render object across rebuilds, so a
        // vertical↔horizontal axis change on the widget must be pushed through
        // (not just the scroll offset) — otherwise layout keeps the stale axis
        // from construction.
        impact |= render_object.set_axis_direction(self.axis_direction);
        // Direction-aware cross axis, after `set_axis_direction` laid down the
        // LTR default.
        impact |= render_object
            .set_cross_axis_direction(resolve_cross_axis_direction(self.axis_direction, ctx));
        impact |= render_object.set_paint_order(self.paint_order);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        // Swap in the position only on an actual identity change — see
        // `Viewport::update_render_object`.
        if !render_object.offset().ptr_eq(&self.position) {
            impact |= render_object.set_offset(self.position.clone());
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
    use super::cross_axis_direction;
    use flui_painting::typography::TextDirection;
    use flui_rendering::constraints::AxisDirection;

    /// The cross-axis direction matrix: a horizontal scroll axis lays its cross
    /// axis top-to-bottom without reading the text direction; a vertical one
    /// lays its horizontal cross axis left-to-right in LTR and right-to-left in
    /// RTL.
    #[test]
    fn cross_axis_direction_matrix() {
        for axis in [AxisDirection::LeftToRight, AxisDirection::RightToLeft] {
            assert_eq!(
                cross_axis_direction(axis, || panic!("{axis:?} must not read the direction")),
                AxisDirection::TopToBottom,
                "{axis:?}"
            );
        }
        for axis in [AxisDirection::TopToBottom, AxisDirection::BottomToTop] {
            assert_eq!(
                cross_axis_direction(axis, || TextDirection::Ltr),
                AxisDirection::LeftToRight,
                "{axis:?} (LTR)"
            );
            assert_eq!(
                cross_axis_direction(axis, || TextDirection::Rtl),
                AxisDirection::RightToLeft,
                "{axis:?} (RTL)"
            );
        }
    }
}
