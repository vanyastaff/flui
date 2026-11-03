//! [`ClipRSuperellipse`] — clips its child to a rounded superellipse.

use flui_foundation::geometry::Radius;
use flui_objects::RenderClipRSuperellipse;
use flui_painting::paint::Clip;
use flui_painting::styling::BorderRadius;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Clips its child to a rounded superellipse whose corners follow
/// `border_radius` and blend continuously into the straight edges.
///
/// The same corner radii read softer than [`ClipRRect`](crate::ClipRRect)'s
/// circular arcs: the curvature rises gradually instead of jumping at the
/// tangent point. Layout is a pass-through; painting and hit testing follow
/// the curve. `clip_behavior` defaults to [`Clip::AntiAlias`];
/// `border_radius` defaults to zero (a plain rectangle) until set.
#[derive(Clone, Debug)]
pub struct ClipRSuperellipse {
    border_radius: BorderRadius,
    clip_behavior: Clip,
    child: Child,
    site: CreationSite,
}

impl Default for ClipRSuperellipse {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl ClipRSuperellipse {
    fn at(site: CreationSite) -> Self {
        Self {
            border_radius: BorderRadius {
                top_left: Radius::ZERO,
                top_right: Radius::ZERO,
                bottom_right: Radius::ZERO,
                bottom_left: Radius::ZERO,
            },
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
            site,
        }
    }

    /// A superellipse clip with zero radius and the default `AntiAlias`
    /// behavior — chain [`border_radius`](Self::border_radius) to round it.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    /// A superellipse clip with the same circular `radius` on all corners.
    #[track_caller]
    pub fn circular(radius: f64) -> Self {
        let corner = Radius::circular(radius);
        Self::new().border_radius(BorderRadius {
            top_left: corner,
            top_right: corner,
            bottom_right: corner,
            bottom_left: corner,
        })
    }

    /// Set the per-corner radii.
    #[must_use]
    pub fn border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = border_radius;
        self
    }

    /// Set the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Set the clipped child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ClipRSuperellipse {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipRSuperellipse;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderClipRSuperellipse::new(self.clip_behavior).with_border_radius(self.border_radius)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_clip_behavior(self.clip_behavior)
            | render_object.set_border_radius(Some(self.border_radius))
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ClipRSuperellipse, site = site);
