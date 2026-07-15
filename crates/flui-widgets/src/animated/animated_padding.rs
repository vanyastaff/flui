//! [`AnimatedPadding`] — animates its child's padding when the target changes.

use std::time::Duration;

use flui_animation::curve::{ArcCurve, Curve};
use flui_animation::{Animatable, Animation};
use flui_geometry::EdgeInsets;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, ImplicitAnimation, default_curve};
use crate::animated::vsync_scope::VsyncScope;
use crate::{AnimatedBuilder, Padding};

/// Animates the [`EdgeInsets`] padding around its child whenever a new padding
/// is given.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedPadding`. First
/// build sits at the given padding; each later build with different insets
/// animates from the current insets to the new ones over `duration` along
/// `curve`. Driven by a binding under a [`VsyncScope`].
#[derive(Clone, StatefulView)]
pub struct AnimatedPadding {
    padding: EdgeInsets,
    duration: Duration,
    curve: ArcCurve,
    child: BoxedView,
}

impl AnimatedPadding {
    /// Animate `child`'s surrounding `padding`, with the 200 ms default duration
    /// and an ease-in-out curve.
    pub fn new(padding: EdgeInsets, child: impl IntoView) -> Self {
        Self {
            padding,
            duration: DEFAULT_DURATION,
            curve: default_curve(),
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
    /// [`Curve`], including elastic and bounce curves.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.curve = ArcCurve::new(curve);
        self
    }
}

impl std::fmt::Debug for AnimatedPadding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedPadding")
            .field("padding", &self.padding)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

/// State for [`AnimatedPadding`] — owns the persistent padding animation.
#[derive(Debug)]
pub struct AnimatedPaddingState {
    animation: ImplicitAnimation<EdgeInsets>,
    child: BoxedView,
}

impl StatefulView for AnimatedPadding {
    type State = AnimatedPaddingState;

    fn create_state(&self) -> Self::State {
        AnimatedPaddingState {
            animation: ImplicitAnimation::new(self.padding, self.duration, self.curve.clone()),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedPadding> for AnimatedPaddingState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.animation.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedPadding, _ctx: &dyn BuildContext) -> impl IntoView {
        let curved = self.animation.curved();
        let tween = self.animation.tween();
        let child = self.child.clone();
        AnimatedBuilder::new(self.animation.listenable(), move || {
            Padding::new(tween.transform(curved.value())).child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedPadding, new_view: &AnimatedPadding) {
        self.child = new_view.child.clone();
        // `build()` re-captures `curved()`/`tween()` fresh on every genuine
        // reconfigure (this widget rebuilds via `AnimatedBuilder`, unlike
        // `AnimatedOpacity`), so there is no downstream recompute to gate —
        // the changed/unchanged report is intentionally discarded here.
        let _ =
            self.animation
                .retarget(new_view.padding, new_view.duration, new_view.curve.clone());
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
