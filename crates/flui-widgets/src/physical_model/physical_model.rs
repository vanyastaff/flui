//! [`PhysicalModel`] — a shadow-casting, filled, `BoxShape`-clipped surface
//! around a single child.

use flui_objects::RenderPhysicalModel;
use flui_painting::BoxShape;
use flui_painting::paint::Clip;
use flui_painting::styling::BorderRadius;
use flui_painting::styling::Color;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// A physical layer that clips its child to a [`BoxShape`] (optionally
/// rounded via `border_radius` when the shape is
/// [`BoxShape::Rectangle`]), casts a drop shadow at `elevation`, and fills
/// the shape with `color`.
///
/// Flutter parity: `widgets/basic.dart` `PhysicalModel` (tag `3.44.0`) over
/// [`RenderPhysicalModel`] (`RenderPhysicalModelBase<RectangleClip>`,
/// `crates/flui-objects/src/proxy/physical_model.rs`) — the render object
/// already implements clip + `Canvas::draw_shadow` + fill; this widget is a
/// thin configuration wrapper over it. `create_render_object` /
/// `update_render_object` push every field, mirroring the oracle's own
/// `createRenderObject`/`updateRenderObject` (both `..`-cascade every
/// property on every call).
///
/// `clip_behavior` defaults to [`Clip::None`] — physical-model surfaces
/// don't clip by default (oracle `proxy_box.dart:2071`, inherited unchanged
/// by [`RenderPhysicalModel`]'s own default).
#[derive(Clone, Debug)]
pub struct PhysicalModel {
    shape: BoxShape,
    clip_behavior: Clip,
    border_radius: Option<BorderRadius>,
    elevation: f64,
    color: Color,
    shadow_color: Color,
    child: Child,
}

impl PhysicalModel {
    /// A flat (`elevation: 0`), unrounded (`border_radius: None`),
    /// rectangular, unclipped (`Clip::None`) surface filled with `color`
    /// and an opaque-black shadow color — Flutter's
    /// `PhysicalModel(color: color)` with every other parameter left at its
    /// oracle default.
    #[must_use]
    pub fn new(color: Color) -> Self {
        Self {
            shape: BoxShape::Rectangle,
            clip_behavior: Clip::None,
            border_radius: None,
            elevation: 0.0,
            color,
            shadow_color: Color::BLACK,
            child: Child::empty(),
        }
    }

    /// Sets the box shape (`Rectangle` or `Circle`).
    #[must_use]
    pub fn shape(mut self, shape: BoxShape) -> Self {
        self.shape = shape;
        self
    }

    /// Sets the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Sets the corner radius. Ignored unless `shape` is
    /// [`BoxShape::Rectangle`]. Unset behaves like `BorderRadius::ZERO`
    /// (oracle default: `borderRadius: null`).
    #[must_use]
    pub fn border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = Some(border_radius);
        self
    }

    /// Sets the elevation. Must be non-negative — the underlying render
    /// object debug-asserts this (oracle: `assert(elevation >= 0.0)`).
    #[must_use]
    pub fn elevation(mut self, elevation: f64) -> Self {
        self.elevation = elevation;
        self
    }

    /// Sets the fill color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the drop-shadow color, used only when `elevation != 0.0`.
    #[must_use]
    pub fn shadow_color(mut self, shadow_color: Color) -> Self {
        self.shadow_color = shadow_color;
        self
    }

    /// Sets the child painted on top of the surface.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for PhysicalModel {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPhysicalModel;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let render_object = RenderPhysicalModel::new(self.color)
            .with_shape(self.shape)
            .with_clip_behavior(self.clip_behavior)
            .with_elevation(self.elevation)
            .with_shadow_color(self.shadow_color);
        if let Some(border_radius) = self.border_radius {
            render_object.with_border_radius(border_radius)
        } else {
            render_object
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_shape(self.shape);
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact |= render_object.set_border_radius(self.border_radius);
        impact |= render_object.set_elevation(self.elevation);
        impact |= render_object.set_color(self.color);
        impact |= render_object.set_shadow_color(self.shadow_color);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(PhysicalModel);
