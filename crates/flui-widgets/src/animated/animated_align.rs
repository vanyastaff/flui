//! [`AnimatedAlign`] — animates its child's alignment when the target changes.

use std::time::Duration;

use flui_animation::curve::Cubic;
use flui_animation::{Animatable, Animation, Curves};
use flui_types::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, BuildContextExt, IntoView, ViewExt, ViewState};

use crate::animated::implicitly_animated::{DEFAULT_DURATION, ImplicitAnimation};
use crate::animated::vsync_scope::VsyncScope;
use crate::{Align, AnimatedBuilder};

/// Animates the [`Alignment`] of its child within itself whenever a new
/// alignment is given.
///
/// Flutter parity: `widgets/implicit_animations.dart` `AnimatedAlign`. First
/// build sits at the given alignment; each later build with a different
/// alignment animates the child's position over `duration` along `curve`.
/// Driven by a binding under a [`VsyncScope`](crate::VsyncScope).
#[derive(Clone, StatefulView)]
pub struct AnimatedAlign {
    alignment: Alignment,
    duration: Duration,
    curve: Cubic,
    child: BoxedView,
}

impl AnimatedAlign {
    /// Animate `child` toward `alignment`, with the 200 ms default duration and
    /// an ease-in-out curve.
    pub fn new(alignment: Alignment, child: impl IntoView) -> Self {
        Self {
            alignment,
            duration: DEFAULT_DURATION,
            curve: Curves::EaseInOut,
            child: child.into_view().boxed(),
        }
    }

    /// Override the transition duration.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Override the easing curve.
    #[must_use]
    pub fn curve(mut self, curve: Cubic) -> Self {
        self.curve = curve;
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
    animation: ImplicitAnimation<Alignment>,
    child: BoxedView,
}

impl StatefulView for AnimatedAlign {
    type State = AnimatedAlignState;

    fn create_state(&self) -> Self::State {
        AnimatedAlignState {
            animation: ImplicitAnimation::new(self.alignment, self.duration, self.curve),
            child: self.child.clone(),
        }
    }
}

impl ViewState<AnimatedAlign> for AnimatedAlignState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            self.animation.register(vsync);
        }
    }

    fn build(&self, _view: &AnimatedAlign, _ctx: &dyn BuildContext) -> impl IntoView {
        let curved = self.animation.curved();
        let tween = self.animation.tween();
        let child = self.child.clone();
        AnimatedBuilder::new(self.animation.listenable(), move || {
            Align::new(tween.transform(curved.value())).child(child.clone())
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedAlign, new_view: &AnimatedAlign) {
        self.child = new_view.child.clone();
        self.animation
            .retarget(new_view.alignment, new_view.duration);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
