//! `ViewportCore` — what a viewport is whatever rule sizes it.
//!
//! Both viewports keep a scroll offset with a relayout subscription on it, an
//! axis, a cache extent, a paint order and a clip behaviour. Both lay out a run
//! of sliver children with one walk, stage where each child goes, position the
//! accepted pass against the final size, and answer paint, hit testing and
//! clips from what that pass committed. How a viewport sizes itself, where its
//! zero-scroll line sits and what it publishes to the offset stay with
//! `RenderViewport` and `RenderShrinkWrappingViewport`; what one pass
//! accumulates is a [`PassRecord`] each of them defines.

use std::{cell::Cell, rc::Rc};

use flui_foundation::Variable;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_foundation::inspect::{Fields, Unit};
use flui_foundation::panic::FirstPanic;
use flui_painting::paint::Clip;
use flui_rendering::{
    RenderResult, RenderUpdateImpact,
    constraints::{
        AxisDirection,
        AxisDirection::{BottomToTop, LeftToRight, RightToLeft, TopToBottom},
        GrowthDirection, SliverConstraints, SliverGeometry,
    },
    context::{BoxHitTestContext, BoxLayoutContext, PaintContext},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::SemanticsClip,
    view::{CacheExtentStyle, ScrollDirection, SliverPaintOrder, ViewportOffset},
};

/// How many correction passes a viewport tries per child before it commits
/// the last one.
pub(super) const MAX_LAYOUT_CYCLES_PER_CHILD: usize = 10;
const DEFAULT_CACHE_EXTENT: f64 = 250.0;

/// A registered [`ViewportOffset`] listener [`Rc`], wrapped so the viewports'
/// `#[derive(Debug)]` doesn't need a hand-written `Debug` impl for a value that
/// is fundamentally an opaque closure — `Debug` just reports that a listener is
/// registered, not what it does.
struct OffsetListener<O> {
    callback: Rc<dyn Fn()>,
    active: Rc<Cell<bool>>,
    cancel: fn(&O, &Rc<dyn Fn()>),
}

impl<O> std::fmt::Debug for OffsetListener<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OffsetListener(..)")
    }
}

/// Builds the render-side [`ViewportOffset`] listener that
/// [`ViewportCore::attach`] (and any later [`ViewportCore::set_offset`]
/// re-registration) installs: a self-mark that requests a re-layout of the node
/// bound to `handle` whenever the offset's `pixels` changes out-of-band — a
/// write of its `ScrollOffsetWriter` (a gesture, a `ScrollController::jump_to`),
/// or the post-frame content-dimension flush.
///
/// `apply_viewport_dimension`/`apply_content_dimensions`/`correct_by` never
/// notify synchronously — that is `ViewportOffset`'s own contract (see the
/// `scroll_position` module docs) — so this listener can only ever fire from
/// OUTSIDE `perform_layout`; there is no synchronous mark-during-layout
/// re-entrancy to guard against here.
fn offset_relayout_listener(
    handle: LocalRenderInvalidationHandle,
    active: Rc<Cell<bool>>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        // Local admission is independent of external channel capacity.
        // A withdrawn attachment has no remaining viewport work to deliver.
        if active.get() {
            let _ = handle.mark_needs_layout();
        }
    })
}

/// Removes `*listener` (if any) from `offset`, via the trait's ptr-eq removal
/// contract, using the callback that was registered. The cancellation function
/// is installed when the offset's trait is known, so owner Drop needs no extra
/// public bound or second owner of the offset.
fn unregister_offset_listener<O>(
    offset: &O,
    listener: &mut Option<OffsetListener<O>>,
    failure: &mut FirstPanic,
) {
    if let Some(listener) = listener.take() {
        listener.active.set(false);
        failure.contain(|| (listener.cancel)(offset, &listener.callback));
        failure.retire(listener);
    }
}

/// Registers a fresh [`offset_relayout_listener`] on `offset`, bound to
/// `handle`.
fn register_offset_listener<O: ViewportOffset>(
    offset: &O,
    handle: LocalRenderInvalidationHandle,
    failure: &mut FirstPanic,
) -> Option<OffsetListener<O>> {
    let active = Rc::new(Cell::new(true));
    let callback = offset_relayout_listener(handle, active.clone());
    let registered = failure.contain(|| offset.add_listener(callback.clone()));
    if registered.is_some() {
        Some(OffsetListener {
            callback,
            active,
            cancel: |offset, callback| offset.remove_listener(callback),
        })
    } else {
        active.set(false);
        // Pointer identity is already a cancellation capability even when
        // admission panics before acknowledging whether it installed it.
        failure.contain(|| offset.remove_listener(&callback));
        failure.retire(callback);
        None
    }
}

/// The cross-axis direction a viewport takes when nothing overrides it: the
/// left-to-right / top-to-bottom one perpendicular to `axis_direction`.
pub(super) const fn default_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction {
        TopToBottom | BottomToTop => LeftToRight,
        LeftToRight | RightToLeft => TopToBottom,
    }
}

/// Parameters for one forward or reverse child walk,
/// [`ViewportCore::layout_child_sequence`].
#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutChildSequenceParams {
    pub(super) scroll_offset: f64,
    pub(super) overlap: f64,
    pub(super) layout_offset: f64,
    pub(super) remaining_paint_extent: f64,
    pub(super) main_axis_extent: f64,
    pub(super) cross_axis_extent: f64,
    pub(super) growth_direction: GrowthDirection,
    pub(super) remaining_cache_extent: f64,
    pub(super) cache_origin: f64,
    pub(super) child_start: usize,
    pub(super) child_end: usize,
    /// Whether this walk visits `[child_start, child_end)` back-to-front —
    /// a `RenderViewport`'s reverse group. `false` walks front-to-back, the
    /// only order `RenderShrinkWrappingViewport` ever uses.
    pub(super) reversed: bool,
}

/// Iterates `[start, end)` either front-to-back or back-to-front, so
/// [`ViewportCore::layout_child_sequence`] can share one loop body for both
/// the forward and reverse groups.
enum ChildIndexWalk {
    Forward(std::ops::Range<usize>),
    Reverse(std::iter::Rev<std::ops::Range<usize>>),
}

impl ChildIndexWalk {
    fn new(start: usize, end: usize, reversed: bool) -> Self {
        if reversed {
            Self::Reverse((start..end).rev())
        } else {
            Self::Forward(start..end)
        }
    }
}

impl Iterator for ChildIndexWalk {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        match self {
            Self::Forward(range) => range.next(),
            Self::Reverse(range) => range.next(),
        }
    }
}

/// Where a pass decided a child goes, in the viewport's logical coordinates.
///
/// A pass stages these instead of positioning as it walks: the physical
/// offset needs the viewport's final size (a reverse axis measures from the
/// far edge), which a shrink-wrapping viewport only knows once its passes
/// have settled. Positioning once, from the accepted pass, is what lets that
/// viewport skip a second layout of every child.
#[derive(Debug, Clone, Copy)]
pub(super) struct StagedPosition {
    slot: usize,
    layout_offset: f64,
    growth_direction: GrowthDirection,
    paint_extent: f64,
    /// How far the leading edge of this child's paint clip is pushed in by
    /// the slivers ahead of it — the room a pinned header occupies. `0.0`
    /// when nothing overlaps it. Staged with the position because it belongs
    /// to the same accepted pass: a clip taken from a rejected pass would
    /// describe a layout the tree never adopted.
    paint_clip_correction: f64,
}

/// What one layout pass of a viewport accumulates: the staged positions and
/// child count every viewport keeps, and the extents its own sizing rule folds
/// from each child's geometry.
///
/// A viewport keeps two records and swaps them around a pass
/// ([`ViewportCore::begin_pass`], [`ViewportCore::discard_pass`]), so a
/// rejected pass cannot replace the metrics of the last published geometry.
pub(super) trait PassRecord: Default {
    /// How many children the pass laid out; hit testing walks this many.
    fn child_count(&self) -> usize;
    /// Whether some child reported visual overflow, which paint and the paint
    /// clip answer from.
    fn has_visual_overflow(&self) -> bool;
    /// The positions the pass staged, drained by
    /// [`ViewportCore::commit_positions`].
    fn staged_positions_mut(&mut self) -> &mut Vec<StagedPosition>;
    /// Folds the geometry the child in `slot` reported, growing in
    /// `growth_direction`, into the pass's extents.
    fn record_child(
        &mut self,
        slot: usize,
        growth_direction: GrowthDirection,
        geometry: &SliverGeometry,
    );
}

/// Per-child sliver constraint fields that vary during a viewport walk.
#[derive(Debug, Clone, Copy)]
struct ChildSliverLayoutFields {
    growth_direction: GrowthDirection,
    user_scroll_direction: ScrollDirection,
    scroll_offset: f64,
    preceding_scroll_extent: f64,
    overlap: f64,
    remaining_paint_extent: f64,
    remaining_cache_extent: f64,
    cache_origin: f64,
}

/// What the accepted pass left for [`ViewportCore::semantics_clip`]
/// and its paint-clip counterpart to answer from.
#[derive(Debug, Default, Clone)]
struct CommittedClipGeometry {
    /// The cache extent in pixels, already resolved from
    /// [`CacheExtentStyle`].
    cache_extent: f64,
    /// Slot-indexed; see [`CommittedChildClip`].
    child_clips: Vec<CommittedChildClip>,
}

/// What the accepted pass committed about ONE child's paint clip.
///
/// The growth direction rides along with the correction because the direction
/// the leading edge is pushed from depends on it, and the staged positions the
/// pass built it from are drained by `commit_positions` — reading them back
/// afterwards would find an empty vector and silently answer "forward" for
/// every reverse-group child.
#[derive(Debug, Default, Clone, Copy)]
struct CommittedChildClip {
    /// How far the leading edge of this child's paint clip is pushed in; see
    /// [`StagedPosition::paint_clip_correction`].
    correction: f64,
    /// Which end of the axis that edge is measured from.
    growth_direction: GrowthDirection,
}

/// Pushes `rect`'s leading edge in by `amount`, along `direction`.
///
/// The moved edge stops at the opposite one. An overlap wider than the
/// viewport would otherwise invert the rect, and `Rect` does not normalize:
/// an inverted clip is a value every consumer has to reason about separately,
/// where a collapsed one already means "nothing survives" everywhere.
fn shrink_leading_edge(rect: Rect<f64>, direction: AxisDirection, amount: f64) -> Rect<f64> {
    let (mut left, mut top) = (rect.min.x, rect.min.y);
    let (mut right, mut bottom) = (rect.max.x, rect.max.y);
    match direction {
        AxisDirection::TopToBottom => top = (top + amount).min(bottom),
        AxisDirection::BottomToTop => bottom = (bottom - amount).max(top),
        AxisDirection::LeftToRight => left = (left + amount).min(right),
        AxisDirection::RightToLeft => right = (right - amount).max(left),
    }
    Rect::from_ltrb(left, top, right, bottom)
}

/// Grows `rect` by `amount` at BOTH ends of `axis`.
fn grow_along_axis(rect: Rect<f64>, axis: Axis, amount: f64) -> Rect<f64> {
    let (left, top) = (rect.min.x, rect.min.y);
    let (right, bottom) = (rect.max.x, rect.max.y);
    match axis {
        Axis::Vertical => Rect::from_ltrb(left, top - amount, right, bottom + amount),
        Axis::Horizontal => Rect::from_ltrb(left - amount, top, right + amount, bottom),
    }
}

/// The state both viewports hold and the operations that read it the same way
/// in each; `L` is the viewport's [`PassRecord`].
///
/// Its `Drop` cancels the offset subscription, so a viewport releases its
/// offset callback with its render owner (flui-objects `ARCHITECTURE.md`,
/// "Animation and viewport subscriptions end with their render owner").
#[derive(Debug)]
pub(super) struct ViewportCore<O, L> {
    pub(super) axis_direction: AxisDirection,
    cross_axis_direction: AxisDirection,
    pub(super) offset: O,
    cache_extent: f64,
    cache_extent_style: CacheExtentStyle,
    paint_order: SliverPaintOrder,
    /// The record of the pass being laid out, or of the last one when no
    /// layout is running.
    pub(super) layout: L,
    /// The other record: the last committed pass while a new one is laid out
    /// into `layout`, restored by [`discard_pass`](Self::discard_pass).
    spare_layout: L,
    /// The repaint handle this node was bound to in [`attach`](Self::attach),
    /// `None` before attach / after [`detach`](Self::detach). `set_offset`
    /// clones it to re-register `offset_listener` on a swapped-in offset while
    /// the node is live in a pipeline.
    render_invalidation_handle: Option<LocalRenderInvalidationHandle>,
    /// The listener `attach` (or a live `set_offset`) registered on `offset` —
    /// retained so `detach`/`set_offset` can remove the SAME `Rc` via
    /// [`ViewportOffset::remove_listener`]'s ptr-eq contract.
    offset_listener: Option<OffsetListener<O>>,
    /// How content that overflows this viewport is clipped when it paints.
    /// `Clip::None` clips nothing at all, so a child may paint outside the
    /// viewport's bounds (default hard-edge).
    clip_behavior: Clip,
    /// The size and cache extent the last pass laid out under, and the
    /// per-slot paint-clip corrections it committed — everything the
    /// semantics walk needs to answer what a child is clipped to, since it
    /// asks after layout has returned and no longer has a layout context.
    ///
    /// Size and cache extent come from this node's own constraints, which no
    /// descendant stand-in can move, so unlike the scroll dimensions they are
    /// written on every pass rather than only on an accepted one.
    committed_clips: CommittedClipGeometry,
}

impl<O: ViewportOffset + 'static, L: PassRecord> ViewportCore<O, L> {
    /// A viewport core with the default cache extent, paint order and clip,
    /// not yet attached.
    pub(super) fn new(
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
            layout: L::default(),
            spare_layout: L::default(),
            render_invalidation_handle: None,
            offset_listener: None,
            clip_behavior: Clip::HardEdge,
            committed_clips: CommittedClipGeometry::default(),
        }
    }

    /// Replaces the offset, moving the relayout subscription to it when the
    /// node is attached: the new offset is committed first, then the outgoing
    /// callback is revoked and cancelled, a fresh one is registered on the new
    /// offset even if cancellation failed, and the outgoing offset is retired,
    /// each under one [`FirstPanic`]. After a failure the node is still marked
    /// for layout before the first failure resumes.
    pub(super) fn set_offset(&mut self, offset: O) -> RenderUpdateImpact {
        let outgoing = std::mem::replace(&mut self.offset, offset);
        let mut listener = self.offset_listener.take();
        let mut failure = FirstPanic::new();
        unregister_offset_listener(&outgoing, &mut listener, &mut failure);
        if let Some(handle) = self.render_invalidation_handle.clone() {
            self.offset_listener = register_offset_listener(&self.offset, handle, &mut failure);
        }
        failure.retire(outgoing);
        if failure.is_held()
            && let Some(handle) = &self.render_invalidation_handle
        {
            failure.contain(|| {
                let _ = handle.mark_needs_layout();
            });
        }
        failure.resume();
        RenderUpdateImpact::LAYOUT
    }

    /// Binds the node to `handle` and subscribes to the offset, after
    /// dropping any earlier subscription. See [`offset_relayout_listener`] for
    /// what fires the mark and why it can never re-enter `perform_layout`.
    pub(super) fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.detach();
        self.render_invalidation_handle = Some(handle.clone());
        let mut failure = FirstPanic::new();
        self.offset_listener = register_offset_listener(&self.offset, handle, &mut failure);
        failure.resume();
    }

    /// Unbinds the node and cancels its offset subscription.
    pub(super) fn detach(&mut self) {
        self.render_invalidation_handle = None;
        let mut failure = FirstPanic::new();
        unregister_offset_listener(&self.offset, &mut self.offset_listener, &mut failure);
        failure.resume();
    }

    pub(super) fn set_clip_behavior(&mut self, clip_behavior: Clip) -> RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return RenderUpdateImpact::NONE;
        }
        let clips_semantics = (self.clip_behavior == Clip::None) != (clip_behavior == Clip::None);
        self.clip_behavior = clip_behavior;
        if clips_semantics {
            RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
        } else {
            RenderUpdateImpact::PAINT
        }
    }

    pub(super) const fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    pub(super) fn set_paint_order(&mut self, paint_order: SliverPaintOrder) -> RenderUpdateImpact {
        if self.paint_order == paint_order {
            return RenderUpdateImpact::NONE;
        }
        self.paint_order = paint_order;
        RenderUpdateImpact::PAINT
    }

    pub(super) const fn set_cache_extent(
        &mut self,
        cache_extent: f64,
        style: CacheExtentStyle,
    ) -> RenderUpdateImpact {
        let same_style = matches!(
            (self.cache_extent_style, style),
            (CacheExtentStyle::Pixel, CacheExtentStyle::Pixel)
                | (CacheExtentStyle::Viewport, CacheExtentStyle::Viewport)
        );
        if self.cache_extent == cache_extent && same_style {
            return RenderUpdateImpact::NONE;
        }
        self.cache_extent = cache_extent;
        self.cache_extent_style = style;
        RenderUpdateImpact::LAYOUT
    }

    pub(super) fn set_axis_direction(
        &mut self,
        axis_direction: AxisDirection,
    ) -> RenderUpdateImpact {
        if self.axis_direction == axis_direction {
            return RenderUpdateImpact::NONE;
        }
        self.axis_direction = axis_direction;
        self.cross_axis_direction = default_cross_axis_direction(axis_direction);
        RenderUpdateImpact::LAYOUT
    }

    pub(super) fn set_cross_axis_direction(
        &mut self,
        cross_axis_direction: AxisDirection,
    ) -> RenderUpdateImpact {
        if self.cross_axis_direction == cross_axis_direction {
            return RenderUpdateImpact::NONE;
        }
        self.cross_axis_direction = cross_axis_direction;
        RenderUpdateImpact::LAYOUT
    }

    /// The cache extent in pixels for a main axis of `main_axis_extent`,
    /// resolved from [`CacheExtentStyle`] and never negative.
    pub(super) fn calculated_cache_extent(&self, main_axis_extent: f64) -> f64 {
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent.max(0.0),
            CacheExtentStyle::Viewport => (self.cache_extent * main_axis_extent).max(0.0),
        }
    }

    /// Makes the spare record the one a new pass writes, keeping the last
    /// pass's record aside until the pass succeeds.
    pub(super) fn begin_pass(&mut self) {
        std::mem::swap(&mut self.layout, &mut self.spare_layout);
    }

    /// Restores the record of the last pass after a pass that failed.
    pub(super) fn discard_pass(&mut self) {
        std::mem::swap(&mut self.layout, &mut self.spare_layout);
    }

    fn child_sliver_constraints(
        &self,
        main_axis_extent: f64,
        cross_axis_extent: f64,
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

    /// Lays out the children `params` names in its order, staging each one's
    /// position and folding its geometry into the pass record. Returns the
    /// first scroll correction a child asks for, or `0.0` when the walk
    /// finished.
    #[must_use = "correction value must be checked; 0.0 means layout accepted"]
    pub(super) fn layout_child_sequence(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        params: LayoutChildSequenceParams,
    ) -> RenderResult<f64> {
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
            reversed,
        } = params;
        let initial_layout_offset = layout_offset;
        let adjusted_user_scroll_direction =
            flui_rendering::constraints::apply_growth_direction_to_scroll_direction(
                self.offset.user_scroll_direction(),
                growth_direction,
            );
        let mut max_paint_offset = layout_offset + overlap;
        let mut preceding_scroll_extent = 0.0;

        for index in ChildIndexWalk::new(child_start, child_end, reversed) {
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

            let geometry = match try_cached_sliver_geometry(
                ctx,
                index,
                constraints,
                child_remaining_paint_extent,
                child_remaining_cache_extent,
                sliver_scroll_offset,
            ) {
                Some(geometry) => geometry,
                None => ctx.layout_sliver_child(index, constraints)?,
            };

            if let Some(correction) = geometry.scroll_offset_correction {
                return Ok(correction);
            }

            let effective_layout_offset = layout_offset + geometry.paint_origin;
            let child_layout_offset = if geometry.visible || scroll_offset > 0.0 {
                effective_layout_offset
            } else {
                -scroll_offset + initial_layout_offset
            };
            // A child nothing overlaps is clipped to the whole viewport; one a pinned header overlaps has
            // its clip's leading edge pushed in to where that overlap starts.
            // Computed here, where the child's own constraints are still in
            // hand, and staged so only an accepted pass commits it.
            let child_overlap = max_paint_offset - layout_offset;
            let paint_clip_correction = if child_overlap == 0.0 || !main_axis_extent.is_finite() {
                0.0
            } else {
                (main_axis_extent - child_remaining_paint_extent) + child_overlap
            };
            self.layout.staged_positions_mut().push(StagedPosition {
                slot: index,
                layout_offset: child_layout_offset,
                growth_direction,
                paint_extent: geometry.paint_extent,
                paint_clip_correction,
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

            self.layout.record_child(index, growth_direction, &geometry);
        }

        Ok(0.0)
    }

    /// Position every child the accepted pass staged, against the final size.
    pub(super) fn commit_positions(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        size: Size,
        cache_extent: f64,
    ) {
        // The cache extent the semantics walk will ask about, recorded here
        // because it asks long after `perform_layout` has returned its
        // context. The SIZE is not recorded: the walk has the node's own size
        // and passes it in, so a committed copy would be a second answer to a
        // question that already has one, and a second thing to keep honest.
        // The extent derives from this viewport's own constraints, which no
        // descendant stand-in can move, so it is recorded on every pass —
        // unlike the scroll dimensions, which a degraded pass withholds.
        self.committed_clips.cache_extent = cache_extent;
        // Taken out and put back so the vector keeps its capacity across
        // frames: this runs on every scroll pixel.
        let mut staged = std::mem::take(self.layout.staged_positions_mut());
        // Slot-indexed: the walk may visit the reverse group first, so write
        // by absolute slot rather than in visit order.
        self.committed_clips
            .child_clips
            .resize(self.layout.child_count(), CommittedChildClip::default());
        self.committed_clips
            .child_clips
            .fill(CommittedChildClip::default());
        for position in &staged {
            if let Some(slot) = self.committed_clips.child_clips.get_mut(position.slot) {
                *slot = CommittedChildClip {
                    correction: position.paint_clip_correction,
                    growth_direction: position.growth_direction,
                };
            }
        }
        for position in staged.drain(..) {
            ctx.position_child(
                position.slot,
                self.compute_absolute_paint_offset(
                    position.layout_offset,
                    position.growth_direction,
                    position.paint_extent,
                    size,
                ),
            );
        }
        *self.layout.staged_positions_mut() = staged;
    }

    fn compute_absolute_paint_offset(
        &self,
        layout_offset: f64,
        growth_direction: GrowthDirection,
        paint_extent: f64,
        size: Size,
    ) -> Offset {
        match growth_direction.apply_to_axis_direction(self.axis_direction) {
            TopToBottom => Offset::new(0.0, layout_offset),
            BottomToTop => Offset::new(0.0, size.height - layout_offset - paint_extent),
            LeftToRight => Offset::new(layout_offset, 0.0),
            RightToLeft => Offset::new(size.width - layout_offset - paint_extent, 0.0),
        }
    }

    /// Reports the settings every viewport has, in the order both report them.
    pub(super) fn inspect(&self, out: &mut Fields<'_>) {
        out.field("axis_direction", &self.axis_direction);
        out.field("cross_axis_direction", &self.cross_axis_direction);
        out.field("scroll_offset", &self.offset.pixels())
            .unit(Unit::LogicalPx);
        out.field("cache_extent", &self.cache_extent)
            .unit(Unit::LogicalPx);
        out.field("cache_extent_style", &self.cache_extent_style);
        out.field("paint_order", &self.paint_order);
    }

    /// Paints the children in paint order, clipped to the viewport's bounds
    /// when the last pass overflowed and the clip behaviour clips.
    pub(super) fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
        let paint_children = |ctx: &mut PaintContext<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.layout.has_visual_overflow() && self.clip_behavior != Clip::None {
            let clip_rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(clip_rect, self.clip_behavior, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    /// The viewport's own bounds, with the
    /// leading edge pushed in by whatever overlaps this child — the room a
    /// pinned header takes. Paint clips only content that overflows the
    /// viewport and never under `Clip::None`; otherwise this reports
    /// nothing, and content painted outside the viewport stays a fully present
    /// accessibility node.
    ///
    /// The direction the correction pushes from follows the child's growth
    /// direction: a reverse-group child is measured from the far edge.
    pub(super) fn describe_approximate_paint_clip(
        &self,
        child_slot: usize,
        size: Size,
    ) -> Option<Rect<f64>> {
        if self.clip_behavior == Clip::None || !self.layout.has_visual_overflow() {
            return None;
        }
        let clip = Rect::from_origin_size(Point::ZERO, size);
        let committed = self
            .committed_clips
            .child_clips
            .get(child_slot)
            .copied()
            .unwrap_or_default();
        if committed.correction == 0.0 {
            return Some(clip);
        }
        let effective = match committed.growth_direction {
            GrowthDirection::Forward => self.axis_direction,
            GrowthDirection::Reverse => self.axis_direction.opposite(),
        };
        Some(shrink_leading_edge(clip, effective, committed.correction))
    }

    /// The viewport's bounds grown by the cache extent along the scroll axis.
    ///
    /// Ordinary publication follows this cache. Materialized descendants with
    /// a reveal ancestor remain addressable beyond it; viewport paint clipping
    /// alone does not hide them from native ScrollIntoView requests. The same
    /// clip applies to every child.
    pub(super) fn semantics_clip(&self, size: Size) -> SemanticsClip {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        let cache = self.committed_clips.cache_extent;
        if cache <= 0.0 {
            return SemanticsClip::ScrollCache(bounds);
        }
        SemanticsClip::ScrollCache(grow_along_axis(bounds, self.axis_direction.axis(), cache))
    }

    /// Hit tests the children in the reverse of paint order, the topmost
    /// first, inside the viewport's own size.
    pub(super) fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>,
    ) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        let child_count = self.layout.child_count();
        match self.paint_order {
            SliverPaintOrder::FirstIsTop => {
                for index in 0..child_count {
                    if ctx.hit_test_child_at_layout_offset(index) {
                        return true;
                    }
                }
            }
            SliverPaintOrder::LastIsTop => {
                for index in (0..child_count).rev() {
                    if ctx.hit_test_child_at_layout_offset(index) {
                        return true;
                    }
                }
            }
        }

        false
    }
}

impl<O, L> Drop for ViewportCore<O, L> {
    fn drop(&mut self) {
        self.render_invalidation_handle = None;
        let mut failure = FirstPanic::new();
        unregister_offset_listener(&self.offset, &mut self.offset_listener, &mut failure);
        failure.resume();
    }
}

fn try_cached_sliver_geometry(
    ctx: &BoxLayoutContext<'_, Variable, BoxParentData>,
    index: usize,
    constraints: SliverConstraints,
    child_remaining_paint_extent: f64,
    child_remaining_cache_extent: f64,
    sliver_scroll_offset: f64,
) -> Option<SliverGeometry> {
    if child_remaining_paint_extent > f64::EPSILON
        || child_remaining_cache_extent > f64::EPSILON
        || sliver_scroll_offset > f64::EPSILON
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
