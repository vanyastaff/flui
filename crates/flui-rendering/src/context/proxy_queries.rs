//! Shared helpers for single-child proxy boxes that pass layout queries
//! through to their child unchanged (Flutter `RenderProxyBoxMixin` parity).

use flui_tree::Single;
use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;
use crate::parent_data::BoxParentData;
use crate::traits::TextBaseline;

use super::{
    BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
};

/// Minimum intrinsic width: child answer, or `0.0` when childless.
#[inline]
pub fn forward_min_intrinsic_width(ctx: &mut BoxIntrinsicsCtx<'_>, height: f32) -> f32 {
    if ctx.child_count() == 0 {
        0.0
    } else {
        ctx.child_min_intrinsic_width(0, height)
    }
}

/// Maximum intrinsic width: child answer, or `0.0` when childless.
#[inline]
pub fn forward_max_intrinsic_width(ctx: &mut BoxIntrinsicsCtx<'_>, height: f32) -> f32 {
    if ctx.child_count() == 0 {
        0.0
    } else {
        ctx.child_max_intrinsic_width(0, height)
    }
}

/// Minimum intrinsic height: child answer, or `0.0` when childless.
#[inline]
pub fn forward_min_intrinsic_height(ctx: &mut BoxIntrinsicsCtx<'_>, width: f32) -> f32 {
    if ctx.child_count() == 0 {
        0.0
    } else {
        ctx.child_min_intrinsic_height(0, width)
    }
}

/// Maximum intrinsic height: child answer, or `0.0` when childless.
#[inline]
pub fn forward_max_intrinsic_height(ctx: &mut BoxIntrinsicsCtx<'_>, width: f32) -> f32 {
    if ctx.child_count() == 0 {
        0.0
    } else {
        ctx.child_max_intrinsic_height(0, width)
    }
}

/// Dry layout: child size under the same constraints, or `smallest()` when
/// childless (matches transparent proxy `perform_layout` with no child).
#[inline]
pub fn forward_dry_layout(constraints: BoxConstraints, ctx: &mut BoxDryLayoutCtx<'_>) -> Size {
    if ctx.child_count() == 0 {
        constraints.smallest()
    } else {
        ctx.child_dry_layout(0, constraints)
    }
}

/// Dry baseline: child answer under the same constraints, or `None` when
/// childless.
#[inline]
pub fn forward_dry_baseline(
    constraints: BoxConstraints,
    baseline: TextBaseline,
    ctx: &mut BoxDryBaselineCtx<'_>,
) -> Option<f32> {
    if ctx.child_count() == 0 {
        None
    } else {
        ctx.child_dry_baseline(0, constraints, baseline)
    }
}

/// Live layout for a transparent proxy: the child laid out under the
/// unmodified constraints, positioned at the origin, its size adopted —
/// or `smallest()` when childless. Child presence is recorded in
/// `has_child` so the paint/hit-test paths can consult it without a
/// context.
#[inline]
pub fn forward_layout(
    has_child: &mut bool,
    ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
) -> Size {
    let constraints = *ctx.constraints();
    if ctx.child_count() > 0 {
        *has_child = true;
        let child_size = ctx.layout_child(0, constraints);
        ctx.position_child(0, Offset::ZERO);
        child_size
    } else {
        *has_child = false;
        constraints.smallest()
    }
}

/// Hit test for a transparent proxy: inside its own bounds the answer is
/// the child's (tested at the origin, where [`forward_layout`] positioned
/// it); outside, or when childless, a miss.
#[inline]
pub fn forward_hit_test(
    has_child: bool,
    ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>,
) -> bool {
    if !ctx.is_within_own_size() {
        return false;
    }
    if has_child {
        ctx.hit_test_child_at_offset(0, Offset::ZERO)
    } else {
        false
    }
}

/// Expands the four intrinsic overrides for a pure single-child passthrough proxy.
#[macro_export]
macro_rules! forward_single_child_intrinsics {
    () => {
        fn compute_min_intrinsic_width(
            &self,
            height: f32,
            ctx: &mut $crate::context::BoxIntrinsicsCtx<'_>,
        ) -> f32 {
            $crate::context::proxy_queries::forward_min_intrinsic_width(ctx, height)
        }

        fn compute_max_intrinsic_width(
            &self,
            height: f32,
            ctx: &mut $crate::context::BoxIntrinsicsCtx<'_>,
        ) -> f32 {
            $crate::context::proxy_queries::forward_max_intrinsic_width(ctx, height)
        }

        fn compute_min_intrinsic_height(
            &self,
            width: f32,
            ctx: &mut $crate::context::BoxIntrinsicsCtx<'_>,
        ) -> f32 {
            $crate::context::proxy_queries::forward_min_intrinsic_height(ctx, width)
        }

        fn compute_max_intrinsic_height(
            &self,
            width: f32,
            ctx: &mut $crate::context::BoxIntrinsicsCtx<'_>,
        ) -> f32 {
            $crate::context::proxy_queries::forward_max_intrinsic_height(ctx, width)
        }
    };
}

/// Expands the six query overrides for a pure single-child passthrough proxy.
#[macro_export]
macro_rules! forward_single_child_box_queries {
    () => {
        $crate::forward_single_child_intrinsics!();

        fn compute_dry_layout(
            &self,
            constraints: $crate::constraints::BoxConstraints,
            ctx: &mut $crate::context::BoxDryLayoutCtx<'_>,
        ) -> flui_types::Size {
            $crate::context::proxy_queries::forward_dry_layout(constraints, ctx)
        }

        fn compute_dry_baseline(
            &self,
            constraints: $crate::constraints::BoxConstraints,
            baseline: $crate::traits::TextBaseline,
            ctx: &mut $crate::context::BoxDryBaselineCtx<'_>,
        ) -> Option<f32> {
            $crate::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
        }

        // The live counterpart of the dry-baseline forward above. It is a flag
        // rather than a forwarding body because
        // `compute_distance_to_actual_baseline` takes no context: the driver
        // walks to the child on this object's behalf. Both forwards live in
        // this one macro so a proxy cannot answer the dry and live queries
        // differently.
        fn forwards_baseline_to_only_child(&self) -> bool {
            true
        }
    };
}

/// Expands the standard `perform_layout` for a pure single-child passthrough
/// proxy: child laid out under the unmodified constraints at the origin, its
/// size adopted (or `smallest()` when childless), with child presence recorded
/// in the implementor's `has_child` field.
/// The emitted signature is deliberately concrete — `Single` arity with
/// `BoxParentData` — because that is the only shape the forwarding helper
/// supports (it lays out and positions child index 0 with plain box parent
/// data). On an implementor with any other `Arity`/`ParentData` the method
/// signature no longer matches the trait's, so a misapplication surfaces as
/// an immediate incompatible-signature error at the impl rather than a type
/// mismatch deep inside the macro body.
#[macro_export]
macro_rules! forward_single_child_box_layout {
    () => {
        fn perform_layout(
            &mut self,
            ctx: &mut $crate::context::BoxLayoutContext<
                '_,
                $crate::prelude::Single,
                $crate::parent_data::BoxParentData,
            >,
        ) -> flui_types::Size {
            $crate::context::proxy_queries::forward_layout(&mut self.has_child, ctx)
        }
    };
}

/// Expands the standard `hit_test` for a pure single-child passthrough proxy:
/// inside its own bounds, defer to the child at the origin (presence read from
/// the implementor's `has_child` field); otherwise a miss.
/// Deliberately concrete over `Single`/`BoxParentData` for the same reason as
/// [`forward_single_child_box_layout!`]: the helper only supports that shape,
/// and a concrete signature turns a misapplication into an immediate
/// incompatible-signature error at the impl.
#[macro_export]
macro_rules! forward_single_child_box_hit_test {
    () => {
        fn hit_test(
            &self,
            ctx: &mut $crate::context::BoxHitTestContext<
                '_,
                $crate::prelude::Single,
                $crate::parent_data::BoxParentData,
            >,
        ) -> bool {
            $crate::context::proxy_queries::forward_hit_test(self.has_child, ctx)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::intrinsics::test_support::{
        leaf_dry_baseline, leaf_dry_layout, leaf_intrinsics,
    };

    #[test]
    fn leaf_helpers_reject_child_queries() {
        let w = leaf_intrinsics(|ctx| forward_min_intrinsic_width(ctx, 100.0));
        assert_eq!(w, 0.0);

        let size = leaf_dry_layout(|ctx| {
            forward_dry_layout(
                BoxConstraints::new(
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(100.0),
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(100.0),
                ),
                ctx,
            )
        });
        assert_eq!(size, Size::ZERO);

        let baseline = leaf_dry_baseline(|ctx| {
            forward_dry_baseline(
                BoxConstraints::new(
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(100.0),
                    flui_types::geometry::px(0.0),
                    flui_types::geometry::px(100.0),
                ),
                TextBaseline::Alphabetic,
                ctx,
            )
        });
        assert!(baseline.is_none());
    }
}
