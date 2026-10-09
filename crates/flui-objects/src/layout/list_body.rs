//! `RenderListBody` — lays children sequentially along one axis.

use flui_foundation::Variable;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Offset, Size};
use flui_rendering::constraints::{
    AxisDirection,
    AxisDirection::{BottomToTop, LeftToRight, RightToLeft, TopToBottom},
};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::ListBodyParentData,
    traits::{RenderBox, TextBaseline},
};

/// Maps a [`TextBaseline`] kind into compact per-kind storage.
#[inline]
const fn baseline_kind_index(baseline: TextBaseline) -> usize {
    match baseline {
        TextBaseline::Alphabetic => 0,
        TextBaseline::Ideographic => 1,
    }
}

/// A multi-child box that stretches children in the cross axis and places them
/// sequentially along [`axis_direction`](Self::axis_direction).
///
/// `RenderListBody` expects unlimited space along its main axis
/// and a bounded cross axis; it does not clip or resize overflow in the main
/// axis.
#[derive(Debug, Clone)]
pub struct RenderListBody {
    axis_direction: AxisDirection,
    child_count: usize,
    /// Baselines recorded during layout using the first-child-in-list rule.
    reported_baselines: [Option<f64>; 2],
}

impl RenderListBody {
    /// Creates a vertical top-to-bottom list body.
    pub const fn new() -> Self {
        Self::with_axis_direction(TopToBottom)
    }

    /// Creates a list body with the given axis direction.
    pub const fn with_axis_direction(axis_direction: AxisDirection) -> Self {
        Self {
            axis_direction,
            child_count: 0,
            reported_baselines: [None; 2],
        }
    }

    /// The direction in which children are laid out.
    #[must_use]
    pub const fn axis_direction(&self) -> AxisDirection {
        self.axis_direction
    }

    /// Updates the axis direction and reports layout when changed.
    pub fn set_axis_direction(
        &mut self,
        axis_direction: AxisDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.axis_direction == axis_direction {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.axis_direction = axis_direction;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    fn main_axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    fn debug_check_constraints(&self, constraints: BoxConstraints) {
        match self.main_axis() {
            Axis::Horizontal => {
                debug_assert!(
                    !constraints.has_bounded_width(),
                    "RenderListBody must have unlimited space along its main axis",
                );
                debug_assert!(
                    constraints.has_bounded_height(),
                    "RenderListBody must have a bounded cross-axis constraint",
                );
            }
            Axis::Vertical => {
                debug_assert!(
                    !constraints.has_bounded_height(),
                    "RenderListBody must have unlimited space along its main axis",
                );
                debug_assert!(
                    constraints.has_bounded_width(),
                    "RenderListBody must have a bounded cross-axis constraint",
                );
            }
        }
    }

    fn child_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        match self.main_axis() {
            Axis::Horizontal => BoxConstraints::tight_for(None, Some(constraints.max_height)),
            Axis::Vertical => BoxConstraints::tight_for(Some(constraints.max_width), None),
        }
    }

    fn child_main_extent(&self, size: Size) -> f64 {
        match self.main_axis() {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        }
    }

    fn constrain_size(&self, constraints: BoxConstraints, main_extent: f64) -> Size {
        match self.main_axis() {
            Axis::Horizontal => {
                constraints.constrain(Size::new(main_extent, constraints.max_height))
            }
            Axis::Vertical => constraints.constrain(Size::new(constraints.max_width, main_extent)),
        }
    }

    fn dry_size(
        &self,
        constraints: BoxConstraints,
        child_count: usize,
        mut measure: impl FnMut(usize, BoxConstraints) -> flui_rendering::RenderResult<Size>,
    ) -> flui_rendering::RenderResult<Size> {
        self.debug_check_constraints(constraints);
        let child_constraints = self.child_constraints(constraints);
        let mut main_extent = 0.0;
        for i in 0..child_count {
            main_extent += self.child_main_extent(measure(i, child_constraints)?);
        }
        Ok(self.constrain_size(constraints, main_extent))
    }

    fn horizontal_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f64,
        mut child_query: impl FnMut(
            &mut BoxIntrinsicsCtx<'_>,
            usize,
            f64,
        ) -> flui_rendering::RenderResult<f64>,
    ) -> flui_rendering::RenderResult<f64> {
        match self.main_axis() {
            Axis::Horizontal => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .sum(),
            Axis::Vertical => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .try_fold(0.0_f64, |max, value| Ok(max.max(value?))),
        }
    }

    fn vertical_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f64,
        mut child_query: impl FnMut(
            &mut BoxIntrinsicsCtx<'_>,
            usize,
            f64,
        ) -> flui_rendering::RenderResult<f64>,
    ) -> flui_rendering::RenderResult<f64> {
        match self.main_axis() {
            Axis::Horizontal => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .sum(),
            Axis::Vertical => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .try_fold(0.0_f64, |max, value| Ok(max.max(value?))),
        }
    }
}

impl Default for RenderListBody {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderListBody {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("axis_direction", self.axis_direction);
    }
}

impl RenderBox for RenderListBody {
    type Arity = Variable;
    type ParentData = ListBodyParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        self.debug_check_constraints(constraints);
        self.child_count = ctx.child_count();
        self.reported_baselines = [None; 2];

        let child_constraints = self.child_constraints(constraints);
        let mut child_sizes = Vec::with_capacity(self.child_count);
        let mut main_extent = 0.0;

        for i in 0..self.child_count {
            let size = ctx.layout_child(i, child_constraints)?;
            main_extent += self.child_main_extent(size);
            child_sizes.push(size);
        }

        let size = self.constrain_size(constraints, main_extent);
        let mut forward_position = 0.0;
        for (i, child_size) in child_sizes.iter().copied().enumerate() {
            let child_extent = self.child_main_extent(child_size);
            let offset = match self.axis_direction {
                LeftToRight => Offset::new(forward_position, 0.0),
                TopToBottom => Offset::new(0.0, forward_position),
                RightToLeft => Offset::new(main_extent - forward_position - child_extent, 0.0),
                BottomToTop => Offset::new(0.0, main_extent - forward_position - child_extent),
            };
            ctx.position_child(i, offset);
            forward_position += child_extent;

            for kind in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
                let slot = baseline_kind_index(kind);
                if self.reported_baselines[slot].is_none() {
                    self.reported_baselines[slot] = ctx
                        .child_distance_to_actual_baseline(i, kind)?
                        .map(|baseline| baseline + offset.dy);
                }
            }
        }

        Ok(size)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        self.dry_size(constraints, ctx.child_count(), |i, c| {
            ctx.child_dry_layout(i, c)
        })
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.horizontal_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.horizontal_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.vertical_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.vertical_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        self.debug_check_constraints(constraints);
        let child_constraints = self.child_constraints(constraints);
        match self.axis_direction {
            LeftToRight | RightToLeft => {
                let mut result: Option<f64> = None;
                for i in 0..ctx.child_count() {
                    if let Some(child_baseline) =
                        ctx.child_dry_baseline(i, child_constraints, baseline)?
                    {
                        result = Some(result.map_or(child_baseline, |v| v.min(child_baseline)));
                    }
                }
                Ok(result)
            }
            TopToBottom | BottomToTop => {
                if self.axis_direction == TopToBottom {
                    let mut main_extent = 0.0;
                    for i in 0..ctx.child_count() {
                        if let Some(child_baseline) =
                            ctx.child_dry_baseline(i, child_constraints, baseline)?
                        {
                            return Ok(Some(child_baseline + main_extent));
                        }
                        main_extent +=
                            self.child_main_extent(ctx.child_dry_layout(i, child_constraints)?);
                    }
                } else {
                    let mut main_extent = 0.0;
                    for i in (0..ctx.child_count()).rev() {
                        if let Some(child_baseline) =
                            ctx.child_dry_baseline(i, child_constraints, baseline)?
                        {
                            return Ok(Some(child_baseline + main_extent));
                        }
                        main_extent +=
                            self.child_main_extent(ctx.child_dry_layout(i, child_constraints)?);
                    }
                }
                Ok(None)
            }
        }
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        Ok(self.reported_baselines[baseline_kind_index(baseline)])
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        ctx.paint_children();
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        for i in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}
