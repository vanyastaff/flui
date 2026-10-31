//! `RenderAnimatedAlign` — a [`RenderAlign`] one of whose properties follows
//! an animation, so an alignment or size-factor transition relayouts this
//! node on each tick without rebuilding the element tree.
//!
//! # Dirty-marking rule
//!
//! The node subscribes in `attach`; a tick whose admitted sample differs from
//! the cached one marks layout. Layout, dry layout, intrinsics and hit-testing
//! read the cached sample and the fixed configuration, never the animation.
//!
//! # Domain
//!
//! A non-finite alignment or factor is refused and the last admitted sample
//! stays. An overshooting factor below zero collapses its axis to zero at
//! layout, the clamp the `Lerp` contract leaves to the property. A factor
//! large enough to overflow its product with the child's extent saturates the
//! box at `f64::MAX` (`RenderAlign`'s sizing rule), so layout never publishes
//! an infinite size.

use flui_animation::ProxyAnimation;
use flui_foundation::Single;
use flui_foundation::geometry::{Axis, Size};
use flui_painting::Alignment;
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{RenderBox, TextBaseline},
};

use crate::animation_sample::AnimationSample;
use crate::layout::RenderAlign;

/// The property of a [`RenderAnimatedAlign`] that follows an animation.
///
/// The other properties keep the values configured with
/// [`RenderAnimatedAlign::set_configuration`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AlignMotion {
    /// The child's alignment.
    Alignment(ProxyAnimation<Alignment>),
    /// The size factor along `axis`: the box is the child's extent on that
    /// axis times the sampled factor.
    Factor {
        /// The axis the factor sizes.
        axis: Axis,
        /// The factor.
        factor: ProxyAnimation<f64>,
    },
}

#[derive(Debug)]
enum Source {
    Alignment(AnimationSample<Alignment>),
    Factor(Axis, AnimationSample<f64>),
}

fn finite_alignment(alignment: &Alignment) -> bool {
    alignment.x.is_finite() && alignment.y.is_finite()
}

fn finite_factor(factor: &f64) -> bool {
    factor.is_finite()
}

/// Positions its child like [`RenderAlign`], with one property sampled from
/// an animation (see [`AlignMotion`]).
#[derive(Debug)]
pub struct RenderAnimatedAlign {
    inner: RenderAlign,
    source: Source,
    alignment: Alignment,
    width_factor: Option<f64>,
    height_factor: Option<f64>,
}

impl RenderAnimatedAlign {
    /// A centred align whose `motion` property follows its animation.
    #[must_use]
    pub fn new(motion: AlignMotion) -> Self {
        let source = match motion {
            AlignMotion::Alignment(alignment) => Source::Alignment(AnimationSample::new(
                alignment,
                Alignment::CENTER,
                finite_alignment,
            )),
            AlignMotion::Factor { axis, factor } => {
                Source::Factor(axis, AnimationSample::new(factor, 1.0, finite_factor))
            }
        };
        Self {
            inner: RenderAlign::new(Alignment::CENTER),
            source,
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Sets the properties the motion does not drive. The animated property's
    /// value here is ignored; negative or non-finite factors are treated as
    /// unset.
    pub fn set_configuration(
        &mut self,
        alignment: Alignment,
        width_factor: Option<f64>,
        height_factor: Option<f64>,
    ) -> RenderUpdateImpact {
        let valid = |factor: Option<f64>| factor.filter(|f| f.is_finite() && *f >= 0.0);
        let alignment = if finite_alignment(&alignment) {
            alignment
        } else {
            self.alignment
        };
        let (width_factor, height_factor) = (valid(width_factor), valid(height_factor));
        if (alignment, width_factor, height_factor)
            == (self.alignment, self.width_factor, self.height_factor)
        {
            return RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        self.width_factor = width_factor;
        self.height_factor = height_factor;
        RenderUpdateImpact::LAYOUT
    }

    /// Moves a [`AlignMotion::Factor`] motion to `axis`; an alignment motion
    /// has no axis and ignores it.
    pub fn set_factor_axis(&mut self, axis: Axis) -> RenderUpdateImpact {
        match &mut self.source {
            Source::Factor(current, _) if *current != axis => {
                *current = axis;
                RenderUpdateImpact::LAYOUT
            }
            _ => RenderUpdateImpact::NONE,
        }
    }

    /// The alignment and factors in effect: the configuration with the
    /// animated property replaced by its last admitted sample.
    #[must_use]
    pub fn resolved(&self) -> (Alignment, Option<f64>, Option<f64>) {
        match &self.source {
            Source::Alignment(sample) => (sample.get(), self.width_factor, self.height_factor),
            Source::Factor(Axis::Horizontal, sample) => (
                self.alignment,
                Some(sample.get().max(0.0)),
                self.height_factor,
            ),
            Source::Factor(Axis::Vertical, sample) => (
                self.alignment,
                self.width_factor,
                Some(sample.get().max(0.0)),
            ),
        }
    }

    /// A [`RenderAlign`] carrying the resolved configuration, for the
    /// queries that take `&self`.
    fn configured(&self) -> RenderAlign {
        let mut align = self.inner.clone();
        let (alignment, width_factor, height_factor) = self.resolved();
        let _ = align.update_configuration(alignment, width_factor, height_factor);
        align
    }
}

impl flui_foundation::inspect::Inspect for RenderAnimatedAlign {
    fn kind(&self) -> &'static str {
        "RenderAnimatedAlign"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        let (alignment, width_factor, height_factor) = self.resolved();
        out.field("alignment", &alignment)
            .unit(flui_foundation::inspect::Unit::Fraction);
        out.field("width_factor", &width_factor);
        out.field("height_factor", &height_factor);
    }
}

impl RenderBox for RenderAnimatedAlign {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let (alignment, width_factor, height_factor) = self.resolved();
        let _ = self
            .inner
            .update_configuration(alignment, width_factor, height_factor);
        self.inner.perform_layout(ctx)
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        self.inner.compute_distance_to_actual_baseline(baseline)
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.configured().compute_min_intrinsic_width(height, ctx)
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.configured().compute_max_intrinsic_width(height, ctx)
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.configured().compute_min_intrinsic_height(width, ctx)
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.configured().compute_max_intrinsic_height(width, ctx)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        self.configured().compute_dry_layout(constraints, ctx)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        self.configured()
            .compute_dry_baseline(constraints, baseline, ctx)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        match &mut self.source {
            Source::Alignment(sample) => sample.attach(&handle, RenderUpdateImpact::LAYOUT),
            Source::Factor(_, sample) => sample.attach(&handle, RenderUpdateImpact::LAYOUT),
        }
    }

    fn detach(&mut self) {
        match &mut self.source {
            Source::Alignment(sample) => sample.detach(),
            Source::Factor(_, sample) => sample.detach(),
        }
    }
}
