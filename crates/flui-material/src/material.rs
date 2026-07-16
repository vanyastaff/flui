//! [`Material`] — a piece of material: a colored, elevated, shaped surface.
//!
//! # Flutter parity
//!
//! `material.dart`'s `Material` widget (oracle tag `3.44.0`). `Material` is
//! responsible for three things (oracle doc, `material.dart` `:112-118`):
//! clipping to a shape, elevating on the Z axis with a shadow, and hosting
//! ink effects (splashes/highlights) below its children. This substrate
//! ships the first two; see "Scope" below for the third.
//!
//! # Wraps the existing render object — no new paint code
//!
//! `Material` is a thin configuration object over
//! [`flui_objects::RenderPhysicalShape`] (`RenderPhysicalModelBase<PathClip>`,
//! `crates/flui-objects/src/proxy/physical_model.rs`) — the render object
//! that already implements clip + `Canvas::draw_shadow` + fill. This mirrors
//! the oracle directly: `Material.build` constructs a `PhysicalModel` or
//! `PhysicalShape` render object (`material.dart`, `_RenderInkFeatures`'s
//! `createRenderObject`), **not** a `kElevationToShadow` lookup table — that
//! table belongs to `BoxShadow`-based widgets (`Card`'s pre-M3 fallback,
//! `PopupMenu`), which `Material` itself has never used. It is not ported
//! here.
//!
//! `Material` always renders through the `RenderPhysicalShape` (path-clip)
//! variant, even for [`MaterialShape::RoundedRect`] — never
//! `RenderPhysicalModel`'s `BoxShape`+`BorderRadius` clip source. Both
//! `MaterialShape` variants resolve to a path via
//! [`MaterialShape::to_path`], registered as one owner-lane path clipper
//! (see [`ClipPath`](crate) 's identical pattern in `flui-widgets`) — so
//! `Material`'s `RenderView::RenderObject` associated type does not need to
//! vary with which shape is configured.
//!
//! # `surfaceTintColor`: verified against 3.44, not ported
//!
//! `elevation_overlay.dart` (oracle tag `3.44.0`) confirms
//! `ElevationOverlay.applySurfaceTint` is still live at this tag — M3
//! components didn't remove the mechanism, they mostly opt out of it by
//! passing `surfaceTintColor: Colors.transparent` in their own
//! `_TokenDefaults` (a per-component default, not a `Material`-level
//! retirement). `Material.surfaceTintColor` also requires a `BuildContext`
//! (`Theme.of(context)`) to resolve `useMaterial3`/elevation opacity, which
//! this substrate does not wire up — `Material` here is a plain
//! color/elevation/shape/clip proxy, no theme lookup. Not implementing
//! `surfaceTintColor` is therefore a named deferral (not a "this API is
//! gone" claim): it lands once `Material` reads `Theme::of`, most likely
//! alongside a future button family's token defaults.
//!
//! # Scope: no ink-features registry
//!
//! Flutter's `Material` doubles as a `MaterialInkController`: an
//! `_RenderInkFeatures` render object that ink effects (`InkSplash`,
//! `InkHighlight`) register onto and paint through, so a splash can bleed
//! outside its originating `InkWell`'s bounds when the `Material` ancestor
//! is larger. **This substrate ships no such registry** — [`crate::ink_well`]'s
//! state overlay paints its own shape-clipped fill locally, with no
//! cross-widget ink feature list. Consequences:
//!
//! - No `Material::of`/`maybeOf` lookup, no `MaterialInkController` trait.
//! - An overlay can never visually exceed its own `InkWell`'s bounds (the
//!   oracle's "ripple crosses into a `Card` above it" effect is out of
//!   reach here).
//! - M3's real splash shader (`InkSparkle`) is nowhere in scope — this
//!   substrate has no ripple animation at all, just a static
//!   resolved-color fill.
//!
//! Upgrade path: introduce `MaterialInkController` as an `InheritedView`
//! publishing a registry keyed by render-object identity once a component
//! needs bounds-exceeding ink (unclear whether the button family this
//! substrate targets ever needs this; buttons clip their own ink to their
//! own shape in the M3 spec, so the gap may never need closing).
//!
//! # Scope: no implicit shape/elevation animation
//!
//! The oracle's `_MaterialInterior` (`material.dart`) animates `elevation`,
//! `shadowColor`, `surfaceTintColor`, and `shape` over
//! `Material.animationDuration` using an implicit `AnimatedWidget`-style
//! tween. `Material` here applies every prop change immediately (paint-only,
//! no interpolation) — a named divergence, not an oversight. It can be
//! layered on later as an `AnimatedMaterial` wrapper (mirroring
//! `flui-widgets::animated`'s existing `ImplicitController` machinery)
//! without changing this type's shape.

use flui_objects::RenderPhysicalShape;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Color;
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

use crate::shape::MaterialShape;

/// A colored, elevated, shaped surface — Flutter's `Material`.
///
/// See the module docs for what this V1 does and does not implement
/// (clipping + elevation + shadow: yes; ink-feature registry, implicit
/// animation, `surfaceTintColor`: named deferrals).
#[derive(Clone, Debug)]
pub struct Material {
    color: Color,
    elevation: f32,
    shape: MaterialShape,
    clip_behavior: Clip,
    child: Child,
}

impl Material {
    /// A flat (`elevation: 0`), sharp-cornered (`MaterialShape::rectangle()`),
    /// unclipped (`Clip::None`) surface painted `color` — Flutter's
    /// `Material(color: color)` with every other parameter left at its
    /// oracle default.
    #[must_use]
    pub fn new(color: Color) -> Self {
        Self {
            color,
            elevation: 0.0,
            shape: MaterialShape::default(),
            clip_behavior: Clip::None,
            child: Child::empty(),
        }
    }

    /// Sets the fill color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the elevation. Applied straight to the render object with no
    /// snapping or curve — a continuous z-depth, matching the oracle's own
    /// `double elevation` (M3 has no discrete elevation "levels" at the
    /// `Material` layer; token tables like `_surfaceTintElevationOpacities`
    /// operate on the same continuous value). Must be non-negative — the
    /// underlying render object debug-asserts this (oracle:
    /// `assert(elevation >= 0.0)`).
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = elevation;
        self
    }

    /// Sets the surface shape (both the clip boundary and the shape the
    /// shadow is cast from).
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = shape;
        self
    }

    /// Sets the clip behavior. Defaults to [`Clip::None`] — Flutter parity:
    /// `Material.clipBehavior` defaults to `Clip.none` "for performance
    /// considerations" (oracle doc, `material.dart`).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Sets the child painted on top of the surface.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Registers (or re-targets) the owner-lane path clipper that resolves
    /// `self.shape` against the render object's laid-out size each paint —
    /// the same pattern `flui_widgets::ClipPath` uses for its owner-local
    /// `Fn(Size) -> Path` clipper.
    fn sync_path_clip_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderPhysicalShape,
    ) {
        let shape = self.shape;
        match render_object.path_clip_target() {
            Some(target) => {
                if let Err(error) =
                    ctx.replace_path_clipper(target, move |size| shape.to_path(size))
                {
                    tracing::warn!(?error, "Material shape clipper replacement failed");
                }
            }
            None => match ctx.register_path_clipper(move |size| shape.to_path(size)) {
                Ok(target) => {
                    render_object.set_path_clip_target(Some(target));
                }
                Err(error) => tracing::debug!(
                    ?error,
                    "Material mounted without an active interaction lane; \
                     shape clip will not be resolved"
                ),
            },
        }
    }
}

impl RenderView for Material {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPhysicalShape;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderPhysicalShape::new(self.color)
            .with_elevation(self.elevation)
            .with_clip_behavior(self.clip_behavior);
        self.sync_path_clip_target(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_color(self.color);
        render_object.set_elevation(self.elevation);
        render_object.set_clip_behavior(self.clip_behavior);
        self.sync_path_clip_target(ctx, render_object);
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.path_clip_target() {
            if let Err(error) = ctx.unregister_path_clipper(target) {
                tracing::debug!(?error, "Material shape clipper unregistration failed");
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

impl_render_view!(Material);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;

    #[test]
    fn create_render_object_applies_color_elevation_and_clip_behavior() {
        let render_object = Material::new(Color::rgb(10, 20, 30))
            .elevation(6.0)
            .clip_behavior(Clip::AntiAlias)
            .create_render_object(&flui_view::RenderObjectContext::detached());

        assert_eq!(render_object.color(), Color::rgb(10, 20, 30));
        assert_eq!(render_object.elevation(), 6.0);
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn create_render_object_defaults_to_flat_unclipped_rectangle() {
        let render_object = Material::new(Color::WHITE)
            .create_render_object(&flui_view::RenderObjectContext::detached());

        assert_eq!(render_object.elevation(), 0.0);
        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn update_render_object_applies_changed_color_and_elevation() {
        let mut render_object = Material::new(Color::BLACK)
            .create_render_object(&flui_view::RenderObjectContext::detached());

        Material::new(Color::WHITE)
            .elevation(3.0)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );

        assert_eq!(render_object.color(), Color::WHITE);
        assert_eq!(render_object.elevation(), 3.0);
    }

    #[test]
    fn detached_creation_does_not_install_a_path_clipper() {
        let render_object = Material::new(Color::WHITE)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(!render_object.has_custom_clipper());
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!Material::new(Color::WHITE).has_children());
        assert!(
            Material::new(Color::WHITE)
                .child(flui_widgets::SizedBox::shrink())
                .has_children()
        );
    }

    #[test]
    fn shape_defaults_to_the_plain_rectangle() {
        assert_eq!(Material::new(Color::WHITE).shape, MaterialShape::default());
    }

    /// The `.shape(...)` builder actually reaches the field
    /// `sync_path_clip_target` closes over (`self.shape`), and that field is
    /// shape-sensitive at the configured paint size — not merely "some
    /// path was produced."
    ///
    /// Shape-sensitive, not bounds-only: a `.bounds()`/`.rect` comparison is
    /// identical for every `MaterialShape` at the same size (a bounding box
    /// doesn't know about corner rounding), so it can't distinguish
    /// `Stadium` from `MaterialShape::rectangle()` — confirmed by running
    /// that exact mutation against the old version of this test, which
    /// passed unchanged. Point-containment near a corner does distinguish
    /// them (see `shape.rs`'s
    /// `to_path_excludes_a_stadium_corner_a_sharp_rectangle_would_include`
    /// for the same probe, unit-tested against `MaterialShape` directly).
    ///
    /// This test covers "the field the builder sets is the field
    /// `sync_path_clip_target` reads." It does NOT exercise the owner-lane
    /// registration/resolution path itself (`RenderObjectContext::detached`
    /// has no active interaction lane, so `sync_path_clip_target` no-ops) —
    /// that end-to-end path is covered by
    /// `tests/material.rs`'s `stadium_shape_excludes_a_corner_a_sharp_rectangle_would_include`,
    /// which hit-tests a real mounted `Material` and so resolves the clip
    /// through the actual registered `PathClipTarget`.
    #[test]
    fn configured_shape_field_is_shape_sensitive_at_the_paint_size() {
        let painted_size = flui_types::Size::new(
            flui_types::geometry::px(120.0),
            flui_types::geometry::px(40.0),
        );
        let corner_probe =
            flui_types::Point::new(flui_types::geometry::px(2.0), flui_types::geometry::px(2.0));

        let stadium = Material::new(Color::WHITE).shape(MaterialShape::Stadium);
        let stadium_path = stadium.shape.to_path(painted_size);
        assert!(
            !stadium_path.contains(corner_probe),
            "the configured Stadium shape's rounded corner must exclude this point"
        );

        let rectangle = Material::new(Color::WHITE).shape(MaterialShape::rectangle());
        let rectangle_path = rectangle.shape.to_path(painted_size);
        assert!(
            rectangle_path.contains(corner_probe),
            "the configured plain-rectangle shape must include the same point"
        );
    }
}
