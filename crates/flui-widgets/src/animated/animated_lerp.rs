//! [`AnimatedLerp`] — implicit motion for any [`Lerp`] value, by re-anchored
//! progress.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec};
use flui_foundation::geometry::Lerp;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::AnimatedBuilder;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

type ValueBuilder<T> = Rc<dyn Fn(&T) -> BoxedView>;

/// Animates `target` implicitly for a value that interpolates but has no
/// vector form, such as a text style or a theme, and builds its subtree from
/// the value shown.
///
/// A change re-anchors: the run goes from the value shown to the new target,
/// so nothing jumps, but unlike [`Animated`](crate::Animated) the value's own
/// velocity does not carry over. Use [`Animated`](crate::Animated) for a
/// [`TwoWayConverter`](flui_animation::TwoWayConverter) value.
///
/// ```
/// use flui_painting::styling::Color;
/// use flui_widgets::{AnimatedLerp, ColoredBox, MotionBuilder};
/// use std::time::Duration;
///
/// let tint = AnimatedLerp::new(Color::rgb(30, 90, 200), |color: &Color| ColoredBox::new(*color))
///     .duration(Duration::from_millis(150));
/// # let _ = tint;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedLerp<T: Lerp + PartialEq + 'static> {
    target: T,
    motion: MotionSpec,
    builder: ValueBuilder<T>,
}

impl<T: Lerp + PartialEq + 'static> AnimatedLerp<T> {
    /// Animate toward `target` with the default motion, building the subtree
    /// from each value shown.
    pub fn new<V, F>(target: T, builder: F) -> Self
    where
        V: IntoView,
        F: Fn(&T) -> V + 'static,
    {
        Self {
            target,
            motion: MotionSpec::default(),
            builder: Rc::new(move |value| builder(value).into_view().boxed()),
        }
    }
}

impl<T: Lerp + PartialEq + 'static> MotionBuilder for AnimatedLerp<T> {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl<T: Lerp + PartialEq + 'static> std::fmt::Debug for AnimatedLerp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedLerp")
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// The value the current run starts from and moves to, and its progress.
pub struct AnimatedLerpState<T: Lerp + PartialEq + 'static> {
    from: T,
    to: T,
    progress: ImplicitMotion<f64>,
    stream: Rc<AnimatedValueView<f64>>,
}

impl<T: Lerp + PartialEq + 'static> std::fmt::Debug for AnimatedLerpState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedLerpState").finish_non_exhaustive()
    }
}

impl<T: Lerp + PartialEq + 'static> StatefulView for AnimatedLerp<T> {
    type State = AnimatedLerpState<T>;

    fn create_state(&self) -> Self::State {
        let progress = ImplicitMotion::new("AnimatedLerp", 1.0, 1.0, &self.motion);
        let stream = Rc::new(progress.stream());
        AnimatedLerpState {
            from: self.target.clone(),
            to: self.target.clone(),
            progress,
            stream,
        }
    }
}

impl<T: Lerp + PartialEq + 'static> ViewState<AnimatedLerp<T>> for AnimatedLerpState<T> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn build(&self, view: &AnimatedLerp<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let (from, to) = (self.from.clone(), self.to.clone());
        let builder = Rc::clone(&view.builder);
        AnimatedBuilder::new(self.stream.clone(), move || {
            builder(&from.lerp_to(&to, stream.value()))
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedLerp<T>, new_view: &AnimatedLerp<T>) {
        if new_view.target == self.to {
            return;
        }
        let shown = self.from.lerp_to(&self.to, self.progress.value());
        if self.progress.restart(0.0, 1.0, &new_view.motion) {
            self.from = shown;
            self.to = new_view.target.clone();
        }
    }

    fn dispose(&mut self) {
        self.progress.dispose();
    }
}
