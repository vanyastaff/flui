//! `RenderListBody` — lays children sequentially along one axis.
//!
//! Flutter parity: `rendering/list_body.dart` `RenderListBody`.

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::ListBodyParentData,
    traits::{RenderBox, TextBaseline},
};
use flui_tree::Variable;
use flui_types::{
    Axis, Offset, Pixels, Size,
    geometry::px,
    layout::AxisDirection,
    layout::AxisDirection::{BottomToTop, LeftToRight, RightToLeft, TopToBottom},
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
/// Like Flutter, `RenderListBody` expects unlimited space along its main axis
/// and a bounded cross axis; it does not clip or resize overflow in the main
/// axis.
#[derive(Debug, Clone)]
pub struct RenderListBody {
    axis_direction: AxisDirection,
    child_count: usize,
    /// Baselines recorded during layout using Flutter's first-child-in-list rule.
    reported_baselines: [Option<f32>; 2],
}

impl RenderListBody {
    /// Creates a vertical top-to-bottom list body, matching Flutter's default
    /// `AxisDirection.down`.
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

    /// Updates the axis direction; returns true when layout-affecting state
    /// changed.
    pub fn set_axis_direction(&mut self, axis_direction: AxisDirection) -> bool {
        if self.axis_direction == axis_direction {
            return false;
        }
        self.axis_direction = axis_direction;
        true
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

    fn child_main_extent(&self, size: Size) -> f32 {
        match self.main_axis() {
            Axis::Horizontal => size.width.get(),
            Axis::Vertical => size.height.get(),
        }
    }

    fn constrain_size(&self, constraints: BoxConstraints, main_extent: f32) -> Size {
        match self.main_axis() {
            Axis::Horizontal => {
                constraints.constrain(Size::new(px(main_extent), constraints.max_height))
            }
            Axis::Vertical => {
                constraints.constrain(Size::new(constraints.max_width, px(main_extent)))
            }
        }
    }

    fn dry_size(
        &self,
        constraints: BoxConstraints,
        child_count: usize,
        mut measure: impl FnMut(usize, BoxConstraints) -> Size,
    ) -> Size {
        self.debug_check_constraints(constraints);
        let child_constraints = self.child_constraints(constraints);
        let mut main_extent = 0.0;
        for i in 0..child_count {
            main_extent += self.child_main_extent(measure(i, child_constraints));
        }
        self.constrain_size(constraints, main_extent)
    }

    fn horizontal_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f32,
        mut child_query: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        match self.main_axis() {
            Axis::Horizontal => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .sum(),
            Axis::Vertical => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .fold(0.0_f32, f32::max),
        }
    }

    fn vertical_intrinsic(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        extent: f32,
        mut child_query: impl FnMut(&mut BoxIntrinsicsCtx<'_>, usize, f32) -> f32,
    ) -> f32 {
        match self.main_axis() {
            Axis::Horizontal => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .sum(),
            Axis::Vertical => (0..ctx.child_count())
                .map(|i| child_query(ctx, i, extent))
                .fold(0.0_f32, f32::max),
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
    ) -> Size {
        let constraints = *ctx.constraints();
        self.debug_check_constraints(constraints);
        self.child_count = ctx.child_count();
        self.reported_baselines = [None; 2];

        let child_constraints = self.child_constraints(constraints);
        let mut child_sizes = Vec::with_capacity(self.child_count);
        let mut main_extent = 0.0;

        for i in 0..self.child_count {
            let size = ctx.layout_child(i, child_constraints);
            main_extent += self.child_main_extent(size);
            child_sizes.push(size);
        }

        let size = self.constrain_size(constraints, main_extent);
        let mut forward_position = 0.0;
        for (i, child_size) in child_sizes.iter().copied().enumerate() {
            let child_extent = self.child_main_extent(child_size);
            let offset = match self.axis_direction {
                LeftToRight => Offset::new(px(forward_position), Pixels::ZERO),
                TopToBottom => Offset::new(Pixels::ZERO, px(forward_position)),
                RightToLeft => {
                    Offset::new(px(main_extent - forward_position - child_extent), px(0.0))
                }
                BottomToTop => {
                    Offset::new(px(0.0), px(main_extent - forward_position - child_extent))
                }
            };
            ctx.position_child(i, offset);
            forward_position += child_extent;

            for kind in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
                let slot = baseline_kind_index(kind);
                if self.reported_baselines[slot].is_none() {
                    self.reported_baselines[slot] = ctx
                        .child_distance_to_actual_baseline(i, kind)
                        .map(|baseline| baseline + offset.dy.get());
                }
            }
        }

        size
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.dry_size(constraints, ctx.child_count(), |i, c| {
            ctx.child_dry_layout(i, c)
        })
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.horizontal_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_min_intrinsic_width(i, extent)
        })
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.horizontal_intrinsic(ctx, height, |ctx, i, extent| {
            ctx.child_max_intrinsic_width(i, extent)
        })
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.vertical_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_min_intrinsic_height(i, extent)
        })
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.vertical_intrinsic(ctx, width, |ctx, i, extent| {
            ctx.child_max_intrinsic_height(i, extent)
        })
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        self.debug_check_constraints(constraints);
        let child_constraints = self.child_constraints(constraints);
        match self.axis_direction {
            LeftToRight | RightToLeft => {
                let mut result: Option<f32> = None;
                for i in 0..ctx.child_count() {
                    if let Some(child_baseline) =
                        ctx.child_dry_baseline(i, child_constraints, baseline)
                    {
                        result = Some(result.map_or(child_baseline, |v| v.min(child_baseline)));
                    }
                }
                result
            }
            TopToBottom | BottomToTop => {
                if self.axis_direction == TopToBottom {
                    let mut main_extent = 0.0;
                    for i in 0..ctx.child_count() {
                        if let Some(child_baseline) =
                            ctx.child_dry_baseline(i, child_constraints, baseline)
                        {
                            return Some(child_baseline + main_extent);
                        }
                        main_extent +=
                            self.child_main_extent(ctx.child_dry_layout(i, child_constraints));
                    }
                } else {
                    let mut main_extent = 0.0;
                    for i in (0..ctx.child_count()).rev() {
                        if let Some(child_baseline) =
                            ctx.child_dry_baseline(i, child_constraints, baseline)
                        {
                            return Some(child_baseline + main_extent);
                        }
                        main_extent +=
                            self.child_main_extent(ctx.child_dry_layout(i, child_constraints));
                    }
                }
                None
            }
        }
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.reported_baselines[baseline_kind_index(baseline)]
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
