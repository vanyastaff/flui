//! An animation-driven fade over a sliver subtree.

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::RenderSliverAnimatedOpacity;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
    prelude::{BuildContext, StatefulView},
};

use super::proxy_slot::ProxySlot;

/// Fades a sliver without rebuilding its child on animation ticks.
///
/// The child remains laid out at zero opacity. Its paint and semantics return
/// when opacity becomes positive. The caller owns the animation's run; replacing
/// it retargets a stable proxy observed by the existing render object.
#[derive(Clone, StatefulView)]
pub struct SliverFadeTransition {
    opacity: Rc<dyn Animation<f64>>,
    child: BoxedView,
}

impl SliverFadeTransition {
    /// Fades `child`, a sliver, with the caller-owned opacity animation.
    pub fn new(opacity: Rc<dyn Animation<f64>>, child: impl IntoView) -> Self {
        Self {
            opacity,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for SliverFadeTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverFadeTransition")
            .finish_non_exhaustive()
    }
}

/// Keeps the render object's proxy stable across animation replacement.
#[derive(Debug)]
pub struct SliverFadeTransitionState {
    opacity: ProxySlot<f64>,
}

impl StatefulView for SliverFadeTransition {
    type State = SliverFadeTransitionState;

    fn create_state(&self) -> Self::State {
        SliverFadeTransitionState {
            opacity: ProxySlot::new(&self.opacity),
        }
    }
}

impl ViewState<SliverFadeTransition> for SliverFadeTransitionState {
    fn build(&self, view: &SliverFadeTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        SliverFadeRenderView {
            opacity: self.opacity.proxy(),
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old: &SliverFadeTransition, new: &SliverFadeTransition) {
        self.opacity.retarget(&new.opacity);
    }
}

#[derive(Clone)]
struct SliverFadeRenderView {
    opacity: ProxyAnimation<f64>,
    child: BoxedView,
}

impl std::fmt::Debug for SliverFadeRenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverFadeRenderView")
            .finish_non_exhaustive()
    }
}

impl RenderView for SliverFadeRenderView {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverAnimatedOpacity;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSliverAnimatedOpacity::new(self.opacity.clone(), false)
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The state retargets the same proxy; no render subscription migrates.
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(SliverFadeRenderView);
