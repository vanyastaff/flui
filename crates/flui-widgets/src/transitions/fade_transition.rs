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

/// Fades its child in and out as an [`Animation<f32>`] (the opacity) changes.
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
    opacity: Arc<dyn Animation<f32>>,
    child: BoxedView,
}

impl FadeTransition {
    /// A fade driven by `opacity`, fading `child`.
    pub fn new(opacity: Arc<dyn Animation<f32>>, child: impl IntoView) -> Self {
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
    proxy: ProxyAnimation<f32>,
    opacity: Arc<dyn Animation<f32>>,
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
    proxy: ProxyAnimation<f32>,
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use flui_animation::AnimationController;
    use flui_foundation::Listenable;

    use super::*;
    use crate::SizedBox;

    fn animation(controller: &AnimationController) -> Arc<dyn Animation<f32>> {
        Arc::new(controller.clone())
    }

    #[test]
    fn same_animation_identity_does_not_swap_the_proxy_parent() {
        let controller = AnimationController::without_ticker(Duration::from_secs(1));
        let opacity = animation(&controller);
        let view = FadeTransition::new(Arc::clone(&opacity), SizedBox::shrink());
        let mut state = view.create_state();
        let parent_before = state.proxy.parent();

        let unchanged = FadeTransition::new(opacity, SizedBox::new(2.0, 2.0));
        state.did_update_view(&view, &unchanged);

        assert!(Arc::ptr_eq(&parent_before, &state.proxy.parent()));
    }

    #[test]
    fn swapping_animation_detaches_old_parent_and_new_parent_drives_proxy() {
        let old_controller = AnimationController::without_ticker(Duration::from_secs(1));
        let new_controller = AnimationController::without_ticker(Duration::from_secs(1));
        let old_animation = animation(&old_controller);
        let view = FadeTransition::new(old_animation, SizedBox::shrink());
        let mut state = view.create_state();
        let notifications = Arc::new(AtomicUsize::new(0));
        let notifications_for_listener = Arc::clone(&notifications);
        let _listener = state.proxy.add_listener(Arc::new(move || {
            notifications_for_listener.fetch_add(1, Ordering::Relaxed);
        }));

        let new_animation = animation(&new_controller);
        let updated = FadeTransition::new(Arc::clone(&new_animation), SizedBox::shrink());
        state.did_update_view(&view, &updated);
        let after_swap = notifications.load(Ordering::Relaxed);

        old_controller.set_value(0.4);
        assert_eq!(notifications.load(Ordering::Relaxed), after_swap);

        new_controller.set_value(0.7);
        assert!(notifications.load(Ordering::Relaxed) > after_swap);
        assert!((state.proxy.value() - 0.7).abs() < f32::EPSILON);
        assert!(Arc::ptr_eq(&state.opacity, &new_animation));
    }
}
