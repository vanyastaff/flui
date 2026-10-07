//! [`AnimatedContainer`] — animates several [`Container`] properties at once.

use std::time::Duration;

use flui_animation::Animation;
use flui_animation::curve::{ArcCurve, Curve};
use flui_foundation::geometry::{EdgeInsets, Matrix4};
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{
    DEFAULT_DURATION, ImplicitController, OptTween, default_curve,
};
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Container};

/// Animates [`Container`]'s alignment, padding, color, width, height, margin and
/// transform whenever any of them changes.
///
/// One controller drives every property in lockstep over `duration` along
/// `curve`. A property animates only across a present→present change; a property
/// that appears or disappears snaps (no value to interpolate from/to). An
/// overshooting curve may carry a value past its target; padding, margin, width
/// and height are clamped at zero, the other properties extrapolate.
///
/// Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedContainer {
    alignment: Option<Alignment>,
    padding: Option<EdgeInsets>,
    color: Option<Color>,
    width: Option<f64>,
    height: Option<f64>,
    margin: Option<EdgeInsets>,
    transform: Option<Matrix4>,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedContainer {
    /// An animated container wrapping `child`, with no properties set yet, the
    /// 200 ms default duration, and an ease-in-out curve.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            alignment: None,
            padding: None,
            color: None,
            width: None,
            height: None,
            margin: None,
            transform: None,
            duration: DEFAULT_DURATION,
            curve: default_curve(),
            child: child.into_view().boxed(),
        }
    }

    /// Animate toward this alignment of the child within the container.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Animate toward this inner padding.
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Animate toward this background color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Animate toward this fixed width.
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// Animate toward this fixed height.
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Animate toward this outer margin.
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Animate toward this paint transform, applied about the container's origin.
    ///
    /// The matrix interpolates by decomposition ([`Matrix4::lerp`]): rotation turns
    /// along the shorter arc, and a scale that starts or ends at zero takes the other
    /// end's rotation, so a scale-in from nothing grows without spinning.
    #[must_use]
    pub fn transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
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

impl std::fmt::Debug for AnimatedContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedContainer")
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedContainer`] — one shared controller plus a tween per
/// animatable property.
#[derive(Debug)]
pub struct AnimatedContainerState {
    controller: ImplicitController,
    alignment: OptTween<Alignment>,
    padding: OptTween<EdgeInsets>,
    color: OptTween<Color>,
    width: OptTween<f64>,
    height: OptTween<f64>,
    margin: OptTween<EdgeInsets>,
    transform: OptTween<Matrix4>,
    child: BoxedView,
}

impl StatefulView for AnimatedContainer {
    type State = AnimatedContainerState;

    fn create_state(&self) -> Self::State {
        AnimatedContainerState {
            controller: ImplicitController::new(self.duration, self.curve.clone()),
            alignment: OptTween::at_rest(self.alignment),
            padding: OptTween::at_rest(self.padding),
            color: OptTween::at_rest(self.color),
            width: OptTween::at_rest(self.width),
            height: OptTween::at_rest(self.height),
            margin: OptTween::at_rest(self.margin),
            transform: OptTween::at_rest(self.transform),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedContainer> for AnimatedContainerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.controller.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedContainer, _ctx: &dyn BuildContext) -> impl IntoView {
        let curved = self.controller.curved();
        let alignment = self.alignment.clone();
        let padding = self.padding.clone();
        let color = self.color.clone();
        let width = self.width.clone();
        let height = self.height.clone();
        let margin = self.margin.clone();
        let transform = self.transform.clone();
        let child = self.child.clone();
        AnimatedBuilder::new(self.controller.listenable(), move || {
            // An overshooting curve extrapolates the tweens past their targets; the
            // insets and the size are clamped into their non-negative domain here,
            // where they meet the property (ADR-0149).
            let t = curved.value();
            let mut container = Container::new();
            if let Some(value) = alignment.current(t) {
                container = container.alignment(value);
            }
            if let Some(value) = padding.current(t) {
                container = container.padding(value.clamp_non_negative());
            }
            if let Some(value) = color.current(t) {
                container = container.color(value);
            }
            if let Some(value) = width.current(t) {
                container = container.width(value.max(0.0));
            }
            if let Some(value) = height.current(t) {
                container = container.height(value.max(0.0));
            }
            if let Some(value) = margin.current(t) {
                container = container.margin(value.clamp_non_negative());
            }
            if let Some(value) = transform.current(t) {
                container = container.transform(value);
            }
            container.child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedContainer, new_view: &AnimatedContainer) {
        self.child = new_view.child.clone();
        // The duration is pushed unconditionally.
        self.controller.set_duration(new_view.duration);
        // A curve-only change swaps the `CurvedAnimation` without
        // restarting. The swap must
        // happen BEFORE `t` is sampled below so a target-changed anchor
        // reads the already-updated curve. `build()` re-captures
        // `controller.curved()` fresh on every reconfigure (this widget
        // rebuilds via `AnimatedBuilder`), so there is no downstream
        // recompute to gate on the swap itself — only on a genuine target
        // change, which decides whether to restart below.
        self.controller.set_curve(new_view.curve.clone());
        let t = self.controller.value();
        // All properties share the controller, so a change to any one restarts it
        // and re-anchors every property at the same instant.
        let restart = self.alignment.animates_toward(new_view.alignment.as_ref())
            || self.padding.animates_toward(new_view.padding.as_ref())
            || self.color.animates_toward(new_view.color.as_ref())
            || self.width.animates_toward(new_view.width.as_ref())
            || self.height.animates_toward(new_view.height.as_ref())
            || self.margin.animates_toward(new_view.margin.as_ref())
            || self.transform.animates_toward(new_view.transform.as_ref());
        self.alignment.retarget(new_view.alignment, t, restart);
        self.padding.retarget(new_view.padding, t, restart);
        self.color.retarget(new_view.color, t, restart);
        self.width.retarget(new_view.width, t, restart);
        self.height.retarget(new_view.height, t, restart);
        self.margin.retarget(new_view.margin, t, restart);
        self.transform.retarget(new_view.transform, t, restart);
        if restart {
            // Restart from zero, gated strictly on a target change — a
            // curve-only change never restarts.
            self.controller.restart_from_zero();
        }
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}
