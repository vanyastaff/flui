//! `RenderAlign` — positions a single child according to an [`Alignment`].
//!
//! Mirrors Flutter's `RenderPositionedBox` (`rendering/shifted_box.dart`).
//! Stores a **resolved** [`Alignment`] (not `AlignmentGeometry` — RTL wiring
//! is Phase 4).
//!
//! Width and height factors are optional multipliers that control how much of
//! the parent's space this object claims when an axis is unconstrained.  See
//! [`positioned_box_size`].

use flui_tree::Single;
use flui_types::{Alignment, Pixels, Size};

use super::shifted_box::AligningShiftedBox;
use crate::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

// ============================================================================
// Helper — shared by RenderAlign and RenderCenter
// ============================================================================

/// Computes the parent size for a positioned box (Align / Center).
///
/// Mirrors Flutter `RenderPositionedBox.performLayout` sizing branches:
///
/// - `shrink_width = width_factor.is_some() || max_width.is_infinite()`
///   → shrinking: `width = child_width * width_factor.unwrap_or(1.0)`;
///   → expanding: `width = Pixels::INFINITY` (clamped by `constrain`).
/// - Same logic for height.
///
/// This is the single canonical source for both live layout and dry-layout so
/// the two never drift.
pub(crate) fn positioned_box_size(
    constraints: &BoxConstraints,
    child_size: Size,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
) -> Size {
    let shrink_width = width_factor.is_some() || constraints.max_width.is_infinite();
    let shrink_height = height_factor.is_some() || constraints.max_height.is_infinite();

    let width = if shrink_width {
        child_size.width * width_factor.unwrap_or(1.0)
    } else {
        Pixels::INFINITY
    };
    let height = if shrink_height {
        child_size.height * height_factor.unwrap_or(1.0)
    } else {
        Pixels::INFINITY
    };
    constraints.constrain(Size::new(width, height))
}

/// Computes the no-child size for a positioned box.
///
/// When there is no child, Flutter uses `0` for a shrinking axis and
/// `double.infinity` for an expanding axis.
pub(crate) fn positioned_box_size_no_child(
    constraints: &BoxConstraints,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
) -> Size {
    let shrink_width = width_factor.is_some() || constraints.max_width.is_infinite();
    let shrink_height = height_factor.is_some() || constraints.max_height.is_infinite();
    constraints.constrain(Size::new(
        if shrink_width {
            Pixels::ZERO
        } else {
            Pixels::INFINITY
        },
        if shrink_height {
            Pixels::ZERO
        } else {
            Pixels::INFINITY
        },
    ))
}

// ============================================================================
// RenderAlign
// ============================================================================

/// A render object that positions its child according to an [`Alignment`].
///
/// By default expands to fill the parent in both axes.  If a factor is set,
/// or the axis is unconstrained, the object shrinks to `child_size * factor`
/// (or `child_size * 1.0 = child_size` when only the unbounded flag fires).
///
/// Factors must be `>= 0.0` (asserted in debug builds, matching Flutter).
///
/// # Flutter parity
///
/// Mirrors `RenderPositionedBox` from `rendering/shifted_box.dart`.  RTL
/// resolution (`AlignmentGeometry` + `text_direction`) is deferred to Phase 4.
#[derive(Debug, Clone)]
pub struct RenderAlign {
    inner: AligningShiftedBox,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
}

impl RenderAlign {
    /// Creates a new `RenderAlign` with the given resolved alignment.
    pub fn new(alignment: Alignment) -> Self {
        Self {
            inner: AligningShiftedBox::new(alignment),
            width_factor: None,
            height_factor: None,
        }
    }

    /// Sets a width factor (`>= 0.0`).
    ///
    /// When set, the object's width becomes `child_width * factor` rather than
    /// expanding to the parent's max width.
    #[must_use]
    pub fn with_width_factor(mut self, factor: f32) -> Self {
        debug_assert!(
            factor >= 0.0,
            "width_factor must be >= 0.0 (got {factor}); Flutter asserts the same"
        );
        self.width_factor = Some(factor);
        self
    }

    /// Sets a height factor (`>= 0.0`).
    ///
    /// When set, the object's height becomes `child_height * factor` rather
    /// than expanding to the parent's max height.
    #[must_use]
    pub fn with_height_factor(mut self, factor: f32) -> Self {
        debug_assert!(
            factor >= 0.0,
            "height_factor must be >= 0.0 (got {factor}); Flutter asserts the same"
        );
        self.height_factor = Some(factor);
        self
    }

    /// Returns the width factor.
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Returns the height factor.
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }
}

impl flui_foundation::Diagnosticable for RenderAlign {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_optional("width_factor", self.width_factor.map(|f| format!("{f:?}")));
        builder.add_optional(
            "height_factor",
            self.height_factor.map(|f| format!("{f:?}")),
        );
    }
}

impl RenderBox for RenderAlign {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            let child_size = ctx.layout_single_child_loose();
            let parent_size = positioned_box_size(
                &constraints,
                child_size,
                self.width_factor,
                self.height_factor,
            );
            self.inner.align_child(ctx, parent_size, child_size);
            self.inner.record_child_baselines(ctx);
            parent_size
        } else {
            self.inner.clear_child_baselines();
            positioned_box_size_no_child(&constraints, self.width_factor, self.height_factor)
        }
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.inner.actual_baseline(baseline)
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_width(0, height) * self.width_factor.unwrap_or(1.0)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_width(0, height) * self.width_factor.unwrap_or(1.0)
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_height(0, width) * self.height_factor.unwrap_or(1.0)
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
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
    ) -> Option<f32> {
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
        // Mirror Flutter RenderPositionedBox.computeDryBaseline:
        //   resolvedAlignment.alongOffset(size − childSize).dy + childBaseline
        let child_offset_dy = self
            .inner
            .dry_child_offset(parent_size, child_size)
            .dy
            .get();
        Some(child_baseline + child_offset_dy)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }
}
