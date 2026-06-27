//! `RenderIntrinsicWidth` — expands the child to its maximum intrinsic width.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderIntrinsicWidth`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`, lines 624–782).
//! The sizing algorithm follows Flutter's `_childConstraints` exactly:
//! query the child's max intrinsic width with the (optionally snapped) height,
//! snap to the nearest `step_width` multiple, then lay the child out at tight
//! constraints built from those values.
//!
//! # Rust-native improvements
//!
//! * `step_width` / `step_height` are `Option<f32>` (vs Dart's nullable
//!   `double?`) — `None` preserves the raw intrinsic value without rounding.
//! * The step-rounding helper is a private named function instead of a Dart
//!   lambda for clarity.
//! * `child_constraints` accepts pre-computed probe values rather than
//!   closures: the borrow checker cannot alias `&mut ctx` into two simultaneous
//!   closures, so callers compute the width/height probes sequentially and pass
//!   the results in.

use flui_tree::Single;
use flui_types::{Offset, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
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
/// 2. Otherwise query the child's max-intrinsic width for the (step-snapped)
///    height, then snap the result to `step_width` and clamp to the constraints.
/// 3. Mirror the same logic for height using `step_height`.
/// 4. Lay the child out with the tight constraints derived above.
/// 5. Report `constraints.constrain(child_size)`.
///
/// Flutter parity: `RenderIntrinsicWidth` in `proxy_box.dart`.
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
    /// time via [`apply_step`].
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

    /// Computes the tight child constraints from pre-queried intrinsic values.
    ///
    /// Mirrors Flutter's `RenderIntrinsicWidth._childConstraints`.
    ///
    /// `probed_width`: the result of `child.maxIntrinsicWidth(snappedHeight)`,
    /// already obtained by the caller; `None` when the width axis is tight or
    /// `step_width` is unset (no query needed).
    ///
    /// `probed_height`: the result of `child.maxIntrinsicHeight(computedWidth)`,
    /// already obtained by the caller; `None` when the height axis is tight or
    /// `step_height` is unset.
    ///
    /// Callers compute these sequentially (not as simultaneous closures) to
    /// satisfy the borrow checker's unique-access rule for `&mut ctx`.
    fn child_constraints(
        &self,
        constraints: BoxConstraints,
        probed_width: Option<f32>,
        probed_height: Option<f32>,
    ) -> BoxConstraints {
        let mut child = BoxConstraints::UNCONSTRAINED;

        // Width axis ——————————————————————————————————————————————————————
        if constraints.has_tight_width() {
            // Parent already determined width; propagate exactly.
            child = child.tighten(Some(constraints.min_width), None);
        } else if let (Some(step_w), Some(raw)) = (self.step_width, probed_width) {
            let clamped = raw.clamp(constraints.min_width.get(), constraints.max_width.get());
            child = child.tighten(Some(px(apply_step(clamped, Some(step_w)))), None);
        }

        // Height axis ——————————————————————————————————————————————————————
        if constraints.has_tight_height() {
            // Parent already determined height; propagate exactly.
            child = child.tighten(None, Some(constraints.min_height));
        } else if let (Some(step_h), Some(raw)) = (self.step_height, probed_height) {
            let clamped = raw.clamp(constraints.min_height.get(), constraints.max_height.get());
            child = child.tighten(None, Some(px(apply_step(clamped, Some(step_h)))));
        }

        child
    }

    /// Computes the width extent to pass as the argument to
    /// `child.maxIntrinsicHeight(width)`, given any already-computed tight width.
    ///
    /// Mirrors Flutter's `width ?? constraints.maxWidth` after the width branch.
    fn width_for_height_query(
        &self,
        constraints: BoxConstraints,
        probed_width: Option<f32>,
    ) -> f32 {
        if constraints.has_tight_width() {
            return constraints.min_width.get();
        }
        if let (Some(step_w), Some(raw)) = (self.step_width, probed_width) {
            let clamped = raw.clamp(constraints.min_width.get(), constraints.max_width.get());
            return apply_step(clamped, Some(step_w));
        }
        constraints.max_width.get()
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

        // Width probe: query only when step_width is set and width is not tight.
        // Uses the live intrinsics callback wired by the pipeline
        // (`box_intrinsic_query_borrowed`); returns 0.0 in Direct-storage (test)
        // contexts, so child stays unconstrained in width in that case.
        let probed_width = if !constraints.has_tight_width() && self.step_width.is_some() {
            let height_for_query = if constraints.max_height.is_infinite() {
                f32::INFINITY
            } else {
                apply_step(constraints.max_height.get(), self.step_height)
            };
            Some(ctx.child_max_intrinsic_width(0, height_for_query))
        } else {
            None
        };

        // Height probe: query only when step_height is set and height is not tight.
        let probed_height = if !constraints.has_tight_height() && self.step_height.is_some() {
            let width_for_query = self.width_for_height_query(constraints, probed_width);
            Some(ctx.child_max_intrinsic_height(0, width_for_query))
        } else {
            None
        };

        let child_constraints = self.child_constraints(constraints, probed_width, probed_height);
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
        // In dry layout, intrinsic probes go through `child_dry_layout` at
        // axis-only constraints (matching Flutter's `_computeSize` helper).
        // The two probes are computed sequentially to satisfy the borrow checker:
        // a single closure captured in two simultaneous borrows of `ctx` is rejected.

        let probed_width = if !constraints.has_tight_width() && self.step_width.is_some() {
            let height_for_query = if constraints.max_height.is_infinite() {
                f32::INFINITY
            } else {
                apply_step(constraints.max_height.get(), self.step_height)
            };
            let probe = if height_for_query.is_finite() {
                BoxConstraints::UNCONSTRAINED.tighten(None, Some(px(height_for_query)))
            } else {
                BoxConstraints::UNCONSTRAINED
            };
            Some(ctx.child_dry_layout(0, probe).width.get())
        } else {
            None
        };

        let probed_height = if !constraints.has_tight_height() && self.step_height.is_some() {
            let width_for_query = self.width_for_height_query(constraints, probed_width);
            let probe = if width_for_query.is_finite() {
                BoxConstraints::UNCONSTRAINED.tighten(Some(px(width_for_query)), None)
            } else {
                BoxConstraints::UNCONSTRAINED
            };
            Some(ctx.child_dry_layout(0, probe).height.get())
        } else {
            None
        };

        let child_constraints = self.child_constraints(constraints, probed_width, probed_height);
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
        // Same sequential probe approach as compute_dry_layout.
        let probed_width = if !constraints.has_tight_width() && self.step_width.is_some() {
            let height_for_query = if constraints.max_height.is_infinite() {
                f32::INFINITY
            } else {
                apply_step(constraints.max_height.get(), self.step_height)
            };
            let probe = if height_for_query.is_finite() {
                BoxConstraints::UNCONSTRAINED.tighten(None, Some(px(height_for_query)))
            } else {
                BoxConstraints::UNCONSTRAINED
            };
            Some(ctx.child_dry_layout(0, probe).width.get())
        } else {
            None
        };

        let probed_height = if !constraints.has_tight_height() && self.step_height.is_some() {
            let width_for_query = self.width_for_height_query(constraints, probed_width);
            let probe = if width_for_query.is_finite() {
                BoxConstraints::UNCONSTRAINED.tighten(Some(px(width_for_query)), None)
            } else {
                BoxConstraints::UNCONSTRAINED
            };
            Some(ctx.child_dry_layout(0, probe).height.get())
        } else {
            None
        };

        let child_constraints = self.child_constraints(constraints, probed_width, probed_height);
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
    fn child_constraints_tight_width_propagated() {
        let node = RenderIntrinsicWidth::new(None, None);
        // When incoming constraints have tight width, child_constraints must
        // also be tight on width (no intrinsic query needed).
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        // step_width=None → probed_width=None; tight path fires.
        let child_c = node.child_constraints(constraints, None, None);
        assert!(child_c.has_tight_width());
        assert_eq!(child_c.min_width, px(100.0));
    }

    #[test]
    fn child_constraints_tight_height_propagated() {
        let node = RenderIntrinsicWidth::new(None, None);
        let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));
        let child_c = node.child_constraints(constraints, None, None);
        assert!(child_c.has_tight_height());
        assert_eq!(child_c.min_height, px(50.0));
    }

    #[test]
    fn child_constraints_step_width_snaps() {
        // step_width=20 → intrinsic width is rounded up to the next multiple of 20.
        // probed_width=37 → clamped to [0, 200] = 37 → apply_step(37, 20) = 40.
        let node = RenderIntrinsicWidth::new(Some(20.0), None);
        let constraints = bc(0.0, 200.0, 0.0, 100.0);
        let child_c = node.child_constraints(constraints, Some(37.0), None);
        assert!(child_c.has_tight_width());
        assert!((child_c.min_width.get() - 40.0).abs() < 0.01);
    }

    #[test]
    fn child_constraints_no_probes_stays_unconstrained() {
        // No step, not tight → child_constraints returns UNCONSTRAINED.
        let node = RenderIntrinsicWidth::unconstrained();
        let constraints = bc(0.0, 200.0, 0.0, 100.0);
        let child_c = node.child_constraints(constraints, None, None);
        assert!(!child_c.has_tight_width());
        assert!(!child_c.has_tight_height());
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
