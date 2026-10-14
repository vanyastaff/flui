//! [`Animated`] — implicit motion for any interpolable value, handed to a
//! builder each frame.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, AnimationVector, MotionSpec, TwoWayConverter};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::AnimatedBuilder;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};

type ValueBuilder<T> = Rc<dyn Fn(&T) -> BoxedView>;

/// Animates `target` implicitly and builds its subtree from the value shown.
///
/// This is the implicit widget for a value no named widget covers: any type
/// that implements [`TwoWayConverter`], including one that derives it. The
/// first build shows `target`; a later build with a different target moves
/// from the value currently shown, keeping its velocity. Only the builder's
/// subtree rebuilds per frame.
///
/// A first target with a non-finite component starts from the all-zero
/// value instead; a later non-finite target is refused and the current
/// motion continues.
///
/// ```
/// use flui_widgets::{Animated, MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let bar = Animated::new(120.0, |width: &f64| SizedBox::new(*width, 8.0))
///     .duration(Duration::from_millis(300));
/// # let _ = bar;
/// ```
#[derive(Clone, StatefulView)]
pub struct Animated<T: TwoWayConverter + 'static> {
    target: T,
    motion: MotionSpec,
    builder: ValueBuilder<T>,
}

impl<T: TwoWayConverter + 'static> Animated<T> {
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

impl<T: TwoWayConverter + 'static> MotionBuilder for Animated<T> {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl<T: TwoWayConverter + 'static> std::fmt::Debug for Animated<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Animated")
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the motion and the stable stream the builder observes.
pub struct AnimatedState<T: TwoWayConverter + 'static> {
    animation: ImplicitMotion<T>,
    stream: Rc<AnimatedValueView<T>>,
}

impl<T: TwoWayConverter + 'static> std::fmt::Debug for AnimatedState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedState").finish_non_exhaustive()
    }
}

impl<T: TwoWayConverter + 'static> StatefulView for Animated<T> {
    type State = AnimatedState<T>;

    fn create_state(&self) -> Self::State {
        let animation = ImplicitMotion::new(
            "Animated",
            self.target.clone(),
            T::from_vector(T::Vector::ZERO),
            &self.motion,
        );
        let stream = Rc::new(animation.stream());
        AnimatedState { animation, stream }
    }
}

impl<T: TwoWayConverter + 'static> ViewState<Animated<T>> for AnimatedState<T> {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.animation.bind(ctx);
    }

    fn build(&self, view: &Animated<T>, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let builder = Rc::clone(&view.builder);
        AnimatedBuilder::new(self.stream.clone(), move || builder(&stream.value()))
    }

    fn did_update_view(&mut self, _old_view: &Animated<T>, new_view: &Animated<T>) {
        self.animation
            .retarget(new_view.target.clone(), &new_view.motion);
    }

    fn dispose(&mut self) {
        self.animation.dispose();
    }
}
