//! `AnimatedOpacity` paints a persistent child through one owning value stream.
//! Retargeting preserves its published position and velocity. Frame samples
//! repaint the existing render object without rebuilding its child.

use std::rc::Rc;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{AnimatedValue, MotionSpec, ProxyAnimation, SpringDescription};
use flui_objects::RenderAnimatedOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{
    BoxedView, IntoView, RenderObjectContext, RenderView, View, ViewExt, ViewState,
    impl_render_view,
};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, default_curve};
use crate::animated::vsync_scope::VsyncScope;

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
            motion: MotionSpec::Curve {
                duration: DEFAULT_DURATION,
                curve: default_curve(),
            },
            child: child.into_view().boxed(),
        }
    }

    /// Select curve motion with this duration, retaining the configured easing.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        let curve = match self.motion {
            MotionSpec::Curve { curve, .. } => curve,
            _ => default_curve(),
        };
        self.motion = MotionSpec::Curve { duration, curve };
        self
    }

    /// Select this easing, retaining the configured curve duration.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        let duration = match self.motion {
            MotionSpec::Curve { duration, .. } => duration,
            _ => DEFAULT_DURATION,
        };
        self.motion = MotionSpec::Curve {
            duration,
            curve: ArcCurve::new(curve),
        };
        self
    }

    /// Select spring motion, retaining velocity across target changes.
    #[must_use]
    pub fn spring(mut self, spring: SpringDescription) -> Self {
        self.motion = MotionSpec::Spring(spring);
        self
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
    animation: AnimatedValue<f64>,
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl StatefulView for AnimatedOpacity {
    type State = AnimatedOpacityState;

    fn create_state(&self) -> Self::State {
        // A non-finite opacity paints as transparent. Do not admit that value
        // to the motion core, whose published components are always finite.
        let initial = if self.opacity.is_finite() {
            self.opacity
        } else {
            0.0
        };
        let animation = AnimatedValue::new(initial, self.motion.clone(), None)
            .expect("BUG: the initial opacity was made finite");
        let proxy = ProxyAnimation::new(Rc::new(animation.animation()));
        AnimatedOpacityState {
            animation,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedOpacity> for AnimatedOpacityState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.animation.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "opacity animation has no clock");
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.init_state(ctx);
    }

    fn build(&self, _view: &AnimatedOpacity, _ctx: &dyn BuildContext) -> impl IntoView {
        AnimatedOpacityRenderView {
            proxy: self.proxy.clone(),
            child: self.child.clone(),
        }
    }

    fn did_update_view(&mut self, _old_view: &AnimatedOpacity, new_view: &AnimatedOpacity) {
        self.child = new_view.child.clone();
        if let Err(error) = self
            .animation
            .retarget(new_view.opacity, new_view.motion.clone())
        {
            tracing::warn!(%error, "opacity motion refused; retaining the published run");
        }
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
