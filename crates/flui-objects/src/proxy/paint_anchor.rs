//! `RenderPaintAnchor` — a repaint boundary its descendants measure their
//! paint position from.
//!
//! Paint is sans-IO: a render object records its fragment in its own
//! coordinates and cannot walk up to an ancestor. A node that has to agree
//! with its relatives on one coordinate space — every skeleton under a
//! shimmer drawing the same band — reads its offset from the nearest anchor
//! through `PaintCx::paint_anchor`, which the paint walk accumulates across
//! inline children and repaint boundaries alike.
//!
//! The anchor is also a repaint boundary: whatever animates against it
//! repaints every frame, and the boundary keeps those frames from repainting
//! the surrounding content.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A pass-through proxy that is a paint anchor and a repaint boundary.
///
/// Layout, paint, hit-testing and the queries all forward to the single
/// child; its only effects are the two flags.
#[derive(Debug, Clone, Default)]
pub struct RenderPaintAnchor {
    /// Whether a child was attached at the last layout — gates hit-testing.
    has_child: bool,
}

impl RenderPaintAnchor {
    /// A paint anchor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl flui_foundation::inspect::Inspect for RenderPaintAnchor {
    fn kind(&self) -> &'static str {
        "RenderPaintAnchor"
    }

    fn inspect(&self, _out: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for RenderPaintAnchor {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        self.has_child = ctx.child_count() > 0;
        if self.has_child {
            let size = ctx.layout_child(0, constraints)?;
            ctx.position_child(0, Offset::ZERO);
            Ok(size)
        } else {
            Ok(constraints.smallest())
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn always_needs_compositing(&self) -> bool {
        true
    }

    fn is_paint_anchor(&self) -> bool {
        true
    }

    flui_rendering::forward_single_child_box_queries!();

    fn debug_name(&self) -> &'static str {
        "RenderPaintAnchor"
    }
}
