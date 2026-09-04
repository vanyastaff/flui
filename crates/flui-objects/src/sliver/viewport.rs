//! `RenderViewport` — Box render object that drives sliver children.
//!
//! This is the first Core.2 W3.4 slice: a forward, non-shrink-wrapping
//! viewport with a bounded correction loop. It is intentionally smaller than
//! Flutter's full `RenderViewport`: center/anchor reverse-side layout,
//! shrink-wrap, `showOnScreen`, and lazy child creation stay out of this PR.

use std::sync::Arc;

use flui_foundation::Diagnosticable;
use flui_tree::Variable;
use flui_types::{
    Offset, Pixels, Point, Rect, Size,
    geometry::px,
    layout::{
        Axis, AxisDirection,
        AxisDirection::{BottomToTop, LeftToRight, RightToLeft, TopToBottom},
    },
    painting::Clip,
};

use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    pipeline::{DirtySendError, RenderInvalidationHandle},
    traits::RenderBox,
    view::{CacheExtentStyle, ScrollableViewportOffset, SliverPaintOrder, ViewportOffset},
};

const MAX_LAYOUT_CYCLES_PER_CHILD: usize = 10;
const DEFAULT_CACHE_EXTENT: f32 = 250.0;
/// Scroll correction returned when layout accepts the current offset unchanged.
const NO_SCROLL_CORRECTION: f32 = 0.0;

/// A registered [`ViewportOffset`] listener [`Arc`], wrapped so
/// [`RenderViewport`]/[`RenderShrinkWrappingViewport`]'s `#[derive(Debug)]`
/// doesn't need a hand-written `Debug` impl for a value that is fundamentally
/// an opaque closure — `Debug` just reports that a listener is registered,
/// not what it does.
struct OffsetListener(Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for OffsetListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OffsetListener(..)")
    }
}

/// Builds the render-side [`ViewportOffset`] listener that `attach` (and any
/// later `set_offset` re-registration) installs: a self-mark that requests a
/// re-layout of the node bound to `handle` whenever the offset's `pixels`
/// changes out-of-band — a gesture's `set_pixels`, a
/// `ScrollController::jump_to`, or the post-frame content-dimension flush.
/// Flutter parity: `RenderViewport`/`RenderShrinkWrappingViewport` share this
/// exact shape (`rendering/viewport.dart`'s `offset.addListener(markNeedsLayout)`
/// wiring in `attach`).
///
/// `apply_viewport_dimension`/`apply_content_dimensions`/`correct_by` never
/// notify synchronously — that is `ViewportOffset`'s own contract (see the
/// `scroll_position` module docs) — so this listener can only ever fire from
/// OUTSIDE `perform_layout`; there is no synchronous mark-during-layout
/// re-entrancy to guard against here.
fn offset_relayout_listener(handle: RenderInvalidationHandle) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        // `SendError::OwnerGone` (pipeline owner torn down — node/tree gone,
        // this is teardown, not a fault) and any future variant (`SendError`
        // is `#[non_exhaustive]`) get silent treatment: nothing left to
        // mark, nothing to warn about. `ChannelFull` gets a `warn!` below —
        // see its comment for why this is a real, unmitigated staleness
        // risk, not routine backpressure noise.
        if let Err(error @ DirtySendError::ChannelFull { .. }) = handle.mark_needs_layout() {
            // A full channel does NOT mean this node's own mark is already
            // queued in it — 256 unrelated marks from elsewhere in the tree
            // fill it just as easily, so this send can be dropped for a
            // node that has no other pending mark at all. There is no
            // retry available: this closure has no way back into the
            // render object to set a retry flag, and neither
            // `perform_layout` nor `paint` runs for a node that isn't
            // already on some dirty list, so nothing revisits it later on
            // its own. Continuous scrolling self-heals (the very next
            // offset mutation fires this listener again and retries the
            // send), but a single one-shot `jump_to` under backpressure can
            // leave this viewport showing a stale frame until some
            // UNRELATED mutation elsewhere happens to mark it dirty.
            tracing::warn!(
                %error,
                "viewport offset listener: mark_needs_layout dropped under backpressure; \
                 this viewport may keep showing a stale frame until another offset mutation \
                 or an unrelated dirty mark triggers a retry"
            );
        }
    })
}

/// Removes `*listener` (if any) from `offset`, via the trait's ptr-eq removal
/// contract — the SAME `Arc` that was registered.
fn unregister_offset_listener<O: ViewportOffset>(
    offset: &O,
    listener: &mut Option<OffsetListener>,
) {
    if let Some(listener) = listener.take() {
        offset.remove_listener(&listener.0);
    }
}

/// Registers a fresh [`offset_relayout_listener`] on `offset`, bound to
/// `handle`.
fn register_offset_listener<O: ViewportOffset>(
    offset: &O,
    handle: RenderInvalidationHandle,
) -> OffsetListener {
    let listener = offset_relayout_listener(handle);
    offset.add_listener(listener.clone());
    OffsetListener(listener)
}

/// Parameters for one forward or reverse child walk inside [`RenderViewport`].
#[derive(Debug, Clone, Copy)]
struct LayoutChildSequenceParams {
    scroll_offset: f32,
    overlap: f32,
    layout_offset: f32,
    remaining_paint_extent: f32,
    main_axis_extent: f32,
    cross_axis_extent: f32,
    growth_direction: GrowthDirection,
    remaining_cache_extent: f32,
    cache_origin: f32,
    child_start: usize,
    child_end: usize,
}

/// Where a pass decided a child goes, in the viewport's logical coordinates.
///
/// A pass stages these instead of positioning as it walks: the physical
/// offset needs the viewport's final size (a reverse axis measures from the
/// far edge), which a shrink-wrapping viewport only knows once its passes
/// have settled. Positioning once, from the accepted pass, is what lets that
/// viewport skip a second layout of every child.
#[derive(Debug, Clone, Copy)]
struct StagedPosition {
    slot: usize,
    layout_offset: f32,
    growth_direction: GrowthDirection,
    paint_extent: f32,
}

/// Per-child sliver constraint fields that vary during a viewport walk.
#[derive(Debug, Clone, Copy)]
struct ChildSliverLayoutFields {
    growth_direction: GrowthDirection,
    user_scroll_direction: flui_rendering::view::ScrollDirection,
    scroll_offset: f32,
    preceding_scroll_extent: f32,
    overlap: f32,
    remaining_paint_extent: f32,
    remaining_cache_extent: f32,
    cache_origin: f32,
}

/// A Box-protocol viewport that lays out Sliver-protocol children.
#[derive(Debug)]
pub struct RenderViewport<O = ScrollableViewportOffset> {
    axis_direction: AxisDirection,
    cross_axis_direction: AxisDirection,
    offset: O,
    cache_extent: f32,
    cache_extent_style: CacheExtentStyle,
    paint_order: SliverPaintOrder,
    /// When set, children before this index use forward growth; from this index
    /// onward use reverse growth (Flutter `center` sliver partition, W3.2 slice).
    center_sliver_index: Option<usize>,
    child_count: usize,
    min_scroll_extent: f32,
    max_scroll_extent: f32,
    max_scroll_obstruction_extent: f32,
    sliver_obstruction_extents: Vec<f32>,
    has_visual_overflow: bool,
    /// The repaint handle this node was bound to in [`attach`](RenderBox::attach),
    /// `None` before attach / after [`detach`](RenderBox::detach). `set_offset`
    /// clones it to re-register `offset_listener` on a swapped-in offset while
    /// the node is live in a pipeline.
    render_invalidation_handle: Option<RenderInvalidationHandle>,
    /// The listener `attach` (or a live `set_offset`) registered on `offset` —
    /// retained so `detach`/`set_offset` can remove the SAME `Arc` via
    /// [`ViewportOffset::remove_listener`]'s ptr-eq contract.
    offset_listener: Option<OffsetListener>,
    /// The child positions the current pass decided; committed by
    /// `commit_positions` once the pass is accepted.
    staged_positions: Vec<StagedPosition>,
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
            center_sliver_index: None,
            child_count: 0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            sliver_obstruction_extents: Vec::new(),
            has_visual_overflow: false,
            render_invalidation_handle: None,
            offset_listener: None,
            staged_positions: Vec::new(),
        }
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

    /// Replaces the viewport offset object wholesale.
    ///
    /// For a widget that injects an external offset (e.g. a shared
    /// `ScrollPosition`), reconciliation compares the new offset's identity
    /// against the current one and calls this only when it actually changed
    /// — swapping in a same-identity offset would discard layout-committed
    /// extents (`min_scroll_extent`/`max_scroll_extent`/`viewport_dimension`)
    /// for no reason.
    ///
    /// If this node is currently attached (a [`RenderInvalidationHandle`] was handed to
    /// [`attach`](RenderBox::attach) and no matching
    /// [`detach`](RenderBox::detach) has run since), the offset-relayout
    /// listener moves with the swap: it is removed from the OLD offset first
    /// (same `Arc`, per the ptr-eq removal contract), then a fresh one is
    /// registered on the new offset. Not attached yet — the listener is left
    /// for `attach` to install once the node actually enters a pipeline.
    #[inline]
    pub fn set_offset(&mut self, offset: O) -> flui_rendering::RenderUpdateImpact {
        unregister_offset_listener(&self.offset, &mut self.offset_listener);
        self.offset = offset;
        if let Some(handle) = self.render_invalidation_handle.clone() {
            self.offset_listener = Some(register_offset_listener(&self.offset, handle));
        }
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the sliver paint order. Hit testing uses the opposite order.
    #[inline]
    pub fn set_paint_order(
        &mut self,
        paint_order: SliverPaintOrder,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.paint_order == paint_order {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.paint_order = paint_order;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Sets the cache extent and interpretation mode.
    #[inline]
    pub const fn set_cache_extent(
        &mut self,
        cache_extent: f32,
        style: CacheExtentStyle,
    ) -> flui_rendering::RenderUpdateImpact {
        let same_style = matches!(
            (self.cache_extent_style, style),
            (CacheExtentStyle::Pixel, CacheExtentStyle::Pixel)
                | (CacheExtentStyle::Viewport, CacheExtentStyle::Viewport)
        );
        if self.cache_extent == cache_extent && same_style {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.cache_extent = cache_extent;
        self.cache_extent_style = style;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the scroll axis direction, re-deriving the cross-axis direction.
    ///
    /// Reports layout when the axis actually changed.
    #[inline]
    pub fn set_axis_direction(
        &mut self,
        axis_direction: AxisDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.axis_direction == axis_direction {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.axis_direction = axis_direction;
        self.cross_axis_direction = default_cross_axis_direction(axis_direction);
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the index of the center sliver for forward/reverse growth partitioning.
    ///
    /// `None` (default) lays out all children with forward growth. When set to
    /// `Some(index)`, children `[0..index)` use forward growth and `[index..)` use
    /// reverse growth from the trailing edge.
    #[inline]
    pub fn set_center_sliver_index(
        &mut self,
        index: Option<usize>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.center_sliver_index == index {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.center_sliver_index = index;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Returns the configured center sliver index, if any.
    #[inline]
    #[must_use]
    pub fn center_sliver_index(&self) -> Option<usize> {
        self.center_sliver_index
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

    fn main_axis_extent(&self, size: Size) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => size.width.get(),
            Axis::Vertical => size.height.get(),
        }
    }

    fn cross_axis_extent(&self, size: Size) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => size.height.get(),
            Axis::Vertical => size.width.get(),
        }
    }

    fn child_sliver_constraints(
        &self,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        fields: ChildSliverLayoutFields,
    ) -> SliverConstraints {
        SliverConstraints::new(
            self.axis_direction,
            fields.growth_direction,
            fields.user_scroll_direction,
            fields.scroll_offset,
            fields.preceding_scroll_extent,
            fields.overlap,
            fields.remaining_paint_extent,
            cross_axis_extent,
            self.cross_axis_direction,
            main_axis_extent,
            fields.remaining_cache_extent,
            fields.cache_origin,
        )
    }

    #[must_use = "scroll correction must be applied when non-zero"]
    fn attempt_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        corrected_offset: f32,
    ) -> f32 {
        self.staged_positions.clear();
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

        let child_count = ctx.child_count();
        let center = match self.center_sliver_index {
            None => child_count,
            Some(index) => {
                debug_assert!(
                    index <= child_count,
                    "center_sliver_index ({index}) must be <= child_count ({child_count})"
                );
                index.min(child_count)
            }
        };

        // Oracle (`rendering/viewport.dart:1810-1834`): the forward sequence's
        // `overlap` is `min(0.0, -centerOffset)` — i.e. `corrected_offset.min(0.0)`
        // here, since `center_offset == -corrected_offset` — ONLY when there is no
        // reverse-growth sliver group ahead of it (`leadingNegativeChild == null`);
        // otherwise BOTH the forward and reverse sequences pin `overlap` to `0.0`
        // unconditionally. The previous formula, `center_offset.min(0.0)`, had the
        // opposite sign whenever `corrected_offset` was positive (a scrolled-forward
        // viewport with no reverse group always reported a negative `overlap`
        // instead of `0.0`) — see the closure note in
        // docs/research/widget-renderobject-map.md ("Two pre-existing
        // infrastructure defects").
        let has_reverse_group = center < child_count;
        let forward_overlap = if has_reverse_group {
            0.0
        } else {
            corrected_offset.min(0.0)
        };

        // Oracle (`rendering/viewport.dart:1831-1846`): the forward sequence
        // starts at the ZERO-SCROLL LINE, not the viewport's leading edge —
        // `centerOffset` clamped into the viewport (and unclamped once fully
        // past it), with the paint budget shrunk by the same amount. The two
        // differ from the previous constants (`0.0` / `main_axis_extent`)
        // exactly when `center_offset > 0`: an overscrolled-at-the-start
        // viewport, where every sliver shifts down by the overscroll and the
        // gap opens below a persistent header's negative `paint_origin`.
        let forward_layout_offset = if center_offset >= main_axis_extent {
            center_offset
        } else {
            center_offset.clamp(0.0, main_axis_extent)
        };
        let forward_remaining_paint_extent =
            (main_axis_extent - center_offset).clamp(0.0, main_axis_extent);

        let sequence_base = LayoutChildSequenceParams {
            scroll_offset: corrected_offset.max(0.0),
            overlap: forward_overlap,
            layout_offset: forward_layout_offset,
            remaining_paint_extent: forward_remaining_paint_extent,
            main_axis_extent,
            cross_axis_extent,
            growth_direction: GrowthDirection::Forward,
            remaining_cache_extent,
            cache_origin: center_offset.clamp(-cache_extent, 0.0),
            child_start: 0,
            child_end: center,
        };

        if center > 0 {
            let correction = self.layout_child_sequence(ctx, sequence_base);
            if correction != 0.0 {
                return correction;
            }
        }

        if has_reverse_group {
            // Known gap: the reverse pass reuses the forward pass's cache
            // window; Flutter derives the reverse pass's own cache parameters
            // from the forward results (`rendering/viewport.dart:1812-1825`).
            let reverse_params = LayoutChildSequenceParams {
                growth_direction: GrowthDirection::Reverse,
                // Oracle: the reverse sequence always lays out with `overlap: 0.0`
                // unconditionally (`rendering/viewport.dart:1818`), independent of
                // `forward_overlap` above — stated explicitly so this invariant
                // survives future changes to the `has_reverse_group` branch.
                overlap: 0.0,
                // Same known gap as the cache note above: the reverse pass
                // keeps the leading-edge origin and full paint budget it has
                // always used, rather than inheriting the forward pass's
                // center-line values — Flutter derives the reverse pass's own
                // window (`rendering/viewport.dart:1812-1825`). Pinned here so
                // the forward-sequence fix cannot change reverse behavior as a
                // side effect.
                layout_offset: 0.0,
                remaining_paint_extent: main_axis_extent,
                child_start: center,
                child_end: child_count,
                ..sequence_base
            };
            self.layout_child_sequence(ctx, reverse_params)
        } else {
            NO_SCROLL_CORRECTION
        }
    }

    #[must_use = "correction value must be checked; 0.0 means layout accepted"]
    fn layout_child_sequence(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        params: LayoutChildSequenceParams,
    ) -> f32 {
        let LayoutChildSequenceParams {
            mut scroll_offset,
            overlap,
            mut layout_offset,
            remaining_paint_extent,
            main_axis_extent,
            cross_axis_extent,
            growth_direction,
            mut remaining_cache_extent,
            mut cache_origin,
            child_start,
            child_end,
        } = params;
        let initial_layout_offset = layout_offset;
        let adjusted_user_scroll_direction =
            flui_rendering::constraints::apply_growth_direction_to_scroll_direction(
                self.offset.user_scroll_direction(),
                growth_direction,
            );
        let mut max_paint_offset = layout_offset + overlap;
        let mut preceding_scroll_extent = 0.0;

        for index in child_start..child_end {
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
            let constraints = self.child_sliver_constraints(
                main_axis_extent,
                cross_axis_extent,
                ChildSliverLayoutFields {
                    growth_direction,
                    user_scroll_direction: adjusted_user_scroll_direction,
                    scroll_offset: sliver_scroll_offset,
                    preceding_scroll_extent,
                    overlap: max_paint_offset - layout_offset,
                    remaining_paint_extent: child_remaining_paint_extent,
                    remaining_cache_extent: child_remaining_cache_extent,
                    cache_origin: corrected_cache_origin,
                },
            );

            let geometry = try_cached_sliver_geometry(
                ctx,
                index,
                constraints,
                child_remaining_paint_extent,
                child_remaining_cache_extent,
                sliver_scroll_offset,
            )
            .unwrap_or_else(|| ctx.layout_sliver_child(index, constraints));

            if let Some(correction) = geometry.scroll_offset_correction {
                return correction;
            }

            let effective_layout_offset = layout_offset + geometry.paint_origin;
            let child_layout_offset = if geometry.visible || scroll_offset > 0.0 {
                effective_layout_offset
            } else {
                -scroll_offset + initial_layout_offset
            };
            self.staged_positions.push(StagedPosition {
                slot: index,
                layout_offset: child_layout_offset,
                growth_direction,
                paint_extent: geometry.paint_extent,
            });

            max_paint_offset =
                max_paint_offset.max(effective_layout_offset + geometry.paint_extent);
            scroll_offset -= geometry.scroll_extent;
            preceding_scroll_extent += geometry.scroll_extent;
            layout_offset += geometry.layout_extent;

            if geometry.cache_extent != 0.0 {
                remaining_cache_extent -= geometry.cache_extent - cache_extent_correction;
                cache_origin = (corrected_cache_origin + geometry.cache_extent).min(0.0);
            }

            self.update_out_of_band_data(growth_direction, &geometry);
        }

        0.0
    }

    fn update_out_of_band_data(
        &mut self,
        growth_direction: GrowthDirection,
        geometry: &SliverGeometry,
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

    /// Position every child the accepted pass staged, against the final size.
    fn commit_positions(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        size: Size,
    ) {
        // Taken out and put back so the vector keeps its capacity across
        // frames: this runs on every scroll pixel.
        let mut staged = std::mem::take(&mut self.staged_positions);
        for position in staged.drain(..) {
            ctx.position_child(
                position.slot,
                self.compute_absolute_paint_offset(
                    px(position.layout_offset),
                    position.growth_direction,
                    px(position.paint_extent),
                    size,
                ),
            );
        }
        self.staged_positions = staged;
    }

    fn compute_absolute_paint_offset(
        &self,
        layout_offset: Pixels,
        growth_direction: GrowthDirection,
        paint_extent: Pixels,
        size: Size,
    ) -> Offset {
        let layout_offset = layout_offset.get();
        let paint_extent = paint_extent.get();
        match growth_direction.apply_to_axis_direction(self.axis_direction) {
            TopToBottom => Offset::new(px(0.0), px(layout_offset)),
            BottomToTop => Offset::new(
                px(0.0),
                px(size.height.get() - layout_offset - paint_extent),
            ),
            LeftToRight => Offset::new(px(layout_offset), px(0.0)),
            RightToLeft => {
                Offset::new(px(size.width.get() - layout_offset - paint_extent), px(0.0))
            }
        }
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
        properties.add_double("scroll_offset", self.offset.pixels(), Some("px"));
        properties.add_double("cache_extent", self.cache_extent, Some("px"));
        properties.add_enum("cache_extent_style", self.cache_extent_style);
        properties.add_enum("paint_order", self.paint_order);
        if let Some(center) = self.center_sliver_index {
            properties.add_int("center_sliver_index", center as i64, None);
        }
    }
}
impl<O: ViewportOffset + 'static> RenderBox for RenderViewport<O> {
    type Arity = Variable;
    type ParentData = BoxParentData;

    // Flutter parity: `RenderViewport`/`RenderAbstractViewport` subscribes to
    // its `ViewportOffset` in `attach` and tears the subscription down in
    // `detach` (`rendering/viewport.dart`). See `offset_relayout_listener`'s
    // docs for what fires the mark and why it can never re-enter `perform_layout`.
    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.offset_listener = Some(register_offset_listener(&self.offset, handle.clone()));
        self.render_invalidation_handle = Some(handle);
    }

    fn detach(&mut self) {
        unregister_offset_listener(&self.offset, &mut self.offset_listener);
        self.render_invalidation_handle = None;
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        let size = ctx.constraints().biggest();
        let main_axis_extent = self.main_axis_extent(size);
        let cross_axis_extent = self.cross_axis_extent(size);
        self.child_count = ctx.child_count();
        // Flutter publishes the viewport dimension before laying anything
        // out, and a `ScrollPosition` may move `pixels` to answer it (a page
        // position keeps its fractional page across a resize). That is
        // correct for a healthy pass; for a degraded one it would move the
        // user's offset in a frame that is supposed to publish nothing, so
        // the offset is restored below if the pass turns out degraded. The
        // dimension itself is kept: it comes from this viewport's
        // constraints, which no stand-in touched.
        let pixels_before_pass = self.offset.pixels();
        let _ = self.offset.apply_viewport_dimension(main_axis_extent);

        if ctx.child_count() == 0 {
            self.min_scroll_extent = 0.0;
            self.max_scroll_extent = 0.0;
            self.max_scroll_obstruction_extent = 0.0;
            self.sliver_obstruction_extents.clear();
            self.has_visual_overflow = false;
            let _ = self.offset.apply_content_dimensions(0.0, 0.0);
            return size;
        }

        let max_layout_cycles = MAX_LAYOUT_CYCLES_PER_CHILD * ctx.child_count();
        let mut accepted = false;
        let mut degraded = false;
        for _ in 0..max_layout_cycles {
            let correction = self.attempt_layout(
                ctx,
                main_axis_extent,
                cross_axis_extent,
                self.offset.pixels(),
            );
            // A descendant's layout failed, or a poisoned one served its
            // stand-in, somewhere in this pass: the extents it accumulated
            // describe geometry this frame did not compute. Position what the
            // pass staged — the tree stays internally consistent, since every
            // offset and every child geometry come from one pass — but
            // publish nothing to the scroll position. A correction or a
            // content extent taken from a stand-in would move the user's
            // offset to a place the real content never had, and the frame
            // after the child recovers would have to move it back.
            if ctx.descendant_layout_degraded() {
                degraded = true;
                break;
            }
            if correction != 0.0 {
                self.offset.correct_by(correction);
                continue;
            }

            if self.offset.apply_content_dimensions(
                // Reverse-side slivers accumulate negative min_scroll_extent; report
                // it so scroll-range semantics match Flutter (pre-W3.2 hardcoded 0.0).
                self.min_scroll_extent,
                (self.max_scroll_extent - main_axis_extent).max(0.0),
            ) {
                accepted = true;
                break;
            }
        }
        if degraded {
            // Undo any offset movement `apply_viewport_dimension` made above.
            let drift = pixels_before_pass - self.offset.pixels();
            if drift != 0.0 {
                self.offset.correct_by(drift);
            }
            tracing::warn!(
                child_count = ctx.child_count(),
                "RenderViewport laid out over a degraded descendant; scroll \
                 dimensions were not published this frame, so the offset \
                 survives until the child recovers"
            );
        } else if !accepted {
            // Pathological non-convergence: a sliver child kept requesting
            // scroll corrections past the bounded budget. The scroll offset
            // is already clamped to a valid range by the loop's
            // `apply_content_dimensions`, so the committed geometry is sound
            // — only child positions reflect the last attempted offset and
            // self-correct on the next frame. Surface it in RELEASE: the
            // prior `debug_assert!` was silent in release (shipped the
            // non-converged frame unobserved) and crashed the app in debug on
            // a third-party widget bug. A warn is the right level — this is a
            // content bug, not a framework-invariant violation.
            tracing::warn!(
                child_count = ctx.child_count(),
                max_layout_cycles,
                "RenderViewport exceeded its bounded layout correction loop; \
                 committed the clamped offset, children self-correct next frame"
            );
        }

        self.commit_positions(ctx, size);
        size
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_children = |ctx: &mut PaintCx<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.has_visual_overflow {
            let clip_rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(clip_rect, Clip::HardEdge, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        if !ctx.is_within_own_size() {
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

/// A Box-protocol viewport that shrink-wraps its sliver children in the main axis.
///
/// Unlike [`RenderViewport`], which expands to the incoming main-axis extent,
/// this render object sizes itself to the sum of its slivers'
/// `max_paint_extent` values, constrained by its parent. It still expands in
/// the cross axis, matching Flutter's `RenderShrinkWrappingViewport`.
#[derive(Debug)]
pub struct RenderShrinkWrappingViewport<O = ScrollableViewportOffset> {
    axis_direction: AxisDirection,
    cross_axis_direction: AxisDirection,
    offset: O,
    cache_extent: f32,
    cache_extent_style: CacheExtentStyle,
    paint_order: SliverPaintOrder,
    child_count: usize,
    max_scroll_extent: f32,
    shrink_wrap_extent: f32,
    has_visual_overflow: bool,
    /// See [`RenderViewport::render_invalidation_handle`]'s matching field docs.
    render_invalidation_handle: Option<RenderInvalidationHandle>,
    /// See [`RenderViewport::offset_listener`]'s matching field docs.
    offset_listener: Option<OffsetListener>,
    /// The child positions the current pass decided; committed by
    /// `commit_positions` once the pass is accepted.
    staged_positions: Vec<StagedPosition>,
}

impl RenderShrinkWrappingViewport<ScrollableViewportOffset> {
    /// Creates a shrink-wrapping viewport with a zero scrollable offset.
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
            axis_direction,
            cross_axis_direction,
            offset,
            cache_extent: DEFAULT_CACHE_EXTENT,
            cache_extent_style: CacheExtentStyle::Pixel,
            paint_order: SliverPaintOrder::FirstIsTop,
            child_count: 0,
            max_scroll_extent: 0.0,
            shrink_wrap_extent: 0.0,
            has_visual_overflow: false,
            render_invalidation_handle: None,
            offset_listener: None,
            staged_positions: Vec::new(),
        }
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

    /// Replaces the viewport offset object wholesale. See
    /// [`RenderViewport::set_offset`] for the identity-check contract a
    /// caller injecting an external offset must follow, and for the
    /// attached-listener re-registration this mirrors exactly.
    #[inline]
    pub fn set_offset(&mut self, offset: O) -> flui_rendering::RenderUpdateImpact {
        unregister_offset_listener(&self.offset, &mut self.offset_listener);
        self.offset = offset;
        if let Some(handle) = self.render_invalidation_handle.clone() {
            self.offset_listener = Some(register_offset_listener(&self.offset, handle));
        }
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the sliver paint order. Hit testing uses the opposite order.
    #[inline]
    pub fn set_paint_order(
        &mut self,
        paint_order: SliverPaintOrder,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.paint_order == paint_order {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.paint_order = paint_order;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Sets the cache extent and interpretation mode.
    #[inline]
    pub const fn set_cache_extent(
        &mut self,
        cache_extent: f32,
        style: CacheExtentStyle,
    ) -> flui_rendering::RenderUpdateImpact {
        let same_style = matches!(
            (self.cache_extent_style, style),
            (CacheExtentStyle::Pixel, CacheExtentStyle::Pixel)
                | (CacheExtentStyle::Viewport, CacheExtentStyle::Viewport)
        );
        if self.cache_extent == cache_extent && same_style {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.cache_extent = cache_extent;
        self.cache_extent_style = style;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the scroll axis direction, re-deriving the cross-axis direction.
    ///
    /// Reports layout when the axis actually changed.
    #[inline]
    pub fn set_axis_direction(
        &mut self,
        axis_direction: AxisDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.axis_direction == axis_direction {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.axis_direction = axis_direction;
        self.cross_axis_direction = default_cross_axis_direction(axis_direction);
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Last total scroll extent reported by the sliver sequence.
    #[inline]
    #[must_use]
    pub const fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    /// Last unconstrained main-axis extent accumulated from child slivers.
    #[inline]
    #[must_use]
    pub const fn shrink_wrap_extent(&self) -> f32 {
        self.shrink_wrap_extent
    }

    /// Whether the last layout pass reported visual overflow.
    #[inline]
    #[must_use]
    pub const fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    fn calculated_cache_extent(&self, main_axis_extent: f32) -> f32 {
        if !main_axis_extent.is_finite() {
            return 0.0;
        }
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent.max(0.0),
            CacheExtentStyle::Viewport => (self.cache_extent * main_axis_extent).max(0.0),
        }
    }

    fn main_axis_extent_from_constraints(&self, constraints: &BoxConstraints) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => constraints.max_width.get(),
            Axis::Vertical => constraints.max_height.get(),
        }
    }

    fn cross_axis_extent_from_constraints(&self, constraints: &BoxConstraints) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => constraints.max_height.get(),
            Axis::Vertical => constraints.max_width.get(),
        }
    }

    fn constrain_main_axis_extent(&self, constraints: &BoxConstraints, extent: f32) -> f32 {
        match self.axis_direction.axis() {
            Axis::Horizontal => constraints.constrain_width(px(extent)).get(),
            Axis::Vertical => constraints.constrain_height(px(extent)).get(),
        }
    }

    fn size_from_extents(&self, cross_axis_extent: f32, main_axis_extent: f32) -> Size {
        match self.axis_direction.axis() {
            Axis::Horizontal => Size::new(px(main_axis_extent), px(cross_axis_extent)),
            Axis::Vertical => Size::new(px(cross_axis_extent), px(main_axis_extent)),
        }
    }

    fn empty_size(&self, constraints: &BoxConstraints) -> Size {
        match self.axis_direction.axis() {
            Axis::Horizontal => Size::new(constraints.min_width, constraints.max_height),
            Axis::Vertical => Size::new(constraints.max_width, constraints.min_height),
        }
    }

    fn debug_check_has_bounded_cross_axis(&self, constraints: &BoxConstraints) {
        match self.axis_direction.axis() {
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

    fn child_sliver_constraints(
        &self,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        fields: ChildSliverLayoutFields,
    ) -> SliverConstraints {
        SliverConstraints::new(
            self.axis_direction,
            fields.growth_direction,
            fields.user_scroll_direction,
            fields.scroll_offset,
            fields.preceding_scroll_extent,
            fields.overlap,
            fields.remaining_paint_extent,
            cross_axis_extent,
            self.cross_axis_direction,
            main_axis_extent,
            fields.remaining_cache_extent,
            fields.cache_origin,
        )
    }

    #[must_use = "scroll correction must be applied when non-zero"]
    fn attempt_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        corrected_offset: f32,
    ) -> f32 {
        self.staged_positions.clear();
        self.max_scroll_extent = 0.0;
        self.shrink_wrap_extent = 0.0;
        self.has_visual_overflow = corrected_offset < 0.0;

        let cache_extent = self.calculated_cache_extent(main_axis_extent);
        let remaining_paint_extent = (main_axis_extent + corrected_offset.min(0.0)).max(0.0);
        self.layout_child_sequence(
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
            },
        )
    }

    #[must_use = "correction value must be checked; 0.0 means layout accepted"]
    fn layout_child_sequence(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        params: LayoutChildSequenceParams,
    ) -> f32 {
        let LayoutChildSequenceParams {
            mut scroll_offset,
            overlap,
            mut layout_offset,
            remaining_paint_extent,
            main_axis_extent,
            cross_axis_extent,
            growth_direction,
            mut remaining_cache_extent,
            mut cache_origin,
            child_start,
            child_end,
        } = params;
        debug_assert_eq!(growth_direction, GrowthDirection::Forward);
        let initial_layout_offset = layout_offset;
        let adjusted_user_scroll_direction =
            flui_rendering::constraints::apply_growth_direction_to_scroll_direction(
                self.offset.user_scroll_direction(),
                growth_direction,
            );
        let mut max_paint_offset = layout_offset + overlap;
        let mut preceding_scroll_extent = 0.0;

        for index in child_start..child_end {
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
            let constraints = self.child_sliver_constraints(
                main_axis_extent,
                cross_axis_extent,
                ChildSliverLayoutFields {
                    growth_direction,
                    user_scroll_direction: adjusted_user_scroll_direction,
                    scroll_offset: sliver_scroll_offset,
                    preceding_scroll_extent,
                    overlap: max_paint_offset - layout_offset,
                    remaining_paint_extent: child_remaining_paint_extent,
                    remaining_cache_extent: child_remaining_cache_extent,
                    cache_origin: corrected_cache_origin,
                },
            );

            let geometry = try_cached_sliver_geometry(
                ctx,
                index,
                constraints,
                child_remaining_paint_extent,
                child_remaining_cache_extent,
                sliver_scroll_offset,
            )
            .unwrap_or_else(|| ctx.layout_sliver_child(index, constraints));

            if let Some(correction) = geometry.scroll_offset_correction {
                return correction;
            }

            let effective_layout_offset = layout_offset + geometry.paint_origin;
            let child_layout_offset = if geometry.visible || scroll_offset > 0.0 {
                effective_layout_offset
            } else {
                -scroll_offset + initial_layout_offset
            };
            self.staged_positions.push(StagedPosition {
                slot: index,
                layout_offset: child_layout_offset,
                growth_direction,
                paint_extent: geometry.paint_extent,
            });

            max_paint_offset =
                max_paint_offset.max(effective_layout_offset + geometry.paint_extent);
            scroll_offset -= geometry.scroll_extent;
            preceding_scroll_extent += geometry.scroll_extent;
            layout_offset += geometry.layout_extent;

            if geometry.cache_extent != 0.0 {
                remaining_cache_extent -= geometry.cache_extent - cache_extent_correction;
                cache_origin = (corrected_cache_origin + geometry.cache_extent).min(0.0);
            }

            self.update_out_of_band_data(&geometry);
        }

        0.0
    }

    fn update_out_of_band_data(&mut self, geometry: &SliverGeometry) {
        self.max_scroll_extent += geometry.scroll_extent;
        self.shrink_wrap_extent += geometry.max_paint_extent;
        if geometry.has_visual_overflow {
            self.has_visual_overflow = true;
        }
    }

    /// Position every child the accepted pass staged, against the final size.
    fn commit_positions(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        size: Size,
    ) {
        // Taken out and put back so the vector keeps its capacity across
        // frames: this runs on every scroll pixel.
        let mut staged = std::mem::take(&mut self.staged_positions);
        for position in staged.drain(..) {
            ctx.position_child(
                position.slot,
                self.compute_absolute_paint_offset(
                    px(position.layout_offset),
                    position.growth_direction,
                    px(position.paint_extent),
                    size,
                ),
            );
        }
        self.staged_positions = staged;
    }

    fn compute_absolute_paint_offset(
        &self,
        layout_offset: Pixels,
        growth_direction: GrowthDirection,
        paint_extent: Pixels,
        size: Size,
    ) -> Offset {
        let layout_offset = layout_offset.get();
        let paint_extent = paint_extent.get();
        match growth_direction.apply_to_axis_direction(self.axis_direction) {
            TopToBottom => Offset::new(px(0.0), px(layout_offset)),
            BottomToTop => Offset::new(
                px(0.0),
                px(size.height.get() - layout_offset - paint_extent),
            ),
            LeftToRight => Offset::new(px(layout_offset), px(0.0)),
            RightToLeft => {
                Offset::new(px(size.width.get() - layout_offset - paint_extent), px(0.0))
            }
        }
    }
}

impl Default for RenderShrinkWrappingViewport<ScrollableViewportOffset> {
    fn default() -> Self {
        Self::new(TopToBottom)
    }
}

impl<O: ViewportOffset + 'static> Diagnosticable for RenderShrinkWrappingViewport<O> {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("axis_direction", self.axis_direction);
        properties.add_enum("cross_axis_direction", self.cross_axis_direction);
        properties.add_double("scroll_offset", self.offset.pixels(), Some("px"));
        properties.add_double("cache_extent", self.cache_extent, Some("px"));
        properties.add_enum("cache_extent_style", self.cache_extent_style);
        properties.add_enum("paint_order", self.paint_order);
        properties.add_double("shrink_wrap_extent", self.shrink_wrap_extent, Some("px"));
    }
}

impl<O: ViewportOffset + 'static> RenderBox for RenderShrinkWrappingViewport<O> {
    type Arity = Variable;
    type ParentData = BoxParentData;

    // See `RenderViewport::attach`/`detach`'s matching docs — identical
    // shape over `RenderShrinkWrappingViewport`'s own `offset`.
    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.offset_listener = Some(register_offset_listener(&self.offset, handle.clone()));
        self.render_invalidation_handle = Some(handle);
    }

    fn detach(&mut self) {
        unregister_offset_listener(&self.offset, &mut self.offset_listener);
        self.render_invalidation_handle = None;
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        let constraints = *ctx.constraints();
        self.debug_check_has_bounded_cross_axis(&constraints);
        self.child_count = ctx.child_count();

        if ctx.child_count() == 0 {
            let size = self.empty_size(&constraints);
            self.max_scroll_extent = 0.0;
            self.shrink_wrap_extent = 0.0;
            self.has_visual_overflow = false;
            let _ = self.offset.apply_viewport_dimension(0.0);
            let _ = self.offset.apply_content_dimensions(0.0, 0.0);
            return size;
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
                self.offset.pixels(),
            );
            // See `RenderViewport::perform_layout`: a pass that read a
            // stand-in publishes nothing. The size still comes from what the
            // pass measured — a viewport must return one — but the scroll
            // position keeps the dimensions it has.
            if ctx.descendant_layout_degraded() {
                degraded = true;
                effective_extent =
                    self.constrain_main_axis_extent(&constraints, self.shrink_wrap_extent);
                break;
            }
            if correction != 0.0 {
                self.offset.correct_by(correction);
                continue;
            }

            effective_extent =
                self.constrain_main_axis_extent(&constraints, self.shrink_wrap_extent);
            let did_accept_viewport_dimension =
                self.offset.apply_viewport_dimension(effective_extent);
            let did_accept_content_dimension = self.offset.apply_content_dimensions(
                0.0,
                (self.max_scroll_extent - effective_extent).max(0.0),
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
        // resolved here, once, from what that pass staged. Flutter stores
        // logical offsets in parent data and resolves them at paint; resolving
        // them at commit is the same contract without a second layout of
        // every child.
        let size = self.size_from_extents(cross_axis_extent, effective_extent);
        self.commit_positions(ctx, size);
        size
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_children = |ctx: &mut PaintCx<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.has_visual_overflow {
            let clip_rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(clip_rect, Clip::HardEdge, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        if !ctx.is_within_own_size() {
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

fn try_cached_sliver_geometry(
    ctx: &BoxLayoutContext<'_, Variable, BoxParentData>,
    index: usize,
    constraints: SliverConstraints,
    child_remaining_paint_extent: f32,
    child_remaining_cache_extent: f32,
    sliver_scroll_offset: f32,
) -> Option<SliverGeometry> {
    if child_remaining_paint_extent > f32::EPSILON
        || child_remaining_cache_extent > f32::EPSILON
        || sliver_scroll_offset > f32::EPSILON
    {
        return None;
    }
    cached_clean_sliver_geometry(ctx, index, constraints)
}

fn cached_clean_sliver_geometry(
    ctx: &BoxLayoutContext<'_, Variable, BoxParentData>,
    index: usize,
    constraints: SliverConstraints,
) -> Option<SliverGeometry> {
    if ctx.sliver_child_needs_layout(index) {
        return None;
    }
    // A child whose geometry came from a degraded pass is laid out again
    // rather than served from the cache: the walk must reach the broken
    // descendant, which is what tells this viewport its own pass is degraded.
    // For a poisoned descendant that re-layout is the skip that serves its
    // stand-in — cheap, and the only thing that keeps the scroll position
    // from being published off collapsed content on every later frame.
    if ctx.sliver_child_geometry_degraded(index) {
        return None;
    }
    let (cached_constraints, cached_geometry) = ctx.cached_sliver_child_layout(index)?;
    if cached_constraints == constraints && cached_geometry.scroll_offset_correction.is_none() {
        Some(cached_geometry)
    } else {
        None
    }
}

#[cfg(test)]
mod offset_listener_tests {
    use super::*;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use flui_rendering::traits::RenderObject;
    use flui_rendering::view::ScrollPosition;

    /// Mints a real [`RenderInvalidationHandle`] by inserting a throwaway anchor node,
    /// rooting it, and running one frame — `RenderInvalidationHandle::new` is
    /// `pub(super)` to `flui_rendering::pipeline`, so a real one can only
    /// come from a live `PipelineOwner`. The one-frame run is the part
    /// `RenderAnimatedOpacity`'s own `anchor_handle` helper
    /// (`proxy/animated_opacity.rs`) doesn't need: every freshly-inserted
    /// node starts on the layout-dirty list ("every new node needs its
    /// first layout" — see `animated_size.rs`'s
    /// `attach_on_changed_state_immediately_marks_needs_layout` doc), so
    /// without running that first layout, a behavior test asserting "the
    /// listener marked this node dirty" could never fail — the baseline
    /// dirty entry would already satisfy it regardless of the listener.
    fn anchor_handle() -> (PipelineOwner, RenderInvalidationHandle) {
        let mut owner = PipelineOwner::new();
        let anchor =
            owner
                .insert(Box::new(RenderViewport::new(TopToBottom))
                    as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(anchor));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
        let (owner, result) = owner.run_frame();
        result.expect("the anchor's first frame must not error");
        let handle = owner
            .render_invalidation_handle(anchor)
            .expect("the rooted anchor id must still be live after its first frame");
        (owner, handle)
    }

    // attach must register a listener and detach must clear it — a
    // white-box assertion on the private `offset_listener`/`render_invalidation_handle`
    // fields is the most direct proof available, mirroring
    // `RenderAnimatedOpacity::attach_registers_listener_and_detach_clears_it`.
    #[test]
    fn attach_registers_a_relayout_listener_and_detach_clears_it() {
        let (_owner, handle) = anchor_handle();
        let mut viewport = RenderViewport::new(TopToBottom);
        assert!(
            viewport.offset_listener.is_none(),
            "no listener before attach"
        );

        RenderBox::attach(&mut viewport, handle);
        assert!(
            viewport.offset_listener.is_some(),
            "attach must register a listener"
        );
        assert!(
            viewport.render_invalidation_handle.is_some(),
            "attach must retain the handle for a later set_offset re-registration"
        );

        RenderBox::detach(&mut viewport);
        assert!(
            viewport.offset_listener.is_none(),
            "detach must clear the listener"
        );
        assert!(
            viewport.render_invalidation_handle.is_none(),
            "detach must clear the retained handle"
        );
    }

    /// At the pipeline level rather than the widget level: after `attach`,
    /// mutating the offset OUTSIDE `perform_layout` (no layout call, no
    /// widget rebuild) must mark the bound node needing layout — this is
    /// the render-side listener the whole change adds. `anchor_handle`
    /// already ran the anchor's first frame, so the
    /// layout-dirty list starts clean: every assertion below is on the
    /// listener's marginal effect, not insert's baseline "needs first
    /// layout" mark.
    #[test]
    fn external_offset_mutation_after_attach_marks_the_bound_node_needing_layout() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        assert!(
            owner.nodes_needing_layout().iter().all(|d| d.id != anchor),
            "the fixture must start with a clean layout-dirty baseline"
        );

        let position = ScrollPosition::new(0.0);
        let mut viewport = RenderViewport::with_offset(TopToBottom, LeftToRight, position.clone());
        RenderBox::attach(&mut viewport, handle);

        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().all(|d| d.id != anchor),
            "attach alone (registering the listener) must not itself mark the node dirty"
        );

        // External mutation: no perform_layout, no rebuild — only the
        // listener `attach` registered on `position`'s shared state.
        position.set_pixels(50.0);

        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().any(|d| d.id == anchor),
            "an external ScrollPosition mutation after attach must mark the bound node \
             needing layout via the offset listener"
        );
    }

    /// `set_offset` while attached must move the listener, not duplicate or
    /// drop it — removing the SAME `Arc` from the OLD offset (the ptr-eq
    /// removal contract `ViewportOffset::add_listener`/`remove_listener`
    /// document) and registering a fresh one on the NEW offset, both bound
    /// to the same retained handle.
    #[test]
    fn set_offset_while_attached_moves_the_relayout_listener_to_the_new_offset() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        let old_position = ScrollPosition::new(0.0);
        let new_position = ScrollPosition::new(0.0);
        let mut viewport =
            RenderViewport::with_offset(TopToBottom, LeftToRight, old_position.clone());

        RenderBox::attach(&mut viewport, handle);
        assert_eq!(
            viewport.set_offset(new_position.clone()),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().all(|d| d.id != anchor),
            "attach + set_offset alone must not mark the node dirty"
        );

        old_position.set_pixels(10.0);
        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().all(|d| d.id != anchor),
            "the OLD offset's listener must be removed by set_offset — mutating the old \
             offset after the swap must not mark layout"
        );

        new_position.set_pixels(10.0);
        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().any(|d| d.id == anchor),
            "the NEW offset must carry the relayout listener after set_offset while attached"
        );
    }

    /// Documents the known limitation `offset_relayout_listener`'s
    /// `ChannelFull` comment describes: a full dirty channel does not mean
    /// THIS node's own mark is already queued — an unrelated request from
    /// elsewhere in the tree can fill the last slot just as easily, and
    /// there is no retry mechanism, so the send is dropped and the node
    /// stays off the layout-dirty list until something else marks it.
    #[test]
    fn channel_full_backpressure_drops_the_mark_and_the_node_stays_off_the_dirty_list() {
        // Capacity 1 makes a single UNRELATED request enough to saturate
        // the channel, isolating the scenario without an elaborate fill loop.
        let mut owner = PipelineOwner::new_with_capacity(1);
        let anchor =
            owner
                .insert(Box::new(RenderViewport::new(TopToBottom))
                    as Box<dyn RenderObject<BoxProtocol>>);
        owner.set_root_id(Some(anchor));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
        let (mut owner, result) = owner.run_frame();
        result.expect("the anchor's first frame must not error");
        let handle = owner
            .render_invalidation_handle(anchor)
            .expect("the rooted anchor id must still be live after its first frame");

        let position = ScrollPosition::new(0.0);
        let mut viewport = RenderViewport::with_offset(TopToBottom, LeftToRight, position.clone());
        RenderBox::attach(&mut viewport, handle);

        // Saturate the one-slot channel with a paint request. It only needs to
        // occupy the slot the layout listener wants.
        owner
            .render_invalidation_handle(anchor)
            .expect("the anchor remains attached")
            .mark_needs_paint()
            .expect("the first send into a freshly-drained 1-capacity channel must fit");

        // The offset listener now tries to send and gets ChannelFull —
        // dropped, per the honest comment on `offset_relayout_listener`.
        position.set_pixels(50.0);

        owner.drain_pending_dirty();
        assert!(
            owner.nodes_needing_layout().iter().all(|d| d.id != anchor),
            "under channel backpressure the offset listener's mark is dropped, not queued — \
             the node must stay off the layout-dirty list until an unrelated mutation frees \
             a slot and retries it"
        );
    }
}
