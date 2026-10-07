//! The render view the transform transitions share: one
//! [`RenderAnimatedTransform`] per transition, created once and updated in
//! place, so an animation tick never rebuilds the element tree.

use flui_objects::{RenderAnimatedTransform, TransformMotion};
use flui_painting::typography::TextDirection;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{BoxedView, RenderObjectContext, RenderView, View, impl_render_view};

/// Carries the transition's motion (whose proxy the transition's state owns)
/// and the two settings a rebuild may change.
#[derive(Clone)]
pub(super) struct AnimatedTransformView {
    pub(super) motion: TransformMotion,
    pub(super) transform_hit_tests: bool,
    pub(super) text_direction: TextDirection,
    pub(super) child: BoxedView,
}

impl std::fmt::Debug for AnimatedTransformView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnimatedTransformView")
            .field("transform_hit_tests", &self.transform_hit_tests)
            .field("text_direction", &self.text_direction)
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedTransformView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedTransform;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut node = RenderAnimatedTransform::new(self.motion.clone());
        let _ = node.set_transform_hit_tests(self.transform_hit_tests);
        let _ = node.set_text_direction(self.text_direction);
        node
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        render_object.set_transform_hit_tests(self.transform_hit_tests)
            | render_object.set_text_direction(self.text_direction)
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(AnimatedTransformView);
