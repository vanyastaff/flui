//! `RenderSkeleton` — a loading placeholder that paints its own share of a
//! shimmer band.
//!
//! # One band across many placeholders
//!
//! Each skeleton draws a [`LinearGradient`](flui_painting::paint::Shader::LinearGradient)
//! laid out across its nearest paint anchor (the shimmer), not across
//! itself: the gradient's points are the band's position in the anchor
//! translated by the skeleton's offset from it (`PaintCx::paint_anchor`).
//! Skeletons at different places therefore cut different windows out of one
//! band and read as a single sweep, without an offscreen group around them.
//!
//! # Dirty-marking rule
//!
//! The node subscribes to its phase in `attach`; a tick marks paint only.
//! Paint reads the cached phase, never the animation. Without a band, or
//! without an anchor above it, the skeleton paints its flat base colour.

use flui_animation::ProxyAnimation;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Point, RRect, Rect, Size};
use flui_painting::Paint;
use flui_painting::styling::Color;
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::RenderBox,
};

use crate::animation_sample::AnimationSample;
use crate::shimmer::ShimmerBand;

/// A placeholder box in a flat base colour, highlighted by the shimmer band
/// of its nearest paint anchor.
///
/// It fills the space its constraints allow: the full width times
/// [`width_factor`](Self::width_factor) when the width is bounded and the
/// full height when the height is bounded, the minimum on an unbounded axis.
/// Corners round by [`corner_radius`](Self::corner_radius), clamped to half
/// the shorter side, so an infinite radius draws a circle in a square box
/// and a stadium otherwise.
#[derive(Debug)]
pub struct RenderSkeleton {
    base: Color,
    corner_radius: f64,
    width_factor: f64,
    band: Option<ShimmerBand>,
    phase: AnimationSample<f64>,
}

impl RenderSkeleton {
    /// A placeholder in `base`, swept by `band` (if any) at `phase`, which
    /// runs `0..1` once per sweep.
    #[must_use]
    pub fn new(base: Color, band: Option<ShimmerBand>, phase: ProxyAnimation<f64>) -> Self {
        Self {
            base,
            corner_radius: 0.0,
            width_factor: 1.0,
            band,
            phase: AnimationSample::new(phase, 0.0, |value| value.is_finite()),
        }
    }

    /// The flat colour, and the band's colour outside the highlight.
    #[must_use]
    pub fn base(&self) -> Color {
        self.base
    }

    /// Sets the base colour.
    pub fn set_base(&mut self, base: Color) -> RenderUpdateImpact {
        if self.base == base {
            return RenderUpdateImpact::NONE;
        }
        self.base = base;
        RenderUpdateImpact::PAINT
    }

    /// The radius the corners round by before clamping.
    #[must_use]
    pub fn corner_radius(&self) -> f64 {
        self.corner_radius
    }

    /// Sets the corner radius; a negative or NaN radius draws square corners.
    pub fn set_corner_radius(&mut self, radius: f64) -> RenderUpdateImpact {
        if self.corner_radius.to_bits() == radius.to_bits() {
            return RenderUpdateImpact::NONE;
        }
        self.corner_radius = radius;
        RenderUpdateImpact::PAINT
    }

    /// The share of a bounded width the placeholder takes.
    #[must_use]
    pub fn width_factor(&self) -> f64 {
        self.width_factor
    }

    /// Sets the share of a bounded width the placeholder takes, clamped to
    /// `0..=1`; a NaN factor takes the full width.
    pub fn set_width_factor(&mut self, factor: f64) -> RenderUpdateImpact {
        let factor = if factor.is_nan() {
            1.0
        } else {
            factor.clamp(0.0, 1.0)
        };
        if self.width_factor.to_bits() == factor.to_bits() {
            return RenderUpdateImpact::NONE;
        }
        self.width_factor = factor;
        RenderUpdateImpact::LAYOUT
    }

    /// The band sweeping the placeholder, if any.
    #[must_use]
    pub fn band(&self) -> Option<ShimmerBand> {
        self.band
    }

    /// Sets the band; `None` paints the flat base colour.
    pub fn set_band(&mut self, band: Option<ShimmerBand>) -> RenderUpdateImpact {
        if self.band == band {
            return RenderUpdateImpact::NONE;
        }
        self.band = band;
        RenderUpdateImpact::PAINT
    }

    /// The last sampled phase.
    #[must_use]
    pub fn phase(&self) -> f64 {
        self.phase.get()
    }

    fn radius(&self, size: Size) -> f64 {
        let half_side = size.width.min(size.height) / 2.0;
        if self.corner_radius > 0.0 {
            self.corner_radius.min(half_side)
        } else {
            0.0
        }
    }

    fn layout_size(&self, constraints: BoxConstraints) -> Size {
        let width = if constraints.max_width.is_finite() {
            constraints.max_width * self.width_factor
        } else {
            constraints.min_width
        };
        let height = if constraints.max_height.is_finite() {
            constraints.max_height
        } else {
            constraints.min_height
        };
        constraints.constrain(Size::new(width, height))
    }
}

impl flui_foundation::inspect::Inspect for RenderSkeleton {
    fn kind(&self) -> &'static str {
        "RenderSkeleton"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("base", &self.base);
        out.field("corner_radius", &self.corner_radius)
            .unit(flui_foundation::inspect::Unit::LogicalPx);
        out.field("width_factor", &self.width_factor)
            .unit(flui_foundation::inspect::Unit::Fraction);
        out.field("band", &self.band.is_some()).default(&false);
    }
}

impl RenderBox for RenderSkeleton {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(self.layout_size(*ctx.constraints()))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(self.layout_size(constraints))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);
        let mut paint = Paint::fill(self.base);
        if let Some(band) = self.band
            && let Some(anchor) = ctx.paint_anchor()
            && let Some(shader) =
                band.gradient(self.phase(), -anchor.offset, anchor.size, self.base)
        {
            paint = paint.with_shader(shader);
        }
        ctx.canvas()
            .draw_rrect(RRect::from_rect_circular(rect, self.radius(size)), &paint);
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.phase.attach(&handle, RenderUpdateImpact::PAINT);
    }

    fn detach(&mut self) {
        self.phase.detach();
    }

    fn debug_name(&self) -> &'static str {
        "RenderSkeleton"
    }
}
