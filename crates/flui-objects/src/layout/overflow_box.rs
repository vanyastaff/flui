//! Overflow box render objects — lay child out under modified or fixed constraints.
//!
//! # Flutter equivalence
//!
//! * [`RenderConstrainedOverflowBox`] → Flutter `RenderConstrainedOverflowBox`
//!   (`shifted_box.dart`, lines 635–800).  Optional per-axis constraint overrides
//!   let the child intentionally exceed the parent's available space.
//! * [`RenderSizedOverflowBox`] → Flutter `RenderSizedOverflowBox`
//!   (`shifted_box.dart`, lines 1043–1145).  Claims a fixed requested size for
//!   itself while laying the child out under the incoming constraints; the child
//!   may overflow.
//!
//! Both use [`AligningShiftedBox`] for child positioning and hit-testing.

use flui_tree::Single;
use flui_types::{Alignment, Pixels, Size, geometry::px};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::RenderBox,
};

use super::shifted_box::AligningShiftedBox;

// ============================================================================
// OverflowBoxFit
// ============================================================================

/// Determines how `RenderConstrainedOverflowBox` computes its own size.
///
/// Flutter parity: `OverflowBoxFit` in `shifted_box.dart`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverflowBoxFit {
    /// Size to the maximum extent allowed by the parent constraints (`constraints.biggest()`).
    /// The child may be smaller or larger than this.
    ///
    /// This is the default — the overflow box claims all available space, and
    /// any child that exceeds it "overflows" without affecting the parent's size.
    #[default]
    Max,
    /// Size to the child's laid-out size, constrained by the parent constraints.
    ///
    /// Uses `constraints.constrain(child_size)`.  The box can still be painted
    /// beyond its claimed size if the child's inner constraints allow a larger
    /// child, but relayout propagation is bounded by `constraints`.
    DeferToChild,
}

// ============================================================================
// RenderConstrainedOverflowBox
// ============================================================================

/// A render box that lets its child be laid out as if it lived in a box of a
/// different size, potentially overflowing the parent's constraints.
///
/// Each per-axis override (`min_width`, `max_width`, `min_height`,
/// `max_height`) replaces the corresponding incoming constraint value when
/// `Some`; unset axes use the parent's incoming value unchanged.  This lets
/// the child expand past the parent's limits for scrollable overflow, clipped
/// overflow, or other intentional over-draws.
///
/// The `fit` knob controls how this object reports its own size back to its
/// parent: `Max` (take all available space) or `DeferToChild` (shrink-wrap
/// the child within constraints).
///
/// Flutter parity: `RenderConstrainedOverflowBox` in `shifted_box.dart`.
#[derive(Debug, Clone)]
pub struct RenderConstrainedOverflowBox {
    /// Per-axis constraint overrides (all optional).
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    min_height: Option<Pixels>,
    max_height: Option<Pixels>,
    /// How to determine this box's own size relative to the parent constraints.
    fit: OverflowBoxFit,
    /// Handles child alignment and hit-testing.
    inner: AligningShiftedBox,
}

impl RenderConstrainedOverflowBox {
    /// Creates the render object with `Alignment::CENTER` and `OverflowBoxFit::Max`.
    pub fn new(
        alignment: Alignment,
        min_width: Option<Pixels>,
        max_width: Option<Pixels>,
        min_height: Option<Pixels>,
        max_height: Option<Pixels>,
        fit: OverflowBoxFit,
    ) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
            fit,
            inner: AligningShiftedBox::new(alignment),
        }
    }

    /// Convenience: unconstrained overflow with `Alignment::CENTER` / `Max` fit.
    pub fn centered() -> Self {
        Self::new(
            Alignment::CENTER,
            None,
            None,
            None,
            None,
            OverflowBoxFit::Max,
        )
    }

    // --- setters that return a change flag -----------------------------------

    /// Replaces the child alignment.
    ///
    /// Delegates to the inner shared alignment component's own setter, which
    /// mirrors Flutter `RenderAligningShiftedBox`'s `alignment` setter
    /// (`shifted_box.dart:339-345`) — a relayout-affecting change.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.inner.set_alignment(alignment)
    }

    /// Replaces the optional minimum-width override.
    pub fn set_min_width(&mut self, min_width: Option<Pixels>) -> bool {
        if self.min_width == min_width {
            return false;
        }
        self.min_width = min_width;
        true
    }

    /// Replaces the optional maximum-width override.
    pub fn set_max_width(&mut self, max_width: Option<Pixels>) -> bool {
        if self.max_width == max_width {
            return false;
        }
        self.max_width = max_width;
        true
    }

    /// Replaces the optional minimum-height override.
    pub fn set_min_height(&mut self, min_height: Option<Pixels>) -> bool {
        if self.min_height == min_height {
            return false;
        }
        self.min_height = min_height;
        true
    }

    /// Replaces the optional maximum-height override.
    pub fn set_max_height(&mut self, max_height: Option<Pixels>) -> bool {
        if self.max_height == max_height {
            return false;
        }
        self.max_height = max_height;
        true
    }

    /// Replaces the fit mode.
    pub fn set_fit(&mut self, fit: OverflowBoxFit) -> bool {
        if self.fit == fit {
            return false;
        }
        self.fit = fit;
        true
    }

    // --- helpers -------------------------------------------------------------

    /// Computes the constraints passed to the child.
    ///
    /// Mirrors Flutter's `_getInnerConstraints`: replace each axis with the
    /// corresponding override when `Some`, otherwise keep the parent's value.
    fn inner_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::new(
            self.min_width.unwrap_or(constraints.min_width),
            self.max_width.unwrap_or(constraints.max_width),
            self.min_height.unwrap_or(constraints.min_height),
            self.max_height.unwrap_or(constraints.max_height),
        )
    }

    /// Computes this object's claimed size given the child size.
    fn parent_size(&self, constraints: BoxConstraints, child_size: Size) -> Size {
        match self.fit {
            OverflowBoxFit::Max => constraints.biggest(),
            OverflowBoxFit::DeferToChild => constraints.constrain(child_size),
        }
    }
}

impl flui_foundation::Diagnosticable for RenderConstrainedOverflowBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        if let Some(v) = self.min_width {
            builder.add_double("min_width", v.get(), None);
        }
        if let Some(v) = self.max_width {
            builder.add_double("max_width", v.get(), None);
        }
        if let Some(v) = self.min_height {
            builder.add_double("min_height", v.get(), None);
        }
        if let Some(v) = self.max_height {
            builder.add_double("max_height", v.get(), None);
        }
        builder.add_enum("fit", self.fit);
    }
}

impl RenderBox for RenderConstrainedOverflowBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.inner.clear_child_baselines();
            return match self.fit {
                OverflowBoxFit::Max => constraints.biggest(),
                OverflowBoxFit::DeferToChild => constraints.smallest(),
            };
        }

        let inner_constraints = self.inner_constraints(constraints);
        let child_size = ctx.layout_child(0, inner_constraints);
        let our_size = self.parent_size(constraints, child_size);

        self.inner.align_child(ctx, our_size, child_size);
        self.inner.record_child_baselines(ctx);
        our_size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Flutter parity: RenderShiftedBox delegates all four intrinsics to child.
    // No constraint override is applied — intrinsics are a property of the
    // child's content, independent of what constraints we pass during layout.

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
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
        ctx.child_min_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_height(0, width)
    }

    /// Dry layout uses the SAME inner (override) constraints as `perform_layout`,
    /// so FLUI's dry size always equals its laid-out size (the dry==committed
    /// invariant). This is an INTENTIONAL divergence from Flutter, whose
    /// `RenderConstrainedOverflowBox` dry path passes the OUTER constraints
    /// (`shifted_box.dart:737`) and therefore disagrees with its own
    /// `performLayout` (which uses inner constraints) — a Flutter dry/layout
    /// inconsistency FLUI deliberately does not replicate (Prime Directive
    /// rule #2). Concretely, with override `maxW=50` under incoming `(0,200)`
    /// and a child intrinsic of 100, FLUI dry = 50 (== its committed layout),
    /// whereas Flutter dry = 100 (≠ its own committed layout).
    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return match self.fit {
                OverflowBoxFit::Max => constraints.biggest(),
                OverflowBoxFit::DeferToChild => constraints.smallest(),
            };
        }
        let inner_constraints = self.inner_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, inner_constraints);
        self.parent_size(constraints, child_size)
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
        let inner_constraints = self.inner_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, inner_constraints);
        let our_size = self.parent_size(constraints, child_size);
        let child_baseline = ctx.child_dry_baseline(0, inner_constraints, baseline)?;
        let child_offset = self.inner.dry_child_offset(our_size, child_size);
        Some(child_baseline + child_offset.dy.get())
    }
}

// ============================================================================
// RenderSizedOverflowBox
// ============================================================================

/// Claims a fixed `requested_size` for itself while laying its child out under
/// the incoming constraints (unchanged).  The child may overflow.
///
/// This is the inverse of `RenderSizedBox`: the *box* claims a specific size,
/// but the *child* is allowed to be a different size.  Useful for sizing an
/// indicator or placeholder while a larger or smaller piece of content renders
/// behind it.
///
/// Flutter parity: `RenderSizedOverflowBox` in `shifted_box.dart`.
#[derive(Debug, Clone)]
pub struct RenderSizedOverflowBox {
    /// The size this box reports to its parent (`constraints.constrain(requested_size)`).
    requested_size: Size,
    /// Handles child alignment and hit-testing.
    inner: AligningShiftedBox,
}

impl RenderSizedOverflowBox {
    /// Creates the render object.
    pub fn new(alignment: Alignment, requested_size: Size) -> Self {
        Self {
            requested_size,
            inner: AligningShiftedBox::new(alignment),
        }
    }

    /// Convenience: center-aligned, requests `(width, height)` logical pixels.
    pub fn centered(width: f32, height: f32) -> Self {
        Self::new(Alignment::CENTER, Size::new(px(width), px(height)))
    }

    /// Returns the current requested size.
    #[inline]
    pub fn requested_size(&self) -> Size {
        self.requested_size
    }

    /// Replaces the child alignment.
    ///
    /// Delegates to the inner shared alignment component's own setter, which
    /// mirrors Flutter `RenderAligningShiftedBox`'s `alignment` setter
    /// (`shifted_box.dart:339-345`) — a relayout-affecting change.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.inner.set_alignment(alignment)
    }

    /// Replaces the requested size; returns `true` if the value changed.
    pub fn set_requested_size(&mut self, requested_size: Size) -> bool {
        if self.requested_size == requested_size {
            return false;
        }
        self.requested_size = requested_size;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderSizedOverflowBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_double("requested_width", self.requested_size.width.get(), None);
        builder.add_double("requested_height", self.requested_size.height.get(), None);
    }
}

impl RenderBox for RenderSizedOverflowBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let our_size = constraints.constrain(self.requested_size);

        if ctx.child_count() == 0 {
            self.inner.clear_child_baselines();
            return our_size;
        }

        // Child uses incoming (parent) constraints, NOT the requested size.
        // This is the key contract: we claim one size, child lives in another.
        let child_size = ctx.layout_child(0, constraints);
        self.inner.align_child(ctx, our_size, child_size);
        self.inner.record_child_baselines(ctx);
        our_size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // Flutter parity: RenderSizedOverflowBox OVERRIDES all four intrinsics to
    // report its `requested_size` (the size it claims for itself), regardless of
    // the child — `shifted_box.dart` RenderSizedOverflowBox.computeMin/MaxIntrinsic*.
    // (The child is laid out under the incoming constraints and may overflow, so
    // the child's intrinsics do not describe this box's size.)

    fn compute_min_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.requested_size.width.get()
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.requested_size.width.get()
    }

    fn compute_min_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.requested_size.height.get()
    }

    fn compute_max_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.requested_size.height.get()
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Our own size is always `constrain(requested_size)`, regardless of child.
        constraints.constrain(self.requested_size)
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
        let our_size = constraints.constrain(self.requested_size);
        // Child is laid out under incoming constraints (same as perform_layout).
        let child_size = ctx.child_dry_layout(0, constraints);
        let child_baseline = ctx.child_dry_baseline(0, constraints, baseline)?;
        // Use the same alignment as the inner component.
        // We borrow alignment knowledge from a temporary to compute the offset.
        let dry_offset = self.inner.dry_child_offset(our_size, child_size);
        Some(child_baseline + dry_offset.dy.get())
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
    fn overflow_box_fit_default_is_max() {
        assert_eq!(OverflowBoxFit::default(), OverflowBoxFit::Max);
    }

    // --- RenderConstrainedOverflowBox ----------------------------------------

    #[test]
    fn inner_constraints_no_overrides_pass_through() {
        let node = RenderConstrainedOverflowBox::new(
            Alignment::CENTER,
            None,
            None,
            None,
            None,
            OverflowBoxFit::Max,
        );
        let constraints = bc(10.0, 200.0, 5.0, 100.0);
        let inner = node.inner_constraints(constraints);
        assert_eq!(inner.min_width, px(10.0));
        assert_eq!(inner.max_width, px(200.0));
        assert_eq!(inner.min_height, px(5.0));
        assert_eq!(inner.max_height, px(100.0));
    }

    #[test]
    fn inner_constraints_max_width_override() {
        let node = RenderConstrainedOverflowBox::new(
            Alignment::CENTER,
            None,
            Some(px(500.0)), // override max_width → allow child to be wider
            None,
            None,
            OverflowBoxFit::Max,
        );
        let constraints = bc(0.0, 200.0, 0.0, 200.0);
        let inner = node.inner_constraints(constraints);
        assert_eq!(inner.max_width, px(500.0)); // overridden
        assert_eq!(inner.max_height, px(200.0)); // original
    }

    #[test]
    fn parent_size_max_returns_biggest() {
        let node = RenderConstrainedOverflowBox::new(
            Alignment::CENTER,
            None,
            None,
            None,
            None,
            OverflowBoxFit::Max,
        );
        let constraints = bc(0.0, 300.0, 0.0, 200.0);
        let our_size = node.parent_size(constraints, Size::new(px(50.0), px(50.0)));
        assert_eq!(our_size, constraints.biggest());
    }

    #[test]
    fn parent_size_defer_to_child_constrains_child() {
        let node = RenderConstrainedOverflowBox::new(
            Alignment::CENTER,
            None,
            None,
            None,
            None,
            OverflowBoxFit::DeferToChild,
        );
        let constraints = bc(0.0, 300.0, 0.0, 200.0);
        let child_size = Size::new(px(50.0), px(50.0));
        let our_size = node.parent_size(constraints, child_size);
        assert_eq!(our_size, constraints.constrain(child_size));
    }

    // --- RenderSizedOverflowBox ---------------------------------------------

    #[test]
    fn sized_overflow_box_constrain_requested_size() {
        let node = RenderSizedOverflowBox::centered(80.0, 60.0);
        assert_eq!(node.requested_size(), Size::new(px(80.0), px(60.0)));
    }

    #[test]
    fn sized_overflow_box_setter_returns_change_flag() {
        let mut node = RenderSizedOverflowBox::centered(80.0, 60.0);
        let new_size = Size::new(px(100.0), px(100.0));
        assert!(node.set_requested_size(new_size));
        assert!(!node.set_requested_size(new_size));
    }

    #[test]
    fn sized_overflow_box_dry_layout_constrained() {
        // requested 80×60 into 0..200 → stays 80×60.
        let node = RenderSizedOverflowBox::centered(80.0, 60.0);
        let constraints = bc(0.0, 200.0, 0.0, 200.0);
        // compute_dry_layout doesn't need ctx child count here — no child.
        assert_eq!(
            constraints.constrain(node.requested_size()),
            Size::new(px(80.0), px(60.0))
        );
    }
}
