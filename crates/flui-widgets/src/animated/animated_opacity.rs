//! `AnimatedOpacity` paints a persistent child through one owning value stream.
//! Retargeting preserves its published position and velocity. Frame samples
//! repaint the existing render object without rebuilding its child.

use std::rc::Rc;

use flui_animation::{MotionSpec, ProxyAnimation};
use flui_objects::RenderAnimatedOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

/// Animates its child's opacity while preserving velocity on interruption.
/// The child stays laid out throughout a fade. An ambient `VsyncScope` drives
/// the motion; an unbound presentation settles at its target synchronously.
#[derive(Clone, StatefulView)]
pub struct AnimatedOpacity {
    opacity: f64,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedOpacity {
    /// Animate toward `opacity` with a 200 ms ease-in-out transition.
    pub fn new(opacity: f64, child: impl IntoView) -> Self {
        Self {
            opacity,
            motion: MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }
}

impl MotionBuilder for AnimatedOpacity {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedOpacity")
            .field("opacity", &self.opacity)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the run and the stable stream observed by the render object.
#[derive(Debug)]
pub struct AnimatedOpacityState {
    animation: ImplicitMotion<f64>,
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl StatefulView for AnimatedOpacity {
    type State = AnimatedOpacityState;

    fn create_state(&self) -> Self::State {
        // A non-finite opacity paints as transparent.
        let animation = ImplicitMotion::new("AnimatedOpacity", self.opacity, 0.0, &self.motion);
        let proxy = ProxyAnimation::new(Rc::new(animation.stream()));
        AnimatedOpacityState {
            animation,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedOpacity> for AnimatedOpacityState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, _view: &AnimatedOpacity, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedOpacityRenderView {
            proxy: self.proxy.clone(),
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AnimatedOpacity, new_view: &AnimatedOpacity) {
        self.child = new_view.child.clone();
        self.animation.retarget(new_view.opacity, &new_view.motion);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}

/// Injects the persistent value stream into a persistent render object.
#[derive(Clone)]
struct AnimatedOpacityRenderView {
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl std::fmt::Debug for AnimatedOpacityRenderView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedOpacityRenderView")
            .finish_non_exhaustive()
    }
}

impl RenderView for AnimatedOpacityRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnimatedOpacity;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        // `always_include_semantics` is false. `AnimatedOpacity`
        // does not expose a builder for it yet — no call site needs it — so
        // this is not a widget-configurable knob today.
        RenderAnimatedOpacity::new(self.proxy.clone(), false)
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // Intentionally empty — see the struct doc.
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

impl_render_view!(AnimatedOpacityRenderView);
