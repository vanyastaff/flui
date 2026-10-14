//! [`ShimmerMaskTransition`] — sweeps the shimmer band an [`Animation`] holds
//! over its child, for the shimmer effect.

use std::rc::Rc;

use flui_animation::Animation;
use flui_objects::{RenderShimmerMask, ShimmerSweep};
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

use super::proxy_slot::ProxySlot;

/// Masks its child with the band `sweep` currently holds; `None` leaves the
/// child as painted. Backed by `RenderShimmerMask`, which listens to the
/// animation itself, so a tick repaints without rebuilding the child.
#[derive(Clone, StatefulView)]
pub(crate) struct ShimmerMaskTransition {
    sweep: Rc<dyn Animation<Option<ShimmerSweep>>>,
    child: BoxedView,
}

impl ShimmerMaskTransition {
    pub(crate) fn new(
        sweep: Rc<dyn Animation<Option<ShimmerSweep>>>,
        child: impl IntoView,
    ) -> Self {
        Self {
            sweep,
            child: child.into_view().boxed(),
        }
    }
}

impl std::fmt::Debug for ShimmerMaskTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShimmerMaskTransition")
            .field("sweep", &self.sweep.value())
            .finish_non_exhaustive()
    }
}

/// State for [`ShimmerMaskTransition`]: the proxy the render object listens
/// to, retargeted in place when the view hands it a different animation.
#[derive(Debug)]
pub(crate) struct ShimmerMaskTransitionState {
    sweep: ProxySlot<Option<ShimmerSweep>>,
}

impl ViewState<ShimmerMaskTransition> for ShimmerMaskTransitionState {
    fn build(&self, view: &ShimmerMaskTransition, _ctx: &dyn BuildContext) -> impl IntoView {
        ShimmerMaskView {
            sweep: self.sweep.proxy(),
            child: view.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old: &ShimmerMaskTransition, new: &ShimmerMaskTransition) {
        self.sweep.retarget(&new.sweep);
    }
}

impl StatefulView for ShimmerMaskTransition {
    type State = ShimmerMaskTransitionState;

    fn create_state(&self) -> Self::State {
        ShimmerMaskTransitionState {
            sweep: ProxySlot::new(&self.sweep),
        }
    }
}

#[derive(Clone)]
struct ShimmerMaskView {
    sweep: flui_animation::ProxyAnimation<Option<ShimmerSweep>>,
    child: BoxedView,
}

impl std::fmt::Debug for ShimmerMaskView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShimmerMaskView").finish_non_exhaustive()
    }
}

impl RenderView for ShimmerMaskView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderShimmerMask;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderShimmerMask::new(self.sweep.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(ShimmerMaskView);
