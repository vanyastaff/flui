//! `RenderViewport` — the viewport that fills its incoming main-axis extent
//! and lays its slivers out around a `center` child.
//!
//! `center` names the first FORWARD child, the prefix before it is the
//! reverse group (walked backwards, laid out first), and `anchor` places the
//! zero-scroll line at `main_axis_extent * anchor` from the leading edge.
//! `showOnScreen` and lazy child creation stay out of this file.

use flui_foundation::Variable;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Rect, Size};
use flui_foundation::inspect::{Fields, Inspect};
use flui_painting::paint::Clip;
use flui_rendering::{
    RenderResult, RenderUpdateImpact,
    constraints::{AxisDirection, AxisDirection::TopToBottom, GrowthDirection, SliverGeometry},
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

/// A Box-protocol viewport that lays out Sliver-protocol children.
#[derive(Debug)]
pub struct RenderViewport<O = ScrollPosition> {
    core: ViewportCore<O, ViewportLayout>,
    /// The index of the first FORWARD child (the `center`). Children
    /// before it grow in reverse, walked backwards and laid out first;
    /// `None` (the default) means index `0` — every child grows forward.
    /// `Some(n)` is only valid for `n < child_count`: the center is always a
    /// direct child, so `set_center` and `attempt_layout` treat `n >= child_count` as
    /// misconfiguration (see `clamp_center`).
    center: Option<usize>,
    /// Where the zero-scroll line sits along the main axis, as a fraction of
    /// `main_axis_extent` from the leading edge (`0.0`..=`1.0`).
    /// `RenderShrinkWrappingViewport` has no equivalent — it has no center to
    /// anchor.
    anchor: f64,
    /// Set once `clamp_center` has warned about an out-of-range `center`, so
    /// a misconfigured viewport does not spam a warning every frame.
    invalid_center_warned: bool,
    /// Latches the out-of-range `anchor` warning, cleared by a usable value.
    invalid_anchor_warned: bool,
}

/// One pass's metrics and positions; see [`PassRecord`].
#[derive(Debug, Default)]
struct ViewportLayout {
    child_count: usize,
    min_scroll_extent: f64,
    max_scroll_extent: f64,
    max_scroll_obstruction_extent: f64,
    sliver_obstruction_extents: Vec<f64>,
    has_visual_overflow: bool,
    staged_positions: Vec<StagedPosition>,
}

impl PassRecord for ViewportLayout {
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
        slot: usize,
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
        // Slot-indexed (see `max_scroll_obstruction_extent_before`): each
        // child is visited exactly once per accepted pass, so writing by its
        // absolute index is correct whichever group the walk visits first.
        if let Some(extent) = self.sliver_obstruction_extents.get_mut(slot) {
            *extent = geometry.max_scroll_obstruction_extent;
        }
        if geometry.has_visual_overflow {
            self.has_visual_overflow = true;
        }
    }
}

impl RenderViewport<ScrollPosition> {
    /// Creates a viewport with a fixed offset at zero.
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
            core: ViewportCore::new(axis_direction, cross_axis_direction, offset),
            center: None,
            anchor: 0.0,
            invalid_center_warned: false,
            invalid_anchor_warned: false,
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

    /// Replaces the viewport offset object wholesale.
    ///
    /// For a widget that injects an external offset (e.g. a shared
    /// `ScrollPosition`), reconciliation compares the new offset's identity
    /// against the current one and calls this only when it actually changed
    /// — swapping in a same-identity offset would discard layout-committed
    /// extents (`min_scroll_extent`/`max_scroll_extent`/`viewport_dimension`)
    /// for no reason.
    ///
    /// If this node is currently attached (a [`LocalRenderInvalidationHandle`] was handed to
    /// [`attach`](RenderBox::attach) and no matching
    /// [`detach`](RenderBox::detach) has run since), the offset-relayout
    /// replacement commits first. The outgoing callback is revoked and removed
    /// by its original identity, then a fresh callback is registered on the new
    /// offset even if cancellation failed. Required layout remains deliverable
    /// before the first failure resumes. An unattached node subscribes at attach.
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
    /// [`set_axis_direction`](Self::set_axis_direction) re-derives.
    ///
    /// A caller that reads an ambient text direction sets this after
    /// [`set_axis_direction`](Self::set_axis_direction), which has already laid
    /// down the LTR default — so an RTL `Directionality` flips a vertical
    /// viewport's cross axis to right-to-left.
    ///
    /// Reports layout when the cross axis actually changed.
    #[inline]
    pub fn set_cross_axis_direction(
        &mut self,
        cross_axis_direction: AxisDirection,
    ) -> RenderUpdateImpact {
        self.core.set_cross_axis_direction(cross_axis_direction)
    }

    /// Sets the index of the first forward child (the `center`).
    ///
    /// `None` (the default) means index `0`: every child grows forward, from
    /// the leading edge. `Some(index)` makes children `[0, index)` grow in
    /// reverse (walked backwards, laid out before the forward group) and
    /// `[index, child_count)` grow forward, starting at `index` itself.
    ///
    /// `index` must be `< child_count` once the viewport has children —
    /// the center is always a direct child, so `index == child_count`
    /// (this render object's former "no center" spelling) has no meaning
    /// under this model; use `None` for that. An out-of-range `index` is
    /// caught by a `debug_assert!` in `perform_layout` and clamped (with a
    /// one-time warning) in release.
    #[inline]
    pub fn set_center(&mut self, index: Option<usize>) -> RenderUpdateImpact {
        if self.center == index {
            return RenderUpdateImpact::NONE;
        }
        self.center = index;
        // A new value gets its own chance to warn: the latch suppresses one
        // value's warning every frame, not every future mistake.
        self.invalid_center_warned = false;
        RenderUpdateImpact::LAYOUT
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
    /// Default `0.0` (the leading edge — no room for a reverse group at rest).
    ///
    /// A value outside `0.0..=1.0`, or a non-finite one, is caller input, not
    /// an internal invariant: it is clamped (a non-finite value to `0.0`) and
    /// warned about once, never asserted: this library
    /// does not panic on a configuration gap — the same rule `RenderTable`
    /// follows for a baseline alignment with no text baseline. Letting `NaN`
    /// through would poison every offset the layout derives from it, and a
    /// viewport that renders nothing is a worse answer than one anchored at
    /// its leading edge.
    #[inline]
    pub fn set_anchor(&mut self, anchor: f64) -> RenderUpdateImpact {
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
            return RenderUpdateImpact::NONE;
        }
        self.anchor = usable;
        RenderUpdateImpact::LAYOUT
    }

    /// Returns the configured anchor fraction.
    #[inline]
    #[must_use]
    pub const fn anchor(&self) -> f64 {
        self.anchor
    }

    /// Resolves `center` against `child_count`, clamping (and warning once)
    /// an out-of-range index — the center is always a direct child, so
    /// `Some(n) >= child_count` cannot be honored. `None` resolves to `0`.
    fn clamp_center(&mut self, child_count: usize) -> usize {
        let Some(index) = self.center else {
            return 0;
        };
        debug_assert!(
            index < child_count,
            "BUG: RenderViewport::center ({index}) must be < child_count ({child_count}); \
             the center is always a direct child"
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
    pub const fn max_scroll_extent(&self) -> f64 {
        self.core.layout.max_scroll_extent
    }

    /// Last total reverse scroll extent reported by the reverse sliver sequence.
    #[inline]
    #[must_use]
    pub const fn min_scroll_extent(&self) -> f64 {
        self.core.layout.min_scroll_extent
    }

    /// Last total pinned obstruction extent reported by the sliver sequence.
    #[inline]
    #[must_use]
    pub const fn max_scroll_obstruction_extent(&self) -> f64 {
        self.core.layout.max_scroll_obstruction_extent
    }

    /// Total obstruction extent contributed by the slivers between `center`
    /// and `child_index`, in growth-direction order.
    ///
    /// A FORWARD child (`child_index >=
    /// center`) sums indices `[center, child_index)`; a REVERSE child
    /// (`child_index < center`) sums indices `(child_index, center)` — the
    /// slivers *closer to center* than it, which for a reverse-growth child
    /// are the ones at higher indices. `sliver_obstruction_extents` is
    /// slot-indexed (written by absolute child index, not layout-walk
    /// order — see `ViewportLayout::record_child`), so this reads correctly
    /// regardless of which group `layout_child_sequence` visited first.
    #[inline]
    #[must_use]
    pub fn max_scroll_obstruction_extent_before(&self, child_index: usize) -> Option<f64> {
        let extents = &self.core.layout.sliver_obstruction_extents;
        let len = extents.len();
        if child_index >= len {
            return None;
        }

        let center = self.center.unwrap_or(0).min(len.saturating_sub(1));
        Some(if child_index >= center {
            extents[center..child_index].iter().sum()
        } else {
            extents[child_index + 1..center].iter().sum()
        })
    }

    /// Whether the last layout pass reported visual overflow.
    #[inline]
    #[must_use]
    pub const fn has_visual_overflow(&self) -> bool {
        self.core.layout.has_visual_overflow
    }

    fn main_axis_extent(&self, size: Size) -> f64 {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        }
    }

    fn cross_axis_extent(&self, size: Size) -> f64 {
        match self.core.axis_direction.axis() {
            Axis::Horizontal => size.height,
            Axis::Vertical => size.width,
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
        layout.min_scroll_extent = 0.0;
        layout.max_scroll_extent = 0.0;
        layout.max_scroll_obstruction_extent = 0.0;
        layout.has_visual_overflow = false;

        let child_count = ctx.child_count();
        // Slot-indexed (not layout-walk order): `record_child` writes
        // `sliver_obstruction_extents[index]` directly, so
        // `max_scroll_obstruction_extent_before` reads correctly regardless
        // of which group — forward or reverse — this pass visits first.
        layout.sliver_obstruction_extents.clear();
        layout.sliver_obstruction_extents.resize(child_count, 0.0);

        let center = self.clamp_center(child_count);

        // `center_offset` is the distance from the
        // viewport's leading edge to the zero-scroll line — the anchor
        // point, shifted by the current scroll position.
        let cache_extent = self.core.calculated_cache_extent(main_axis_extent);
        let center_offset = main_axis_extent * self.anchor - corrected_offset;
        let reverse_remaining_paint_extent = center_offset.clamp(0.0, main_axis_extent);
        let forward_remaining_paint_extent =
            (main_axis_extent - center_offset).clamp(0.0, main_axis_extent);

        let full_cache_extent = main_axis_extent + 2.0 * cache_extent;
        let center_cache_offset = center_offset + cache_extent;
        let reverse_remaining_cache_extent = center_cache_offset.clamp(0.0, full_cache_extent);
        let forward_remaining_cache_extent =
            (full_cache_extent - center_cache_offset).clamp(0.0, full_cache_extent);

        // No children precede `center`, so there is no reverse group at all.
        let has_reverse_group = center > 0;

        if has_reverse_group {
            // The reverse group
            // — the prefix before `center` — is laid out FIRST, walking
            // backwards from `center - 1` to `0`. A non-zero correction is
            // returned NEGATED: a scroll correction is always expressed in
            // the forward (caller's) coordinate system.
            let result = self.core.layout_child_sequence(
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
            )?;
            if result != 0.0 {
                return Ok(-result);
            }
        }

        // The forward group starts AT `center` and always runs, even when the reverse group is
        // empty — `overlap` folds in the leading-edge overscroll only when
        // there is no reverse group ahead of it to have already claimed it.
        self.core.layout_child_sequence(
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

    /// One `perform_layout` pass into the record [`ViewportCore::begin_pass`]
    /// handed out.
    fn layout_pass(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>,
    ) -> RenderResult<Size> {
        let size = ctx.constraints().biggest();
        let main_axis_extent = self.main_axis_extent(size);
        let cross_axis_extent = self.cross_axis_extent(size);
        self.core.layout.child_count = ctx.child_count();
        // Native constraints are accepted independently of descendant measurement.
        // Their fractional-page mapping survives a rejected content proposal.
        let pixels_before_pass = self.core.offset.pixels();
        let _ = self.core.offset.apply_viewport_dimension(main_axis_extent);
        let mut layout_offset = self.core.offset.begin_layout();

        if ctx.child_count() == 0 {
            let layout = &mut self.core.layout;
            layout.min_scroll_extent = 0.0;
            layout.max_scroll_extent = 0.0;
            layout.max_scroll_obstruction_extent = 0.0;
            layout.sliver_obstruction_extents.clear();
            layout.has_visual_overflow = false;
            let _ = layout_offset.apply_content_dimensions(0.0, 0.0);
            self.core.offset.accept_layout(layout_offset)?;
            return Ok(size);
        }

        let max_layout_cycles = MAX_LAYOUT_CYCLES_PER_CHILD * ctx.child_count();
        let mut accepted = false;
        let mut degraded = false;
        for _ in 0..max_layout_cycles {
            let correction = self.attempt_layout(
                ctx,
                main_axis_extent,
                cross_axis_extent,
                layout_offset.pixels(),
            )?;
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
                layout_offset.correct_by(correction);
                continue;
            }

            // The anchor shifts both ends of the published scroll range by the room it opens
            // on each side of the zero-scroll line — an anchor > 0 lets the
            // reverse group scroll `main_axis_extent * anchor` further
            // before `min_scroll_extent` clamps, and symmetrically shrinks
            // how far the forward group can scroll before `max_scroll_extent`
            // clamps.
            if layout_offset.apply_content_dimensions(
                (self.core.layout.min_scroll_extent + main_axis_extent * self.anchor).min(0.0),
                (self.core.layout.max_scroll_extent - main_axis_extent * (1.0 - self.anchor))
                    .max(0.0),
            ) {
                accepted = true;
                break;
            }
        }
        if degraded {
            // Undo any offset movement `apply_viewport_dimension` made above.
            let drift = pixels_before_pass - self.core.offset.pixels();
            if drift != 0.0 {
                self.core.offset.correct_by(drift);
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

        if !degraded {
            self.core.offset.accept_layout(layout_offset)?;
        }
        let cache_extent = self.core.calculated_cache_extent(main_axis_extent);
        self.core.commit_positions(ctx, size, cache_extent);
        Ok(size)
    }
}

impl Default for RenderViewport<ScrollPosition> {
    fn default() -> Self {
        Self::new(TopToBottom)
    }
}

impl<O: ViewportOffset + 'static> Inspect for RenderViewport<O> {
    fn kind(&self) -> &'static str {
        "RenderViewport"
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        args(std::any::type_name::<O>());
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        self.core.inspect(out);
        out.field("anchor", &self.anchor);
        out.field("center", &self.center);
    }
}

impl<O: ViewportOffset + 'static> RenderBox for RenderViewport<O> {
    type Arity = Variable;
    type ParentData = BoxParentData;

    // The viewport subscribes to its `ViewportOffset` in `attach` and tears
    // the subscription down in `detach`; `ViewportCore::attach` says what
    // fires the mark and why it can never re-enter `perform_layout`.
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
