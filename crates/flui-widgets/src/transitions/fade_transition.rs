//! [`FadeTransition`] — animates its child's opacity without rebuilding its child.

use std::sync::Arc;

use flui_animation::{Animation, ProxyAnimation};
use flui_objects::RenderAnimatedOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

/// Fades its child in and out as an [`Animation<f64>`] (the opacity) changes.
///
/// Flutter parity: `widgets/transitions.dart` `FadeTransition` backed by
/// `RenderAnimatedOpacity`. The render object listens to `opacity` directly,
/// so a tick marks paint/compositing work without rebuilding the element tree.
/// `0.0` is fully transparent, `1.0` fully opaque; the child is always laid out.
///
/// ```rust,ignore
/// let controller = AnimationController::without_ticker(Duration::from_millis(300));
/// let fade = FadeTransition::new(Arc::new(controller), Text::new("hi"));
/// controller.forward(); // each frame re-reads the opacity into the child
/// ```
#[derive(Clone, StatefulView)]
pub struct FadeTransition {
    opacity: Arc<dyn Animation<f64>>,
    child: BoxedView,
}

impl FadeTransition {
    /// A fade driven by `opacity`, fading `child`.
    pub fn new(opacity: Arc<dyn Animation<f64>>, child: impl IntoView) -> Self {
        Self {
            opacity,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for FadeTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FadeTransition")
            .field("opacity", &self.opacity.value())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
/// State that keeps the render object's animation proxy stable across view updates.
pub struct FadeTransitionState {
    proxy: ProxyAnimation<f64>,
    opacity: Arc<dyn Animation<f64>>,
    child: BoxedView,
}

impl ViewState<FadeTransition> for FadeTransitionState {
    fn build(&self, _view: &FadeTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        FadeTransitionRenderView {
            proxy: self.proxy.clone(),
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &FadeTransition, new_view: &FadeTransition) {
        self.child = new_view.child.clone();
        if !Arc::ptr_eq(&self.opacity, &new_view.opacity) {
            self.opacity = Arc::clone(&new_view.opacity);
            self.proxy.set_parent(Arc::clone(&new_view.opacity));
        }
    }
}

impl StatefulView for FadeTransition {
    type State = FadeTransitionState;

    fn create_state(&self) -> Self::State {
        FadeTransitionState {
            proxy: ProxyAnimation::new(Arc::clone(&self.opacity)),
            opacity: Arc::clone(&self.opacity),
            child: self.child.clone(),
        }
    }
}

#[derive(Clone)]
struct FadeTransitionRenderView {
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl std::fmt::Debug for FadeTransitionRenderView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FadeTransitionRenderView")
            .finish_non_exhaustive()
    }
}

impl RenderView for FadeTransitionRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedOpacity;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderAnimatedOpacity::new(self.proxy.clone(), false)
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(FadeTransitionRenderView);
