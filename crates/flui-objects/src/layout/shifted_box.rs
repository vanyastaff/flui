//! Base component for aligned single-child box render objects.
//!
//! [`AligningShiftedBox`] owns the child-positioning concern: given an
//! [`Alignment`] and the parent/child sizes, it computes and stores the child
//! offset, positions the child through the layout context, and handles
//! hit-testing at that offset.
//!
//! It is intentionally **factor-free**: width/height scaling factors belong to
//! the wrapping object (`RenderAlign`, `RenderCenter`), not here.  This keeps
//! the base reusable for factor-less objects such as `RotatedBox` (Phase 4).
//!
//! # Composition pattern
//!
//! A `RenderBox` impl stores an `AligningShiftedBox` and forwards the relevant
//! calls:
//!
//! ```ignore
//! fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
//!     let child_size = ctx.layout_single_child_loose();
//!     let parent_size = positioned_box_size(&constraints, child_size, wf, hf);
//!     self.inner.align_child(ctx, parent_size, child_size);
//!     parent_size
//! }
//! fn compute_dry_baseline(...) -> Option<f32> {
//!     // ...
//!     let dy = self.inner.dry_child_offset(parent_size, child_size).dy.get();
//!     Some(child_baseline + dy)
//! }
//! ```

use flui_tree::Single;
use flui_types::{Alignment, Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::TextBaseline,
};

/// Component that aligns a single child within free space.
///
/// Stores the alignment, the last-computed child offset, and a `has_child`
/// flag.  Wrapping objects store one instance alongside their own sizing state
/// and call through to [`align_child`] / [`hit_test`].
///
/// The component also caches both baseline kinds (alphabetic/ideographic) from
/// the most recent layout so that [`actual_baseline`] can forward the live
/// baseline upward — mirroring Flutter
/// `RenderShiftedBox.computeDistanceToActualBaseline` which returns
/// `child.getDistanceToActualBaseline(baseline) + childParentData.offset.dy`.
///
/// [`align_child`]: AligningShiftedBox::align_child
/// [`hit_test`]: AligningShiftedBox::hit_test
/// [`actual_baseline`]: AligningShiftedBox::actual_baseline
#[derive(Debug, Clone)]
pub(crate) struct AligningShiftedBox {
    alignment: Alignment,
    /// Child's top-left offset within the parent, set during `align_child`.
    child_offset: Offset,
    /// True after the first successful `align_child`; guards hit-testing.
    has_child: bool,
    /// Child's live actual baseline per kind, cached during `record_child_baselines`.
    /// Index 0 = `TextBaseline::Alphabetic`, index 1 = `TextBaseline::Ideographic`.
    /// `None` when no child is present or the child reports no baseline for that kind.
    child_baselines: [Option<f32>; 2],
}

impl AligningShiftedBox {
    /// Creates a new component with the given alignment.
    ///
    /// `has_child` starts `false`; it is set to `true` by the first call to
    /// [`align_child`].
    ///
    /// [`align_child`]: AligningShiftedBox::align_child
    #[inline]
    pub(crate) fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            child_offset: Offset::ZERO,
            has_child: false,
            child_baselines: [None; 2],
        }
    }

    /// Returns the current alignment.
    #[inline]
    pub(crate) fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Updates the alignment; returns `true` if the value changed.
    ///
    /// Mirrors Flutter `RenderAligningShiftedBox`'s `alignment` setter
    /// (`shifted_box.dart:339-345`), which triggers a relayout (not just a
    /// repaint) because the child offset depends on it — the caller is
    /// responsible for marking the owning render object dirty on `true`.
    pub(crate) fn set_alignment(&mut self, alignment: Alignment) -> bool {
        if self.alignment == alignment {
            return false;
        }
        self.alignment = alignment;
        true
    }

    /// Returns the child offset set by the most recent [`align_child`] call.
    ///
    /// Returns `Offset::ZERO` before the first layout.
    ///
    /// [`align_child`]: AligningShiftedBox::align_child
    #[inline]
    pub(crate) fn child_offset(&self) -> Offset {
        self.child_offset
    }

    /// Computes the child offset for the given sizes **without mutating state**.
    ///
    /// Used by dry-layout queries (`compute_dry_baseline`) where the dry sizes
    /// differ from the last laid-out sizes and side effects are forbidden.
    ///
    /// Returns `alignment.along_size(parent_size − child_size)`.
    #[inline]
    pub(crate) fn dry_child_offset(&self, parent_size: Size, child_size: Size) -> Offset {
        self.alignment.along_size(parent_size - child_size)
    }

    /// Positions the child within `parent_size` using the stored alignment.
    ///
    /// Computes `child_offset = alignment.along_size(parent_size − child_size)`
    /// then calls `ctx.position_child(0, child_offset)`.
    ///
    /// Must be called after the child has been laid out and `parent_size` is
    /// known.
    pub(crate) fn align_child(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
        parent_size: Size,
        child_size: Size,
    ) {
        self.child_offset = self.dry_child_offset(parent_size, child_size);
        ctx.position_child(0, self.child_offset);
        self.has_child = true;
    }

    /// Caches the child's live actual baseline for both [`TextBaseline`] kinds.
    ///
    /// Must be called **after** [`align_child`] so that `child_offset.dy` is
    /// already set.  The cached values are returned by [`actual_baseline`].
    ///
    /// The baseline kind is not known until the parent queries it, so both
    /// kinds are stored eagerly during layout rather than on demand.
    ///
    /// [`align_child`]: AligningShiftedBox::align_child
    /// [`actual_baseline`]: AligningShiftedBox::actual_baseline
    pub(crate) fn record_child_baselines(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) {
        self.child_baselines = [
            ctx.child_distance_to_actual_baseline(0, TextBaseline::Alphabetic),
            ctx.child_distance_to_actual_baseline(0, TextBaseline::Ideographic),
        ];
    }

    /// Returns the live actual baseline of this box for the given kind.
    ///
    /// Mirrors Flutter `RenderShiftedBox.computeDistanceToActualBaseline`:
    /// returns `child_raw_baseline + child_offset.dy`, or `None` when the
    /// child has no child, or the child reports no baseline for that kind.
    ///
    /// `child_offset.dy` is the stored value from the most recent [`align_child`]
    /// call; the result is valid only after layout.
    ///
    /// [`align_child`]: AligningShiftedBox::align_child
    pub(crate) fn actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        let kind_index = match baseline {
            TextBaseline::Alphabetic => 0,
            TextBaseline::Ideographic => 1,
        };
        self.child_baselines[kind_index]
            .map(|raw_baseline| raw_baseline + self.child_offset.dy.get())
    }

    /// Clears both cached baselines.  Call when the child is removed so that
    /// stale values are never returned.
    #[inline]
    pub(crate) fn clear_child_baselines(&mut self) {
        self.child_baselines = [None; 2];
    }

    /// Hit-tests the child at its laid-out offset.
    ///
    /// Returns `false` immediately if the position lies outside the parent's
    /// own size (mirrors Flutter `RenderShiftedBox.hitTestChildren`
    /// `addWithPaintOffset` guard).  If a child exists, delegates via
    /// [`hit_test_child_at_layout_offset`], which resolves the child's offset
    /// from `RenderState` (committed by [`align_child`]) AND records the paint
    /// offset into the `HitTestResult` so `HitTestResult::dispatch` can localize
    /// pointer/scroll events to the child's coordinate space. This is the
    /// canonical path (`hit_test_child_at_offset` only transforms the recursive
    /// descent and would leave handlers on an aligned child receiving parent
    /// coordinates — Flutter `RenderShiftedBox.hitTestChildren` records the
    /// offset via `addWithPaintOffset`).
    ///
    /// [`hit_test_child_at_layout_offset`]: BoxHitTestContext::hit_test_child_at_layout_offset
    /// [`align_child`]: AligningShiftedBox::align_child
    pub(crate) fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_layout_offset(0)
        } else {
            false
        }
    }
}
