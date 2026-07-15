//! [`AnimatedContainer`] — animates several [`Container`] properties at once.

use std::time::Duration;

use flui_animation::Animation;
use flui_animation::curve::{ArcCurve, Curve};
use flui_geometry::EdgeInsets;
use flui_types::{Alignment, Color};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{
    DEFAULT_DURATION, ImplicitController, OptTween, default_curve,
};
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Container};

/// Animates [`Container`]'s alignment, padding, color, width, height, and margin
/// whenever any of them changes.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedContainer`. One
/// controller drives every property in lockstep over `duration` along `curve`.
/// A property animates only across a present→present change; a property that
/// appears or disappears snaps (no value to interpolate from/to). The
/// `decoration`, `constraints`, and `transform` of [`Container`] are not yet
/// animated (they pass straight through when set) — those need dedicated tweens
/// and are tracked as follow-up.
///
/// Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedContainer {
    alignment: Option<Alignment>,
    padding: Option<EdgeInsets>,
    color: Option<Color>,
    width: Option<f32>,
    height: Option<f32>,
    margin: Option<EdgeInsets>,
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
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Animate toward this fixed height.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Animate toward this outer margin.
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
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
    width: OptTween<f32>,
    height: OptTween<f32>,
    margin: OptTween<EdgeInsets>,
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
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedContainer> for AnimatedContainerState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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
        let child = self.child.clone();
        AnimatedBuilder::new(self.controller.listenable(), move || {
            let t = curved.value();
            let mut container = Container::new();
            if let Some(value) = alignment.current(t) {
                container = container.alignment(value);
            }
            if let Some(value) = padding.current(t) {
                container = container.padding(value);
            }
            if let Some(value) = color.current(t) {
                container = container.color(value);
            }
            if let Some(value) = width.current(t) {
                container = container.width(value);
            }
            if let Some(value) = height.current(t) {
                container = container.height(value);
            }
            if let Some(value) = margin.current(t) {
                container = container.margin(value);
            }
            container.child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedContainer, new_view: &AnimatedContainer) {
        self.child = new_view.child.clone();
        // Oracle: `controller.duration = widget.duration;` is unconditional
        // (`implicit_animations.dart:305`).
        self.controller.set_duration(new_view.duration);
        // Oracle: a curve-only change swaps the `CurvedAnimation` without
        // restarting (`implicit_animations.dart:298-303`). The swap must
        // happen BEFORE `t` is sampled below so a target-changed anchor
        // reads the already-updated curve — matches
        // `tween.evaluate(_animation)` reading the just-rebuilt `_animation`
        // at `implicit_animations.dart:310`. `build()` re-captures
        // `controller.curved()` fresh on every reconfigure (this widget
        // rebuilds via `AnimatedBuilder`), so there is no downstream
        // recompute to gate on the swap itself — only on a genuine target
        // change, which decides whether to restart below.
        self.controller.set_curve(new_view.curve.clone());
        let t = self.controller.value();
        // Re-anchor every property at the same instant; `|=` (not `||`) so each
        // property's tween is updated even after an earlier one already changed.
        let mut any_target_changed = false;
        any_target_changed |= self.alignment.retarget(new_view.alignment, t);
        any_target_changed |= self.padding.retarget(new_view.padding, t);
        any_target_changed |= self.color.retarget(new_view.color, t);
        any_target_changed |= self.width.retarget(new_view.width, t);
        any_target_changed |= self.height.retarget(new_view.height, t);
        any_target_changed |= self.margin.retarget(new_view.margin, t);
        if any_target_changed {
            // Oracle: `controller.forward(from: 0.0)`, gated strictly on
            // `_constructTweens()` (a target change) — a curve-only change
            // never restarts (`implicit_animations.dart:311-317`).
            self.controller.restart_from_zero();
        }
    }

    fn dispose(&mut self) {
        self.controller.dispose();
    }
}
