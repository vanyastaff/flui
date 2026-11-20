//! `RenderShrinkWrappingViewport` — the viewport that sizes its main axis to
//! the sum of its slivers' `max_paint_extent`, within its parent's
//! constraints.
//!
//! It has no `center` or `anchor`: every child grows forward from the first,
//! and the scroll range starts at zero.

use flui_foundation::Variable;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Rect, Size};
use flui_foundation::inspect::{Fields, Inspect, Unit};
use flui_painting::paint::Clip;
use flui_rendering::{
    RenderResult, RenderUpdateImpact,
    constraints::{
        AxisDirection, AxisDirection::TopToBottom, BoxConstraints, GrowthDirection, SliverGeometry,
    },
    context::{BoxHitTestContext, BoxLayoutContext, PaintContext},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{RenderBox, SemanticsClip},
    view::{
        CacheExtentStyle, ScrollPosition, SliverPaintOrder, ViewportLayout as _, ViewportOffset,
    },
};

use super::core::{
    LayoutChildSequenceParams, MAX_LAYOUT_CYCLES_PER_CHILD, PassRecord, StagedPosition,
    ViewportCore, default_cross_axis_direction,
};

/// A Box-protocol viewport that shrink-wraps its sliver children in the main axis.
///
/// Unlike [`RenderViewport`](super::RenderViewport), which expands to the
/// incoming main-axis extent, this render object sizes itself to the sum of its
/// slivers' `max_paint_extent` values, constrained by its parent. It still
/// expands in the cross axis.
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport<O = ScrollPosition> {
    core: ViewportCore<O, ShrinkWrappingLayout>,
}

/// One pass's metrics and positions; see [`PassRecord`].
#[derive(Debug, Default)]
struct ShrinkWrappingLayout {
    child_count: usize,
    max_scroll_extent: f64,
    shrink_wrap_extent: f64,
    has_visual_overflow: bool,
    staged_positions: Vec<StagedPosition>,
}

impl PassRecord for ShrinkWrappingLayout {
    fn child_count(&self) -> usize {
        self.child_count
    }

    fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    fn staged_positions_mut(&mut self) -> &mut Vec<StagedPosition> {
        &mut self.staged_positions
    }

    fn record_child(
        &mut self,
        _slot: usize,
        growth_direction: GrowthDirection,
        geometry: &SliverGeometry,
    ) {
        debug_assert_eq!(
            growth_direction,
            GrowthDirection::Forward,
            "BUG: RenderShrinkWrappingViewport has no center/anchor; every walk is forward"
        );
        self.max_scroll_extent += geometry.scroll_extent;
        self.shrink_wrap_extent += geometry.max_paint_extent;
        if geometry.has_visual_overflow {
            self.has_visual_overflow = true;
        }
    }
}

impl RenderShrinkWrappingViewport<ScrollPosition> {
    /// Creates a shrink-wrapping viewport with a fixed offset at zero.
    #[inline]
    #[must_use]
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self::with_offset(
            axis_direction,
            default_cross_axis_direction(axis_direction),
            ScrollPosition::fixed(0.0),
        )
    }
}

impl<O: ViewportOffset + 'static> RenderShrinkWrappingViewport<O> {
    /// Creates a shrink-wrapping viewport with explicit axis directions and offset storage.
    #[inline]
    #[must_use]
    pub fn with_offset(
        axis_direction: AxisDirection,
        cross_axis_direction: AxisDirection,
        offset: O,
    ) -> Self {
        Self {
            core: ViewportCore::new(axis_direction, cross_axis_direction, offset),
        }
    }

    /// Returns the viewport offset object.
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> &O {
        &self.core.offset
    }

    /// Mutable access to the viewport offset object.
    #[inline]
    #[must_use]
    pub const fn offset_mut(&mut self) -> &mut O {
        &mut self.core.offset
    }

    /// Replaces the viewport offset object wholesale. See
    /// [`RenderViewport::set_offset`](super::RenderViewport::set_offset) for
    /// the identity-check contract a caller injecting an external offset must
    /// follow, and for the attached-listener re-registration this shares.
    #[inline]
    pub fn set_offset(&mut self, offset: O) -> RenderUpdateImpact {
        self.core.set_offset(offset)
    }

    /// Sets how overflowing content is clipped when this viewport paints.
    ///
    /// `Clip::None` clips nothing, so a child may paint outside the
    /// viewport's bounds; every other behaviour clips to them, and only when
    /// the content actually overflows. Switching to or from `Clip::None` also
    /// changes which children's semantics the viewport clips, so it reports
    /// semantics too.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> RenderUpdateImpact {
        self.core.set_clip_behavior(clip_behavior)
    }

    /// How overflowing content is clipped when this viewport paints.
    #[must_use]
    pub const fn clip_behavior(&self) -> Clip {
        self.core.clip_behavior()
    }

    /// Sets the sliver paint order. Hit testing uses the opposite order.
    #[inline]
    pub fn set_paint_order(&mut self, paint_order: SliverPaintOrder) -> RenderUpdateImpact {
        self.core.set_paint_order(paint_order)
    }

    /// Sets the cache extent and interpretation mode.
    #[inline]
    pub const fn set_cache_extent(
        &mut self,
        cache_extent: f64,
        style: CacheExtentStyle,
    ) -> RenderUpdateImpact {
        self.core.set_cache_extent(cache_extent, style)
    }

    /// Sets the scroll axis direction, re-deriving the cross-axis direction.
    ///
    /// Reports layout when the axis actually changed.
    #[inline]
    pub fn set_axis_direction(&mut self, axis_direction: AxisDirection) -> RenderUpdateImpact {
        self.core.set_axis_direction(axis_direction)
    }

    /// Sets the cross-axis direction, overriding the LTR default
    /// [`set_axis_direction`](Self::set_axis_direction) re-derives — an RTL
    /// `Directionality` flips a vertical viewport's cross axis to
    /// right-to-left.
    ///
    /// Reports layout when the cross axis actually changed.
    #[inline]
    pub fn set_cross_axis_direction(
        &mut self,
        cross_axis_direction: AxisDirection,
    ) -> RenderUpdateImpact {
        self.core.set_cross_axis_direction(cross_axis_direction)
    }

    /// Last total scroll extent reported by the sliver sequence.
    #[inline]
    #[must_use]
    pub const fn max_scroll_extent(&self) -> f64 {
        self.core.layout.max_scroll_extent
    }

    /// Last unconstrained main-axis extent accumulated from child slivers.
    #[inline]
    #[must_use]
    pub const fn shrink_wrap_extent(&self) -> f64 {
        self.core.layout.shrink_wrap_extent
    }

    /// Whether the last layout pass reported visual overflow.
    #[inline]
    #[must_use]
    pub const fn has_visual_overflow(&self) -> bool {
        self.core.layout.has_visual_overflow
    }

    /// The cache extent in pixels. An unbounded main axis — the case a
    /// shrink-wrapping viewport exists for — has no cache beyond its content.
    fn calculated_cache_extent(&self, main_axis_extent: f64) -> f64 {
        if !main_axis_extent.is_finite() {
            return 0.0;
        }
        self.core.calculated_cache_extent(main_axis_extent)
    }

    fn main_axis_extent_from_constraints(&self, constraints: &BoxConstraints) -> f64 {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        }
    }

    fn cross_axis_extent_from_constraints(&self, constraints: &BoxConstraints) -> f64 {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => constraints.max_height,
            Axis::Vertical => constraints.max_width,
        }
    }

    fn constrain_main_axis_extent(&self, constraints: &BoxConstraints, extent: f64) -> f64 {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => constraints.constrain_width(extent),
            Axis::Vertical => constraints.constrain_height(extent),
        }
    }

    fn size_from_extents(&self, cross_axis_extent: f64, main_axis_extent: f64) -> Size {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => Size::new(main_axis_extent, cross_axis_extent),
            Axis::Vertical => Size::new(cross_axis_extent, main_axis_extent),
        }
    }

    fn empty_size(&self, constraints: &BoxConstraints) -> Size {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => Size::new(constraints.min_width, constraints.max_height),
            Axis::Vertical => Size::new(constraints.max_width, constraints.min_height),
        }
    }

    fn debug_check_has_bounded_cross_axis(&self, constraints: &BoxConstraints) {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => debug_assert!(
                constraints.has_bounded_height(),
                "horizontal RenderShrinkWrappingViewport requires bounded height"
            ),
            Axis::Vertical => debug_assert!(
                constraints.has_bounded_width(),
                "vertical RenderShrinkWrappingViewport requires bounded width"
            ),
        }
    }

    #[must_use = "scroll correction must be applied when non-zero"]
    fn attempt_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        main_axis_extent: f64,
        cross_axis_extent: f64,
        corrected_offset: f64,
    ) -> RenderResult<f64> {
        let layout = &mut self.core.layout;
        layout.staged_positions.clear();
        layout.max_scroll_extent = 0.0;
        layout.shrink_wrap_extent = 0.0;
        layout.has_visual_overflow = corrected_offset < 0.0;

        let cache_extent = self.calculated_cache_extent(main_axis_extent);
        let remaining_paint_extent = (main_axis_extent + corrected_offset.min(0.0)).max(0.0);
        self.core.layout_child_sequence(
            ctx,
            LayoutChildSequenceParams {
                scroll_offset: corrected_offset.max(0.0),
                overlap: corrected_offset.min(0.0),
                layout_offset: (-corrected_offset).max(0.0),
                remaining_paint_extent,
                main_axis_extent,
                cross_axis_extent,
                growth_direction: GrowthDirection::Forward,
                remaining_cache_extent: main_axis_extent + 2.0 * cache_extent,
                cache_origin: -cache_extent,
                child_start: 0,
                child_end: ctx.child_count(),
                reversed: false,
            },
        )
    }

    /// One `perform_layout` pass into the record [`ViewportCore::begin_pass`]
    /// handed out.
    fn layout_pass(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
    ) -> RenderResult<Size> {
        let constraints = *ctx.constraints();
        self.debug_check_has_bounded_cross_axis(&constraints);
        self.core.layout.child_count = ctx.child_count();
        let mut layout_offset = self.core.offset.begin_layout();

        if ctx.child_count() == 0 {
            let size = self.empty_size(&constraints);
            let layout = &mut self.core.layout;
            layout.max_scroll_extent = 0.0;
            layout.shrink_wrap_extent = 0.0;
            layout.has_visual_overflow = false;
            let _ = layout_offset.apply_viewport_dimension(0.0);
            let _ = layout_offset.apply_content_dimensions(0.0, 0.0);
            self.core.offset.accept_layout(layout_offset)?;
            return Ok(size);
        }

        let main_axis_extent = self.main_axis_extent_from_constraints(&constraints);
        let cross_axis_extent = self.cross_axis_extent_from_constraints(&constraints);

        let max_layout_cycles = MAX_LAYOUT_CYCLES_PER_CHILD * ctx.child_count();
        let mut accepted = false;
        let mut degraded = false;
        let mut effective_extent = 0.0;
        for _ in 0..max_layout_cycles {
            let correction = self.attempt_layout(
                ctx,
                main_axis_extent,
                cross_axis_extent,
                layout_offset.pixels(),
            )?;
            // See `RenderViewport`'s pass: a pass that read a stand-in
            // publishes nothing. The size still comes from what the pass
            // measured — a viewport must return one — but the scroll position
            // keeps the dimensions it has.
            if ctx.descendant_layout_degraded() {
                degraded = true;
                effective_extent = self
                    .constrain_main_axis_extent(&constraints, self.core.layout.shrink_wrap_extent);
                break;
            }
            if correction != 0.0 {
                layout_offset.correct_by(correction);
                continue;
            }

            effective_extent =
                self.constrain_main_axis_extent(&constraints, self.core.layout.shrink_wrap_extent);
            let did_accept_viewport_dimension =
                layout_offset.apply_viewport_dimension(effective_extent);
            let did_accept_content_dimension = layout_offset.apply_content_dimensions(
                0.0,
                (self.core.layout.max_scroll_extent - effective_extent).max(0.0),
            );
            if did_accept_viewport_dimension && did_accept_content_dimension {
                accepted = true;
                break;
            }
        }
        if degraded {
            tracing::warn!(
                child_count = ctx.child_count(),
                "RenderShrinkWrappingViewport laid out over a degraded \
         descendant; scroll dimensions were not published this frame"
            );
        } else if !accepted {
            tracing::warn!(
                child_count = ctx.child_count(),
                max_layout_cycles,
                "RenderShrinkWrappingViewport exceeded its bounded layout correction loop; \
         committed the last computed extent"
            );
        }

        // The accepted pass laid every child out against constraints that do
        // not depend on this viewport's final size; only the physical offsets
        // do (a reverse axis measures from the far edge), and they are
        // resolved here, once, from what that pass staged. Storing logical
        // offsets in parent data and resolving them at paint would need a
        // second layout of every child; resolving them at commit does not.
        let size = self.size_from_extents(cross_axis_extent, effective_extent);
        if !degraded {
            self.core.offset.accept_layout(layout_offset)?;
        }
        let cache_extent = self.calculated_cache_extent(main_axis_extent);
        self.core.commit_positions(ctx, size, cache_extent);
        Ok(size)
    }
}

impl Default for RenderShrinkWrappingViewport<ScrollPosition> {
    fn default() -> Self {
        Self::new(TopToBottom)
    }
}

impl<O: ViewportOffset + 'static> Inspect for RenderShrinkWrappingViewport<O> {
    fn kind(&self) -> &'static str {
        "RenderShrinkWrappingViewport"
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        args(std::any::type_name::<O>());
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        self.core.inspect(out);
        out.field("shrink_wrap_extent", &self.core.layout.shrink_wrap_extent)
            .unit(Unit::LogicalPx);
    }
}

impl<O: ViewportOffset + 'static> RenderBox for RenderShrinkWrappingViewport<O> {
    type Arity = Variable;
    type ParentData = BoxParentData;

    // `ViewportCore::attach`/`detach` hold the offset subscription, the same
    // for both viewports.
    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.core.attach(handle);
    }

    fn detach(&mut self) {
        self.core.detach();
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> RenderResult<Size> {
        self.core.begin_pass();
        let result = self.layout_pass(ctx);
        if result.is_err() {
            self.core.discard_pass();
        }
        result
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
        self.core.paint(ctx);
    }

    fn describe_approximate_paint_clip(&self, child_slot: usize, size: Size) -> Option<Rect<f64>> {
        self.core.describe_approximate_paint_clip(child_slot, size)
    }

    fn describe_semantics_clip(&self, _child_slot: usize, size: Size) -> Option<SemanticsClip> {
        Some(self.core.semantics_clip(size))
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        self.core.hit_test(ctx)
    }
}
