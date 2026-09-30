//! `RenderIntrinsicHeight` — expands the child to its maximum intrinsic height.
//!
//! The child is asked for its maximum intrinsic height for the incoming raw
//! `max_width`, then laid out tight to that height.  Width is left unconstrained
//! so the child can take whatever width it needs within the parent's bounds.
//!
//! `RenderIntrinsicHeight` has no `step_width`/`step_height` knobs — those
//! belong to `RenderIntrinsicWidth` only.
//!
//! # Dry intrinsics
//!
//! Approximating the intrinsic height via a `child_dry_layout` probe at
//! unconstrained width would diverge from `perform_layout` for width-filling
//! children (e.g. a flex row with `MainAxisSize::Max`).  All three compute
//! passes therefore share one `child_constraints` helper that issues the real
//! child-intrinsic query through the appropriate context channel — dry ≡
//! committed.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, BoxLayoutContext},
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
    /// - **Width axis**: unchanged (the incoming width range passes through
    ///   unmodified; `tighten(None, Some(height))` preserves `min_width`
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
        mut intrinsic: impl FnMut(IntrinsicDimension, f64) -> f64,
    ) -> BoxConstraints {
        // Height axis.
        let height = if constraints.has_tight_height() {
            // Parent already determined height; skip the intrinsic query.
            constraints.min_height
        } else {
            // Raw query arg: constraints.max_width, not computed/snapped.
            // tighten will clamp to [min_height, max_height].
            intrinsic(IntrinsicDimension::MaxHeight, constraints.max_width)
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

    flui_rendering::forward_single_child_box_hit_test!();

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Width queries delegate to child; height queries use the tightened-height
    // child constraints to get the accurate value.

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        // An infinite height resolves to the child's own max intrinsic height
        // at infinity before querying its min intrinsic width — "min width at
        // infinite height" is not a meaningful query on its own.
        let height = if height.is_finite() {
            height
        } else {
            ctx.child_max_intrinsic_height(0, f64::INFINITY)
        };
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let height = if height.is_finite() {
            height
        } else {
            ctx.child_max_intrinsic_height(0, f64::INFINITY)
        };
        ctx.child_max_intrinsic_width(0, height)
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        // The intrinsic height is determined by the child's max intrinsic height,
        // which is also what this widget sizes itself to.
        ctx.child_max_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
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
        // (flui-rendering ARCHITECTURE.md, dry intrinsics), routed by the driver to the memoized intrinsic_query.
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
    ) -> Option<f64> {
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
