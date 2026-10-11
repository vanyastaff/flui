//! `AnimatedPadding` retains component velocities when its target changes.

use std::rc::Rc;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{
    AnimatedValue, AnimatedValueView, Animation, MotionSpec, SpringDescription, TwoWayConverter,
};
use flui_foundation::geometry::EdgeInsets;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, default_curve};
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Padding};

/// Animates its child's padding with independent component velocities.
/// The initial build rests at the configured insets; later targets flow from
/// the last layout sample. An ambient `VsyncScope` drives the motion.
#[derive(Clone, StatefulView)]
pub struct AnimatedPadding {
    padding: EdgeInsets,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedPadding {
    /// Animate these insets with a 200 ms ease-in-out transition.
    pub fn new(padding: EdgeInsets, child: impl IntoView) -> Self {
        Self {
            padding,
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

    /// Select spring motion, retaining every component's incoming velocity.
    #[must_use]
    pub fn spring(mut self, spring: SpringDescription) -> Self {
        self.motion = MotionSpec::Spring(spring);
        self
    }
}

impl std::fmt::Debug for AnimatedPadding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedPadding")
            .field("padding", &self.padding)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the motion and the stable stream observed by the layout builder.
#[derive(Debug)]
pub struct AnimatedPaddingState {
    animation: AnimatedValue<EdgeInsets>,
    stream: Rc<AnimatedValueView<EdgeInsets>>,
    child: BoxedView,
}

impl StatefulView for AnimatedPadding {
    type State = AnimatedPaddingState;

    fn create_state(&self) -> Self::State {
        let initial = if self
            .padding
            .to_vector()
            .iter()
            .all(|component| component.is_finite())
        {
            self.padding
        } else {
            EdgeInsets::ZERO
        };
        let animation = AnimatedValue::new(initial, self.motion.clone(), None)
            .expect("BUG: initial padding components were made finite");
        let stream = Rc::new(animation.animation());
        AnimatedPaddingState {
            animation,
            stream,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedPadding> for AnimatedPaddingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.animation.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "padding animation has no clock");
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.init_state(ctx);
    }

    fn build(&self, _view: &AnimatedPadding, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let child = self.child.clone();
        AnimatedBuilder::new(self.stream.clone(), move || {
            // Layout receives non-negative padding even while motion overshoots.
            Padding::new(stream.value().clamp_non_negative()).child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedPadding, new_view: &AnimatedPadding) {
        self.child = new_view.child.clone();
        if let Err(error) = self
            .animation
            .retarget(new_view.padding, new_view.motion.clone())
        {
            tracing::warn!(%error, "padding motion refused; retaining the published run");
        }
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
