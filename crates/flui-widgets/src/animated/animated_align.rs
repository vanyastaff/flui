//! [`AnimatedAlign`] — animates its child's alignment when the target changes.

use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{Animatable, Animation};
use flui_types::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{
    DEFAULT_DURATION, ImplicitController, OptTween, default_curve,
};
use crate::animated::vsync_scope::VsyncScope;
use crate::{Align, AnimatedBuilder};

/// Animates the [`Alignment`] of its child within itself whenever a new
/// alignment is given.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedAlign`. First
/// build sits at the given alignment; each later build with a different
/// alignment animates the child's position over `duration` along `curve`.
/// Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedAlign {
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedAlign {
    /// Animate `child` toward `alignment`, with the 200 ms default duration and
    /// an ease-in-out curve.
    pub fn new(alignment: Alignment, child: impl IntoView) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            duration: DEFAULT_DURATION,
            curve: default_curve(),
            child: child.into_view().boxed(),
        }
    }

    /// Size the box to `factor` x the child's width (must be `>= 0`).
    #[must_use]
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size the box to `factor` x the child's height (must be `>= 0`).
    #[must_use]
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve; accepts any type implementing
    /// [`Curve`], including elastic and bounce curves.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedAlign {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedAlign")
            .field("alignment", &self.alignment)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedAlign`] — owns the persistent alignment animation.
#[derive(Debug)]
pub struct AnimatedAlignState {
    controller: ImplicitController,
    alignment: OptTween<Alignment>,
    width_factor: OptTween<f32>,
    height_factor: OptTween<f32>,
    child: BoxedView,
}

impl StatefulView for AnimatedAlign {
    type State = AnimatedAlignState;

    fn create_state(&self) -> Self::State {
        AnimatedAlignState {
            controller: ImplicitController::new(self.duration, self.curve.clone()),
            alignment: OptTween::at_rest(Some(self.alignment)),
            width_factor: OptTween::at_rest(self.width_factor),
            height_factor: OptTween::at_rest(self.height_factor),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedAlign> for AnimatedAlignState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.controller.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedAlign, _ctx: &dyn BuildContext) -> impl IntoView {
        let curved = self.controller.curved();
        let alignment = self.alignment.clone();
        let width_factor = self.width_factor.clone();
        let height_factor = self.height_factor.clone();
        let child = self.child.clone();
        AnimatedBuilder::new(self.controller.listenable(), move || {
            let t = curved.value();
            let mut align = Align::new(
                alignment
                    .current(t)
                    .expect("BUG: AnimatedAlign alignment tween is always set"),
            );
            if let Some(factor) = width_factor.current(t) {
                align = align.width_factor(factor);
            }
            if let Some(factor) = height_factor.current(t) {
                align = align.height_factor(factor);
            }
            align.child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedAlign, new_view: &AnimatedAlign) {
        self.child = new_view.child.clone();
        // `build()` re-captures `curved()`/`tween()` fresh on every genuine
        // reconfigure (this widget rebuilds via `AnimatedBuilder`, unlike
        // `AnimatedOpacity`), so there is no downstream recompute to gate —
        // the changed/unchanged report is intentionally discarded here.
        self.controller.set_duration(new_view.duration);
        self.controller.set_curve(new_view.curve.clone());
        let t = self.controller.value();
        let mut any_target_changed = false;
        any_target_changed |= self.alignment.retarget(Some(new_view.alignment), t);
        any_target_changed |= self.width_factor.retarget(new_view.width_factor, t);
        any_target_changed |= self.height_factor.retarget(new_view.height_factor, t);
        if any_target_changed {
            self.controller.restart_from_zero();
        }
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}
