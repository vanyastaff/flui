//! RenderBaseline — positions a child so its baseline sits at a fixed offset.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
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
    baseline_offset: f64,
    has_child: bool,
    child_offset: Offset,
}

impl RenderBaseline {
    /// Creates a baseline container for `baseline` at `baseline_offset`.
    pub fn new(baseline: TextBaseline, baseline_offset: f64) -> Self {
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
    pub fn baseline_offset(&self) -> f64 {
        self.baseline_offset
    }

    /// Sets the baseline kind. Caller marks layout dirty.
    pub fn set_baseline(&mut self, baseline: TextBaseline) -> flui_rendering::RenderUpdateImpact {
        if self.baseline == baseline {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.baseline = baseline;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the baseline offset. Caller marks layout dirty.
    pub fn set_baseline_offset(&mut self, offset: f64) -> flui_rendering::RenderUpdateImpact {
        if self.baseline_offset == offset {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.baseline_offset = offset;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl flui_foundation::Diagnosticable for RenderBaseline {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("baseline", self.baseline);
        properties.add("baseline_offset", format!("{:.0}px", self.baseline_offset));
    }
}

impl RenderBox for RenderBaseline {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.has_child = false;
            return Ok(constraints.smallest());
        }

        self.has_child = true;
        // The child is laid out under *loosened* constraints, so a tight
        // incoming axis does not force the child to fill it.
        let child_size = ctx.layout_child(0, constraints.loosen())?;

        // The effective baseline is the child's real baseline distance, or —
        // when the child reports none (e.g. a plain box) — the child's full
        // height. The child is shifted down so that baseline sits
        // `baseline_offset` below the top; the box's height becomes `top +
        // child.height` (= `baseline_offset` plus any descent below the baseline).
        let baseline_distance = ctx
            .child_distance_to_actual_baseline(0, self.baseline)?
            .unwrap_or(child_size.height);
        let top = self.baseline_offset - baseline_distance;
        self.child_offset = Offset::new(0.0, top);
        let size = Size::new(child_size.width, top + child_size.height);

        ctx.position_child(0, self.child_offset);
        Ok(constraints.constrain(size))
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        if baseline == self.baseline {
            Ok(Some(self.baseline_offset))
        } else {
            Ok(None)
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        if ctx.child_count() == 0 {
            return Ok(None);
        }
        // Probe the child under *loosened* constraints (consistent with the
        // live path) for BOTH the requested baseline kind and the box's own
        // baseline type, returning `baseline_offset + requested - own`. For a
        // same-kind query the two terms cancel to `baseline_offset`; a
        // cross-kind query still resolves through the child's actual values.
        let loosened = constraints.loosen();
        let Some(requested) = ctx.child_dry_baseline(0, loosened, baseline)? else {
            return Ok(None);
        };
        let Some(own) = ctx.child_dry_baseline(0, loosened, self.baseline)? else {
            return Ok(None);
        };
        Ok(Some(self.baseline_offset + requested - own))
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
