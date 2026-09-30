//! `RenderTheater` — the `Overlay`'s stack, with the first `skip_count`
//! children held offstage.
//!
//! A special version of a `Stack` that doesn't lay out or render the first
//! `skip_count` children; those are considered "offstage".
//!
//! `skip_count` exists to serve `OverlayEntry.opaque`: entries below the topmost
//! opaque one are dropped from the tree unless they set `maintainState`, and the
//! ones kept are the ones skipped here. Because the overlay collects entries
//! top-first and then reverses, the skipped children are always the **leading**
//! ones of the child list.
//!
//! With `skip_count == 0` this is exactly `RenderStack` with `StackFit::Expand`:
//! `size = constraints.biggest` and every child gets `BoxConstraints::tight(size)`.
//!
//! # Design notes
//!
//! * **No positioned children.** `RenderStack` runs the full
//!   positioned/non-positioned split, because an app may put a `Positioned` at
//!   the root of an entry. FLUI's `Overlay` builds one non-positioned
//!   `OverlayEntryView` per entry and nothing else, so every child here is
//!   non-positioned. `StackParentData` is kept as the parent-data type for
//!   compatibility with `RenderStack` tooling; its positioning fields are ignored.
//! * **No `canSizeOverlay` / `alwaysSizeToContent`.** Those only matter under
//!   *unbounded* constraints, and FLUI has no such flag, so an
//!   unbounded theater falls back to `constraints.smallest()`, matching
//!   `RenderStack`'s own no-non-positioned-children fallback rather than panicking
//!   — see [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md).
//! * **Offstage entries publish no semantics**, by two mechanisms.
//!
//!   `visits_child_for_semantics` answers from `skip_count` directly, so
//!   an entry offstage from its very first pass is excluded regardless of
//!   layout history. The
//!   placed-generation stamp independently excludes an entry that was laid out
//!   while visible and then covered, since its stamp goes stale.
//!
//!   The stamp alone was not enough, which is why the visitor exists: it may
//!   only remove a child a parent demonstrably *stopped* laying out (otherwise
//!   a parent laying out through a path of its own would hide its whole
//!   subtree), so a child skipped from pass one was never stamped and read as
//!   placed. An app starting with an opaque entry above another announced a
//!   route the user could neither see nor touch.

use flui_foundation::Variable;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::StackParentData,
    traits::RenderBox,
};

/// A `StackFit::Expand` stack whose first `skip_count` children are offstage:
/// not laid out, not painted, not hit-tested.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderTheater {
    skip_count: usize,
    /// Cached at layout so `hit_test` — which has no `child_count()` — can bound
    /// its reverse walk. Same trick as [`RenderStack`](super::RenderStack).
    child_count: usize,
}

impl RenderTheater {
    /// A theater with nothing skipped, i.e. a plain expanding stack.
    pub const fn new() -> Self {
        Self {
            skip_count: 0,
            child_count: 0,
        }
    }

    /// Builder form of [`set_skip_count`](Self::set_skip_count).
    pub const fn with_skip_count(mut self, skip_count: usize) -> Self {
        self.skip_count = skip_count;
        self
    }

    /// How many leading (bottom-most) children are offstage.
    pub const fn skip_count(&self) -> usize {
        self.skip_count
    }

    /// Updates the skip count and reports layout when changed.
    pub const fn set_skip_count(
        &mut self,
        skip_count: usize,
    ) -> flui_rendering::RenderUpdateImpact {
        let changed = self.skip_count != skip_count;
        self.skip_count = skip_count;
        if changed {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// The index of the first onstage child.
    ///
    /// `skip_count` is expected to be at most the child count; clamping is
    /// total for every input, so a caller error here cannot corrupt the child
    /// walk.
    const fn first_onstage(&self, child_count: usize) -> usize {
        if self.skip_count > child_count {
            child_count
        } else {
            self.skip_count
        }
    }

    /// `size = constraints.biggest` when finite. See the module docs for the
    /// unbounded fallback.
    fn theater_size(constraints: BoxConstraints) -> Size {
        if constraints.biggest().is_finite() {
            constraints.biggest()
        } else {
            constraints.smallest()
        }
    }

    /// The stack's intrinsic dimension over the first onstage child and its
    /// later siblings.
    fn max_onstage_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f64,
        mut query: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f64) -> f64,
    ) -> f64 {
        let child_count = ctx.child_count();
        let mut max = 0.0_f64;
        for i in self.first_onstage(child_count)..child_count {
            max = max.max(query(ctx, i, extent));
        }
        max
    }
}

impl flui_foundation::Diagnosticable for RenderTheater {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("skip_count", self.skip_count.to_string());
    }
}

impl RenderBox for RenderTheater {
    type Arity = Variable;
    type ParentData = StackParentData;

    /// The first `skip_count` entries are offstage and publish no semantics.
    ///
    /// This is the whole point of the hook: an entry beneath the topmost
    /// opaque one is a real child that is deliberately not shown, and a screen
    /// reader must not find it. The placed-generation stamp gets the common
    /// case for free — an entry laid out while visible and then covered has a
    /// stale stamp — but not the one that matters most: an app that STARTS
    /// with an opaque route above another never lays the lower one out, so
    /// nothing ever stamps it and it reads as placed. Answering here is
    /// history-independent.
    ///
    /// The visited children are exactly the entries from `skip_count` onward.
    fn visits_child_for_semantics(&self, child_slot: usize) -> bool {
        child_slot >= self.skip_count
    }

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, StackParentData>,
    ) -> Size {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        self.child_count = child_count;

        let size = Self::theater_size(constraints);
        let child_constraints = BoxConstraints::tight(size);

        // Only the onstage children. The skipped ones keep whatever geometry they
        // last had, and nothing reads it: they are absent from paint, hit-test
        // and semantics.
        for i in self.first_onstage(child_count)..child_count {
            ctx.layout_child(i, child_constraints);
            ctx.position_child(i, Offset::ZERO);
        }

        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        Self::theater_size(constraints)
    }

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_onstage_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_onstage_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_onstage_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_onstage_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    /// Bottom → top over the onstage children only.
    ///
    /// No clip: a clip behavior would only bite when a `Positioned`
    /// entry overflows, and FLUI's theater has no positioned children.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Variable>) {
        for i in self.first_onstage(self.child_count)..self.child_count {
            ctx.paint_child(i);
        }
    }

    /// Top → bottom over the onstage children only, stopping after
    /// `child_count - skip_count` children.
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, StackParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        for i in (self.first_onstage(self.child_count)..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}
