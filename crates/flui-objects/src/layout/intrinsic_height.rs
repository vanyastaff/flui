//! `RenderIntrinsicHeight` — expands the child to its maximum intrinsic height.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderIntrinsicHeight`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`, lines 783–850).
//! The child is asked for its maximum intrinsic height for the incoming max
//! width, then laid out tight to that height.  Width is left unconstrained so
//! the child can take whatever width it needs within the parent's bounds.
//!
//! `RenderIntrinsicHeight` has no `step_width`/`step_height` knobs — those
//! belong to `RenderIntrinsicWidth` only.

use flui_tree::Single;
use flui_types::{Offset, Pixels, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// Sizes itself to the child's maximum intrinsic height.
///
/// Useful when a widget should be exactly as tall as its natural content.
/// When the parent's height is already tight, that tight value is propagated
/// directly without querying the child's intrinsic.  When the height is
/// unbounded, the child is asked for its maximum intrinsic height and the
/// result is clamped to the incoming height range before being tightened.
///
/// Flutter parity: `RenderIntrinsicHeight` in `proxy_box.dart`.
#[derive(Debug, Clone)]
pub struct RenderIntrinsicHeight {
    /// True after the first successful `perform_layout` with a child present.
    has_child: bool,
}

impl RenderIntrinsicHeight {
    /// Creates the render object.
    pub fn new() -> Self {
        Self { has_child: false }
    }

    /// Computes the tight child constraints for layout.
    ///
    /// Mirrors Flutter's `RenderIntrinsicHeight._childConstraints`.
    /// Width is passed through unchanged (the base unconstrained min=0..max=∞
    /// enforced against `constraints` restores the original width range).
    /// Height is tightened to `max_intrinsic_height` clamped to `[min_h, max_h]`,
    /// unless the incoming height is already tight.
    fn child_constraints(
        &self,
        constraints: BoxConstraints,
        max_intrinsic_height: Pixels,
    ) -> BoxConstraints {
        let tight_height = if constraints.has_tight_height() {
            constraints.min_height
        } else {
            // Clamp to the incoming height range before tightening.
            px(max_intrinsic_height
                .get()
                .clamp(constraints.min_height.get(), constraints.max_height.get()))
        };
        // Start from the incoming constraints (preserves width range) and
        // tighten only the height axis.
        constraints.tighten(None, Some(tight_height))
    }
}

impl Default for RenderIntrinsicHeight {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderIntrinsicHeight {
    fn debug_fill_properties(&self, _builder: &mut flui_foundation::DiagnosticsBuilder) {
        // No configuration knobs to expose.
    }
}

impl RenderBox for RenderIntrinsicHeight {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            return constraints.smallest();
        }
        self.has_child = true;

        // Query the child's max intrinsic height for the incoming max width.
        // The live intrinsics callback (wired by the pipeline) routes this
        // through `box_intrinsic_query_borrowed`.  On Direct-storage (test)
        // contexts it returns 0.0 (conservative fallback).
        let max_h = px(ctx.child_max_intrinsic_height(0, constraints.max_width.get()));
        let child_constraints = self.child_constraints(constraints, max_h);
        let child_size = ctx.layout_child(0, child_constraints);
        ctx.position_child(0, Offset::ZERO);
        constraints.constrain(child_size)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Flutter parity: proxy_box.dart RenderIntrinsicHeight.
    // Width queries delegate to child; height queries use the tightened-height
    // child constraints to get the accurate value.

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        // When height is finite, the caller already knows the height extent;
        // pass it through to the child.  When infinite, the child is
        // unconstrained in height and should report its own unconstrained
        // intrinsic width.
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_width(0, height)
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        // The intrinsic height is determined by the child's max intrinsic height,
        // which is also what this widget sizes itself to.
        ctx.child_max_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_height(0, width)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return constraints.smallest();
        }
        // Probe the child's intrinsic height via a dry layout at unconstrained
        // width + max incoming width (matching Flutter's `_computeSize` helper).
        let max_h_raw = ctx
            .child_dry_layout(
                0,
                BoxConstraints::UNCONSTRAINED.tighten(Some(constraints.max_width), None),
            )
            .height;
        let child_constraints = self.child_constraints(constraints, max_h_raw);
        let child_size = ctx.child_dry_layout(0, child_constraints);
        constraints.constrain(child_size)
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
        let max_h_raw = ctx
            .child_dry_layout(
                0,
                BoxConstraints::UNCONSTRAINED.tighten(Some(constraints.max_width), None),
            )
            .height;
        let child_constraints = self.child_constraints(constraints, max_h_raw);
        ctx.child_dry_baseline(0, child_constraints, baseline)
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
    fn child_constraints_tight_height_propagated() {
        let node = RenderIntrinsicHeight::new();
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        let child_c = node.child_constraints(constraints, px(999.0));
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(50.0));
    }

    #[test]
    fn child_constraints_intrinsic_height_clamped() {
        let node = RenderIntrinsicHeight::new();
        // Incoming height range [20, 80]; child says max intrinsic = 150 → clamp to 80.
        let constraints = bc(0.0, 200.0, 20.0, 80.0);
        let child_c = node.child_constraints(constraints, px(150.0));
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(80.0));
    }

    #[test]
    fn child_constraints_intrinsic_height_within_range() {
        let node = RenderIntrinsicHeight::new();
        // Child says 60, range [20, 80] → stays at 60.
        let constraints = bc(0.0, 200.0, 20.0, 80.0);
        let child_c = node.child_constraints(constraints, px(60.0));
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(60.0));
    }

    #[test]
    fn intrinsics_zero_without_child() {
        let node = RenderIntrinsicHeight::new();
        flui_rendering::context::intrinsics_test_support::leaf_intrinsics(|ctx| {
            assert_eq!(node.compute_min_intrinsic_width(100.0, ctx), 0.0);
            assert_eq!(node.compute_max_intrinsic_width(100.0, ctx), 0.0);
            assert_eq!(node.compute_min_intrinsic_height(100.0, ctx), 0.0);
            assert_eq!(node.compute_max_intrinsic_height(100.0, ctx), 0.0);
        });
    }

    #[test]
    fn default_creates_node() {
        let _node = RenderIntrinsicHeight::default();
    }
}
