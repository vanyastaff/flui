//! [`AnimatedRotation`] — rotates its child, animating to each new [`Angle`].

use std::rc::Rc;

use flui_animation::{MotionSpec, ProxyAnimation};
use flui_foundation::geometry::Angle;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::RotationTransition;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

/// Which way an [`AnimatedRotation`] turns toward a new angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RotationPath {
    /// Turn through the numeric difference: from 0° to 270° is three quarters of a turn
    /// forward, and from 0° to 720° is two full turns.
    #[default]
    Numeric,
    /// Turn along the shorter arc to the same orientation: from 0° to 270° is a quarter
    /// turn back. An exact half turn goes toward the increasing angle.
    Shorter,
}

/// Rotates its child about its center, animating to each new angle it is given.
///
/// The first build shows the child at `angle` with no motion; a later build with a
/// different angle turns from the angle currently shown to the new one over `duration`
/// along `curve`, the way [`path`](Self::path) says. Rotation is paint-only: the child is
/// laid out unrotated.
///
/// Driven by a binding under a [`VsyncScope`](crate::VsyncScope).
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_foundation::geometry::Angle;
/// use flui_widgets::{AnimatedRotation, MotionBuilder, RotationPath, SizedBox};
///
/// let dial = AnimatedRotation::new(Angle::from_degrees(270.0), SizedBox::new(40.0, 40.0))
///     .path(RotationPath::Shorter)
///     .duration(Duration::from_millis(300));
/// # let _ = dial;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedRotation {
    angle: Angle,
    path: RotationPath,
    motion: MotionSpec,
    child: BoxedView,
}

impl AnimatedRotation {
    /// Rotate `child` to `angle`, turning numerically, over the 200 ms default duration
    /// along an ease-in-out curve.
    pub fn new(angle: Angle, child: impl IntoView) -> Self {
        Self {
            angle,
            path: RotationPath::default(),
            motion: MotionSpec::default(),
            child: child.into_view().boxed(),
        }
    }

    /// Choose how the rotation reaches a new angle.
    #[must_use]
    pub fn path(mut self, path: RotationPath) -> Self {
        self.path = path;
        self
    }
}

impl MotionBuilder for AnimatedRotation {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedRotation")
            .field("angle", &self.angle)
            .field("path", &self.path)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns angular motion and the stable stream observed by the rotation transition.
#[derive(Debug)]
pub struct AnimatedRotationState {
    animation: ImplicitMotion<f64>,
    /// The last configured angle, compared to detect a new target. With
    /// [`RotationPath::Shorter`] the motion's goal is an equivalent of it.
    target: Angle,
    /// The path the running motion was laid out for.
    path: RotationPath,
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl StatefulView for AnimatedRotation {
    type State = AnimatedRotationState;

    fn create_state(&self) -> Self::State {
        let animation =
            ImplicitMotion::new("AnimatedRotation", self.angle.turns(), 0.0, &self.motion);
        let proxy = ProxyAnimation::new(Rc::new(animation.stream()));
        AnimatedRotationState {
            animation,
            target: self.angle,
            path: self.path,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedRotation> for AnimatedRotationState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, _view: &AnimatedRotation, _ctx: &dyn BuildContext) -> impl IntoView {
        RotationTransition::new(Rc::new(self.proxy.clone()), self.child.clone())
    }

    fn did_update_view(&mut self, _old_view: &AnimatedRotation, new_view: &AnimatedRotation) {
        self.child = new_view.child.clone();
        let target_changed = new_view.angle != self.target || new_view.path != self.path;
        let target = if target_changed {
            match new_view.path {
                RotationPath::Numeric => new_view.angle.turns(),
                RotationPath::Shorter => new_view
                    .angle
                    .nearest_equivalent(Angle::from_turns(self.animation.value()))
                    .turns(),
            }
        } else {
            // An unrelated rebuild retains the chosen equivalent and its deadline.
            *self.animation.target()
        };
        if self.animation.retarget(target, &new_view.motion) {
            self.target = new_view.angle;
            self.path = new_view.path;
        }
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
