//! Shared helpers for single-child proxy boxes that pass layout queries
//! through to their child unchanged (Flutter `RenderProxyBoxMixin` parity).

use flui_types::Size;

use crate::constraints::BoxConstraints;
use crate::traits::TextBaseline;

use super::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx};

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
