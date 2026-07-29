//! [`PhysicalShape`] — a shadow-casting, filled, arbitrary-path-clipped
//! surface around a single child.

use std::rc::Rc;

use flui_objects::RenderPhysicalShape;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Color;
use flui_types::Size;
use flui_types::painting::{Clip, Path};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// The user-supplied clip-shape function: maps the laid-out box size to the
/// [`Path`] to clip against — Flutter's `CustomClipper<Path>` (`clipper`).
/// The closure stays owner-local (UI-realm affine, never sent across
/// threads); render storage receives only a data-plane target token — the
/// same convention [`ClipPath`](crate::ClipPath) uses for its own
/// `Fn(Size) -> Path` clipper.
type PathClipper = Rc<dyn Fn(Size) -> Path>;

/// A physical layer that clips its child to an arbitrary [`Path`] computed
/// from the child's laid-out size, casts a drop shadow at `elevation`, and
/// fills the shape with `color`.
///
/// Flutter parity: `widgets/basic.dart` `PhysicalShape` (tag `3.44.0`) over
/// [`RenderPhysicalShape`] (`RenderPhysicalModelBase<PathClip>`,
/// `crates/flui-objects/src/proxy/physical_model.rs`) — the render object
/// already implements clip + `Canvas::draw_shadow` + fill; this widget is a
/// thin configuration wrapper over it. `create_render_object` /
/// `update_render_object` push every field, mirroring the oracle's own
/// `createRenderObject`/`updateRenderObject` (both `..`-cascade every
/// property on every call).
///
/// `clip_behavior` defaults to [`Clip::None`] — physical-model surfaces
/// don't clip by default (oracle `proxy_box.dart:2071`, inherited unchanged
/// by [`RenderPhysicalShape`]'s own default).
#[derive(Clone)]
pub struct PhysicalShape {
    clipper: PathClipper,
    clip_behavior: Clip,
    elevation: f32,
    color: Color,
    shadow_color: Color,
    child: Child,
}

impl PhysicalShape {
    /// Clips to the path returned by `clipper` for the laid-out size, filled
    /// with `color`, at Flutter's defaults: `elevation: 0`,
    /// `clip_behavior: Clip::None`, opaque-black `shadow_color`.
    #[must_use]
    pub fn new(clipper: impl Fn(Size) -> Path + 'static, color: Color) -> Self {
        Self {
            clipper: Rc::new(clipper),
            clip_behavior: Clip::None,
            elevation: 0.0,
            color,
            shadow_color: Color::BLACK,
            child: Child::empty(),
        }
    }

    /// Sets the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
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

    fn sync_path_clip_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderPhysicalShape,
    ) {
        let clipper = Rc::clone(&self.clipper);
        match render_object.path_clip_target() {
            Some(target) => {
                if let Err(error) = ctx.replace_path_clipper(target, move |size| clipper(size)) {
                    tracing::warn!(?error, "PhysicalShape clipper replacement failed");
                }
            }
            None => match ctx.register_path_clipper(move |size| clipper(size)) {
                Ok(target) => {
                    render_object.set_path_clip_target(Some(target));
                }
                Err(error) => tracing::debug!(
                    ?error,
                    "PhysicalShape mounted without an active interaction lane; \
                     custom path clipper will not be resolved"
                ),
            },
        }
    }
}

impl std::fmt::Debug for PhysicalShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalShape")
            .field("clip_behavior", &self.clip_behavior)
            .field("elevation", &self.elevation)
            .field("color", &self.color)
            .field("shadow_color", &self.shadow_color)
            .finish_non_exhaustive()
    }
}

impl RenderView for PhysicalShape {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPhysicalShape;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderPhysicalShape::new(self.color)
            .with_clip_behavior(self.clip_behavior)
            .with_elevation(self.elevation)
            .with_shadow_color(self.shadow_color);
        self.sync_path_clip_target(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_clip_behavior(self.clip_behavior);
        render_object.set_elevation(self.elevation);
        render_object.set_color(self.color);
        render_object.set_shadow_color(self.shadow_color);
        self.sync_path_clip_target(ctx, render_object);
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.path_clip_target() {
            if let Err(error) = ctx.unregister_path_clipper(target) {
                tracing::debug!(?error, "PhysicalShape clipper unregistration failed");
            }
            render_object.set_path_clip_target(None);
        }
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

impl_render_view!(PhysicalShape);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn detached() -> flui_view::RenderObjectContext<'static> {
        flui_view::RenderObjectContext::detached()
    }

    fn whole_box_clipper() -> impl Fn(Size) -> Path + 'static {
        |size: Size| {
            let mut path = Path::new();
            path.add_rect(flui_types::Rect::from_origin_size(
                flui_types::Point::ZERO,
                size,
            ));
            path
        }
    }

    fn physical_shape() -> PhysicalShape {
        PhysicalShape::new(whole_box_clipper(), Color::RED)
    }

    #[test]
    fn create_render_object_applies_oracle_defaults() {
        let render_object = physical_shape().create_render_object(&detached());
        assert_eq!(render_object.clip_behavior(), Clip::None);
        assert_eq!(render_object.elevation(), 0.0);
        assert_eq!(render_object.color(), Color::RED);
        assert_eq!(render_object.shadow_color(), Color::BLACK);
    }

    #[test]
    fn create_render_object_applies_every_configured_field() {
        let render_object = physical_shape()
            .clip_behavior(Clip::AntiAlias)
            .elevation(6.0)
            .color(Color::BLUE)
            .shadow_color(Color::GREEN)
            .create_render_object(&detached());

        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
        assert_eq!(render_object.elevation(), 6.0);
        assert_eq!(render_object.color(), Color::BLUE);
        assert_eq!(render_object.shadow_color(), Color::GREEN);
    }

    #[test]
    fn update_render_object_pushes_every_field() {
        let mut render_object = physical_shape().create_render_object(&detached());

        physical_shape()
            .clip_behavior(Clip::HardEdge)
            .elevation(3.0)
            .color(Color::BLUE)
            .shadow_color(Color::GREEN)
            .update_render_object(&detached(), &mut render_object);

        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
        assert_eq!(render_object.elevation(), 3.0);
        assert_eq!(render_object.color(), Color::BLUE);
        assert_eq!(render_object.shadow_color(), Color::GREEN);
    }

    #[test]
    fn detached_creation_does_not_install_a_path_clipper() {
        let render_object = physical_shape().create_render_object(&detached());
        assert!(!render_object.has_custom_clipper());
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!physical_shape().has_children());
        assert!(physical_shape().child(SizedBox::shrink()).has_children());
    }

    #[test]
    fn debug_reports_configured_fields() {
        let debug = format!(
            "{:?}",
            physical_shape().clip_behavior(Clip::None).elevation(2.0)
        );
        assert!(
            debug.contains("clip_behavior: None") && debug.contains("elevation: 2.0"),
            "Debug output must include clip_behavior and elevation, got: {debug}",
        );
    }
}
