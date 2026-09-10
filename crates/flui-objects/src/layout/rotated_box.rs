//! `RenderRotatedBox` — rotates its child by a whole number of quarter turns.
//!
//! # Flutter equivalence
//!
//! Port of Flutter's `RenderRotatedBox`
//! (`packages/flutter/lib/src/rendering/rotated_box.dart`): layout swaps
//! width↔height constraints for odd turn counts; the paint matrix rotates the
//! child around the center of the parent's slot. Two recorded divergences,
//! both in `flui-rendering/ARCHITECTURE.md` (`## Mapping decisions`): a
//! same-parity turn change is served as a composited-layer update rather
//! than a relayout, and an even turn reports the child's baseline where
//! upstream reports none.
//!
//! # Rust-native improvements
//!
//! * `quarter_turns: i32` (vs Dart's unconstrained `int`) — negative values
//!   rotate counter-clockwise, and the angle is reduced via `rem_euclid(4)`
//!   before constructing the paint matrix so large inputs don't accumulate
//!   floating-point error.
//! * The paint matrix is a pure computation over `(parent_size, child_size,
//!   quarter_turns)` — no stale cached `_paintTransform` field that can
//!   drift from state.
//! * `set_quarter_turns` reports the narrowest impact the change actually
//!   needs instead of upstream's relayout on every changed value: an unchanged
//!   effective angle (mod 4) reports no impact at all, a parity-preserving
//!   turn (same even/odd class, different quadrant) reports an update-only
//!   composited-layer commit instead of a full repaint, and only a parity
//!   change (odd ↔ even) forces layout. See `set_quarter_turns`'s own doc
//!   for the exact split and the premise it rests on.

use std::f32::consts::FRAC_PI_2;

use flui_tree::Single;
use flui_types::{Matrix4, Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::RenderBox,
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
///
/// Flutter parity: `RenderRotatedBox` in `rotated_box.dart`.
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
    ///   repainted. Pinned by
    ///   `harness_rotated_box_layout_is_turn_blind_up_to_parity` (the dry
    ///   queries, same-parity equality) and
    ///   `harness_rotated_box_baseline_follows_the_child_for_even_turns_and_is_absent_for_odd`
    ///   (both baseline halves, by value, at every quadrant) in
    ///   `flui-objects/tests/render_object_harness.rs`.
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
    /// through this predicate, never the exact raw value — pinned by
    /// `harness_rotated_box_layout_is_turn_blind_up_to_parity` for the dry
    /// queries and by
    /// `harness_rotated_box_baseline_follows_the_child_for_even_turns_and_is_absent_for_odd`
    /// for the live baseline flag
    /// (`flui-objects/tests/render_object_harness.rs`). A method that
    /// branches on the exact turn instead would go stale under that fast
    /// path: the setter would report no relayout for a change the method
    /// actually treats differently.
    #[inline]
    fn is_vertical(&self) -> bool {
        // `rem_euclid(2)` handles negative values correctly:
        // -1.rem_euclid(2) = 1 ≠ 0, so -1 turn is still vertical.
        self.quarter_turns.rem_euclid(2) != 0
    }

    /// Builds the paint matrix for the given parent and child sizes.
    ///
    /// Flutter parity: `RenderRotatedBox.performLayout` paint-transform
    /// computation via `Matrix4.identity()..translate..rotateZ..translate`.
    ///
    /// Step 1: shift to the parent's center (`parent_size / 2`).
    /// Step 2: rotate by `quarter_turns mod 4 × π/2`.
    /// Step 3: shift back by the child's center (`-child_size / 2`).
    ///
    /// The resulting matrix transforms child-local coordinates to parent-local
    /// coordinates.  The pipeline applies it during paint; `hit_test` inverts
    /// it to recover the child-local position from the incoming pointer.
    fn build_paint_matrix(parent_size: Size, child_size: Size, quarter_turns: i32) -> Matrix4 {
        let angle = FRAC_PI_2 * (quarter_turns.rem_euclid(4) as f32);
        Matrix4::translation(
            parent_size.width.get() / 2.0,
            parent_size.height.get() / 2.0,
            0.0,
        ) * Matrix4::rotation_z(angle)
            * Matrix4::translation(
                -child_size.width.get() / 2.0,
                -child_size.height.get() / 2.0,
                0.0,
            )
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

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        // Reads `quarter_turns` only through `is_vertical()` — see that
        // method's doc and `set_quarter_turns`'s fast path.
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            self.child_size = Size::ZERO;
            // Nothing to rotate: the smallest size the constraints allow,
            // whatever the turn — as upstream. Flipping first would swap the
            // axes of a non-square constraint and answer a size outside it.
            return constraints.smallest();
        }
        self.has_child = true;

        // Odd turns: pass flipped constraints to child so it sizes within the
        // parent's available space along the rotated axes.
        let child_constraints = if self.is_vertical() {
            constraints.flipped()
        } else {
            constraints
        };
        let child_size = ctx.layout_child(0, child_constraints);
        self.child_size = child_size;

        // Position child at origin — the paint matrix handles centering.
        ctx.position_child(0, Offset::ZERO);

        // Our claimed size: swap child dimensions for odd turns.
        if self.is_vertical() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
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

    // ---- paint-transform hooks ----------------------------------------------

    fn paint_transform(&self, size: Size) -> Option<Matrix4> {
        if !self.has_child {
            return None;
        }
        Some(Self::build_paint_matrix(
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
    // Flutter parity: rotated_box.dart RenderRotatedBox.
    // Odd quarter_turns swap width↔height axes; even turns pass through.

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        if self.is_vertical() {
            ctx.child_min_intrinsic_height(0, height)
        } else {
            ctx.child_min_intrinsic_width(0, height)
        }
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        if self.is_vertical() {
            ctx.child_max_intrinsic_height(0, height)
        } else {
            ctx.child_max_intrinsic_width(0, height)
        }
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        if self.is_vertical() {
            ctx.child_min_intrinsic_width(0, width)
        } else {
            ctx.child_min_intrinsic_height(0, width)
        }
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
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
    ) -> Size {
        if ctx.child_count() == 0 {
            // Same answer as `perform_layout`'s childless branch: the turn
            // does not enter it.
            return constraints.smallest();
        }
        let child_constraints = if self.is_vertical() {
            constraints.flipped()
        } else {
            constraints
        };
        let child_size = ctx.child_dry_layout(0, child_constraints);
        if self.is_vertical() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
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
    /// Upstream `RenderRotatedBox` has no baseline override at all
    /// (`rotated_box.dart`, 3.44.0): its live query reports `null` for every
    /// turn and its dry query falls through to `RenderBox`'s default, which
    /// asserts in debug builds. The even-turn answer is a recorded divergence
    /// — see
    /// `flui-rendering/ARCHITECTURE.md` (`## Mapping decisions`,
    /// "`RenderRotatedBox` reports a baseline only for an even turn") and its
    /// replacement oracle,
    /// `harness_rotated_box_baseline_follows_the_child_for_even_turns_and_is_absent_for_odd`.
    /// Reads the turn only through `is_vertical()`, which is what keeps
    /// `set_quarter_turns`'s same-parity fast path valid.
    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 || self.is_vertical() {
            return None;
        }
        ctx.child_dry_baseline(0, constraints, baseline)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    #[test]
    fn is_vertical_even_turns_false() {
        assert!(!RenderRotatedBox::new(0).is_vertical());
        assert!(!RenderRotatedBox::new(2).is_vertical());
        assert!(!RenderRotatedBox::new(-2).is_vertical());
        assert!(!RenderRotatedBox::new(4).is_vertical());
    }

    #[test]
    fn is_vertical_odd_turns_true() {
        assert!(RenderRotatedBox::new(1).is_vertical());
        assert!(RenderRotatedBox::new(3).is_vertical());
        assert!(RenderRotatedBox::new(-1).is_vertical());
        assert!(RenderRotatedBox::new(-3).is_vertical());
    }

    #[test]
    fn paint_matrix_even_turns_is_identity_like() {
        // 0 turns: matrix should map (0,0) to (w/2-w/2, h/2-h/2) = (0,0)
        let size = Size::new(px(100.0), px(50.0));
        let m = RenderRotatedBox::build_paint_matrix(size, size, 0);
        let (ox, oy) = m.transform_point(px(0.0), px(0.0));
        assert!((ox.get()).abs() < 1e-4, "ox = {ox:?}");
        assert!((oy.get()).abs() < 1e-4, "oy = {oy:?}");
    }

    #[test]
    fn paint_matrix_90_degree_rotates_child_center_to_parent_center() {
        // Parent 60×100, child 100×60 (after 90° turn the axes are swapped).
        let parent_size = Size::new(px(60.0), px(100.0));
        let child_size = Size::new(px(100.0), px(60.0));
        let m = RenderRotatedBox::build_paint_matrix(parent_size, child_size, 1);
        // Child center (50, 30) should map to parent center (30, 50).
        let (px_out, py_out) = m.transform_point(px(50.0), px(30.0));
        assert!((px_out.get() - 30.0).abs() < 1e-3, "px = {px_out:?}");
        assert!((py_out.get() - 50.0).abs() < 1e-3, "py = {py_out:?}");
    }

    #[test]
    fn build_paint_matrix_is_invertible() {
        let parent_size = Size::new(px(100.0), px(200.0));
        let child_size = Size::new(px(200.0), px(100.0));
        let m = RenderRotatedBox::build_paint_matrix(parent_size, child_size, 1);
        assert!(m.try_inverse().is_some(), "paint matrix must be invertible");
    }

    #[test]
    fn setter_returns_change_flag() {
        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(2),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(
            node.set_quarter_turns(2),
            flui_rendering::RenderUpdateImpact::NONE
        );
        assert_eq!(node.quarter_turns(), 2);
    }

    /// `set_quarter_turns` reports `NONE` whenever the new value reduces to
    /// the same quadrant mod 4 as the old one — the paint matrix and layout
    /// are byte-identical (`build_paint_matrix` and every layout-phase
    /// method key off `rem_euclid`) — but the raw value is still stored, so
    /// `quarter_turns()` reflects exactly what was set, not the reduced
    /// form.
    #[test]
    fn set_quarter_turns_same_quadrant_is_a_noop_but_stores_the_raw_value() {
        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(1),
            flui_rendering::RenderUpdateImpact::NONE,
            "identical raw value",
        );
        assert_eq!(node.quarter_turns(), 1);

        assert_eq!(
            node.set_quarter_turns(5),
            flui_rendering::RenderUpdateImpact::NONE,
            "1 and 5 both reduce to quadrant 1 mod 4",
        );
        assert_eq!(node.quarter_turns(), 5);

        let mut node = RenderRotatedBox::new(-2);
        assert_eq!(
            node.set_quarter_turns(2),
            flui_rendering::RenderUpdateImpact::NONE,
            "-2 and 2 both reduce to quadrant 2 mod 4",
        );
        assert_eq!(node.quarter_turns(), 2);

        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(-3),
            flui_rendering::RenderUpdateImpact::NONE,
            "1 and -3 both reduce to quadrant 1 mod 4",
        );
        assert_eq!(node.quarter_turns(), -3);
    }

    /// Same parity (even ↔ even or odd ↔ odd), different quadrant: layout is
    /// unchanged (`is_vertical` reads the same on both sides) so only the
    /// paint matrix rotates — an update-only composited-layer commit, not a
    /// repaint.
    #[test]
    fn set_quarter_turns_same_parity_different_quadrant_reports_layer_update() {
        let mut node = RenderRotatedBox::new(0);
        assert_eq!(
            node.set_quarter_turns(2),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(3),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(-1),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );

        let mut node = RenderRotatedBox::new(-2);
        assert_eq!(
            node.set_quarter_turns(0),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    /// A parity change (even ↔ odd) swaps which axis the child is laid out
    /// against, so it must relayout — the one case `set_quarter_turns` still
    /// reports the pre-existing `LAYOUT` on any change.
    #[test]
    fn set_quarter_turns_parity_change_reports_layout() {
        let mut node = RenderRotatedBox::new(0);
        assert_eq!(
            node.set_quarter_turns(1),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );

        let mut node = RenderRotatedBox::new(1);
        assert_eq!(
            node.set_quarter_turns(2),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );

        let mut node = RenderRotatedBox::new(3);
        assert_eq!(
            node.set_quarter_turns(4),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );

        let mut node = RenderRotatedBox::new(-1);
        assert_eq!(
            node.set_quarter_turns(0),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
    }

    #[test]
    fn constraints_flipped_for_odd_turns() {
        // bc(0, 200, 0, 100).flipped() = bc(0, 100, 0, 200)
        let c = bc(0.0, 200.0, 0.0, 100.0).flipped();
        assert_eq!(c.max_width, px(100.0));
        assert_eq!(c.max_height, px(200.0));
    }

    #[test]
    fn intrinsics_zero_without_child() {
        let node = RenderRotatedBox::new(1);
        flui_rendering::context::intrinsics_test_support::leaf_intrinsics(|ctx| {
            assert_eq!(node.compute_min_intrinsic_width(100.0, ctx), 0.0);
            assert_eq!(node.compute_max_intrinsic_height(100.0, ctx), 0.0);
        });
    }
}
