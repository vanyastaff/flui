//! RenderBaseline — positions a child so its baseline sits at a fixed offset.
//!
//! Flutter parity: `shifted_box.dart` `RenderBaseline`.

use flui_tree::Single;
use flui_types::{Offset, Pixels, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// Positions its child so the child's [`TextBaseline`] sits at
/// [`baseline_offset`](Self::baseline_offset) from the top of this box.
#[derive(Debug, Clone)]
pub struct RenderBaseline {
    baseline: TextBaseline,
    baseline_offset: Pixels,
    has_child: bool,
    child_offset: Offset,
}

impl RenderBaseline {
    /// Creates a baseline container for `baseline` at `baseline_offset`.
    pub fn new(baseline: TextBaseline, baseline_offset: Pixels) -> Self {
        Self {
            baseline,
            baseline_offset,
            has_child: false,
            child_offset: Offset::ZERO,
        }
    }

    /// Which baseline kind to align.
    pub fn baseline(&self) -> TextBaseline {
        self.baseline
    }

    /// Distance from the top of this box to the aligned baseline.
    pub fn baseline_offset(&self) -> Pixels {
        self.baseline_offset
    }

    /// Sets the baseline kind. Caller marks layout dirty.
    pub fn set_baseline(&mut self, baseline: TextBaseline) {
        self.baseline = baseline;
    }

    /// Sets the baseline offset. Caller marks layout dirty.
    pub fn set_baseline_offset(&mut self, offset: Pixels) {
        self.baseline_offset = offset;
    }
}

impl flui_foundation::Diagnosticable for RenderBaseline {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("baseline", self.baseline);
        properties.add(
            "baseline_offset",
            format!("{:.0}px", self.baseline_offset.get()),
        );
    }
}

impl RenderBox for RenderBaseline {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            return constraints.smallest();
        }

        self.has_child = true;
        let child_size = ctx.layout_child(0, constraints);

        let size = if let Some(distance) = ctx.child_distance_to_actual_baseline(0, self.baseline) {
            self.child_offset =
                Offset::new(Pixels::ZERO, self.baseline_offset - Pixels::new(distance));
            Size::new(
                child_size.width,
                child_size.height - Pixels::new(distance) + self.baseline_offset,
            )
        } else {
            self.child_offset = Offset::ZERO;
            child_size
        };

        ctx.position_child(0, self.child_offset);
        constraints.constrain(size)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        if baseline == self.baseline {
            Some(self.baseline_offset.get())
        } else {
            None
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if baseline != self.baseline || ctx.child_count() == 0 {
            return None;
        }
        ctx.child_dry_baseline(0, constraints, baseline)
            .map(|_| self.baseline_offset.get())
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_layout_offset(0)
        } else {
            false
        }
    }
}
