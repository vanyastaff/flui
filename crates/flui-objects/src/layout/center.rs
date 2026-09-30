//! `RenderCenter` — centers a single child within available space.
//!
//! Delegates to [`AligningShiftedBox`] at `Alignment::CENTER` and the shared
//! [`positioned_box_size`] helper:
//!
//! - **Unbounded shrink-wrap:** no factor + `max_width = ∞` shrinks to the
//!   child width instead of returning an infinite size.
//! - **Factors are not clamped:** `with_width_factor` / `with_height_factor`
//!   only require `>= 0.0`; values above 1.0 are valid.
//!
//! # Byte-identity guarantee
//!
//! `Alignment::CENTER.along_size(size − child) == (size − child) * 0.5`
//! (IEEE-754: `x * 0.5 == x / 2.0`), so all bounded `harness_center_*` tests
//! remain green without modification.

use flui_foundation::Single;
use flui_foundation::geometry::Size;
use flui_painting::Alignment;

use crate::layout::{
    align::{positioned_box_size, positioned_box_size_no_child},
    shifted_box::AligningShiftedBox,
};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// A render object that centers its child within the available space.
///
/// The child receives loose constraints (can be any size up to the parent's
/// max), then is placed at the center of the available space.
///
/// # Example
///
/// ```ignore
/// let center = RenderCenter::new();
/// // Add a child, then layout with constraints.
/// ```
#[derive(Debug, Clone)]
pub struct RenderCenter {
    inner: AligningShiftedBox,
    /// Width factor (`>= 0.0`); if set, width = `child.width * factor`.
    width_factor: Option<f64>,
    /// Height factor (`>= 0.0`); if set, height = `child.height * factor`.
    height_factor: Option<f64>,
}

impl Default for RenderCenter {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderCenter {
    /// Creates a new center render object.
    pub fn new() -> Self {
        Self {
            inner: AligningShiftedBox::new(Alignment::CENTER),
            width_factor: None,
            height_factor: None,
        }
    }

    /// Creates a center with a width factor.
    ///
    /// The factor must be `>= 0.0`; values above 1.0 are valid.
    #[must_use]
    pub fn with_width_factor(mut self, factor: f64) -> Self {
        debug_assert!(factor >= 0.0, "width_factor must be >= 0.0 (got {factor})");
        self.width_factor = Some(factor);
        self
    }

    /// Creates a center with a height factor.
    ///
    /// The factor must be `>= 0.0`; values above 1.0 are valid.
    #[must_use]
    pub fn with_height_factor(mut self, factor: f64) -> Self {
        debug_assert!(factor >= 0.0, "height_factor must be >= 0.0 (got {factor})");
        self.height_factor = Some(factor);
        self
    }

    /// Returns the width factor.
    pub fn width_factor(&self) -> Option<f64> {
        self.width_factor
    }

    /// Returns the height factor.
    pub fn height_factor(&self) -> Option<f64> {
        self.height_factor
    }

    /// Updates widget-owned sizing factors while preserving layout caches.
    pub fn update_factors(
        &mut self,
        width_factor: Option<f64>,
        height_factor: Option<f64>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.width_factor == width_factor && self.height_factor == height_factor {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.width_factor = width_factor;
        self.height_factor = height_factor;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl flui_foundation::Diagnosticable for RenderCenter {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_optional("width_factor", self.width_factor.map(|f| format!("{f:?}")));
        builder.add_optional(
            "height_factor",
            self.height_factor.map(|f| format!("{f:?}")),
        );
    }
}

impl RenderBox for RenderCenter {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        tracing::debug!(
            "RenderCenter::perform_layout: constraints={:?}, child_count={}",
            constraints,
            ctx.child_count()
        );

        if ctx.child_count() > 0 {
            let child_size = ctx.layout_single_child_loose();

            tracing::debug!("RenderCenter: child_size={:?}", child_size);

            let parent_size = positioned_box_size(
                &constraints,
                child_size,
                self.width_factor,
                self.height_factor,
            );
            self.inner.align_child(ctx, parent_size, child_size);
            self.inner.record_child_baselines(ctx);

            tracing::debug!(
                "RenderCenter: my_size={:?}, child_offset={:?}",
                parent_size,
                self.inner.child_offset()
            );

            parent_size
        } else {
            self.inner.clear_child_baselines();
            let size =
                positioned_box_size_no_child(&constraints, self.width_factor, self.height_factor);
            tracing::debug!("RenderCenter: no child, size={:?}", size);
            size
        }
    }

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_width(0, height) * self.width_factor.unwrap_or(1.0)
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_width(0, height) * self.width_factor.unwrap_or(1.0)
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_height(0, width) * self.height_factor.unwrap_or(1.0)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_height(0, width) * self.height_factor.unwrap_or(1.0)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return positioned_box_size_no_child(
                &constraints,
                self.width_factor,
                self.height_factor,
            );
        }
        let child_size = ctx.child_dry_layout(0, constraints.loosen());
        positioned_box_size(
            &constraints,
            child_size,
            self.width_factor,
            self.height_factor,
        )
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = constraints.loosen();
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_size = ctx.child_dry_layout(0, child_constraints);
        let parent_size = positioned_box_size(
            &constraints,
            child_size,
            self.width_factor,
            self.height_factor,
        );
        // Dry baseline = the alignment's vertical offset of the child inside
        // the parent, plus the child's baseline.
        // For CENTER: along_size gives (size-child).dy * 0.5 = free_h * 0.5.
        let child_offset_dy = self.inner.dry_child_offset(parent_size, child_size).dy;
        Some(child_baseline + child_offset_dy)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> {
        self.inner.actual_baseline(baseline)
    }

    // paint() uses the default no-op — Center just positions children.

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }
}
