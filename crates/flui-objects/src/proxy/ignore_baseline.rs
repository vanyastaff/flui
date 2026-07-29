//! [`RenderIgnoreBaseline`] — hides its child's baseline from the parent.
//!
//! A pure proxy in every other respect: it lays the child out under its own
//! constraints, adopts the child's size, paints and hit-tests straight
//! through, and forwards all four intrinsics. Only the two baseline queries
//! are answered `None`.
//!
//! # What it is for
//!
//! A baseline-aligning parent — a `RenderFlex` row with
//! `CrossAxisAlignment::Baseline` — shifts each child down so their baselines
//! meet, and sizes itself to the tallest ascent plus the deepest descent. A
//! child with a large font therefore drags the whole row taller and pushes its
//! siblings down. Wrapping that child here takes it out of the baseline
//! computation: the parent sees no baseline, so it neither aligns the child
//! nor lets it grow the row, and the child sits flush at the cross start.
//!
//! Flutter parity: `RenderIgnoreBaseline` in `rendering/proxy_box.dart`
//! (tag `3.44.0`), which overrides `computeDistanceToActualBaseline` and
//! `computeDryBaseline` to return null and inherits everything else from
//! `RenderProxyBox`.

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// Excludes its child from the parent's baseline computation.
///
/// Layout, paint, hit-testing and intrinsics pass straight through to the
/// child; both baseline queries answer `None` regardless of what the child
/// reports, so a baseline-aligning parent treats this subtree as having no
/// baseline at all.
///
/// Flutter parity: `RenderIgnoreBaseline` (`proxy_box.dart`, tag `3.44.0`).
#[derive(Debug, Clone, Default)]
pub struct RenderIgnoreBaseline;

impl RenderIgnoreBaseline {
    /// Creates a baseline-hiding proxy.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl flui_foundation::Diagnosticable for RenderIgnoreBaseline {}

impl RenderBox for RenderIgnoreBaseline {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            constraints.smallest()
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    /// Always `None` — this is the whole point of the type. The child's own
    /// baseline is deliberately not forwarded.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Always `None`, for the same reason as the live query — a dry pass that
    /// forwarded the child's baseline would let a baseline-aligning parent
    /// size itself around a baseline that layout then refuses to report.
    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
        _ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        None
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        flui_rendering::context::proxy_queries::forward_dry_layout(constraints, ctx)
    }

    fn debug_name(&self) -> &'static str {
        "RenderIgnoreBaseline"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::context::intrinsics_test_support::leaf_dry_baseline;

    #[test]
    fn both_baseline_queries_are_none_for_both_kinds() {
        let node = RenderIgnoreBaseline::new();
        let constraints = BoxConstraints::tight(Size::ZERO);

        for kind in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
            assert_eq!(
                node.compute_distance_to_actual_baseline(kind),
                None,
                "the live baseline must be hidden for every baseline kind",
            );
            assert_eq!(
                leaf_dry_baseline(|ctx| node.compute_dry_baseline(constraints, kind, ctx)),
                None,
                "the dry baseline must be hidden for every baseline kind — and \
                 answered without consulting the child, which the leaf context \
                 enforces by panicking on any child query",
            );
        }
    }
}
