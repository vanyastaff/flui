//! `RenderAnimatedDecoratedBox` — a [`RenderDecoratedBox`](crate::RenderDecoratedBox)
//! whose decoration follows an animation, repainting on each tick without
//! rebuilding the element tree.
//!
//! # Dirty-marking rule
//!
//! The node subscribes in `attach`; a tick whose decoration differs from the
//! cached one marks paint only. Layout is a pass-through and does not depend
//! on the decoration. Paint and hit-testing read the cached decoration, never
//! the animation.

use flui_animation::ProxyAnimation;
use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::styling::BoxDecoration;
use flui_painting::{DecorationPaintOptions, box_decoration_hit_test, paint_box_decoration};
use flui_rendering::{
    RenderUpdateImpact,
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::RenderBox,
};

use crate::animation_sample::AnimationSample;
use crate::proxy::DecorationPosition;

/// Paints an animated [`BoxDecoration`] before or after its child.
#[derive(Debug)]
pub struct RenderAnimatedDecoratedBox {
    decoration: AnimationSample<BoxDecoration<f64>>,
    position: DecorationPosition,
    has_child: bool,
}

impl RenderAnimatedDecoratedBox {
    /// A node painting `decoration`'s current value behind its child.
    #[must_use]
    pub fn new(decoration: ProxyAnimation<BoxDecoration<f64>>) -> Self {
        Self {
            decoration: AnimationSample::new(decoration, BoxDecoration::new(), |_| true),
            position: DecorationPosition::Background,
            has_child: false,
        }
    }

    /// The last sampled decoration.
    #[must_use]
    pub fn decoration(&self) -> BoxDecoration<f64> {
        self.decoration.get()
    }

    /// The decoration's position relative to the child.
    #[must_use]
    pub fn position(&self) -> DecorationPosition {
        self.position
    }

    /// Sets the decoration's position relative to the child.
    pub fn set_position(&mut self, position: DecorationPosition) -> RenderUpdateImpact {
        if self.position == position {
            return RenderUpdateImpact::NONE;
        }
        self.position = position;
        RenderUpdateImpact::PAINT
    }

    fn paint_decoration(ctx: &mut PaintCx<'_, Single>, decoration: &BoxDecoration<f64>) {
        let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
        paint_box_decoration(
            ctx.canvas(),
            rect,
            decoration,
            DecorationPaintOptions::default(),
        );
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedDecoratedBox {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("decoration", self.decoration());
        properties.add_default_enum("position", self.position, DecorationPosition::Background);
    }
}

impl RenderBox for RenderAnimatedDecoratedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        self.has_child = ctx.child_count() > 0;
        if self.has_child {
            let child_size = ctx.layout_child(0, constraints)?;
            ctx.position_child(0, Offset::ZERO);
            Ok(child_size)
        } else {
            Ok(constraints.smallest())
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        let decoration = self.decoration();
        if self.position == DecorationPosition::Background {
            Self::paint_decoration(ctx, &decoration);
        }
        if self.has_child {
            ctx.paint_child();
        }
        if self.position == DecorationPosition::Foreground {
            Self::paint_decoration(ctx, &decoration);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, Self::ParentData>) -> bool {
        // The child first, then the decoration's own shape, as
        // `RenderDecoratedBox` does.
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO) {
            return true;
        }
        box_decoration_hit_test(
            Rect::from_origin_size(Point::ZERO, ctx.own_size()),
            &self.decoration(),
            *ctx.position(),
        )
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.decoration.attach(&handle, RenderUpdateImpact::PAINT);
    }

    fn detach(&mut self) {
        self.decoration.detach();
    }
}
