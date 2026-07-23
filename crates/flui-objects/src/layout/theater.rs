//! `RenderTheater` — the `Overlay`'s stack, with the first `skip_count`
//! children held offstage.
//!
//! # Flutter equivalence
//!
//! `_Theater` / `_RenderTheater` (`packages/flutter/lib/src/widgets/overlay.dart`,
//! master `3.33.0-0.0.pre-6280-g88e87cd963f`). Flutter's own summary
//! (`overlay.dart:979-981`):
//!
//! > Special version of a `Stack`, that doesn't layout and render the first
//! > `skipCount` children. The first `skipCount` children are considered
//! > "offstage".
//!
//! `skip_count` exists to serve `OverlayEntry.opaque`: entries below the topmost
//! opaque one are dropped from the tree unless they set `maintainState`, and the
//! ones kept are the ones skipped here. Because `OverlayState.build` collects
//! top-first and then reverses (`overlay.dart:894`, `:916`), the skipped
//! children are always the **leading** ones of the child list.
//!
//! With `skip_count == 0` this is exactly `RenderStack` with `StackFit::Expand`:
//! `size = constraints.biggest` and every child gets `BoxConstraints::tight(size)`
//! — Flutter comments the very same line as "Equivalent to BoxConstraints used by
//! RenderStack for StackFit.expand" (`overlay.dart:1478`).
//!
//! # Divergences, deliberate and recorded
//!
//! * **No positioned children.** `_RenderTheater` runs the full `RenderStack`
//!   positioned/non-positioned split, because an app may put a `Positioned` at the
//!   root of an `OverlayEntry`. FLUI's `Overlay` builds one non-positioned
//!   `OverlayEntryView` per entry and nothing else, so every child here is
//!   non-positioned. `StackParentData` is kept as the parent-data type for
//!   compatibility with `RenderStack` tooling; its positioning fields are ignored.
//! * **No `canSizeOverlay` / `alwaysSizeToContent`.** Those only matter under
//!   *unbounded* constraints, where Flutter throws unless an entry opts in
//!   (`overlay.dart:1511-1525`). FLUI has no `canSizeOverlay` flag, so an
//!   unbounded theater falls back to `constraints.smallest()`, matching
//!   `RenderStack`'s own no-non-positioned-children fallback rather than panicking
//!   — see [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md).
//! * **Semantics are not skipped.** Flutter's `visitChildrenForSemantics` walks
//!   `_childrenInPaintOrder()` (`overlay.dart:1427-1428`), so offstage entries are
//!   absent from the semantics tree. FLUI's `RenderBox` has no per-child semantics
//!   visitor — only the whole-subtree `excludes_semantics_subtree` — so a
//!   `maintainState` entry beneath an opaque one is still announced. Parity is
//!   **not** claimed for this; `RenderOffstage` (which a `ModalRoute` puts around
//!   its page) does suppress semantics for the case that matters today.

use flui_tree::Variable;
use flui_types::{Offset, Size};

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

    /// Returns whether the value changed, so the caller can skip
    /// `mark_needs_layout`.
    pub const fn set_skip_count(&mut self, skip_count: usize) -> bool {
        let changed = self.skip_count != skip_count;
        self.skip_count = skip_count;
        changed
    }

    /// The index of the first onstage child.
    ///
    /// Flutter asserts `skipCount <= children.length` (`overlay.dart:989`) and
    /// would then walk off the end. Clamping is the same behavior for every legal
    /// input and is total for the rest; a caller error here must not corrupt the
    /// child walk.
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

    /// Flutter's `RenderStack.getIntrinsicDimension` over `_firstOnstageChild`
    /// and its later siblings (`overlay.dart:1359-1389`).
    fn max_onstage_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f32,
        mut query: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        let child_count = ctx.child_count();
        let mut max = 0.0f32;
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
        // last had — Flutter does the same, and nothing reads it: they are absent
        // from paint, hit-test and (in Flutter) semantics.
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

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.max_onstage_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.max_onstage_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.max_onstage_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.max_onstage_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    /// Bottom → top over the onstage children only — Flutter's
    /// `_childrenInPaintOrder()` starting at `_firstOnstageChild`
    /// (`overlay.dart:1424-1440`).
    ///
    /// No clip: `_RenderTheater`'s `clipBehavior` only bites when a `Positioned`
    /// entry overflows, and FLUI's theater has no positioned children.
    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Variable>) {
        for i in self.first_onstage(self.child_count)..self.child_count {
            ctx.paint_child(i);
        }
    }

    /// Top → bottom over the onstage children only — Flutter's
    /// `_childrenInHitTestOrder()`, which stops after `childCount - skipCount`
    /// children (`overlay.dart:1443-1458`).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Flutter asserts `skipCount <= children.length` (`overlay.dart:989`) and
    /// would then walk off the end. Clamping is the same behavior for every legal
    /// input, and total for the rest.
    #[test]
    fn first_onstage_clamps_an_out_of_range_skip_count() {
        let theater = RenderTheater::new().with_skip_count(5);
        assert_eq!(theater.first_onstage(2), 2, "never past the last child");
        assert_eq!(theater.first_onstage(7), 5);
        assert_eq!(RenderTheater::new().first_onstage(3), 0);
    }

    /// The setter reports change so the caller can skip `mark_needs_layout`, as
    /// every other Wave-3a render object does.
    #[test]
    fn set_skip_count_reports_only_real_changes() {
        let mut theater = RenderTheater::new();
        assert!(theater.set_skip_count(2));
        assert!(!theater.set_skip_count(2));
        assert_eq!(theater.skip_count(), 2);
    }
}
