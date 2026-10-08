//! [`PhysicalShape`] — a shadow-casting, filled, arbitrary-path-clipped
//! surface around a single child.

use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_objects::{ClipSourceToken, RenderPhysicalShape};
use flui_painting::paint::{Clip, Path};
use flui_painting::styling::Color;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// The user-supplied clip-shape function: maps the laid-out box size to the
/// [`Path`] to clip against. The closure stays owner-local (UI-UI runtime affine, never sent across
/// threads); render storage receives only a data-plane target token — the
/// same convention [`ClipPath`](crate::ClipPath) uses for its own
/// `Fn(Size) -> Path` clipper.
type PathClipper = Rc<dyn Fn(Size) -> Path>;

/// A physical layer that clips its child to an arbitrary [`Path`] computed
/// from the child's laid-out size, casts a drop shadow at `elevation`, and
/// fills the shape with `color`.
///
/// Backed by [`RenderPhysicalShape`]
/// (`crates/flui-objects/src/proxy/physical_model.rs`) — the render object
/// already implements clip + `Canvas::draw_shadow` + fill; this widget is a
/// thin configuration wrapper over it. `create_render_object` /
/// `update_render_object` push every field on every call.
///
/// `clip_behavior` defaults to [`Clip::None`] — physical-model surfaces
/// don't clip by default, matching [`RenderPhysicalShape`]'s own default.
#[derive(Clone)]
pub struct PhysicalShape {
    clipper: PathClipper,
    clip_source_token: ClipSourceToken,
    clip_behavior: Clip,
    elevation: f64,
    color: Color,
    shadow_color: Color,
    child: Child,
}

impl PhysicalShape {
    /// Clips to the path returned by `clipper` for the laid-out size, filled
    /// with `color`, at the defaults `elevation: 0`,
    /// `clip_behavior: Clip::None`, opaque-black `shadow_color`.
    #[must_use]
    pub fn new(clipper: impl Fn(Size) -> Path + 'static, color: Color) -> Self {
        Self {
            clipper: Rc::new(clipper),
            clip_source_token: ClipSourceToken::fresh(),
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
    /// object debug-asserts this.
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

    fn sync_path_clip_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderPhysicalShape,
        replace_existing: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        let clipper = Rc::clone(&self.clipper);
        match render_object.path_clip_target() {
            Some(target) if replace_existing => {
                if let Err(error) = ctx.replace_path_clipper(target, move |size| clipper(size)) {
                    tracing::warn!(?error, "PhysicalShape clipper replacement failed");
                }
                render_object.set_path_clip_target(Some(target))
            }
            Some(_) => flui_rendering::RenderUpdateImpact::NONE,
            None => match ctx.register_path_clipper(move |size| clipper(size)) {
                Ok(target) => render_object.set_path_clip_target(Some(target)),
                Err(error) => {
                    tracing::debug!(
                        ?error,
                        "PhysicalShape mounted without an active interaction lane; \
                         custom path clipper will not be resolved"
                    );
                    flui_rendering::RenderUpdateImpact::NONE
                }
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
            .with_path_clip_source_token(self.clip_source_token.clone())
            .with_clip_behavior(self.clip_behavior)
            .with_elevation(self.elevation)
            .with_shadow_color(self.shadow_color);
        // Creation is not a mounted update; the initial target is already
        // reflected in the new render object's first frame.
        let _initial_target_impact = self.sync_path_clip_target(ctx, &mut render_object, false);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        impact |= render_object.set_elevation(self.elevation);
        impact |= render_object.set_color(self.color);
        impact |= render_object.set_shadow_color(self.shadow_color);
        let source_impact = render_object.set_path_clip_source_token(&self.clip_source_token);
        impact |= source_impact;
        impact |= self.sync_path_clip_target(ctx, render_object, !source_impact.is_none());
        impact
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
            // Unmount has no owner-side update application; unregistering
            // removes the target before the render node is disposed.
            let _unmount_target_impact = render_object.set_path_clip_target(None);
        }
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(PhysicalShape);
