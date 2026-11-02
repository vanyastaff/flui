//! `RenderShimmerMask` — sweeps a highlight band over arbitrary content
//! through a shader mask.
//!
//! The band is a gradient from transparent through the highlight and back,
//! composited [`SrcATop`](BlendMode::SrcATop) over the child: it tints only
//! what the child painted, and leaves its transparent pixels and its shape
//! alone. At rest (no sweep, or one at either end) the child paints without
//! a mask.
//!
//! # Dirty-marking rule
//!
//! The node subscribes to its sweep in `attach`; a tick marks paint only.
//! Paint reads the cached sweep, never the animation.

use flui_animation::ProxyAnimation;
use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::paint::BlendMode;
use flui_painting::styling::Color;
use flui_rendering::{
    RenderUpdateImpact,
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::RenderBox,
};

use crate::animation_sample::AnimationSample;
use crate::shimmer::ShimmerSweep;

/// Masks its child with the shimmer band a [`ShimmerSweep`] animation
/// currently holds, across the child's own bounds.
#[derive(Debug)]
pub struct RenderShimmerMask {
    sweep: AnimationSample<Option<ShimmerSweep>>,
    has_child: bool,
}

impl RenderShimmerMask {
    /// A mask following `sweep`; `None` paints the child unmasked.
    #[must_use]
    pub fn new(sweep: ProxyAnimation<Option<ShimmerSweep>>) -> Self {
        Self {
            sweep: AnimationSample::new(sweep, None, |sweep| {
                sweep.is_none_or(|sweep| sweep.progress.is_finite())
            }),
            has_child: false,
        }
    }

    /// The last sampled sweep.
    #[must_use]
    pub fn sweep(&self) -> Option<ShimmerSweep> {
        self.sweep.get()
    }
}

impl flui_foundation::inspect::Inspect for RenderShimmerMask {
    fn kind(&self) -> &'static str {
        "RenderShimmerMask"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("progress", &self.sweep().map(|sweep| sweep.progress))
            .unit(flui_foundation::inspect::Unit::Fraction);
    }
}

impl RenderBox for RenderShimmerMask {
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

    flui_rendering::forward_single_child_box_queries!();

    // Closure is load-bearing: `PaintCx::paint_child` is ambiguous as a method
    // path across arities.
    #[expect(clippy::redundant_closure_for_method_calls)]
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }
        let shader = self
            .sweep()
            .filter(|sweep| sweep.progress > 0.0 && sweep.progress < 1.0)
            .and_then(|sweep| {
                sweep
                    .band
                    .gradient(sweep.progress, Offset::ZERO, ctx.size(), Color::TRANSPARENT)
            });
        match shader {
            Some(shader) => {
                ctx.with_shader_mask(shader, BlendMode::SrcATop, |ctx| ctx.paint_child());
            }
            None => ctx.paint_child(),
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    fn always_needs_compositing(&self) -> bool {
        self.has_child
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.sweep.attach(&handle, RenderUpdateImpact::PAINT);
    }

    fn detach(&mut self) {
        self.sweep.detach();
    }

    fn debug_name(&self) -> &'static str {
        "RenderShimmerMask"
    }
}
