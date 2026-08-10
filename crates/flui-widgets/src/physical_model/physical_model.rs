//! [`PhysicalModel`] — a shadow-casting, filled, `BoxShape`-clipped surface
//! around a single child.

use flui_objects::RenderPhysicalModel;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Color;
use flui_types::layout::BoxShape;
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

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
    elevation: f32,
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
    pub fn elevation(mut self, elevation: f32) -> Self {
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

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(PhysicalModel);

#[cfg(test)]
mod tests {
    use flui_types::styling::BorderRadiusExt;
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn detached() -> flui_view::RenderObjectContext<'static> {
        flui_view::RenderObjectContext::detached()
    }

    #[test]
    fn create_render_object_applies_oracle_defaults() {
        let render_object = PhysicalModel::new(Color::RED).create_render_object(&detached());
        assert_eq!(render_object.shape(), BoxShape::Rectangle);
        assert_eq!(render_object.clip_behavior(), Clip::None);
        assert_eq!(render_object.border_radius(), None);
        assert_eq!(render_object.elevation(), 0.0);
        assert_eq!(render_object.color(), Color::RED);
        assert_eq!(render_object.shadow_color(), Color::BLACK);
    }

    #[test]
    fn create_render_object_applies_every_configured_field() {
        let br = BorderRadius::circular(flui_types::geometry::px(4.0));
        let render_object = PhysicalModel::new(Color::RED)
            .shape(BoxShape::Circle)
            .clip_behavior(Clip::AntiAlias)
            .border_radius(br)
            .elevation(6.0)
            .color(Color::BLUE)
            .shadow_color(Color::GREEN)
            .create_render_object(&detached());

        assert_eq!(render_object.shape(), BoxShape::Circle);
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
        assert_eq!(render_object.border_radius(), Some(br));
        assert_eq!(render_object.elevation(), 6.0);
        assert_eq!(render_object.color(), Color::BLUE);
        assert_eq!(render_object.shadow_color(), Color::GREEN);
    }

    #[test]
    fn update_render_object_pushes_every_field() {
        let mut render_object = PhysicalModel::new(Color::RED).create_render_object(&detached());

        let br = BorderRadius::circular(flui_types::geometry::px(8.0));
        let impact = PhysicalModel::new(Color::BLUE)
            .shape(BoxShape::Circle)
            .clip_behavior(Clip::HardEdge)
            .border_radius(br)
            .elevation(3.0)
            .shadow_color(Color::GREEN)
            .update_render_object(&detached(), &mut render_object);
        assert_eq!(
            impact,
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        );

        assert_eq!(render_object.shape(), BoxShape::Circle);
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
        assert_eq!(render_object.border_radius(), Some(br));
        assert_eq!(render_object.elevation(), 3.0);
        assert_eq!(render_object.color(), Color::BLUE);
        assert_eq!(render_object.shadow_color(), Color::GREEN);
    }

    #[test]
    fn update_render_object_clears_a_previously_set_border_radius() {
        let br = BorderRadius::circular(flui_types::geometry::px(8.0));
        let mut render_object = PhysicalModel::new(Color::RED)
            .border_radius(br)
            .create_render_object(&detached());
        assert_eq!(render_object.border_radius(), Some(br));

        let impact =
            PhysicalModel::new(Color::RED).update_render_object(&detached(), &mut render_object);
        assert_eq!(
            impact,
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        );
        assert_eq!(render_object.border_radius(), None);
    }

    #[test]
    fn shape_change_reports_paint_and_semantics_and_identical_is_none() {
        let original = PhysicalModel::new(Color::RED);
        let mut render_object = original.create_render_object(&detached());
        assert_eq!(
            original.update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            original
                .clone()
                .shape(BoxShape::Circle)
                .update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    #[test]
    fn clip_behavior_change_reports_paint_only_and_identical_is_none() {
        let original = PhysicalModel::new(Color::RED);
        let mut render_object = original.create_render_object(&detached());
        assert_eq!(
            original.update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        let changed = original.clip_behavior(Clip::AntiAlias);
        assert_eq!(
            changed.update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::PAINT,
        );
        assert_eq!(
            changed.update_render_object(&detached(), &mut render_object),
            flui_rendering::RenderUpdateImpact::NONE,
        );
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!PhysicalModel::new(Color::RED).has_children());
        assert!(
            PhysicalModel::new(Color::RED)
                .child(SizedBox::shrink())
                .has_children()
        );
    }
}
