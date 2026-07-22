//! [`AspectRatio`] — sizes its child to a given width:height ratio.

use flui_objects::{AspectRatioFactor, RenderAspectRatio};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Sizes its child to a given `width / height` aspect ratio, fitting within the
/// incoming constraints.
///
/// Flutter parity: `widgets/basic.dart` `AspectRatio` over `RenderAspectRatio`.
/// The ratio must be finite and `> 0` (debug-asserted in the render object).
#[derive(Clone, Debug)]
pub struct AspectRatio {
    aspect_ratio: f32,
    child: Child,
}

impl AspectRatio {
    /// Create an `AspectRatio` enforcing `aspect_ratio` (= width / height).
    pub fn new(aspect_ratio: f32) -> Self {
        Self {
            aspect_ratio,
            child: Child::empty(),
        }
    }

    /// Set the child to size by the ratio.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for AspectRatio {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAspectRatio;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderAspectRatio::new(AspectRatioFactor::new_unchecked(self.aspect_ratio))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_aspect_ratio(AspectRatioFactor::new_unchecked(self.aspect_ratio));
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

impl_render_view!(AspectRatio);
