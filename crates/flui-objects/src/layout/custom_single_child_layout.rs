//! `RenderCustomSingleChildLayoutBox` — delegates size, child constraints, and
//! child position to a [`SingleChildLayoutDelegate`].
//!
//! Flutter parity: `rendering/shifted_box.dart`
//! `RenderCustomSingleChildLayoutBox`. The render object keeps Flutter's
//! contract: the delegate's parent size is always constrained by incoming
//! constraints, intrinsics probe `_getSize(tightForFinite(...))`, dry layout
//! never touches the child, and live/dry baselines add the delegated child
//! offset to the child's baseline.

use std::sync::Arc;

use flui_tree::Single;
use flui_types::{Offset, Pixels, Size};

use flui_rendering::{
    constraints::{BoxConstraints, Constraints},
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    delegates::SingleChildLayoutDelegate,
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// A single-child render box whose layout is controlled by a delegate.
#[derive(Debug, Clone)]
pub struct RenderCustomSingleChildLayoutBox {
    delegate: Arc<dyn SingleChildLayoutDelegate>,
    has_child: bool,
    child_offset: Offset,
    child_baselines: [Option<f32>; 2],
}

impl RenderCustomSingleChildLayoutBox {
    /// Creates a custom single-child layout box.
    pub fn new(delegate: Arc<dyn SingleChildLayoutDelegate>) -> Self {
        Self {
            delegate,
            has_child: false,
            child_offset: Offset::ZERO,
            child_baselines: [None; 2],
        }
    }

    /// Returns the current layout delegate.
    pub fn delegate(&self) -> &dyn SingleChildLayoutDelegate {
        &*self.delegate
    }

    /// Replaces the delegate and returns whether layout must be recomputed.
    ///
    /// Mirrors Flutter's setter: the identical delegate instance is a no-op;
    /// changing the concrete delegate type forces relayout; otherwise the new
    /// delegate's `should_relayout(old_delegate)` decides.
    pub fn set_delegate(&mut self, delegate: Arc<dyn SingleChildLayoutDelegate>) -> bool {
        if Arc::ptr_eq(&self.delegate, &delegate) {
            return false;
        }
        let type_changed = self.delegate.as_any().type_id() != delegate.as_any().type_id();
        let relayout = type_changed || delegate.should_relayout(&*self.delegate);
        self.delegate = delegate;
        relayout
    }

    /// Flutter's private `_getSize`: delegate size, then incoming constraints.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(self.delegate.get_size(constraints))
    }

    fn child_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.delegate.get_constraints_for_child(constraints)
    }

    fn child_size_for_position(child_constraints: BoxConstraints, actual_child_size: Size) -> Size {
        if child_constraints.is_tight() {
            child_constraints.smallest()
        } else {
            actual_child_size
        }
    }

    fn intrinsic_width(&self, height: f32) -> f32 {
        let width = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::INFINITY,
                Pixels::new(height),
            ))
            .width;
        if width.is_finite() { width.get() } else { 0.0 }
    }

    fn intrinsic_height(&self, width: f32) -> f32 {
        let height = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::new(width),
                Pixels::INFINITY,
            ))
            .height;
        if height.is_finite() {
            height.get()
        } else {
            0.0
        }
    }

    fn clear_child_state(&mut self) {
        self.has_child = false;
        self.child_offset = Offset::ZERO;
        self.child_baselines = [None; 2];
    }

    fn record_child_baselines(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        self.child_baselines = [
            ctx.child_distance_to_actual_baseline(0, TextBaseline::Alphabetic),
            ctx.child_distance_to_actual_baseline(0, TextBaseline::Ideographic),
        ];
    }
}

impl flui_foundation::Diagnosticable for RenderCustomSingleChildLayoutBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("delegate", format!("{:?}", self.delegate));
    }
}

impl RenderBox for RenderCustomSingleChildLayoutBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let size = self.get_size(constraints);

        if ctx.child_count() == 0 {
            self.clear_child_state();
            return size;
        }

        self.has_child = true;
        let child_constraints = self.child_constraints(constraints);
        let child_size = ctx.layout_child(0, child_constraints);
        let child_size_for_position = Self::child_size_for_position(child_constraints, child_size);
        self.child_offset = self
            .delegate
            .get_position_for_child(size, child_size_for_position);
        ctx.position_child(0, self.child_offset);
        self.record_child_baselines(ctx);

        size
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        let index = match baseline {
            TextBaseline::Alphabetic => 0,
            TextBaseline::Ideographic => 1,
        };
        self.child_baselines[index].map(|raw| raw + self.child_offset.dy.get())
    }

    fn compute_min_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.get_size(constraints)
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
        let child_constraints = self.child_constraints(constraints);
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_size = if child_constraints.is_tight() {
            child_constraints.smallest()
        } else {
            ctx.child_dry_layout(0, child_constraints)
        };
        let size = self.get_size(constraints);
        let child_offset = self.delegate.get_position_for_child(size, child_size);
        Some(child_baseline + child_offset.dy.get())
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        self.has_child && ctx.hit_test_child_at_layout_offset(0)
    }
}
