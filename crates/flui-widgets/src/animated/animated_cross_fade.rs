//! [`AnimatedCrossFade`] — fades between two children and animates its size
//! between theirs.

use std::rc::Rc;

use flui_animation::{AnimatedValueView, Animation, MotionSpec};
use flui_painting::Alignment;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use crate::animated::animated_size::AnimatedSize;
use crate::animated::implicitly_animated::{ImplicitMotion, MotionBuilder};
use crate::{AnimatedBuilder, ExcludeSemantics, IgnorePointer, Opacity, Positioned, Stack};

/// Which of an [`AnimatedCrossFade`]'s two children is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossFadeState {
    /// The first child.
    #[default]
    ShowFirst,
    /// The second child.
    ShowSecond,
}

/// Fades between two children and animates its size from one child's to the
/// other's.
///
/// The shown child sizes the box; the other is stretched over it while it
/// fades. Only the shown child receives pointer events and appears in the
/// semantics tree, so a fading-out child cannot be tapped or announced. Both
/// children keep their state across a switch. Interrupting a fade reverses
/// it from the opacity shown, keeping its velocity.
///
/// The fade and the size follow the same configured motion.
///
/// ```
/// use flui_widgets::{AnimatedCrossFade, CrossFadeState, MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let details = AnimatedCrossFade::new(
///     SizedBox::new(100.0, 20.0),
///     SizedBox::new(100.0, 80.0),
///     CrossFadeState::ShowSecond,
/// )
/// .duration(Duration::from_millis(300));
/// # let _ = details;
/// ```
#[derive(Clone, StatefulView)]
pub struct AnimatedCrossFade {
    first: BoxedView,
    second: BoxedView,
    shown: CrossFadeState,
    alignment: Alignment,
    motion: MotionSpec,
}

impl AnimatedCrossFade {
    /// Show `shown` of `first` and `second`, with the default motion.
    pub fn new(first: impl IntoView, second: impl IntoView, shown: CrossFadeState) -> Self {
        Self {
            first: first.into_view().boxed(),
            second: second.into_view().boxed(),
            shown,
            alignment: Alignment::TOP_CENTER,
            motion: MotionSpec::default(),
        }
    }

    /// Where the children sit while the box is larger than the one shown.
    /// Defaults to the top center.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    fn progress(shown: CrossFadeState) -> f64 {
        match shown {
            CrossFadeState::ShowFirst => 0.0,
            CrossFadeState::ShowSecond => 1.0,
        }
    }
}

impl MotionBuilder for AnimatedCrossFade {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl std::fmt::Debug for AnimatedCrossFade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedCrossFade")
            .field("shown", &self.shown)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

/// Owns the fade progress, `0` showing the first child and `1` the second,
/// and the selection the motion admitted, which layout, hit testing and
/// semantics follow.
#[derive(Debug)]
pub struct AnimatedCrossFadeState {
    shown: CrossFadeState,
    progress: ImplicitMotion<f64>,
    stream: Rc<AnimatedValueView<f64>>,
}

impl StatefulView for AnimatedCrossFade {
    type State = AnimatedCrossFadeState;

    fn create_state(&self) -> Self::State {
        let progress = ImplicitMotion::new(
            "AnimatedCrossFade",
            Self::progress(self.shown),
            0.0,
            &self.motion,
        );
        let stream = Rc::new(progress.stream());
        AnimatedCrossFadeState {
            shown: self.shown,
            progress,
            stream,
        }
    }
}

impl ViewState<AnimatedCrossFade> for AnimatedCrossFadeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.progress.bind(ctx);
    }

    fn build(&self, view: &AnimatedCrossFade, _ctx: &dyn BuildContext) -> impl IntoView {
        let stream = Rc::clone(&self.stream);
        let first = view.first.clone();
        let second = view.second.clone();
        let second_shown = self.shown == CrossFadeState::ShowSecond;
        let fading = AnimatedBuilder::new(self.stream.clone(), move || {
            let t = stream.value().clamp(0.0, 1.0);
            Stack::new(vec![
                layer(first.clone(), 1.0 - t, !second_shown),
                layer(second.clone(), t, second_shown),
            ])
        });
        AnimatedSize::new()
            .motion(view.motion.clone())
            .alignment(view.alignment)
            .child(fading)
    }

    fn did_update_view(&mut self, _old_view: &AnimatedCrossFade, new_view: &AnimatedCrossFade) {
        // The selection commits with the admitted run, before any listener
        // runs; a refused retarget keeps the run shown, and the selection
        // stays with it.
        let shown = &mut self.shown;
        self.progress.retarget_then(
            AnimatedCrossFade::progress(new_view.shown),
            &new_view.motion,
            || *shown = new_view.shown,
        );
    }

    fn dispose(&mut self) {
        self.progress.dispose();
    }
}

/// One child at `opacity`. The shown child sizes the stack; the other is
/// stretched over it and kept out of hit testing and semantics. The wrapper
/// types never change, so each child keeps its element across a switch.
fn layer(child: BoxedView, opacity: f64, shown: bool) -> BoxedView {
    let faded = ExcludeSemantics::new()
        .excluding(!shown)
        .child(Opacity::new(opacity).child(child));
    let routed = IgnorePointer::new().ignoring(!shown).child(faded);
    if shown {
        Positioned::new(routed).into_view().boxed()
    } else {
        Positioned::fill(routed).into_view().boxed()
    }
}
