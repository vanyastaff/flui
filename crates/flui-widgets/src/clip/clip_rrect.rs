//! [`ClipRRect`] — clips its child to a rounded rectangle.

use flui_objects::RenderClipRRect;
use flui_rendering::protocol::BoxProtocol;
use flui_types::geometry::{RRect, Radius};
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_types::{Point, Rect, Size};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Clips its child to a rounded rectangle whose corners follow `border_radius`.
///
/// Flutter parity: `widgets/basic.dart` `ClipRRect` over `RenderClipRRect`.
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::AntiAlias`] (Flutter's `ClipRRect` default — smooth rounded
/// edges); `border_radius` defaults to zero (a sharp rectangle, i.e. a plain
/// `ClipRect`) until set.
#[derive(Clone, Debug)]
pub struct ClipRRect {
    border_radius: BorderRadius,
    clip_behavior: Clip,
    child: Child,
}

impl Default for ClipRRect {
    fn default() -> Self {
        Self {
            border_radius: BorderRadius {
                top_left: Radius::ZERO,
                top_right: Radius::ZERO,
                bottom_right: Radius::ZERO,
                bottom_left: Radius::ZERO,
            },
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
        }
    }
}

impl ClipRRect {
    /// A rounded-rect clip with zero radius (a sharp rect) and Flutter's default
    /// `AntiAlias` behavior — chain [`border_radius`](Self::border_radius) to
    /// round the corners.
    pub fn new() -> Self {
        Self::default()
    }

    /// A rounded-rect clip with the same circular `radius` on all four corners.
    pub fn circular(radius: f32) -> Self {
        Self::new().border_radius(BorderRadius {
            top_left: Radius::circular(flui_types::geometry::px(radius)),
            top_right: Radius::circular(flui_types::geometry::px(radius)),
            bottom_right: Radius::circular(flui_types::geometry::px(radius)),
            bottom_left: Radius::circular(flui_types::geometry::px(radius)),
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

    /// The `Fn(Size) -> RRect` clipper this widget installs on its render
    /// object: the laid-out box rect rounded by the configured corner radii.
    fn clipper(&self) -> impl Fn(Size) -> RRect + Send + Sync + 'static {
        let radius = self.border_radius;
        move |size| {
            let bounds = Rect::from_origin_size(Point::ZERO, size);
            RRect::from_rect_and_corners(
                bounds,
                radius.top_left,
                radius.top_right,
                radius.bottom_right,
                radius.bottom_left,
            )
        }
    }
}

impl RenderView for ClipRRect {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipRRect;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderClipRRect::new(self.clip_behavior).with_clipper(self.clipper())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_clip_behavior(self.clip_behavior);
        render_object.set_clipper(Some(self.clipper()));
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

impl_render_view!(ClipRRect);
