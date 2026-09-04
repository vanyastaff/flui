//! `RenderViewport` — Box render object that drives sliver children.
//!
//! `RenderViewport` ports Flutter's `center`/`anchor` model in full
//! (`rendering/viewport.dart`'s `_attemptLayout`): `center` names the first
//! FORWARD child, the prefix before it is the reverse group (walked
//! backwards, laid out first), and `anchor` places the zero-scroll line at
//! `main_axis_extent * anchor` from the leading edge. `showOnScreen` and lazy
//! child creation stay out of this file. `RenderShrinkWrappingViewport` has
//! no `center`/`anchor` in Flutter either — it always lays out every child
//! forward from the first.

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
    /// Whether this walk visits `[child_start, child_end)` back-to-front —
    /// Flutter's reverse group (`advance: childBefore`). `false` walks
    /// front-to-back, the only order `RenderShrinkWrappingViewport` ever uses.
    reversed: bool,
}

/// Iterates `[start, end)` either front-to-back or back-to-front, so
/// [`RenderViewport::layout_child_sequence`] can share one loop body for
/// both the forward and reverse groups (Flutter's `advance: childAfter` /
/// `advance: childBefore`).
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
struct StagedPosition {
    slot: usize,
    layout_offset: f32,
    growth_direction: GrowthDirection,
    paint_extent: f32,
    /// How far the leading edge of this child's paint clip is pushed in by
    /// the slivers ahead of it — the room a pinned header occupies. `0.0`
    /// when nothing overlaps it. Staged with the position because it belongs
    /// to the same accepted pass: a clip taken from a rejected pass would
    /// describe a layout the tree never adopted.
    paint_clip_correction: f32,
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

/// Pushes `rect`'s leading edge in by `amount`, along `direction`.
///
/// The moved edge stops at the opposite one. An overlap wider than the
/// viewport would otherwise invert the rect, and `Rect` does not normalize:
/// an inverted clip is a value every consumer has to reason about separately,
/// where a collapsed one already means "nothing survives" everywhere.
fn shrink_leading_edge(rect: Rect<Pixels>, direction: AxisDirection, amount: f32) -> Rect<Pixels> {
    let (mut left, mut top) = (rect.min.x.get(), rect.min.y.get());
    let (mut right, mut bottom) = (rect.max.x.get(), rect.max.y.get());
    match direction {
        AxisDirection::TopToBottom => top = (top + amount).min(bottom),
        AxisDirection::BottomToTop => bottom = (bottom - amount).max(top),
        AxisDirection::LeftToRight => left = (left + amount).min(right),
        AxisDirection::RightToLeft => right = (right - amount).max(left),
    }
    Rect::from_ltrb(px(left), px(top), px(right), px(bottom))
}

/// Grows `rect` by `amount` at BOTH ends of `axis`.
fn grow_along_axis(rect: Rect<Pixels>, axis: Axis, amount: f32) -> Rect<Pixels> {
    let (left, top) = (rect.min.x.get(), rect.min.y.get());
    let (right, bottom) = (rect.max.x.get(), rect.max.y.get());
    match axis {
        Axis::Vertical => {
            Rect::from_ltrb(px(left), px(top - amount), px(right), px(bottom + amount))
        }
        Axis::Horizontal => {
            Rect::from_ltrb(px(left - amount), px(top), px(right + amount), px(bottom))
        }
    }
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
    /// The index of the first FORWARD child (Flutter's `center`). Children
    /// before it grow in reverse, walked backwards and laid out first;
    /// `None` (the default) means index `0` — every child grows forward,
    /// matching Flutter's `children.first` default. `Some(n)` is only valid
    /// for `n < child_count`: Flutter's center is always a direct child, so
    /// `set_center` and `attempt_layout` treat `n >= child_count` as
    /// misconfiguration (see `clamp_center`).
    center: Option<usize>,
    /// Where the zero-scroll line sits along the main axis, as a fraction of
    /// `main_axis_extent` from the leading edge (`0.0`..=`1.0`). Flutter's
    /// `RenderViewport.anchor`; `RenderShrinkWrappingViewport` has no
    /// equivalent — it has no center to anchor.
    anchor: f32,
    /// Set once `clamp_center` has warned about an out-of-range `center`, so
    /// a misconfigured viewport does not spam a warning every frame.
    invalid_center_warned: bool,
    /// Latches the out-of-range `anchor` warning, cleared by a usable value.
    invalid_anchor_warned: bool,
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
    /// How content that overflows this viewport is clipped when it paints.
    /// `Clip::None` clips nothing at all, so a child may paint outside the
    /// viewport's bounds (Flutter's `clipBehavior`, default `hardEdge`).
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

/// What the accepted pass left for [`RenderViewport::describe_semantics_clip`]
/// and its paint-clip counterpart to answer from.
#[derive(Debug, Default, Clone)]
struct CommittedClipGeometry {
    /// The cache extent in pixels, already resolved from
    /// [`CacheExtentStyle`].
    cache_extent: f32,
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
    correction: f32,
    /// Which end of the axis that edge is measured from.
    growth_direction: GrowthDirection,
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
            center: None,
            anchor: 0.0,
            invalid_center_warned: false,
            invalid_anchor_warned: false,
            child_count: 0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            sliver_obstruction_extents: Vec::new(),
            has_visual_overflow: false,
            render_invalidation_handle: None,
            offset_listener: None,
            staged_positions: Vec::new(),
            clip_behavior: Clip::HardEdge,
            committed_clips: CommittedClipGeometry::default(),
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

    /// Sets how overflowing content is clipped when this viewport paints.
    ///
    /// `Clip::None` clips nothing, so a child may paint outside the
    /// viewport's bounds; every other behaviour clips to them, and only when
    /// the content actually overflows.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// How overflowing content is clipped when this viewport paints.
    #[must_use]
    pub const fn clip_behavior(&self) -> Clip {
        self.clip_behavior
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

    /// Sets the index of the first forward child (Flutter's `center`).
    ///
    /// `None` (the default) means index `0`: every child grows forward, from
    /// the leading edge. `Some(index)` makes children `[0, index)` grow in
    /// reverse (walked backwards, laid out before the forward group) and
    /// `[index, child_count)` grow forward, starting at `index` itself.
    ///
    /// `index` must be `< child_count` once the viewport has children —
    /// Flutter's center is always a direct child, so `index == child_count`
    /// (this render object's former "no center" spelling) has no meaning
    /// under this model; use `None` for that. An out-of-range `index` is
    /// caught by a `debug_assert!` in `perform_layout` and clamped (with a
    /// one-time warning) in release.
    #[inline]
    pub fn set_center(&mut self, index: Option<usize>) -> flui_rendering::RenderUpdateImpact {
        if self.center == index {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.center = index;
        // A new value gets its own chance to warn: the latch suppresses one
        // value's warning every frame, not every future mistake.
        self.invalid_center_warned = false;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Returns the configured first-forward-child index, if any.
    #[inline]
    #[must_use]
    pub fn center(&self) -> Option<usize> {
        self.center
    }

    /// Sets where the zero-scroll line sits along the main axis, as a
    /// fraction of `main_axis_extent` from the leading edge.
    ///
    /// Flutter's `RenderViewport.anchor`, default `0.0` (the leading edge — no
    /// room for a reverse group at rest).
    ///
    /// A value outside `0.0..=1.0`, or a non-finite one, is caller input, not
    /// an internal invariant: it is clamped (a non-finite value to `0.0`) and
    /// warned about once, never asserted. Flutter asserts here; this library
    /// does not panic on a configuration gap — the same rule `RenderTable`
    /// follows for a baseline alignment with no text baseline. Letting `NaN`
    /// through would poison every offset the layout derives from it, and a
    /// viewport that renders nothing is a worse answer than one anchored at
    /// its leading edge.
    #[inline]
    pub fn set_anchor(&mut self, anchor: f32) -> flui_rendering::RenderUpdateImpact {
        let usable = if anchor.is_finite() {
            anchor.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if usable == anchor {
            // A later bad value gets its own warning.
            self.invalid_anchor_warned = false;
        } else if !self.invalid_anchor_warned {
            self.invalid_anchor_warned = true;
            tracing::warn!(
                requested = anchor,
                used = usable,
                "RenderViewport: anchor must be finite and in 0.0..=1.0; using the \
                 clamped value so layout can proceed"
            );
        }
        if self.anchor == usable {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.anchor = usable;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Returns the configured anchor fraction.
    #[inline]
    #[must_use]
    pub const fn anchor(&self) -> f32 {
        self.anchor
    }

    /// Resolves `center` against `child_count`, clamping (and warning once)
    /// an out-of-range index — Flutter's center is always a direct child, so
    /// `Some(n) >= child_count` cannot be honored. `None` resolves to `0`.
    fn clamp_center(&mut self, child_count: usize) -> usize {
        let Some(index) = self.center else {
            return 0;
        };
        debug_assert!(
            index < child_count,
            "BUG: RenderViewport::center ({index}) must be < child_count ({child_count}); \
             Flutter's center is always a direct child"
        );
        if index < child_count {
            return index;
        }
        if !self.invalid_center_warned {
            self.invalid_center_warned = true;
            tracing::warn!(
                center = index,
                child_count,
                "RenderViewport: center is out of range (must be < child_count); \
                 clamping to the last child so layout can proceed"
            );
        }
        child_count.saturating_sub(1)
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

    /// Total obstruction extent contributed by the slivers between `center`
    /// and `child_index`, in growth-direction order.
    ///
    /// Mirrors Flutter's `maxScrollObstructionExtentBefore`
    /// (`rendering/viewport.dart:1905`): a FORWARD child (`child_index >=
    /// center`) sums indices `[center, child_index)`; a REVERSE child
    /// (`child_index < center`) sums indices `(child_index, center)` — the
    /// slivers *closer to center* than it, which for a reverse-growth child
    /// are the ones at higher indices. `sliver_obstruction_extents` is
    /// slot-indexed (written by absolute child index, not layout-walk
    /// order — see `update_out_of_band_data`), so this reads correctly
    /// regardless of which group `layout_child_sequence` visited first.
    #[inline]
    #[must_use]
    pub fn max_scroll_obstruction_extent_before(&self, child_index: usize) -> Option<f32> {
        let len = self.sliver_obstruction_extents.len();
        if child_index >= len {
            return None;
        }

        let center = self.center.unwrap_or(0).min(len.saturating_sub(1));
        Some(if child_index >= center {
            self.sliver_obstruction_extents[center..child_index]
                .iter()
                .sum()
        } else {
            self.sliver_obstruction_extents[child_index + 1..center]
                .iter()
                .sum()
        })
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
        self.has_visual_overflow = false;

        let child_count = ctx.child_count();
        // Slot-indexed (not layout-walk order): `update_out_of_band_data`
        // writes `sliver_obstruction_extents[index]` directly, so
        // `max_scroll_obstruction_extent_before` reads correctly regardless
        // of which group — forward or reverse — this pass visits first.
        self.sliver_obstruction_extents.clear();
        self.sliver_obstruction_extents.resize(child_count, 0.0);

        let center = self.clamp_center(child_count);

        // Oracle (`rendering/viewport.dart:1767-1846`, `_attemptLayout`,
        // ported line for line): `center_offset` is the distance from the
        // viewport's leading edge to the zero-scroll line — the anchor
        // point, shifted by the current scroll position.
        let cache_extent = self.calculated_cache_extent(main_axis_extent);
        let center_offset = main_axis_extent * self.anchor - corrected_offset;
        let reverse_remaining_paint_extent = center_offset.clamp(0.0, main_axis_extent);
        let forward_remaining_paint_extent =
            (main_axis_extent - center_offset).clamp(0.0, main_axis_extent);

        let full_cache_extent = main_axis_extent + 2.0 * cache_extent;
        let center_cache_offset = center_offset + cache_extent;
        let reverse_remaining_cache_extent = center_cache_offset.clamp(0.0, full_cache_extent);
        let forward_remaining_cache_extent =
            (full_cache_extent - center_cache_offset).clamp(0.0, full_cache_extent);

        // `leadingNegativeChild == null` in the oracle: no children precede
        // `center`, so there is no reverse group at all.
        let has_reverse_group = center > 0;

        if has_reverse_group {
            // Oracle (`rendering/viewport.dart:1808-1828`): the reverse group
            // — the prefix before `center` — is laid out FIRST, walking
            // backwards from `center - 1` to `0`. A non-zero correction is
            // returned NEGATED: a scroll correction is always expressed in
            // the forward (caller's) coordinate system.
            let result = self.layout_child_sequence(
                ctx,
                LayoutChildSequenceParams {
                    scroll_offset: center_offset.max(main_axis_extent) - main_axis_extent,
                    overlap: 0.0,
                    layout_offset: forward_remaining_paint_extent,
                    remaining_paint_extent: reverse_remaining_paint_extent,
                    main_axis_extent,
                    cross_axis_extent,
                    growth_direction: GrowthDirection::Reverse,
                    remaining_cache_extent: reverse_remaining_cache_extent,
                    cache_origin: (main_axis_extent - center_offset).clamp(-cache_extent, 0.0),
                    child_start: 0,
                    child_end: center,
                    reversed: true,
                },
            );
            if result != 0.0 {
                return -result;
            }
        }

        // Oracle (`rendering/viewport.dart:1830-1845`): the forward group
        // starts AT `center` and always runs, even when the reverse group is
        // empty — `overlap` folds in the leading-edge overscroll only when
        // there is no reverse group ahead of it to have already claimed it.
        self.layout_child_sequence(
            ctx,
            LayoutChildSequenceParams {
                scroll_offset: (-center_offset).max(0.0),
                overlap: if has_reverse_group {
                    0.0
                } else {
                    (-center_offset).min(0.0)
                },
                layout_offset: if center_offset >= main_axis_extent {
                    center_offset
                } else {
                    reverse_remaining_paint_extent
                },
                remaining_paint_extent: forward_remaining_paint_extent,
                main_axis_extent,
                cross_axis_extent,
                growth_direction: GrowthDirection::Forward,
                remaining_cache_extent: forward_remaining_cache_extent,
                cache_origin: center_offset.clamp(-cache_extent, 0.0),
                child_start: center,
                child_end: child_count,
                reversed: false,
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
            // Oracle (`rendering/viewport.dart:902-934`,
            // `describeApproximatePaintClip`): a child nothing overlaps is
            // clipped to the whole viewport; one a pinned header overlaps has
            // its clip's leading edge pushed in to where that overlap starts.
            // Computed here, where the child's own constraints are still in
            // hand, and staged so only an accepted pass commits it.
            let child_overlap = max_paint_offset - layout_offset;
            let paint_clip_correction = if child_overlap == 0.0 || !main_axis_extent.is_finite() {
                0.0
            } else {
                (main_axis_extent - child_remaining_paint_extent) + child_overlap
            };
            self.staged_positions.push(StagedPosition {
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

            self.update_out_of_band_data(growth_direction, index, &geometry);
        }

        0.0
    }

    fn update_out_of_band_data(
        &mut self,
        growth_direction: GrowthDirection,
        index: usize,
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
        // Slot-indexed (see `max_scroll_obstruction_extent_before`): each
        // child is visited exactly once per accepted pass, so writing by its
        // absolute index is equivalent to the old push-in-visit-order when
        // that order was ascending, and correct when it isn't.
        if let Some(slot) = self.sliver_obstruction_extents.get_mut(index) {
            *slot = geometry.max_scroll_obstruction_extent;
        }
        if geometry.has_visual_overflow {
            self.has_visual_overflow = true;
        }
    }

    /// Position every child the accepted pass staged, against the final size.
    fn commit_positions(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
        size: Size,
        cache_extent: f32,
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
        let mut staged = std::mem::take(&mut self.staged_positions);
        // Slot-indexed, like `sliver_obstruction_extents`: the walk may visit
        // the reverse group first, so write by absolute slot rather than in
        // visit order.
        self.committed_clips
            .child_clips
            .resize(self.child_count, CommittedChildClip::default());
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
        properties.add_double("anchor", self.anchor, None);
        if let Some(center) = self.center {
            properties.add_int("center", center as i64, None);
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

            // Oracle (`rendering/viewport.dart:1732-1735`): the anchor shifts
            // both ends of the published scroll range by the room it opens
            // on each side of the zero-scroll line — an anchor > 0 lets the
            // reverse group scroll `main_axis_extent * anchor` further
            // before `min_scroll_extent` clamps, and symmetrically shrinks
            // how far the forward group can scroll before `max_scroll_extent`
            // clamps.
            if self.offset.apply_content_dimensions(
                (self.min_scroll_extent + main_axis_extent * self.anchor).min(0.0),
                (self.max_scroll_extent - main_axis_extent * (1.0 - self.anchor)).max(0.0),
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

        self.commit_positions(ctx, size, self.calculated_cache_extent(main_axis_extent));
        size
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_children = |ctx: &mut PaintCx<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let clip_rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(clip_rect, self.clip_behavior, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    /// Oracle (`rendering/viewport.dart:886-934`,
    /// `describeApproximatePaintClip`): the viewport's own bounds, with the
    /// leading edge pushed in by whatever overlaps this child — the room a
    /// pinned header takes. `Clip::None` clips nothing, so it reports
    /// nothing and content outside the viewport stays a fully present
    /// accessibility node.
    ///
    /// The direction the correction pushes from follows the child's growth
    /// direction: a reverse-group child is measured from the far edge.
    fn describe_approximate_paint_clip(
        &self,
        child_slot: usize,
        size: Size,
    ) -> Option<Rect<Pixels>> {
        if self.clip_behavior == Clip::None {
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

    /// Oracle (`rendering/viewport.dart:938-966`, `describeSemanticsClip`):
    /// the viewport's bounds grown by the cache extent along the scroll axis.
    ///
    /// Wider than the paint clip on purpose — a row just past the edge is
    /// off-screen but reachable, so it stays in the tree (flagged hidden by
    /// the paint clip) for a screen reader to scroll to. A row past the cache
    /// area is not there at all.
    fn describe_semantics_clip(&self, _child_slot: usize, size: Size) -> Option<Rect<Pixels>> {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        let cache = self.committed_clips.cache_extent;
        if cache <= 0.0 {
            return Some(bounds);
        }
        Some(grow_along_axis(bounds, self.axis_direction.axis(), cache))
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
    /// How content that overflows this viewport is clipped when it paints.
    /// `Clip::None` clips nothing at all, so a child may paint outside the
    /// viewport's bounds (Flutter's `clipBehavior`, default `hardEdge`).
    clip_behavior: Clip,
    /// See [`RenderViewport::committed_clips`]'s matching field docs.
    committed_clips: CommittedClipGeometry,
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
            clip_behavior: Clip::HardEdge,
            committed_clips: CommittedClipGeometry::default(),
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

    /// Sets how overflowing content is clipped when this viewport paints.
    ///
    /// `Clip::None` clips nothing, so a child may paint outside the
    /// viewport's bounds; every other behaviour clips to them, and only when
    /// the content actually overflows.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// How overflowing content is clipped when this viewport paints.
    #[must_use]
    pub const fn clip_behavior(&self) -> Clip {
        self.clip_behavior
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
                // `RenderShrinkWrappingViewport` has no `center`/`anchor` in
                // Flutter either — every child grows forward, front-to-back.
                reversed: false,
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
            reversed,
        } = params;
        debug_assert_eq!(growth_direction, GrowthDirection::Forward);
        debug_assert!(
            !reversed,
            "BUG: RenderShrinkWrappingViewport has no center/anchor; every walk is forward"
        );
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
            // Oracle (`rendering/viewport.dart:902-934`,
            // `describeApproximatePaintClip`): a child nothing overlaps is
            // clipped to the whole viewport; one a pinned header overlaps has
            // its clip's leading edge pushed in to where that overlap starts.
            // Computed here, where the child's own constraints are still in
            // hand, and staged so only an accepted pass commits it.
            let child_overlap = max_paint_offset - layout_offset;
            let paint_clip_correction = if child_overlap == 0.0 || !main_axis_extent.is_finite() {
                0.0
            } else {
                (main_axis_extent - child_remaining_paint_extent) + child_overlap
            };
            self.staged_positions.push(StagedPosition {
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
        cache_extent: f32,
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
        let mut staged = std::mem::take(&mut self.staged_positions);
        // Slot-indexed, like `sliver_obstruction_extents`: the walk may visit
        // the reverse group first, so write by absolute slot rather than in
        // visit order.
        self.committed_clips
            .child_clips
            .resize(self.child_count, CommittedChildClip::default());
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
        self.commit_positions(ctx, size, self.calculated_cache_extent(main_axis_extent));
        size
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_children = |ctx: &mut PaintCx<'_, Variable>| match self.paint_order {
            SliverPaintOrder::FirstIsTop => ctx.paint_children_reverse(),
            SliverPaintOrder::LastIsTop => ctx.paint_children(),
        };

        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let clip_rect = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(clip_rect, self.clip_behavior, paint_children);
        } else {
            paint_children(ctx);
        }
    }

    /// Oracle (`rendering/viewport.dart:886-934`,
    /// `describeApproximatePaintClip`): the viewport's own bounds, with the
    /// leading edge pushed in by whatever overlaps this child — the room a
    /// pinned header takes. `Clip::None` clips nothing, so it reports
    /// nothing and content outside the viewport stays a fully present
    /// accessibility node.
    ///
    /// The direction the correction pushes from follows the child's growth
    /// direction: a reverse-group child is measured from the far edge.
    fn describe_approximate_paint_clip(
        &self,
        child_slot: usize,
        size: Size,
    ) -> Option<Rect<Pixels>> {
        if self.clip_behavior == Clip::None {
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

    /// Oracle (`rendering/viewport.dart:938-966`, `describeSemanticsClip`):
    /// the viewport's bounds grown by the cache extent along the scroll axis.
    ///
    /// Wider than the paint clip on purpose — a row just past the edge is
    /// off-screen but reachable, so it stays in the tree (flagged hidden by
    /// the paint clip) for a screen reader to scroll to. A row past the cache
    /// area is not there at all.
    fn describe_semantics_clip(&self, _child_slot: usize, size: Size) -> Option<Rect<Pixels>> {
        let bounds = Rect::from_origin_size(Point::ZERO, size);
        let cache = self.committed_clips.cache_extent;
        if cache <= 0.0 {
            return Some(bounds);
        }
        Some(grow_along_axis(bounds, self.axis_direction.axis(), cache))
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

#[cfg(test)]
mod paint_clip_direction_tests {
    use super::*;

    /// The box these unit tests treat the viewport as occupying. The hook takes
    /// the size from its caller now, so the fixture states it once here rather
    /// than committing a copy.
    const TEST_SIZE: Size = Size::new(px(100.0), px(100.0));

    fn viewport_with_committed_child(
        axis_direction: AxisDirection,
        growth_direction: GrowthDirection,
    ) -> RenderViewport {
        let mut viewport = RenderViewport::new(axis_direction);
        viewport.committed_clips = CommittedClipGeometry {
            cache_extent: 0.0,
            child_clips: vec![CommittedChildClip {
                correction: 30.0,
                growth_direction,
            }],
        };
        viewport
    }

    /// The direction a paint clip's edge is pushed from follows the CHILD's
    /// growth direction, and that direction has to survive the commit.
    ///
    /// It is read from committed state rather than from the staged positions
    /// the pass built it from, because `commit_positions` drains those: a
    /// lookup there finds an empty vector and answers "forward" for every
    /// child. A forward-only viewport — nearly every viewport — cannot tell
    /// the difference, which is why this is pinned directly.
    #[test]
    fn a_reverse_group_child_has_its_paint_clip_pushed_from_the_far_edge() {
        let forward =
            viewport_with_committed_child(AxisDirection::TopToBottom, GrowthDirection::Forward);
        let clip = forward
            .describe_approximate_paint_clip(0, TEST_SIZE)
            .expect("a clipping viewport reports a paint clip");
        assert_eq!(
            (clip.min.y.get(), clip.max.y.get()),
            (30.0, 100.0),
            "a forward child's clip loses its LEADING edge to the overlap",
        );

        let reverse =
            viewport_with_committed_child(AxisDirection::TopToBottom, GrowthDirection::Reverse);
        let clip = reverse
            .describe_approximate_paint_clip(0, TEST_SIZE)
            .expect("a clipping viewport reports a paint clip");
        assert_eq!(
            (clip.min.y.get(), clip.max.y.get()),
            (0.0, 70.0),
            "a reverse child grows from the far edge, so its clip loses THAT one",
        );
    }

    /// An overlap wider than the viewport collapses the clip instead of
    /// inverting it.
    ///
    /// `Rect::from_ltrb` does not normalize, so pushing the leading edge past
    /// the trailing one would hand every consumer a rect whose min exceeds its
    /// max — a second empty-ish state to reason about beside the collapsed
    /// one. A pinned header taller than the viewport it is pinned in is the
    /// case that produces it.
    #[test]
    fn an_overlap_wider_than_the_viewport_collapses_the_clip_rather_than_inverting_it() {
        let mut viewport =
            viewport_with_committed_child(AxisDirection::TopToBottom, GrowthDirection::Forward);
        viewport.committed_clips.child_clips[0].correction = 400.0;

        let clip = viewport
            .describe_approximate_paint_clip(0, TEST_SIZE)
            .expect("a clipping viewport reports a paint clip");

        assert_eq!(
            (clip.min.y.get(), clip.max.y.get()),
            (100.0, 100.0),
            "the pushed edge stops at the opposite one, leaving an empty clip",
        );
        assert!(clip.is_empty(), "and an empty clip is what keeps nothing");
    }

    /// `Clip::None` means no clip at all, not a clip the size of the viewport:
    /// a child painting outside the bounds keeps its full accessibility rect.
    #[test]
    fn clip_none_reports_no_paint_clip() {
        let mut viewport =
            viewport_with_committed_child(AxisDirection::TopToBottom, GrowthDirection::Forward);
        let _ = viewport.set_clip_behavior(Clip::None);

        assert!(
            viewport
                .describe_approximate_paint_clip(0, TEST_SIZE)
                .is_none(),
            "an unclipped viewport imposes nothing on its children",
        );
    }
}
