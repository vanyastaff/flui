//! [`AnimatedLerp`] — implicit motion for any [`Lerp`] value, by re-anchored
//! progress.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec};
use flui_foundation::geometry::Lerp;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::AnimatedBuilder;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};
use crate::support::retirement::Terminal;

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
///
/// The anchors are user values whose destructors may panic; each is its own
/// terminal slot, so the second does not run while the first unwinds.
pub struct AnimatedLerpState<T: Lerp + PartialEq + 'static> {
    from: Terminal<T>,
    to: Terminal<T>,
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
            from: Terminal::new(self.target.clone()),
            to: Terminal::new(self.target.clone()),
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
        let (from, to) = (
            Terminal::new(T::clone(&self.from)),
            Terminal::new(T::clone(&self.to)),
        );
        let builder = Rc::clone(&view.builder);
        AnimatedBuilder::new(self.stream.clone(), move || {
            builder(&shown(&*from, &*to, stream.value()))
        })
    }

    fn did_update_view(&mut self, _old_view: &AnimatedLerp<T>, new_view: &AnimatedLerp<T>) {
        if new_view.target == *self.to {
            // A motion change alone applies to the run in progress.
            self.progress.retarget(1.0, &new_view.motion);
            return;
        }
        let shown = shown(&*self.from, &*self.to, self.progress.value());
        // The new anchors are installed by the restart's admission callback,
        // so they change exactly when the run does: a motion whose curve
        // fails during preparation leaves both the anchors and the run as
        // they were. Outgoing values (user types, whose destructors may panic
        // or reenter) retire only once the state is consistent. Every user
        // call (interpolation, clone) runs before the restart starts, so a
        // panic there leaves the state untouched. The outgoing values sit in
        // terminal slots while the restart delivers: a listener failing
        // there unwinds past them.
        let target = new_view.target.clone();
        // The incoming values wait in terminal slots too: a refused restart
        // never runs the callback, and they must retire on their own, not as
        // captures of a dropped closure.
        let mut incoming_from = Terminal::new(shown);
        let mut incoming_to = Terminal::new(target);
        let mut outgoing: Option<(Terminal<T>, Terminal<T>)> = None;
        let (from_slot, to_slot) = (&mut self.from, &mut self.to);
        self.progress.restart_then(0.0, 1.0, &new_view.motion, || {
            outgoing = Some((
                Terminal::new(std::mem::replace(
                    &mut **from_slot,
                    incoming_from.take_value(),
                )),
                Terminal::new(std::mem::replace(&mut **to_slot, incoming_to.take_value())),
            ));
        });
        // Each value retires on its own, so a second panicking destructor
        // cannot run while the first unwinds; the first failure resumes.
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        if let Some((retired_from, retired_to)) = outgoing {
            recovery.retire(retired_from);
            recovery.retire(retired_to);
        }
        recovery.retire(incoming_from);
        recovery.retire(incoming_to);
        recovery.finish();
    }

    fn dispose(&mut self) {
        self.progress.dispose();
    }
}

/// The value at `progress`: exactly `from` or `to` at the two ends, since
/// an interpolation need not round-trip and a settled run publishes its
/// target, and the interpolation everywhere else, overshoot included.
#[expect(
    clippy::float_cmp,
    reason = "a run starts and settles at exactly 0 and 1"
)]
fn shown<T: Lerp>(from: &T, to: &T, progress: f64) -> T {
    if progress == 1.0 {
        to.clone()
    } else if progress == 0.0 {
        from.clone()
    } else {
        from.lerp_to(to, progress)
    }
}
