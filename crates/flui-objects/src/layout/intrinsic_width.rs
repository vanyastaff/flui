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
//! * `step_width` / `step_height` are `Option<f32>` (vs Dart's nullable
//!   `double?`) — `None` preserves the raw intrinsic value without rounding.
//! * The step-rounding helper is a private named function instead of a Dart
//!   lambda for clarity.
//! * `child_constraints` takes a generic `intrinsic` closure, routing the
//!   same constraint math through all three compute passes (`perform_layout`,
//!   `compute_dry_layout`, `compute_dry_baseline`) — one fact, one place.
//!   The borrow checker is satisfied because the closure captures `ctx` once
//!   and is consumed inside `child_constraints` before any subsequent ctx call.

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

// ============================================================================
// HELPERS
// ============================================================================

/// Flutter's `_applyStep`: rounds `input` up to the nearest multiple of `step`
/// when `step` is `Some`.  Returns `input` unchanged when `step` is `None`.
///
/// Mirrors `_applyStep(double input, double? step)` in `proxy_box.dart`.
#[inline]
fn apply_step(input: f32, step: Option<f32>) -> f32 {
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
    step_width: Option<f32>,
    /// Optional row-height quantum.  When set, the height extent passed to the
    /// intrinsic-width query is rounded up to the nearest multiple of this value.
    step_height: Option<f32>,
    /// True after the first successful `perform_layout` with a child present.
    has_child: bool,
}

impl RenderIntrinsicWidth {
    /// Creates the render object.
    ///
    /// Both `step_width` and `step_height` default to `None` (no snapping).
    /// Non-positive or non-finite step values are treated as `None` at layout
    /// time via `apply_step`.
    pub fn new(step_width: Option<f32>, step_height: Option<f32>) -> Self {
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
    pub fn step_width(&self) -> Option<f32> {
        self.step_width
    }

    /// Replaces the step-width quantum; returns `true` if the value changed.
    pub fn set_step_width(&mut self, step_width: Option<f32>) -> bool {
        if self.step_width == step_width {
            return false;
        }
        self.step_width = step_width;
        true
    }

    /// Returns the current step-height quantum.
    #[inline]
    pub fn step_height(&self) -> Option<f32> {
        self.step_height
    }

    /// Replaces the step-height quantum; returns `true` if the value changed.
    pub fn set_step_height(&mut self, step_height: Option<f32>) -> bool {
        if self.step_height == step_height {
            return false;
        }
        self.step_height = step_height;
        true
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
        mut intrinsic: impl FnMut(IntrinsicDimension, f32) -> f32,
    ) -> BoxConstraints {
        // Width axis — proxy_box.dart:713-715
        let width = if constraints.has_tight_width() {
            // Parent already determined width; skip the intrinsic query.
            None
        } else {
            // Always force to intrinsic (apply_step with None ≡ identity).
            // Raw query arg: constraints.max_height, not step-snapped.
            let raw = intrinsic(IntrinsicDimension::MaxWidth, constraints.max_height.get());
            Some(px(apply_step(raw, self.step_width)))
        };

        // Height axis — proxy_box.dart:716-718
        let height = if self.step_height.is_none() {
            // No step_height configured; leave the height axis unchanged.
            None
        } else {
            // Raw query arg: constraints.max_width (NOT the computed width above).
            let raw = intrinsic(IntrinsicDimension::MaxHeight, constraints.max_width.get());
            Some(px(apply_step(raw, self.step_height)))
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
    // Flutter parity: proxy_box.dart RenderIntrinsicWidth.
    // Width queries apply `apply_step` twice (height-extent snap + width snap);
    // height queries just delegate to the child unchanged.

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let snapped_height = apply_step(height, self.step_height);
        let child_min = ctx.child_min_intrinsic_width(0, snapped_height);
        apply_step(child_min, self.step_width)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let snapped_height = apply_step(height, self.step_height);
        let child_max = ctx.child_max_intrinsic_width(0, snapped_height);
        apply_step(child_max, self.step_width)
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_height(0, width)
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
        // intrinsic sub-queries through the new DryLayoutChildRequest::Intrinsic
        // channel (ADR-0011 Slice 1), routed by the driver to the same memoized
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
    ) -> Option<f32> {
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
    use flui_types::geometry::px;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    #[test]
    fn apply_step_no_step_returns_input() {
        assert_eq!(apply_step(37.0, None), 37.0);
    }

    #[test]
    fn apply_step_rounds_up() {
        // 37 / 10 = 3.7 → ceil → 4 → × 10 = 40
        assert!((apply_step(37.0, Some(10.0)) - 40.0).abs() < 0.001);
    }

    #[test]
    fn apply_step_already_multiple_unchanged() {
        assert!((apply_step(40.0, Some(10.0)) - 40.0).abs() < 0.001);
    }

    #[test]
    fn apply_step_negative_step_treated_as_none() {
        assert_eq!(apply_step(37.0, Some(-5.0)), 37.0);
    }

    #[test]
    fn new_and_setters() {
        let mut node = RenderIntrinsicWidth::new(Some(10.0), Some(5.0));
        assert_eq!(node.step_width(), Some(10.0));
        assert_eq!(node.step_height(), Some(5.0));
        assert!(node.set_step_width(Some(20.0)));
        assert!(!node.set_step_width(Some(20.0)));
        assert!(node.set_step_height(None));
        assert_eq!(node.step_height(), None);
    }

    #[test]
    fn child_constraints_tight_width_not_queried() {
        // When incoming constraints have tight width, child_constraints must
        // also be tight on width and must NOT call the intrinsic closure.
        let node = RenderIntrinsicWidth::new(None, None);
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        // Closure panics if called — verifies no intrinsic query on tight width.
        let child_c = node.child_constraints(constraints, |_, _| {
            panic!("intrinsic queried on tight width")
        });
        assert!(child_c.has_tight_width());
        assert_eq!(child_c.min_width, px(100.0));
    }

    #[test]
    fn child_constraints_tight_height_propagated() {
        let node = RenderIntrinsicWidth::new(None, None);
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        // No step_height → height axis not queried; tight height preserved.
        let child_c = node.child_constraints(constraints, |_, _| {
            panic!("intrinsic queried on tight width")
        });
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(50.0));
    }

    #[test]
    fn child_constraints_step_width_snaps() {
        // step_width=20 → intrinsic width is rounded up to the next multiple of 20.
        // Closure returns 37 for MaxWidth → apply_step(37, 20) = 40 → clamped to [0, 200].
        let node = RenderIntrinsicWidth::new(Some(20.0), None);
        let constraints = bc(0.0, 200.0, 0.0, 100.0);
        let child_c = node.child_constraints(constraints, |dim, _extent| match dim {
            IntrinsicDimension::MaxWidth => 37.0,
            _ => panic!("unexpected intrinsic dimension"),
        });
        assert!(child_c.has_tight_width());
        assert!((child_c.min_width.get() - 40.0).abs() < 0.01);
    }

    #[test]
    fn child_constraints_no_step_forces_to_raw_intrinsic() {
        // Without a step, apply_step(x, None) == x, so the child is forced to the
        // raw intrinsic (the "always-force" fix: behavior now matches Flutter when
        // step_width is None).
        let node = RenderIntrinsicWidth::unconstrained();
        let constraints = bc(0.0, 200.0, 0.0, 100.0);
        // Closure returns 120.0 for MaxWidth — must be returned as the tight width.
        let child_c = node.child_constraints(constraints, |dim, _extent| match dim {
            IntrinsicDimension::MaxWidth => 120.0,
            _ => panic!("unexpected intrinsic dimension"),
        });
        // Child should be tight at 120 (clamped to [0, 200] by tighten).
        assert!(child_c.has_tight_width());
        assert!((child_c.min_width.get() - 120.0).abs() < 0.01);
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
        assert!((child_c.min_width.get() - 35.0).abs() < 0.01);
    }

    #[test]
    fn child_constraints_height_raw_arg_is_max_width() {
        // When step_height is set, the height query uses constraints.max_width
        // (NOT the computed width). Here max_width = 80.
        let node = RenderIntrinsicWidth::new(None, Some(10.0));
        let constraints = bc(0.0, 80.0, 0.0, 200.0);
        let mut saw_height_extent = f32::NAN;
        node.child_constraints(constraints, |dim, extent| match dim {
            IntrinsicDimension::MaxWidth => 40.0, // width query
            IntrinsicDimension::MaxHeight => {
                // The extent argument must be constraints.max_width = 80.0.
                saw_height_extent = extent;
                25.0
            }
            _ => panic!("unexpected intrinsic dimension"),
        });
        assert!(
            (saw_height_extent - 80.0).abs() < 0.01,
            "height query arg should be constraints.max_width (80), got {saw_height_extent}"
        );
    }

    #[test]
    fn child_constraints_width_raw_arg_is_max_height() {
        // The width query arg is constraints.max_height (raw, un-snapped).
        // Here max_height = 300.
        let node = RenderIntrinsicWidth::new(None, None);
        let constraints = bc(0.0, 500.0, 0.0, 300.0);
        let mut saw_width_extent = f32::NAN;
        node.child_constraints(constraints, |dim, extent| match dim {
            IntrinsicDimension::MaxWidth => {
                saw_width_extent = extent;
                100.0
            }
            _ => panic!("unexpected intrinsic dimension"),
        });
        assert!(
            (saw_width_extent - 300.0).abs() < 0.01,
            "width query arg should be constraints.max_height (300), got {saw_width_extent}"
        );
    }

    #[test]
    fn intrinsics_delegate_to_ctx_for_height() {
        let node = RenderIntrinsicWidth::unconstrained();
        flui_rendering::context::intrinsics_test_support::leaf_intrinsics(|ctx| {
            // Childless → always 0.
            assert_eq!(node.compute_min_intrinsic_height(100.0, ctx), 0.0);
            assert_eq!(node.compute_max_intrinsic_height(100.0, ctx), 0.0);
        });
    }
}
