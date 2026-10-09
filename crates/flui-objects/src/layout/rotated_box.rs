//! `RenderRotatedBox` — rotates its child by a whole number of quarter turns.
//!
//! Layout swaps width↔height constraints for odd turn counts; the paint matrix
//! rotates the child around the center of the parent's slot. Two recorded
//! decisions, both in `flui-rendering/ARCHITECTURE.md` (`## Mapping decisions`):
//! a same-parity turn change is served as a composited-layer update rather
//! than a relayout, and an even turn reports the child's baseline.
//!
//! # Design notes
//!
//! * `quarter_turns: i32` — negative values
//!   rotate counter-clockwise, and the angle is reduced via `rem_euclid(4)`
//!   before constructing the paint matrix so large inputs don't accumulate
//!   floating-point error.
//! * The paint matrix is a pure computation over `(parent_size, child_size,
//!   quarter_turns)` — no stale cached transform field that can
//!   drift from state.
//! * `set_quarter_turns` reports the narrowest impact the change actually
//!   needs instead of a relayout on every changed value: an unchanged
//!   effective angle (mod 4) reports no impact at all, a parity-preserving
//!   turn (same even/odd class, different quadrant) reports an update-only
//!   composited-layer commit instead of a full repaint, and only a parity
//!   change (odd ↔ even) forces layout. See `set_quarter_turns`'s own doc
//!   for the exact split and the premise it rests on.

use std::f64::consts::FRAC_PI_2;

use flui_foundation::Single;
use flui_foundation::geometry::{Matrix4, Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::{PaintEffects, RenderBox},
};

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// Rotates its child by a whole number of quarter turns (multiples of 90°).
///
/// Odd turn counts (1, 3, −1, −3, …) swap the width and height axes: the
/// child is laid out in a "portrait" slot when the parent is "landscape" and
/// vice versa.  Even turn counts (0, 2, −2, …) preserve axis orientation.
///
/// The widget's own size is:
/// - **Even turns**: same as the child's size.
/// - **Odd turns**: `(child.height, child.width)` — width and height swapped.
#[derive(Debug, Clone)]
pub struct RenderRotatedBox {
    /// Number of clockwise 90° rotations.  Negative = counter-clockwise.
    /// Reduced mod 4 when computing the paint matrix angle.
    quarter_turns: i32,
    /// Size of the most recently laid-out child, used by the paint matrix and
    /// hit-test transform to compute child-center offsets.
    child_size: Size,
    /// True after the first successful `perform_layout` with a child present.
    has_child: bool,
}

impl RenderRotatedBox {
    /// Creates the render object with the given quarter-turn count.
    pub fn new(quarter_turns: i32) -> Self {
        Self {
            quarter_turns,
            child_size: Size::ZERO,
            has_child: false,
        }
    }

    /// Returns the current quarter-turn count.
    #[inline]
    pub fn quarter_turns(&self) -> i32 {
        self.quarter_turns
    }

    /// Replaces the quarter-turn count and reports the narrowest observable
    /// impact for the change.
    ///
    /// - Unchanged raw value, or a value that reduces to the same quadrant
    ///   mod 4 (e.g. `1` → `5`, `-2` → `2`, `1` → `-3`):
    ///   [`NONE`](flui_rendering::RenderUpdateImpact::NONE). The raw value is
    ///   still stored — `quarter_turns()` reflects exactly what was set — but
    ///   the paint matrix and layout are byte-identical, so nothing
    ///   downstream needs to react.
    /// - Same parity (even ↔ even or odd ↔ odd), different quadrant (e.g.
    ///   `0` → `2`, `1` → `-1`):
    ///   [`COMPOSITED_LAYER_UPDATE`](flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE)
    ///   `|` [`SEMANTICS`](flui_rendering::RenderUpdateImpact::SEMANTICS).
    ///   `perform_layout`, `compute_dry_layout`, the four intrinsic queries,
    ///   `compute_dry_baseline`, and `forwards_baseline_to_only_child` all
    ///   key off `is_vertical` (private) — the turn's parity — and never the
    ///   exact value, so layout is unchanged and only the paint matrix
    ///   rotates: the retained subtree can be patched in place instead of
    ///   repainted. No test pins that these methods are turn-blind up to
    ///   parity.
    /// - Parity change (e.g. `0` → `1`, `3` → `4`):
    ///   [`LAYOUT`](flui_rendering::RenderUpdateImpact::LAYOUT) — axes swap,
    ///   so the child must be re-laid-out under (un)flipped constraints.
    pub fn set_quarter_turns(&mut self, quarter_turns: i32) -> flui_rendering::RenderUpdateImpact {
        if self.quarter_turns == quarter_turns {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let old_mod4 = self.quarter_turns.rem_euclid(4);
        let new_mod4 = quarter_turns.rem_euclid(4);
        self.quarter_turns = quarter_turns;

        if old_mod4 == new_mod4 {
            // Same effective angle: identical paint matrix, identical layout.
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        // `old_mod4`/`new_mod4` are already reduced to [0, 3] by
        // `rem_euclid(4)`, so `% 2` on them reads their parity directly —
        // no separate `rem_euclid(2)` pass over the (possibly negative) raw
        // values needed.
        if old_mod4 % 2 != new_mod4 % 2 {
            // Axis swap: the child must be re-laid-out under (un)flipped
            // constraints.
            return flui_rendering::RenderUpdateImpact::LAYOUT;
        }
        // Same parity, different quadrant: layout, child size, and origin
        // are all unchanged — only the paint matrix rotates, so the retained
        // subtree can be patched in place instead of repainted.
        flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
            | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Returns `true` when the quarter-turn count is odd (axes are swapped).
    ///
    /// `set_quarter_turns`'s parity-preserving fast path (module doc)
    /// depends on every layout-phase method reading `quarter_turns` ONLY
    /// through this predicate, never the exact raw value; no test pins it. A
    /// method that branches on the exact turn instead would go stale under
    /// that fast path: the setter would report no relayout for a change the
    /// method actually treats differently.
    #[inline]
    fn is_vertical(&self) -> bool {
        // `rem_euclid(2)` handles negative values correctly:
        // -1.rem_euclid(2) = 1 ≠ 0, so -1 turn is still vertical.
        self.quarter_turns.rem_euclid(2) != 0
    }

    /// Builds the paint matrix for the given parent and child sizes.
    ///
    /// Step 1: shift to the parent's center (`parent_size / 2`).
    /// Step 2: rotate by `quarter_turns mod 4 × π/2`.
    /// Step 3: shift back by the child's center (`-child_size / 2`).
    ///
    /// The resulting matrix transforms child-local coordinates to parent-local
    /// coordinates.  The pipeline applies it during paint; `hit_test` inverts
    /// it to recover the child-local position from the incoming pointer.
    fn build_paint_matrix(parent_size: Size, child_size: Size, quarter_turns: i32) -> Matrix4 {
        let angle = FRAC_PI_2 * (quarter_turns.rem_euclid(4) as f64);
        Matrix4::translation(parent_size.width / 2.0, parent_size.height / 2.0, 0.0)
            * Matrix4::rotation_z(angle)
            * Matrix4::translation(-child_size.width / 2.0, -child_size.height / 2.0, 0.0)
    }
}

impl flui_foundation::Diagnosticable for RenderRotatedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_int("quarter_turns", self.quarter_turns.into(), None);
    }
}

impl RenderBox for RenderRotatedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        // Reads `quarter_turns` only through `is_vertical()` — see that
        // method's doc and `set_quarter_turns`'s fast path.
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            self.child_size = Size::ZERO;
            // Nothing to rotate: the smallest size the constraints allow,
            // whatever the turn. Flipping first would swap the
            // axes of a non-square constraint and answer a size outside it.
            return Ok(constraints.smallest());
        }
        self.has_child = true;

        // Odd turns: pass flipped constraints to child so it sizes within the
        // parent's available space along the rotated axes.
        let child_constraints = if self.is_vertical() {
            constraints.flipped()
        } else {
            constraints
        };
        let child_size = ctx.layout_child(0, child_constraints)?;
        self.child_size = child_size;

        // Position child at origin — the paint matrix handles centering.
        ctx.position_child(0, Offset::ZERO);

        // Our claimed size: swap child dimensions for odd turns.
        if self.is_vertical() {
            Ok(Size::new(child_size.height, child_size.width))
        } else {
            Ok(child_size)
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !self.has_child {
            return false;
        }
        let own_size = ctx.own_size();
        let paint_matrix = Self::build_paint_matrix(own_size, self.child_size, self.quarter_turns);
        let Some(inverse) = paint_matrix.try_inverse() else {
            // Degenerate rotation matrix (e.g. child has zero size in one
            // axis).  Nothing is hittable.
            return false;
        };
        let pos = ctx.position();
        let (child_local_x, child_local_y) = inverse.transform_point(pos.dx, pos.dy);
        ctx.hit_test_child(0, Offset::new(child_local_x, child_local_y))
    }

    // ---- paint effects / hit-test transform ---------------------------------

    fn paint_effects(&self, size: Size) -> PaintEffects {
        if !self.has_child {
            return PaintEffects::NONE;
        }
        PaintEffects::NONE.with_transform(Self::build_paint_matrix(
            size,
            self.child_size,
            self.quarter_turns,
        ))
    }

    fn hit_test_transform(&self, size: Size) -> Option<Matrix4> {
        // Same matrix as paint; the pipeline uses hit_test_transform to push
        // an accumulated-transform entry onto the HitTestResult.
        if !self.has_child {
            return None;
        }
        Some(Self::build_paint_matrix(
            size,
            self.child_size,
            self.quarter_turns,
        ))
    }

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Odd quarter_turns swap width↔height axes; even turns pass through.

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        if self.is_vertical() {
            ctx.child_min_intrinsic_height(0, height)
        } else {
            ctx.child_min_intrinsic_width(0, height)
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        if self.is_vertical() {
            ctx.child_max_intrinsic_height(0, height)
        } else {
            ctx.child_max_intrinsic_width(0, height)
        }
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        if self.is_vertical() {
            ctx.child_min_intrinsic_width(0, width)
        } else {
            ctx.child_min_intrinsic_height(0, width)
        }
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        if self.is_vertical() {
            ctx.child_max_intrinsic_width(0, width)
        } else {
            ctx.child_max_intrinsic_height(0, width)
        }
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        if ctx.child_count() == 0 {
            // Same answer as `perform_layout`'s childless branch: the turn
            // does not enter it.
            return Ok(constraints.smallest());
        }
        let child_constraints = if self.is_vertical() {
            constraints.flipped()
        } else {
            constraints
        };
        let child_size = ctx.child_dry_layout(0, child_constraints)?;
        if self.is_vertical() {
            Ok(Size::new(child_size.height, child_size.width))
        } else {
            Ok(child_size)
        }
    }

    /// The live half of the baseline contract below: for an even turn the
    /// layout driver walks to the child and answers with its baseline
    /// unchanged, exactly as it does for a pure proxy; for an odd turn there
    /// is no horizontal baseline to offer. Reads the turn only through its
    /// parity, like every other layout-phase method here.
    fn forwards_baseline_to_only_child(&self) -> bool {
        !self.is_vertical()
    }

    /// A rotated box has a baseline only for an even turn, and then it is the
    /// child's own, unchanged. A baseline is a layout line: an even turn keeps
    /// the box's size and its horizontal axis, so the box takes part in
    /// baseline alignment as its unrotated self would (the glyphs flip in
    /// place at turn 2) — the same model draw-time rotations use, here in
    /// `RenderTransform` and in Compose's `Modifier.rotate` / SwiftUI's
    /// `.rotationEffect`. An odd turn rotates the baseline axis into the
    /// vertical, so there is no horizontal baseline and the box is treated
    /// like any child without one.
    ///
    /// The even-turn answer follows the child. See
    /// `flui-rendering/ARCHITECTURE.md` (`## Mapping decisions`,
    /// "`RenderRotatedBox` reports a baseline only for an even turn"); no test
    /// asserts it. Reads the turn only through `is_vertical()`, which is what
    /// keeps `set_quarter_turns`'s same-parity fast path valid.
    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        if ctx.child_count() == 0 || self.is_vertical() {
            return Ok(None);
        }
        ctx.child_dry_baseline(0, constraints, baseline)
    }
}

// ===========================================================================
// Tests
// ===========================================================================
