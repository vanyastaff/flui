//! [`AnimatedOpacity`] — animates its child's opacity when the target changes.

use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{Animatable, Animation, Curves};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, ImplicitAnimation};
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Opacity};

/// Animates the opacity of its child whenever a new `opacity` is given.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedOpacity`. On the
/// first build the child sits at the given opacity with no motion; each later
/// build with a *different* opacity animates from the current value to the new
/// one over `duration` along `curve`. The child is always laid out — only its
/// painting fades.
///
/// Driven deterministically by a binding when a
/// [`VsyncScope`](crate::VsyncScope) is above it; otherwise driven by its own
/// scheduler ticker on a real display.
#[derive(Clone, StatefulView)]
pub struct AnimatedOpacity {
    opacity: f32,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedOpacity {
    /// Animate `child` toward `opacity` (`0.0` transparent … `1.0` opaque),
    /// with the 200 ms default duration and an ease-in-out curve.
    pub fn new(opacity: f32, child: impl IntoView) -> Self {
        Self {
            opacity,
            duration: DEFAULT_DURATION,
            curve: ArcCurve::new(Curves::EaseInOut),
            child: child.into_view().boxed(),
        }
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve; accepts any type implementing
    /// [`Curve`](flui_animation::Curve), including elastic and bounce curves
    /// from [`flui_animation::Curves`](flui_animation::Curves).
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedOpacity")
            .field("opacity", &self.opacity)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedOpacity`] — owns the persistent opacity animation.
#[derive(Debug)]
pub struct AnimatedOpacityState {
    animation: ImplicitAnimation<f32>,
    child: BoxedView,
}

impl StatefulView for AnimatedOpacity {
    type State = AnimatedOpacityState;

    fn create_state(&self) -> Self::State {
        AnimatedOpacityState {
            animation: ImplicitAnimation::new(self.opacity, self.duration, self.curve.clone()),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedOpacity> for AnimatedOpacityState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.animation.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedOpacity, _ctx: &dyn BuildContext) -> impl IntoView {
        let curved = self.animation.curved();
        let tween = self.animation.tween();
        let child = self.child.clone();
        AnimatedBuilder::new(self.animation.listenable(), move || {
            Opacity::new(tween.transform(curved.value())).child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedOpacity, new_view: &AnimatedOpacity) {
        self.child = new_view.child.clone();
        self.animation.retarget(new_view.opacity, new_view.duration);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
