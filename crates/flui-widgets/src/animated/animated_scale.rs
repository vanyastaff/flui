//! [`AnimatedScale`] — scales its child about its center, animating to each
//! new scale while keeping the velocity it had.

use std::rc::Rc;

use flui_animation::{MotionSpec, ProxyAnimation};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{Child, IntoView, ViewState};

use crate::ScaleTransition;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

/// Scales its child about its center, animating to each new scale it is given.
///
/// The first build shows the child at `scale` with no motion; a later build
/// with a different scale moves from the scale currently shown. Scaling is
/// paint-only: the child is laid out at its own size. A non-finite first
/// scale is shown as `1.0`; a later non-finite scale is refused and the
/// current motion continues.
///
/// ```
/// use flui_widgets::{AnimatedScale, MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let pressed = AnimatedScale::new(0.95).child(SizedBox::new(40.0, 40.0))
///     .duration(Duration::from_millis(100));
/// # let _ = pressed;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedScale {
    scale: f64,
    motion: MotionSpec,
    child: Child,
}

impl AnimatedScale {
    /// Scale toward `scale` with the default motion, with no child yet.
    pub fn new(scale: f64) -> Self {
        Self {
            scale,
            motion: MotionSpec::default(),
            child: Child::empty(),
        }
    }

    /// Sets the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl MotionBuilder for AnimatedScale {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedScale")
            .field("scale", &self.scale)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the scale motion and the stable stream the transition observes.
#[derive(Debug)]
pub struct AnimatedScaleState {
    animation: ImplicitMotion<f64>,
    proxy: ProxyAnimation<f64>,
    child: Child,
}

impl StatefulView for AnimatedScale {
    type State = AnimatedScaleState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        let animation = ImplicitMotion::new("AnimatedScale", self.scale, 1.0, &self.motion);
        let proxy = ProxyAnimation::new(Rc::new(animation.stream()));
        AnimatedScaleState {
            animation,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedScale> for AnimatedScaleState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, _view: &AnimatedScale, _ctx: &dyn BuildContext) -> impl IntoView {
        ScaleTransition::new(Rc::new(self.proxy.clone())).child_slot(self.child.clone())
    }

    fn did_update_view(&mut self, _old_view: &AnimatedScale, new_view: &AnimatedScale) {
        self.child = new_view.child.clone();
        self.animation.retarget(new_view.scale, &new_view.motion);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
