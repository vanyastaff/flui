//! `RenderIntrinsicWidth` — expands the child to its maximum intrinsic width.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderIntrinsicWidth`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`, lines 624–782).
//! The sizing algorithm follows Flutter's `_childConstraints` exactly:
//! query the child's max intrinsic width with the incoming (raw, un-snapped)
//! height, snap the result to the nearest `step_width` multiple, then lay the
//! child out at tight constraints built from those values.  When the parent's
//! width is already tight, the intrinsic width is not queried.  When
//! `step_height` is set, the child's max intrinsic height is queried using the
//! raw `constraints.max_width` (not the computed step-snapped width), and the
//! result is snapped and tightened on the height axis.
//!
//! # Rust-native improvements
//!
//! * `step_width` / `step_height` are `Option<f64>` (vs Dart's nullable
//!   `double?`) — `None` preserves the raw intrinsic value without rounding.
//! * The step-rounding helper is a private named function instead of a Dart
//!   lambda for clarity.
//! * `child_constraints` takes a generic `intrinsic` closure, routing the
//!   same constraint math through all three compute passes (`perform_layout`,
//!   `compute_dry_layout`, `compute_dry_baseline`) — one fact, one place.
//!   The borrow checker is satisfied because the closure captures `ctx` once
//!   and is consumed inside `child_constraints` before any subsequent ctx call.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::BoxParentData,
    storage::IntrinsicDimension,
    traits::RenderBox,
};

// ============================================================================
// HELPERS
// ============================================================================

/// Flutter's `_applyStep`: rounds `input` up to the nearest multiple of `step`
/// when `step` is `Some`.  Returns `input` unchanged when `step` is `None`.
///
/// Mirrors `_applyStep(double input, double? step)` in `proxy_box.dart`.
#[inline]
fn apply_step(input: f64, step: Option<f64>) -> f64 {
    match step {
        None => input,
        Some(s) if s <= 0.0 || !s.is_finite() => input,
        Some(s) => (input / s).ceil() * s,
    }
}

// ============================================================================
// RENDER OBJECT
// ============================================================================

/// Sizes itself to the child's maximum intrinsic width, optionally snapped to
/// a step grid.
///
/// Useful when a widget should be exactly as wide as its natural content, not
/// merely constrained by it.  The optional `step_width` and `step_height` knobs
/// round up to the nearest multiple so that adjacent widgets snap to a common
/// size grid, reducing relayout churn in dynamic lists.
///
/// # Layout contract
///
/// 1. If the parent's width is already tight, use it directly (no intrinsic query).
/// 2. Otherwise query the child's max-intrinsic width for the raw `max_height`,
///    then snap the result to `step_width` and clamp to the constraints.
/// 3. If `step_height` is set, query the child's max-intrinsic height for the
///    raw `max_width` (not the computed width), snap to `step_height`, and
///    clamp to the constraints.
/// 4. Lay the child out with the tight constraints derived above.
/// 5. Report `constraints.constrain(child_size)`.
///
/// Flutter parity: `RenderIntrinsicWidth` in `proxy_box.dart`, including
/// `_childConstraints` (proxy_box.dart:712-720) and `_computeSize`
/// (proxy_box.dart:723-734).
#[derive(Debug, Clone)]
pub struct RenderIntrinsicWidth {
    /// Optional column-width quantum.  When set, the computed intrinsic width
    /// is rounded up to the nearest multiple of this value.
    step_width: Option<f64>,
    /// Optional row-height quantum.  When set, the height extent passed to the
    /// intrinsic-width query is rounded up to the nearest multiple of this value.
    step_height: Option<f64>,
    /// True after the first successful `perform_layout` with a child present.
    has_child: bool,
}

impl RenderIntrinsicWidth {
    /// Creates the render object.
    ///
    /// Both `step_width` and `step_height` default to `None` (no snapping).
    /// Non-positive or non-finite step values are treated as `None` at layout
    /// time via `apply_step`.
    pub fn new(step_width: Option<f64>, step_height: Option<f64>) -> Self {
        Self {
            step_width,
            step_height,
            has_child: false,
        }
    }

    /// Convenience constructor with no step snapping.
    pub fn unconstrained() -> Self {
        Self::new(None, None)
    }

    /// Returns the current step-width quantum.
    #[inline]
    pub fn step_width(&self) -> Option<f64> {
        self.step_width
    }

    /// Replaces the step-width quantum and reports layout when changed.
    pub fn set_step_width(
        &mut self,
        step_width: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.step_width == step_width {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.step_width = step_width;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Returns the current step-height quantum.
    #[inline]
    pub fn step_height(&self) -> Option<f64> {
        self.step_height
    }

    /// Replaces the step-height quantum and reports layout when changed.
    pub fn set_step_height(
        &mut self,
        step_height: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.step_height == step_height {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.step_height = step_height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Computes the tight child constraints using an `intrinsic` closure.
    ///
    /// Mirrors Flutter's `RenderIntrinsicWidth._childConstraints`
    /// (proxy_box.dart:712-720) exactly:
    ///
    /// - **Width axis**: if the incoming width is already tight, keep it.
    ///   Otherwise call `intrinsic(MaxWidth, constraints.max_height)` with the
    ///   RAW `max_height` (not step-snapped), apply `step_width`, and tighten.
    ///   `apply_step(x, None) == x`, so the no-step case forces the child to
    ///   its raw intrinsic width (the core behavioral fix vs. the old code that
    ///   only forced when `step_width.is_some()`).
    ///
    /// - **Height axis**: if `step_height` is `None`, keep the incoming height.
    ///   Otherwise call `intrinsic(MaxHeight, constraints.max_width)` with the
    ///   RAW `max_width` (not the computed step-snapped width), apply
    ///   `step_height`, and tighten.
    ///
    /// FLUI's `BoxConstraints::tighten` clamps the argument to `[min, max]`,
    /// so the ordering is step → tighten(clamp) — matching Flutter's
    /// step-then-clamp contract.
    ///
    /// The `intrinsic` closure is called at most twice — once for each non-tight
    /// axis that needs forcing — and is consumed by this method.  Callers pass
    /// `|dim, extent| ctx.child_intrinsic(0, dim, extent)` for all three compute
    /// passes; only the ctx type differs.
    ///
    /// # Note on `#[cfg]`-gated Direct-storage paths
    ///
    /// In a Direct-storage (test) context without a live pipeline,
    /// `ctx.child_intrinsic` returns `0.0` as a conservative fallback.  The
    /// resulting forced width/height will be `0.0` (or the `apply_step` of it).
    /// Harness tests that need accurate intrinsic values go through
    /// `PipelineOwner::box_dry_layout` / `run.dry_layout()` — those use the
    /// real memoized `intrinsic_query`, which IS accurate.
    fn child_constraints(
        &self,
        constraints: BoxConstraints,
        mut intrinsic: impl FnMut(IntrinsicDimension, f64) -> f64,
    ) -> BoxConstraints {
        // Width axis — proxy_box.dart:713-715
        let width = if constraints.has_tight_width() {
            // Parent already determined width; skip the intrinsic query.
            None
        } else {
            // Always force to intrinsic (apply_step with None ≡ identity).
            // Raw query arg: constraints.max_height, not step-snapped.
            let raw = intrinsic(IntrinsicDimension::MaxWidth, constraints.max_height);
            Some(apply_step(raw, self.step_width))
        };

        // Height axis — proxy_box.dart:716-718
        let height = if self.step_height.is_none() {
            // No step_height configured; leave the height axis unchanged.
            None
        } else {
            // Raw query arg: constraints.max_width (NOT the computed width above).
            let raw = intrinsic(IntrinsicDimension::MaxHeight, constraints.max_width);
            Some(apply_step(raw, self.step_height))
        };

        // tighten clamps to [min, max]: step-then-clamp, matching Flutter.
        constraints.tighten(width, height)
    }
}

impl flui_foundation::Diagnosticable for RenderIntrinsicWidth {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        if let Some(sw) = self.step_width {
            builder.add_double("step_width", sw, None);
        }
        if let Some(sh) = self.step_height {
            builder.add_double("step_height", sh, None);
        }
    }
}

impl RenderBox for RenderIntrinsicWidth {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            return constraints.smallest();
        }
        self.has_child = true;

        // `child_constraints` queries the child's intrinsics through the live
        // `box_intrinsic_query_borrowed` callback, which is the same memoized
        // walk the dry paths now use — real and dry layout are structurally
        // identical here.
        let child_constraints = self.child_constraints(constraints, |dim, extent| {
            ctx.child_intrinsic(0, dim, extent)
        });
        let child_size = ctx.layout_child(0, child_constraints);
        ctx.position_child(0, Offset::ZERO);
        constraints.constrain(child_size)
    }

    flui_rendering::forward_single_child_box_hit_test!();

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Flutter parity: proxy_box.dart RenderIntrinsicWidth.
    //
    // * `computeMinIntrinsicWidth(height) => getMaxIntrinsicWidth(height)` —
    //   the min-width query delegates to the max-width query verbatim instead
    //   of asking the child for its own min. Forcing the child to a single
    //   width (the max intrinsic) is the entire point of this render object,
    //   so its own reported min and max width are always equal.
    // * `computeMaxIntrinsicWidth(height)` queries the child's max intrinsic
    //   width at the RAW `height` — no `step_height` snapping here; stepping
    //   only tightens the layout height axis (`child_constraints`), not this
    //   intrinsic query — then applies `step_width`.
    // * `computeMinIntrinsicHeight`/`computeMaxIntrinsicHeight(width)` resolve
    //   an infinite `width` to this object's own forced max intrinsic width
    //   first (proxy_box.dart: `if (!width.isFinite) { width =
    //   getMaxIntrinsicWidth(double.infinity); }`), then query the child's
    //   min/max intrinsic height at that resolved width and apply `step_height`.

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.compute_max_intrinsic_width(height, ctx)
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let child_max = ctx.child_max_intrinsic_width(0, height);
        apply_step(child_max, self.step_width)
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let width = if width.is_finite() {
            width
        } else {
            self.compute_max_intrinsic_width(f64::INFINITY, ctx)
        };
        let child_min = ctx.child_min_intrinsic_height(0, width);
        apply_step(child_min, self.step_height)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let width = if width.is_finite() {
            width
        } else {
            self.compute_max_intrinsic_width(f64::INFINITY, ctx)
        };
        let child_max = ctx.child_max_intrinsic_height(0, width);
        apply_step(child_max, self.step_height)
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
        // intrinsic sub-queries through the new DryLayoutChildRequest::Intrinsic
        // channel (flui-rendering ARCHITECTURE.md, dry intrinsics), routed by the driver to the same memoized
        // intrinsic_query — dry ≡ committed.
        let child_constraints = self.child_constraints(constraints, |dim, extent| {
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
        let child_constraints = self.child_constraints(constraints, |dim, extent| {
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

    fn bc(min_w: f64, max_w: f64, min_h: f64, max_h: f64) -> BoxConstraints {
        BoxConstraints::new(min_w, max_w, min_h, max_h)
    }

    #[test]
    fn apply_step_rounds_up() {
        // 37 / 10 = 3.7 → ceil → 4 → × 10 = 40
        assert!((apply_step(37.0, Some(10.0)) - 40.0).abs() < 0.001);
    }

    #[test]
    fn child_constraints_step_then_clamp_ordering() {
        // Step-then-clamp: apply_step(raw, step) first, then tighten clamps.
        // Intrinsic = 37, step_width = 20 → step gives 40.
        // max_width = 35 → clamp(40, [0, 35]) = 35.
        let node = RenderIntrinsicWidth::new(Some(20.0), None);
        let constraints = bc(0.0, 35.0, 0.0, 100.0);
        let child_c = node.child_constraints(constraints, |dim, _extent| match dim {
            IntrinsicDimension::MaxWidth => 37.0,
            _ => panic!("unexpected intrinsic dimension"),
        });
        assert!(child_c.has_tight_width());
        assert!((child_c.min_width - 35.0).abs() < 0.01);
    }
}
