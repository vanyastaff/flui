//! The render view the align and size transitions share: one
//! [`RenderAnimatedAlign`] per transition, created once and reconfigured in
//! place, so an animation tick relays out without rebuilding the element tree.

use flui_objects::{AlignMotion, RenderAnimatedAlign};
use flui_painting::Alignment;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{BoxedView, RenderObjectContext, RenderView, View, impl_render_view};

/// Carries the transition's motion (whose proxy the transition's state owns)
/// and the fixed properties a rebuild may change.
#[derive(Clone)]
pub(super) struct AnimatedAlignView {
    pub(super) motion: AlignMotion,
    pub(super) alignment: Alignment,
    pub(super) width_factor: Option<f64>,
    pub(super) height_factor: Option<f64>,
    pub(super) child: BoxedView,
}

impl std::fmt::Debug for AnimatedAlignView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnimatedAlignView")
            .field("alignment", &self.alignment)
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedAlignView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedAlign;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut node = RenderAnimatedAlign::new(self.motion.clone());
        let _ = node.set_configuration(self.alignment, self.width_factor, self.height_factor);
        node
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        let mut impact =
            render_object.set_configuration(self.alignment, self.width_factor, self.height_factor);
        if let AlignMotion::Factor { axis, .. } = &self.motion {
            impact |= render_object.set_factor_axis(*axis);
        }
        impact
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(AnimatedAlignView);
