//! Overflow box render objects — lay child out under modified or fixed constraints.
//!
//! * [`RenderConstrainedOverflowBox`] — optional per-axis constraint overrides
//!   let the child intentionally exceed the parent's available space.
//! * [`RenderSizedOverflowBox`] — claims a fixed requested size for
//!   itself while laying the child out under the incoming constraints; the child
//!   may overflow.
//!
//! Both use [`AligningShiftedBox`] for child positioning and hit-testing.

use flui_foundation::Single;
use flui_foundation::geometry::Size;
use flui_painting::Alignment;

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
#[derive(Debug, Clone)]
pub struct RenderConstrainedOverflowBox {
    /// Per-axis constraint overrides (all optional).
    min_width: Option<f64>,
    max_width: Option<f64>,
    min_height: Option<f64>,
    max_height: Option<f64>,
    /// How to determine this box's own size relative to the parent constraints.
    fit: OverflowBoxFit,
    /// Handles child alignment and hit-testing.
    inner: AligningShiftedBox,
}

impl RenderConstrainedOverflowBox {
    /// Creates the render object with `Alignment::CENTER` and `OverflowBoxFit::Max`.
    pub fn new(
        alignment: Alignment,
        min_width: Option<f64>,
        max_width: Option<f64>,
        min_height: Option<f64>,
        max_height: Option<f64>,
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

    // --- authoritative impact setters ----------------------------------------

    /// Replaces the child alignment.
    ///
    /// Delegates to the inner shared alignment component's own setter; an
    /// alignment change is a relayout-affecting change.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.inner.set_alignment(alignment) {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Replaces the optional minimum-width override.
    pub fn set_min_width(&mut self, min_width: Option<f64>) -> flui_rendering::RenderUpdateImpact {
        if self.min_width == min_width {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.min_width = min_width;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Replaces the optional maximum-width override.
    pub fn set_max_width(&mut self, max_width: Option<f64>) -> flui_rendering::RenderUpdateImpact {
        if self.max_width == max_width {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.max_width = max_width;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Replaces the optional minimum-height override.
    pub fn set_min_height(
        &mut self,
        min_height: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.min_height == min_height {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.min_height = min_height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Replaces the optional maximum-height override.
    pub fn set_max_height(
        &mut self,
        max_height: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.max_height == max_height {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.max_height = max_height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Replaces the fit mode.
    pub fn set_fit(&mut self, fit: OverflowBoxFit) -> flui_rendering::RenderUpdateImpact {
        if self.fit == fit {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.fit = fit;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    // --- helpers -------------------------------------------------------------

    /// Computes the constraints passed to the child.
    ///
    /// Replaces each axis with the corresponding override when `Some`,
    /// otherwise keeps the parent's value.
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
            builder.add_double("min_width", v, None);
        }
        if let Some(v) = self.max_width {
            builder.add_double("max_width", v, None);
        }
        if let Some(v) = self.min_height {
            builder.add_double("min_height", v, None);
        }
        if let Some(v) = self.max_height {
            builder.add_double("max_height", v, None);
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
    // All four intrinsics delegate to the child.
    // No constraint override is applied — intrinsics are a property of the
    // child's content, independent of what constraints we pass during layout.

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_width(0, height)
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_height(0, width)
    }

    /// Dry layout uses the SAME inner (override) constraints as `perform_layout`,
    /// so FLUI's dry size always equals its laid-out size (the dry==committed
    /// invariant), so the two never diverge. Concretely, with override `maxW=50` under incoming
    /// `(0,200)` and a child intrinsic of 100, dry = 50 (== the committed
    /// layout), where passing the outer constraints would give 100.
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
    ) -> Option<f64> {
        if ctx.child_count() == 0 {
            return None;
        }
        let inner_constraints = self.inner_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, inner_constraints);
        let our_size = self.parent_size(constraints, child_size);
        let child_baseline = ctx.child_dry_baseline(0, inner_constraints, baseline)?;
        let child_offset = self.inner.dry_child_offset(our_size, child_size);
        Some(child_baseline + child_offset.dy)
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
    pub fn centered(width: f64, height: f64) -> Self {
        Self::new(Alignment::CENTER, Size::new(width, height))
    }

    /// Returns the current requested size.
    #[inline]
    pub fn requested_size(&self) -> Size {
        self.requested_size
    }

    /// Replaces the child alignment.
    ///
    /// Delegates to the inner shared alignment component's own setter; an
    /// alignment change is a relayout-affecting change.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.inner.set_alignment(alignment) {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Replaces the requested size and reports layout when changed.
    pub fn set_requested_size(
        &mut self,
        requested_size: Size,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.requested_size == requested_size {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.requested_size = requested_size;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl flui_foundation::Diagnosticable for RenderSizedOverflowBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_double("requested_width", self.requested_size.width, None);
        builder.add_double("requested_height", self.requested_size.height, None);
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
    // All four intrinsics report `requested_size` (the size this box claims
    // for itself), regardless of the child.
    // (The child is laid out under the incoming constraints and may overflow, so
    // the child's intrinsics do not describe this box's size.)

    fn compute_min_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.requested_size.width
    }

    fn compute_max_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.requested_size.width
    }

    fn compute_min_intrinsic_height(&self, _width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.requested_size.height
    }

    fn compute_max_intrinsic_height(&self, _width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.requested_size.height
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
    ) -> Option<f64> {
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
        Some(child_baseline + dry_offset.dy)
    }
}

// ===========================================================================
// Tests
// ===========================================================================
