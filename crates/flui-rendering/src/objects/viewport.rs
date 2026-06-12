//! `RenderViewport` — Box render object that drives sliver children.
//!
//! This is the first Core.2 W3.4 slice: a forward, non-shrink-wrapping
//! viewport with a bounded correction loop. It is intentionally smaller than
//! Flutter's full `RenderViewport`: center/anchor reverse-side layout,
//! shrink-wrap, `showOnScreen`, and lazy child creation stay out of this PR.

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::{
    Offset, Point, Rect, Size,
    geometry::px,
    layout::{Axis, AxisDirection, AxisDirection::*},
    painting::Clip,
};

use crate::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
    view::{
        CacheExtentStyle, ScrollDirection, ScrollableViewportOffset, SliverPaintOrder,
        ViewportOffset,
    },
};

const MAX_LAYOUT_CYCLES_PER_CHILD: usize = 10;
const DEFAULT_CACHE_EXTENT: f32 = 250.0;

/// A Box-protocol viewport that lays out Sliver-protocol children.
#[derive(Debug)]
pub struct RenderViewport<O = ScrollableViewportOffset> {
    axis_direction: AxisDirection,
    cross_axis_direction: AxisDirection,
    offset: O,
    cache_extent: f32,
    cache_extent_style: CacheExtentStyle,
    paint_order: SliverPaintOrder,
    size: Size,
    child_count: usize,
    min_scroll_extent: f32,
    max_scroll_extent: f32,
    max_scroll_obstruction_extent: f32,
    sliver_obstruction_extents: Vec<f32>,
    has_visual_overflow: bool,
}

impl RenderViewport<ScrollableViewportOffset> {
    /// Creates a viewport with a zero scrollable offset.
    #[inline]
    #[must_use]
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self::with_offset(
            axis_direction,
            default_cross_axis_direction(axis_direction),
            ScrollableViewportOffset::zero(),
        )
    }
}

impl<O: ViewportOffset + 'static> RenderViewport<O> {
    /// Creates a viewport with explicit axis directions and offset storage.
    #[inline]
    #[must_use]
    pub fn with_offset(
        axis_direction: AxisDirection,
        cross_axis_direction: AxisDirection,
        offset: O,
    ) -> Self {
        Self {
            axis_direction,
            cross_axis_direction,
            offset,
            cache_extent: DEFAULT_CACHE_EXTENT,
            cache_extent_style: CacheExtentStyle::Pixel,
            paint_order: SliverPaintOrder::FirstIsTop,
            size: Size::ZERO,
            child_count: 0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            sliver_obstruction_extents: Vec::new(),
            has_visual_overflow: false,
        }
    }

    /// Last laid-out viewport size.
    #[inline]
    #[must_use]
    pub const fn size(&self) -> &Size {
        &self.size
    }

    /// Returns the viewport offset object.
    #[inline]
    #[must_use]
    pub const fn offset(&self) -> &O {
        &self.offset
    }

    /// Mutable access to the viewport offset object.
    #[inline]
    #[must_use]
    pub const fn offset_mut(&mut self) -> &mut O {
        &mut self.offset
    }

    /// Sets the sliver paint order. Hit testing uses the opposite order.
    #[inline]
    pub const fn set_paint_order(&mut self, paint_order: SliverPaintOrder) {
        self.paint_order = paint_order;
    }

    /// Sets the cache extent and interpretation mode.
    #[inline]
    pub const fn set_cache_extent(&mut self, cache_extent: f32, style: CacheExtentStyle) {
        self.cache_extent = cache_extent;
        self.cache_extent_style = style;
    }

    /// Last total scroll extent reported by the forward sliver sequence.
    #[inline]
    #[must_use]
    pub const fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    /// Last total reverse scroll extent reported by the reverse sliver sequence.
    #[inline]
    #[must_use]
    pub const fn min_scroll_extent(&self) -> f32 {
        self.min_scroll_extent
    }

    /// Last total pinned obstruction extent reported by the sliver sequence.
    #[inline]
    #[must_use]
    pub const fn max_scroll_obstruction_extent(&self) -> f32 {
        self.max_scroll_obstruction_extent
    }

    /// Total obstruction extent contributed by slivers before `child_index`.
    ///
    /// This mirrors Flutter's `maxScrollObstructionExtentBefore` shape for
    /// FLUI's current direct-child, forward-sequence viewport.
    #[inline]
    #[must_use]
    pub fn max_scroll_obstruction_extent_before(&self, child_index: usize) -> Option<f32> {
        if child_index >= self.sliver_obstruction_extents.len() {
            return None;
        }

        Some(
            self.sliver_obstruction_extents
                .iter()
                .take(child_index)
                .sum(),
        )
    }

    /// Whether the last layout pass reported visual overflow.
    #[inline]
    #[must_use]
    pub const fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    fn calculated_cache_extent(&self, main_axis_extent: f32) -> f32 {
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent.max(0.0),
            CacheExtentStyle::Viewport => (self.cache_extent * main_axis_extent).max(0.0),
        }
    }

    fn main_axis_extent(&self) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => self.size.width.get(),
            Axis::Vertical => self.size.height.get(),
        }
    }

    fn cross_axis_extent(&self) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => self.size.height.get(),
            Axis::Vertical => self.size.width.get(),
        }
    }

    fn attempt_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        corrected_offset: f32,
    ) -> f32 {
        self.min_scroll_extent = 0.0;
        self.max_scroll_extent = 0.0;
        self.max_scroll_obstruction_extent = 0.0;
        self.sliver_obstruction_extents.clear();
        self.has_visual_overflow = false;

        let center_offset = -corrected_offset;
        let cache_extent = self.calculated_cache_extent(main_axis_extent);
        let full_cache_extent = main_axis_extent + 2.0 * cache_extent;
        let center_cache_offset = center_offset + cache_extent;
        let remaining_cache_extent =
            (full_cache_extent - center_cache_offset).clamp(0.0, full_cache_extent);

        self.layout_child_sequence(
            ctx,
            corrected_offset.max(0.0),
            center_offset.min(0.0),
            0.0,
            main_axis_extent,
            main_axis_extent,
            cross_axis_extent,
            GrowthDirection::Forward,
            remaining_cache_extent,
            center_offset.clamp(-cache_extent, 0.0),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_child_sequence(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        mut scroll_offset: f32,
        overlap: f32,
        mut layout_offset: f32,
        remaining_paint_extent: f32,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        growth_direction: GrowthDirection,
        mut remaining_cache_extent: f32,
        mut cache_origin: f32,
    ) -> f32 {
        let initial_layout_offset = layout_offset;
        let adjusted_user_scroll_direction = apply_growth_direction_to_scroll_direction(
            self.offset.user_scroll_direction(),
            growth_direction,
        );
        let mut max_paint_offset = layout_offset + overlap;
        let mut preceding_scroll_extent = 0.0;

        for index in 0..ctx.child_count() {
            let sliver_scroll_offset = if scroll_offset <= 0.0 {
                0.0
            } else {
                scroll_offset
            };
            let corrected_cache_origin = cache_origin.max(-sliver_scroll_offset);
            let cache_extent_correction = cache_origin - corrected_cache_origin;
            let child_remaining_paint_extent =
                (remaining_paint_extent - layout_offset + initial_layout_offset).max(0.0);
            let child_remaining_cache_extent =
                (remaining_cache_extent + cache_extent_correction).max(0.0);
            let constraints = SliverConstraints {
                axis_direction: self.axis_direction,
                growth_direction,
                user_scroll_direction: adjusted_user_scroll_direction,
                scroll_offset: sliver_scroll_offset,
                preceding_scroll_extent,
                overlap: max_paint_offset - layout_offset,
                remaining_paint_extent: child_remaining_paint_extent,
                cross_axis_extent,
                cross_axis_direction: self.cross_axis_direction,
                viewport_main_axis_extent: main_axis_extent,
                remaining_cache_extent: child_remaining_cache_extent,
                cache_origin: corrected_cache_origin,
            };

            let geometry = if child_remaining_paint_extent <= f32::EPSILON
                && child_remaining_cache_extent <= f32::EPSILON
                && sliver_scroll_offset <= f32::EPSILON
                && let Some(cached_geometry) = cached_clean_sliver_geometry(ctx, index, constraints)
            {
                cached_geometry
            } else {
                ctx.layout_sliver_child(index, constraints)
            };

            if let Some(correction) = geometry.scroll_offset_correction {
                return correction;
            }

            let effective_layout_offset = layout_offset + geometry.paint_origin;
            let child_layout_offset = if geometry.visible || scroll_offset > 0.0 {
                effective_layout_offset
            } else {
                -scroll_offset + initial_layout_offset
            };
            ctx.position_child(
                index,
                self.compute_absolute_paint_offset(
                    child_layout_offset,
                    growth_direction,
                    geometry.paint_extent,
                ),
            );

            max_paint_offset =
                max_paint_offset.max(effective_layout_offset + geometry.paint_extent);
            scroll_offset -= geometry.scroll_extent;
            preceding_scroll_extent += geometry.scroll_extent;
            layout_offset += geometry.layout_extent;

            if geometry.cache_extent != 0.0 {
                remaining_cache_extent -= geometry.cache_extent - cache_extent_correction;
                cache_origin = (corrected_cache_origin + geometry.cache_extent).min(0.0);
            }

            self.update_out_of_band_data(growth_direction, geometry);
        }

        0.0
    }

    fn update_out_of_band_data(
        &mut self,
        growth_direction: GrowthDirection,
        geometry: SliverGeometry,
    ) {
        match growth_direction {
            GrowthDirection::Forward => {
                self.max_scroll_extent += geometry.scroll_extent;
            }
            GrowthDirection::Reverse => {
                self.min_scroll_extent -= geometry.scroll_extent;
            }
        }
        self.max_scroll_obstruction_extent += geometry.max_scroll_obstruction_extent;
        self.sliver_obstruction_extents
            .push(geometry.max_scroll_obstruction_extent);
        if geometry.has_visual_overflow {
            self.has_visual_overflow = true;
        }
    }

    fn compute_absolute_paint_offset(
        &self,
        layout_offset: f32,
        growth_direction: GrowthDirection,
        paint_extent: f32,
    ) -> Offset {
        match growth_direction.apply_to_axis_direction(self.axis_direction) {
            TopToBottom => Offset::new(px(0.0), px(layout_offset)),
            BottomToTop => Offset::new(
                px(0.0),
                px(self.size.height.get() - layout_offset - paint_extent),
            ),
            LeftToRight => Offset::new(px(layout_offset), px(0.0)),
            RightToLeft => Offset::new(
                px(self.size.width.get() - layout_offset - paint_extent),
                px(0.0),
            ),
        }
    }

    fn viewport_rect(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

impl Default for RenderViewport<ScrollableViewportOffset> {
    fn default() -> Self {
        Self::new(TopToBottom)
    }
}

impl<O: ViewportOffset + 'static> Diagnosticable for RenderViewport<O> {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("axis_direction", self.axis_direction);
        properties.add_enum("cross_axis_direction", self.cross_axis_direction);
        properties.add_double("offset", self.offset.pixels(), Some("px"));
        properties.add_double("cache_extent", self.cache_extent, Some("px"));
        properties.add_enum("cache_extent_style", self.cache_extent_style);
        properties.add_enum("paint_order", self.paint_order);
    }
}
impl<O: ViewportOffset + 'static> PaintEffectsCapability for RenderViewport<O> {}
impl<O: ViewportOffset + 'static> SemanticsCapability for RenderViewport<O> {}
impl<O: ViewportOffset + 'static> HotReloadCapability for RenderViewport<O> {}

impl<O: ViewportOffset + 'static> RenderBox for RenderViewport<O> {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>) {
        self.size = ctx.constraints().biggest();
        let main_axis_extent = self.main_axis_extent();
        let cross_axis_extent = self.cross_axis_extent();
        self.child_count = ctx.child_count();
        let _ = self.offset.apply_viewport_dimension(main_axis_extent);

        if ctx.child_count() == 0 {
            self.min_scroll_extent = 0.0;
            self.max_scroll_extent = 0.0;
            self.max_scroll_obstruction_extent = 0.0;
            self.sliver_obstruction_extents.clear();
            self.has_visual_overflow = false;
            let _ = self.offset.apply_content_dimensions(0.0, 0.0);
            ctx.complete_with_size(self.size);
            return;
        }

        let max_layout_cycles = MAX_LAYOUT_CYCLES_PER_CHILD * ctx.child_count();
        let mut accepted = false;
        for _ in 0..max_layout_cycles {
            let correction = self.attempt_layout(
                ctx,
                main_axis_extent,
                cross_axis_extent,
                self.offset.pixels(),
            );
            if correction != 0.0 {
                self.offset.correct_by(correction);
                continue;
            }

            if self
                .offset
                .apply_content_dimensions(0.0, (self.max_scroll_extent - main_axis_extent).max(0.0))
            {
                accepted = true;
                break;
            }
        }
        debug_assert!(
            accepted,
            "RenderViewport exceeded its bounded layout correction loop"
        );

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_children = |ctx: &mut PaintCx<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.has_visual_overflow {
            ctx.with_clip_rect(self.viewport_rect(), Clip::HardEdge, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        match self.paint_order {
            SliverPaintOrder::FirstIsTop => {
                for index in 0..self.child_count {
                    if ctx.hit_test_child_at_layout_offset(index) {
                        return true;
                    }
                }
            }
            SliverPaintOrder::LastIsTop => {
                for index in (0..self.child_count).rev() {
                    if ctx.hit_test_child_at_layout_offset(index) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

const fn default_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction {
        TopToBottom | BottomToTop => LeftToRight,
        LeftToRight | RightToLeft => TopToBottom,
    }
}

fn apply_growth_direction_to_scroll_direction(
    scroll_direction: ScrollDirection,
    growth_direction: GrowthDirection,
) -> ScrollDirection {
    match growth_direction {
        GrowthDirection::Forward => scroll_direction,
        GrowthDirection::Reverse => scroll_direction.flip(),
    }
}

fn cached_clean_sliver_geometry(
    ctx: &BoxLayoutContext<'_, Variable, BoxParentData>,
    index: usize,
    constraints: SliverConstraints,
) -> Option<SliverGeometry> {
    if ctx.sliver_child_needs_layout(index) {
        return None;
    }
    let (cached_constraints, cached_geometry) = ctx.cached_sliver_child_layout(index)?;
    if cached_constraints == constraints && cached_geometry.scroll_offset_correction.is_none() {
        Some(cached_geometry)
    } else {
        None
    }
}
