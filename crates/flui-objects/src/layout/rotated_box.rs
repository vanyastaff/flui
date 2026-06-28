//! `RenderRotatedBox` — rotates its child by a whole number of quarter turns.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderRotatedBox`
//! (`packages/flutter/lib/src/rendering/rotated_box.dart`).
//! Layout swaps width↔height constraints for odd turn counts; the paint matrix
//! rotates the child around the center of the parent's slot.
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

    /// Replaces the quarter-turn count; returns `true` if the value changed.
    pub fn set_quarter_turns(&mut self, quarter_turns: i32) -> bool {
        if self.quarter_turns == quarter_turns {
            return false;
        }
        self.quarter_turns = quarter_turns;
        true
    }

    /// Returns `true` when the quarter-turn count is odd (axes are swapped).
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
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            self.child_size = Size::ZERO;
            return if self.is_vertical() {
                constraints.flipped().smallest()
            } else {
                constraints.smallest()
            };
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
            return if self.is_vertical() {
                constraints.flipped().smallest()
            } else {
                constraints.smallest()
            };
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

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        // Rotation means the child's baseline is in a rotated coordinate frame;
        // for non-trivial rotations the concept of a text baseline does not map
        // directly. Flutter returns the raw child baseline only for even turns
        // (no axis swap). For odd turns (vertical), there is no conventional
        // horizontal baseline — return None to match Flutter's RenderBox default
        // for objects where a baseline cannot be determined.
        if self.is_vertical() {
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
        assert!(node.set_quarter_turns(2));
        assert!(!node.set_quarter_turns(2));
        assert_eq!(node.quarter_turns(), 2);
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
