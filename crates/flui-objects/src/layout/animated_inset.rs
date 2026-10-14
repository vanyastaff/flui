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
//! # Dirty-marking rule
//!
//! The node subscribes in `attach`; a tick whose finite sample differs from
//! the cached one marks layout. A non-finite inset is refused and the last
//! admitted rectangle stays.

use flui_animation::ProxyAnimation;
use flui_foundation::Single;
use flui_foundation::geometry::{Offset, RelativeRect, Size};
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

fn finite_rect(rect: &RelativeRect) -> bool {
    [rect.left, rect.top, rect.right, rect.bottom]
        .iter()
        .all(|inset| inset.is_finite())
}

/// Places its child at an animated [`RelativeRect`] inside itself.
#[derive(Debug)]
pub struct RenderAnimatedInset {
    rect: AnimationSample<RelativeRect>,
    has_child: bool,
}

impl RenderAnimatedInset {
    /// A node whose child follows `rect`.
    #[must_use]
    pub fn new(rect: ProxyAnimation<RelativeRect>) -> Self {
        Self {
            rect: AnimationSample::new(rect, RelativeRect::default(), finite_rect),
            has_child: false,
        }
    }

    /// The last admitted rectangle.
    #[must_use]
    pub fn rect(&self) -> RelativeRect {
        self.rect.get()
    }

    /// The horizontal and vertical inset totals.
    fn totals(rect: &RelativeRect) -> (f64, f64) {
        (rect.left + rect.right, rect.top + rect.bottom)
    }

    /// The incoming constraints shrunk (or, for negative insets, grown) by
    /// the insets, never below zero.
    fn child_constraints(rect: &RelativeRect, constraints: &BoxConstraints) -> BoxConstraints {
        let (horizontal, vertical) = Self::totals(rect);
        BoxConstraints::new(
            (constraints.min_width - horizontal).max(0.0),
            (constraints.max_width - horizontal).max(0.0),
            (constraints.min_height - vertical).max(0.0),
            (constraints.max_height - vertical).max(0.0),
        )
    }

    fn own_size(rect: &RelativeRect, constraints: &BoxConstraints, child: Size) -> Size {
        let (horizontal, vertical) = Self::totals(rect);
        constraints.constrain(Size::new(
            (child.width + horizontal).max(0.0),
            (child.height + vertical).max(0.0),
        ))
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedInset {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("rect", self.rect());
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
        let child = if self.has_child {
            let child = ctx.layout_child(0, Self::child_constraints(&rect, &constraints))?;
            ctx.position_child(0, Offset::new(rect.left, rect.top));
            child
        } else {
            Size::ZERO
        };
        Ok(Self::own_size(&rect, &constraints, child))
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
            ctx.child_min_intrinsic_width(0, (height - vertical).max(0.0))?
        };
        Ok((child + horizontal).max(0.0))
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
            ctx.child_max_intrinsic_width(0, (height - vertical).max(0.0))?
        };
        Ok((child + horizontal).max(0.0))
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
            ctx.child_min_intrinsic_height(0, (width - horizontal).max(0.0))?
        };
        Ok((child + vertical).max(0.0))
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
            ctx.child_max_intrinsic_height(0, (width - horizontal).max(0.0))?
        };
        Ok((child + vertical).max(0.0))
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
            .map(|child_baseline| child_baseline + rect.top))
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
