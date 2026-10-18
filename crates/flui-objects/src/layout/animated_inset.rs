//! `RenderAnimatedInset` — insets its child from its own edges by an animated
//! [`RelativeRect`], relaying out on each tick without rebuilding the element
//! tree.
//!
//! Under tight constraints (a `Positioned.fill` slot of a stack) the child is
//! laid out tight to the rectangle the insets describe, at `(left, top)`:
//! the geometry a stack gives a child positioned on all four edges. Insets
//! may be negative, placing the child partly outside this box, as a
//! positioned stack child may sit partly outside its stack; a rectangle whose
//! insets overlap collapses the child to zero on that axis.
//!
//! # Clipping
//!
//! A stack clips a positioned child that overflows it, but this node fills
//! its stack slot exactly, so the stack never sees the child overflow. The
//! node therefore clips the child to its own bounds itself, with its
//! `clip_behavior` (hard edge by default, a stack's default); `Clip::None`
//! lets the child paint outside, as under a non-clipping stack.
//!
//! # Dirty-marking rule
//!
//! The node subscribes in `attach`; a tick whose admitted sample differs from
//! the cached one marks layout. A rectangle with a non-finite inset, or whose
//! horizontal or vertical inset total overflows, is refused and the last
//! admitted rectangle stays. Sizes derived from an admitted rectangle
//! saturate at `f64::MAX`, so layout never publishes non-finite geometry.

use flui_animation::ProxyAnimation;
use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Point, Rect, RelativeRect, Size};
use flui_painting::paint::Clip;
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{RenderBox, TextBaseline},
};

use crate::animation_sample::AnimationSample;
use crate::layout::shifted_box::saturate;

/// Every inset and both inset totals are finite.
fn admitted_rect(rect: &RelativeRect) -> bool {
    [
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        rect.left + rect.right,
        rect.top + rect.bottom,
    ]
    .iter()
    .all(|value| value.is_finite())
}

/// A non-negative extent that saturates at `f64::MAX` instead of overflowing.
fn extent(value: f64) -> f64 {
    value.clamp(0.0, f64::MAX)
}

/// A constraint bound shrunk by `total`: an unbounded maximum stays unbounded,
/// a finite one saturates.
fn deflated(bound: f64, total: f64) -> f64 {
    if bound.is_infinite() {
        bound
    } else {
        extent(bound - total)
    }
}

/// Places its child at an animated [`RelativeRect`] inside itself.
#[derive(Debug)]
pub struct RenderAnimatedInset {
    rect: AnimationSample<RelativeRect>,
    clip_behavior: Clip,
    has_child: bool,
    /// Whether the child's laid-out rectangle leaves this box.
    overflows: bool,
    /// The child's alphabetic and ideographic baselines, offset by the top
    /// inset it was laid out at.
    baselines: [Option<f64>; 2],
}

impl RenderAnimatedInset {
    /// A node whose child follows `rect`, clipped with a hard edge.
    #[must_use]
    pub fn new(rect: ProxyAnimation<RelativeRect>) -> Self {
        Self {
            rect: AnimationSample::new(rect, RelativeRect::default(), admitted_rect),
            clip_behavior: Clip::HardEdge,
            has_child: false,
            overflows: false,
            baselines: [None; 2],
        }
    }

    /// The last admitted rectangle.
    #[must_use]
    pub fn rect(&self) -> RelativeRect {
        self.rect.get()
    }

    /// How a child outside this box is clipped.
    #[must_use]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets how a child outside this box is clipped.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        // The clip bounds the child's accessible geometry as well as its paint.
        RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
    }

    /// The horizontal and vertical inset totals, finite for an admitted
    /// rectangle.
    fn totals(rect: &RelativeRect) -> (f64, f64) {
        (rect.left + rect.right, rect.top + rect.bottom)
    }

    /// The incoming constraints shrunk (or, for negative insets, grown) by
    /// the insets, never below zero.
    fn child_constraints(rect: &RelativeRect, constraints: &BoxConstraints) -> BoxConstraints {
        let (horizontal, vertical) = Self::totals(rect);
        BoxConstraints::new(
            deflated(constraints.min_width, horizontal),
            deflated(constraints.max_width, horizontal),
            deflated(constraints.min_height, vertical),
            deflated(constraints.max_height, vertical),
        )
    }

    fn own_size(rect: &RelativeRect, constraints: &BoxConstraints, child: Size) -> Size {
        let (horizontal, vertical) = Self::totals(rect);
        constraints.constrain(Size::new(
            extent(child.width + horizontal),
            extent(child.height + vertical),
        ))
    }

    /// Whether paint clips the child to this box: it overflows and clipping
    /// is on.
    fn clips(&self) -> bool {
        self.has_child && self.overflows && self.clip_behavior != Clip::None
    }

    fn baseline_index(baseline: TextBaseline) -> usize {
        match baseline {
            TextBaseline::Alphabetic => 0,
            TextBaseline::Ideographic => 1,
        }
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedInset {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("rect", self.rect());
        properties.add_default_enum("clip_behavior", self.clip_behavior, Clip::HardEdge);
    }
}

impl RenderBox for RenderAnimatedInset {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        let rect = self.rect();
        self.has_child = ctx.child_count() > 0;
        if !self.has_child {
            self.overflows = false;
            self.baselines = [None; 2];
            return Ok(Self::own_size(&rect, &constraints, Size::ZERO));
        }
        let child = ctx.layout_child(0, Self::child_constraints(&rect, &constraints))?;
        ctx.position_child(0, Offset::new(rect.left, rect.top));
        let size = Self::own_size(&rect, &constraints, child);
        self.overflows = rect.left < 0.0
            || rect.top < 0.0
            || rect.left + child.width > size.width
            || rect.top + child.height > size.height;
        for baseline in [TextBaseline::Alphabetic, TextBaseline::Ideographic] {
            self.baselines[Self::baseline_index(baseline)] = ctx
                .child_distance_to_actual_baseline(0, baseline)?
                .map(|child_baseline| saturate(child_baseline + rect.top));
        }
        Ok(size)
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        Ok(self.baselines[Self::baseline_index(baseline)])
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let (horizontal, vertical) = Self::totals(&self.rect());
        let child = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_min_intrinsic_width(0, deflated(height, vertical))?
        };
        Ok(extent(child + horizontal))
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let (horizontal, vertical) = Self::totals(&self.rect());
        let child = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_max_intrinsic_width(0, deflated(height, vertical))?
        };
        Ok(extent(child + horizontal))
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let (horizontal, vertical) = Self::totals(&self.rect());
        let child = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_min_intrinsic_height(0, deflated(width, horizontal))?
        };
        Ok(extent(child + vertical))
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let (horizontal, vertical) = Self::totals(&self.rect());
        let child = if ctx.child_count() == 0 {
            0.0
        } else {
            ctx.child_max_intrinsic_height(0, deflated(width, horizontal))?
        };
        Ok(extent(child + vertical))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        let rect = self.rect();
        let child = if ctx.child_count() == 0 {
            Size::ZERO
        } else {
            ctx.child_dry_layout(0, Self::child_constraints(&rect, &constraints))?
        };
        Ok(Self::own_size(&rect, &constraints, child))
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
        let rect = self.rect();
        Ok(ctx
            .child_dry_baseline(0, Self::child_constraints(&rect, &constraints), baseline)?
            .map(|child_baseline| saturate(child_baseline + rect.top)))
    }

    // Closure is load-bearing: `PaintCx::paint_child` is ambiguous as a method path
    // (Single's zero-arg overload vs the indexed variant on other arities).
    #[expect(clippy::redundant_closure_for_method_calls)]
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }
        if self.clips() {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(bounds, self.clip_behavior, |ctx| ctx.paint_child());
        } else {
            ctx.paint_child();
        }
    }

    fn describe_approximate_paint_clip(&self, _child_slot: usize, size: Size) -> Option<Rect<f64>> {
        self.clips()
            .then(|| Rect::from_origin_size(Point::ZERO, size))
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        ctx.is_within_own_size() && self.has_child && ctx.hit_test_child_at_layout_offset(0)
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.rect.attach(&handle, RenderUpdateImpact::LAYOUT);
    }

    fn detach(&mut self) {
        self.rect.detach();
    }
}
