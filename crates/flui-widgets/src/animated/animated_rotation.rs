//! [`AnimatedRotation`] — rotates its child, animating to each new [`Angle`].

use std::sync::Arc;
use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{Animatable, AnimatableExt, Animation, ProxyAnimation, Tween};
use flui_foundation::geometry::Angle;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::RotationTransition;
use crate::animated::implicitly_animated::{DEFAULT_DURATION, ImplicitController, default_curve};
use crate::animated::vsync_scope::VsyncScope;

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
/// Driven by a binding under a [`VsyncScope`].
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_foundation::geometry::Angle;
/// use flui_widgets::{AnimatedRotation, RotationPath, SizedBox};
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
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedRotation {
    /// Rotate `child` to `angle`, turning numerically, over the 200 ms default duration
    /// along an ease-in-out curve.
    pub fn new(angle: Angle, child: impl IntoView) -> Self {
        Self {
            angle,
            path: RotationPath::default(),
            duration: DEFAULT_DURATION,
            curve: default_curve(),
            child: child.into_view().boxed(),
        }
    }

    /// Choose how the rotation reaches a new angle.
    #[must_use]
    pub fn path(mut self, path: RotationPath) -> Self {
        self.path = path;
        self
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve; accepts any [`Curve`], including the overshooting
    /// elastic and back curves.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedRotation")
            .field("angle", &self.angle)
            .field("path", &self.path)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// The rotation in turns at a curved progress: [`RotationTransition`]'s input.
#[derive(Debug, Clone)]
struct Turns(Tween<Angle>);

impl Animatable<f64> for Turns {
    fn transform(&self, t: f64) -> f64 {
        self.0.transform(t).turns()
    }
}

/// State for [`AnimatedRotation`]: the controller, the angle tween it drives, and the
/// [`ProxyAnimation`] the persistent [`RotationTransition`] listens to.
#[derive(Debug)]
pub struct AnimatedRotationState {
    controller: ImplicitController,
    /// The last configured angle, compared to detect a new target. With
    /// [`RotationPath::Shorter`] the tween's end is an equivalent of it, not it.
    target: Angle,
    tween: Tween<Angle>,
    proxy: ProxyAnimation<f64>,
    child: BoxedView,
}

impl AnimatedRotationState {
    /// The tween over the curved controller, in turns; swapped into the proxy on a
    /// retarget or a curve change.
    fn compose(&self) -> Arc<dyn Animation<f64>> {
        let curved: Arc<dyn Animation<f64>> = Arc::new(self.controller.curved());
        Arc::new(Turns(self.tween).animate(curved))
    }
}

impl StatefulView for AnimatedRotation {
    type State = AnimatedRotationState;

    fn create_state(&self) -> Self::State {
        let controller = ImplicitController::new(self.duration, self.curve.clone());
        let tween = Tween::new(self.angle, self.angle);
        let curved: Arc<dyn Animation<f64>> = Arc::new(controller.curved());
        let proxy = ProxyAnimation::new(Arc::new(Turns(tween).animate(curved)));
        AnimatedRotationState {
            controller,
            target: self.angle,
            tween,
            proxy,
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedRotation> for AnimatedRotationState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.controller.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedRotation, _ctx: &dyn BuildContext) -> impl IntoView {
        RotationTransition::new(Arc::new(self.proxy.clone()), self.child.clone())
    }

    fn did_update_view(&mut self, _old_view: &AnimatedRotation, new_view: &AnimatedRotation) {
        self.child = new_view.child.clone();
        self.controller.set_duration(new_view.duration);
        // The curve swaps first, so the angle shown now is read on the new curve.
        let curve_changed = self.controller.set_curve(new_view.curve.clone());
        let target_changed = new_view.angle != self.target;
        if target_changed {
            let from = self.tween.transform(self.controller.value());
            let to = match new_view.path {
                RotationPath::Numeric => new_view.angle,
                // Measured from the angle shown now, so a retarget mid-turn never adds a
                // revolution.
                RotationPath::Shorter => new_view.angle.nearest_equivalent(from),
            };
            self.target = new_view.angle;
            self.tween = Tween::new(from, to);
            self.controller.restart_from_zero();
        }
        if target_changed || curve_changed {
            self.proxy.set_parent(self.compose());
        }
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}
