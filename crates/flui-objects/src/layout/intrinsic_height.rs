//! `RenderIntrinsicHeight` — expands the child to its maximum intrinsic height.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderIntrinsicHeight`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`, lines 783–850).
//! The child is asked for its maximum intrinsic height for the incoming raw
//! `max_width`, then laid out tight to that height.  Width is left unconstrained
//! so the child can take whatever width it needs within the parent's bounds.
//!
//! `RenderIntrinsicHeight` has no `step_width`/`step_height` knobs — those
//! belong to `RenderIntrinsicWidth` only.
//!
//! # ADR-0011 fix
//!
//! The old `compute_dry_layout` / `compute_dry_baseline` approximated the
//! intrinsic height via a `child_dry_layout` probe at unconstrained width, which
//! diverges from `perform_layout` for width-filling children (e.g. a flex row
//! with `MainAxisSize::Max`).  All three compute passes now share one
//! `child_constraints` helper that issues the real child-intrinsic query through
//! the appropriate context channel — dry ≡ committed.

use flui_tree::Single;
use flui_types::{Offset, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    storage::IntrinsicDimension,
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

    /// Computes the tight child constraints using an `intrinsic` closure.
    ///
    /// Mirrors Flutter's `RenderIntrinsicHeight._childConstraints`
    /// (proxy_box.dart:816-819):
    ///
    /// - **Width axis**: unchanged (Flutter passes the incoming width range
    ///   through unmodified; `tighten(None, Some(height))` preserves `min_width`
    ///   and `max_width`).
    ///
    /// - **Height axis**: if the incoming height is already tight, keep it.
    ///   Otherwise call `intrinsic(MaxHeight, constraints.max_width)` with the
    ///   RAW `max_width`, and tighten (which clamps to `[min_height, max_height]`).
    ///   IntrinsicHeight always forces height when not tight — unlike
    ///   IntrinsicWidth's width axis, there is no step gate here.
    ///
    /// The `intrinsic` closure is called at most once and is consumed by this
    /// method.  Callers pass `|dim, extent| ctx.child_intrinsic(0, dim, extent)`
    /// for all three compute passes; only the ctx type differs.
    fn child_constraints(
        constraints: BoxConstraints,
        mut intrinsic: impl FnMut(IntrinsicDimension, f32) -> f32,
    ) -> BoxConstraints {
        // Height axis — proxy_box.dart:816-819
        let height = if constraints.has_tight_height() {
            // Parent already determined height; skip the intrinsic query.
            constraints.min_height
        } else {
            // Raw query arg: constraints.max_width, not computed/snapped.
            // tighten will clamp to [min_height, max_height].
            px(intrinsic(
                IntrinsicDimension::MaxHeight,
                constraints.max_width.get(),
            ))
        };
        // Width axis: None = keep incoming width range.
        constraints.tighten(None, Some(height))
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

        // `child_constraints` queries the child's max intrinsic height through
        // the live `box_intrinsic_query_borrowed` callback, same as before.
        let child_constraints = Self::child_constraints(constraints, |dim, extent| {
            ctx.child_intrinsic(0, dim, extent)
        });
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
        // Flutter (proxy_box.dart): an infinite height resolves to the
        // child's own max intrinsic height at infinity before querying its
        // min intrinsic width — "min width at infinite height" is not a
        // meaningful query on its own.
        let height = if height.is_finite() {
            height
        } else {
            ctx.child_max_intrinsic_height(0, f32::INFINITY)
        };
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let height = if height.is_finite() {
            height
        } else {
            ctx.child_max_intrinsic_height(0, f32::INFINITY)
        };
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
        // Structurally identical to perform_layout: child_constraints issues
        // the real intrinsic sub-query through DryLayoutChildRequest::Intrinsic
        // (ADR-0011 Slice 1), routed by the driver to the memoized intrinsic_query.
        // The old `child_dry_layout`-based approximation is removed — dry ≡ committed.
        let child_constraints = Self::child_constraints(constraints, |dim, extent| {
            ctx.child_intrinsic(0, dim, extent)
        });
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
        // Same child_constraints helper; the intrinsic closure routes through
        // BoxDryBaselineCtx's intrinsic channel.
        let child_constraints = Self::child_constraints(constraints, |dim, extent| {
            ctx.child_intrinsic(0, dim, extent)
        });
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
    fn child_constraints_tight_height_not_queried() {
        // When incoming height is tight, the closure must NOT be called.
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        let child_c = RenderIntrinsicHeight::child_constraints(constraints, |_, _| {
            panic!("intrinsic queried on tight height")
        });
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(50.0));
    }

    #[test]
    fn child_constraints_intrinsic_height_clamped() {
        // Incoming height range [20, 80]; child says max intrinsic = 150 → clamp to 80.
        let constraints = bc(0.0, 200.0, 20.0, 80.0);
        let child_c = RenderIntrinsicHeight::child_constraints(constraints, |_, _| 150.0);
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(80.0));
    }

    #[test]
    fn child_constraints_intrinsic_height_within_range() {
        // Child says 60, range [20, 80] → stays at 60.
        let constraints = bc(0.0, 200.0, 20.0, 80.0);
        let child_c = RenderIntrinsicHeight::child_constraints(constraints, |_, _| 60.0);
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(60.0));
    }

    #[test]
    fn child_constraints_height_raw_arg_is_max_width() {
        // The height query arg must be constraints.max_width (raw).
        let constraints = bc(0.0, 120.0, 0.0, 200.0);
        let mut saw_extent = f32::NAN;
        RenderIntrinsicHeight::child_constraints(constraints, |dim, extent| {
            assert_eq!(dim, IntrinsicDimension::MaxHeight);
            saw_extent = extent;
            40.0
        });
        assert!(
            (saw_extent - 120.0).abs() < 0.01,
            "height query arg should be constraints.max_width (120), got {saw_extent}"
        );
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
