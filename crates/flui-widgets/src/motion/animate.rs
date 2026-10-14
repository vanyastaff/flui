//! [`Animate`] — plays [`Effects`] or a [`Transition`] on one child.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animatable, Animation, AnimationController, AnimationError, AnimationRunFuture,
    AnimationStatus, ArcCurve, CurvedAnimation, DrivenController, StatusSubscription,
};
use flui_foundation::panic::{PanicRecovery, RecoveryScope};
use flui_objects::{ShimmerSweep, TranslationFraction};
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::effect::{Pose, Stages};
use super::effects::Effects;
use super::progress::Progress;
use super::transition::{Phase, Transition};
use crate::animated::VsyncScope;
use crate::transitions::{
    FadeTransition, RotationTransition, ScaleTransition, ShimmerMaskTransition, SlideTransition,
};

/// What moves an [`Animate`]'s progress.
///
/// Progress is a position on the choreography: for [`Effects`], `0` is the
/// start of the timeline and `1` its end; for a [`Transition`], `0` is absent
/// and `1` present. Every trigger but [`Controller`](Self::Controller) moves
/// the widget's own progress on the ambient [`VsyncScope`]'s
/// clock, linearly in time: a full run takes [`Effects::total`] (each entry
/// easing itself) or the transition's duration (read through its curve), and
/// a partial run its share of that. A reduced-motion presentation settles
/// each run on its next frame, so the end state paints at once.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum Trigger {
    /// Play from the start once, when the widget mounts.
    #[default]
    OnMount,
    /// Play from the start on mount, and again whenever a rebuild carries a
    /// different value. A rebuild with the same value leaves the run alone.
    OnChange(u64),
    /// Rest at this position (clamped to `0..=1`) on mount; a rebuild with a
    /// new position moves there from wherever progress is, so a reversal
    /// mid-run continues from the displayed pose without a jump.
    Target(f64),
    /// Follow an animation the caller drives, such as an
    /// `AnimationController` or a route's animation: its value is the
    /// progress, unchanged. For a [`Transition`], a run that leaves a settled
    /// end forwards plays the entry and one that leaves it in reverse plays
    /// the exit; a reversal mid-run keeps the side it started on.
    Controller(Rc<dyn Animation<f64>>),
}

impl fmt::Debug for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OnMount => f.write_str("OnMount"),
            Self::OnChange(value) => f.debug_tuple("OnChange").field(value).finish(),
            Self::Target(position) => f.debug_tuple("Target").field(position).finish(),
            Self::Controller(animation) => f
                .debug_tuple("Controller")
                .field(&animation.value())
                .finish(),
        }
    }
}

impl PartialEq for Trigger {
    /// Controllers compare by identity.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::OnMount, Self::OnMount) => true,
            (Self::OnChange(a), Self::OnChange(b)) => a == b,
            (Self::Target(a), Self::Target(b)) => a.to_bits() == b.to_bits(),
            (Self::Controller(a), Self::Controller(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// What an [`Animate`] plays.
#[derive(Clone, Debug, PartialEq)]
enum Choreography {
    Effects(Effects),
    /// Boxed: five effect slots per side dwarf a timeline's header.
    Transition(Box<Transition>),
}

impl Choreography {
    fn stages(&self) -> Stages {
        match self {
            Self::Effects(effects) => effects.stages(),
            Self::Transition(transition) => transition.stages(),
        }
    }

    /// How long a full run of the widget's own progress takes.
    fn duration(&self) -> Duration {
        match self {
            Self::Effects(effects) => effects.total(),
            Self::Transition(transition) => transition.run_duration(),
        }
    }

    /// The easing of the widget's own progress: none for a timeline, whose
    /// entries ease themselves along real time.
    fn curve(&self) -> Option<ArcCurve> {
        match self {
            Self::Effects(_) => None,
            Self::Transition(transition) => Some(transition.run_curve()),
        }
    }
}

/// Plays [`Effects`] or a [`Transition`] on its child, started by a
/// [`Trigger`] (by default, once on mount).
///
/// The child is wrapped in the explicit transitions its effects need —
/// [`FadeTransition`], [`SlideTransition`], [`ScaleTransition`],
/// [`RotationTransition`], and a shimmer mask innermost — each following the
/// widget's progress, so a frame repaints the transforms without rebuilding
/// the child. Which wrappers exist
/// depends only on which kinds of effect are configured; changing that set
/// rebuilds the child's subtree, while changing timings or values keeps it.
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::motion::{Animate, AnimateExt, Effects, Trigger};
///
/// let entrance = Effects::new().fade_in().slide_y(0.2).then().scale(0.9);
/// let card = SizedBox::new(80.0, 40.0).animate(entrance.clone());
/// let pulse = Animate::new(entrance, Text::new("3")).trigger(Trigger::OnChange(3));
/// # let _ = (card, pulse);
/// ```
#[derive(Clone, StatefulView)]
pub struct Animate {
    choreography: Choreography,
    trigger: Trigger,
    child: BoxedView,
}

impl Animate {
    /// Play `effects` on `child`.
    pub fn new(effects: Effects, child: impl IntoView) -> Self {
        Self::with(Choreography::Effects(effects), child)
    }

    /// Show `child` through `transition`. With [`Trigger::Target`], `1.0`
    /// shows it and `0.0` hides it; [`Trigger::OnMount`] plays the entry.
    ///
    /// The child stays mounted and laid out at `0.0`, painted in the pose its
    /// effects end on: it disappears only as far as they take it, so an exit
    /// without a fade (or a scale to zero) leaves it visible. Removing the
    /// child once its exit settles is the job of the widget that owns the
    /// children, such as a switcher or a list.
    pub fn transition(transition: Transition, child: impl IntoView) -> Self {
        Self::with(Choreography::Transition(Box::new(transition)), child)
    }

    fn with(choreography: Choreography, child: impl IntoView) -> Self {
        Self {
            choreography,
            trigger: Trigger::default(),
            child: child.into_view().boxed(),
        }
    }

    /// Start the choreography with `trigger` instead of on mount.
    #[must_use]
    pub fn trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }
}

impl fmt::Debug for Animate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Animate")
            .field("choreography", &self.choreography)
            .field("trigger", &self.trigger)
            .finish_non_exhaustive()
    }
}

/// Plays [`Effects`] on any view: `view.animate(effects)` is
/// [`Animate::new(effects, view)`](Animate::new).
pub trait AnimateExt: IntoView + Sized {
    /// Play `effects` on this view, once on mount unless
    /// [`trigger`](Animate::trigger) says otherwise.
    fn animate(self, effects: Effects) -> Animate {
        Animate::new(effects, self)
    }
}

impl<V: IntoView> AnimateExt for V {}

/// The choreography the stage animations read, with the side a transition is
/// playing.
#[derive(Debug)]
struct Source {
    choreography: Choreography,
    progress: Rc<Progress>,
    /// The last pose and the sample it was composed for, so the stages of one
    /// frame share one composition.
    last: Cell<Option<(u64, Phase, Pose)>>,
}

impl Source {
    fn pose(&self, progress: f64) -> Pose {
        let phase = self.progress.phase();
        let key = progress.to_bits();
        if let Some((cached, cached_phase, pose)) = self.last.get()
            && cached == key
            && cached_phase == phase
        {
            return pose;
        }
        let pose = match &self.choreography {
            Choreography::Effects(effects) => effects.pose(progress),
            Choreography::Transition(transition) => transition.pose(phase, progress),
        };
        self.last.set(Some((key, phase, pose)));
        pose
    }
}

/// One stage's value of the pose at the parent's progress.
struct Stage<T> {
    source: Rc<Source>,
    read: fn(&Pose) -> T,
}

impl<T> fmt::Debug for Stage<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stage")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl<T> Animatable for Stage<T> {
    type Value = T;

    fn transform(&self, t: f64) -> T {
        (self.read)(&self.source.pose(t))
    }
}

/// The animation each configured stage follows.
#[derive(Debug, Default)]
struct StageAnimations {
    fade: Option<Rc<dyn Animation<f64>>>,
    rotate: Option<Rc<dyn Animation<f64>>>,
    scale: Option<Rc<dyn Animation<f64>>>,
    slide: Option<Rc<dyn Animation<TranslationFraction>>>,
    shimmer: Option<Rc<dyn Animation<Option<ShimmerSweep>>>>,
}

impl StageAnimations {
    fn new(source: &Rc<Source>) -> Self {
        let progress: &Rc<dyn Animation<f64>> = &(Rc::clone(&source.progress) as _);
        fn stage<T: Clone + 'static>(
            source: &Rc<Source>,
            progress: &Rc<dyn Animation<f64>>,
            read: fn(&Pose) -> T,
        ) -> Rc<dyn Animation<T>> {
            let stage = Stage {
                source: Rc::clone(source),
                read,
            };
            Rc::new(stage.animate(Rc::clone(progress)))
        }
        let configured = source.choreography.stages();
        Self {
            fade: configured
                .fade
                .then(|| stage(source, progress, |pose| pose.opacity)),
            rotate: configured
                .rotate
                .then(|| stage(source, progress, |pose| pose.turns)),
            scale: configured
                .scale
                .then(|| stage(source, progress, |pose| pose.scale)),
            slide: configured
                .slide
                .then(|| stage(source, progress, |pose| pose.slide)),
            shimmer: configured
                .shimmer
                .then(|| stage(source, progress, |pose| pose.shimmer)),
        }
    }
}

/// Owns the widget's progress and the stage animations reading it.
pub struct AnimateState {
    /// The widget's own progress, linear in time: `animate_to` scales a run
    /// by the distance it covers, so a timeline plays in real time from
    /// anywhere.
    driver: DrivenController,
    /// The progress the stages read: the owned one (eased for a transition)
    /// or the caller's controller, with the side a transition plays.
    progress: Rc<Progress>,
    stages: StageAnimations,
    /// Latches a transition's side from the caller's controller.
    controller_phase: Option<StatusSubscription>,
    /// Whether the trigger plays once the clock is bound.
    plays_on_mount: bool,
}

impl fmt::Debug for AnimateState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimateState")
            .field("progress", &self.progress)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Animate {
    type State = AnimateState;

    fn create_state(&self) -> Self::State {
        // Bound to the ambient clock in `init_state`.
        let driver = AnimationController::builder(self.choreography.duration())
            .debug_label("Animate")
            .build_on(None);
        if let Trigger::Target(position) = self.trigger
            && position.is_finite()
        {
            driver.controller().set_value(position.clamp(0.0, 1.0));
        }
        let (progress, controller_phase) = match &self.trigger {
            Trigger::Controller(controller) => {
                let progress = Progress::new(Rc::clone(controller));
                let latch = latch_phase(controller, &progress);
                (progress, Some(latch))
            }
            _ => (
                Progress::new(own_progress(&driver, &self.choreography)),
                None,
            ),
        };
        let stages = StageAnimations::new(&source(&self.choreography, &progress));
        AnimateState {
            driver,
            progress,
            stages,
            controller_phase,
            plays_on_mount: matches!(self.trigger, Trigger::OnMount | Trigger::OnChange(_)),
        }
    }
}

/// The owned progress as the stages read it.
fn own_progress(driver: &DrivenController, choreography: &Choreography) -> Rc<dyn Animation<f64>> {
    let linear: Rc<dyn Animation<f64>> = Rc::new(driver.controller().clone());
    match choreography.curve() {
        Some(curve) => Rc::new(CurvedAnimation::new(linear, curve)),
        None => linear,
    }
}

/// Log a refused run; the displayed progress stays where it is.
fn admit(run: Result<AnimationRunFuture, AnimationError>) {
    if let Err(error) = run {
        tracing::warn!(%error, "Animate refused a run; keeping the displayed progress");
    }
}

fn source(choreography: &Choreography, progress: &Rc<Progress>) -> Rc<Source> {
    Rc::new(Source {
        choreography: choreography.clone(),
        progress: Rc::clone(progress),
        last: Cell::new(None),
    })
}

/// Follow `controller`'s direction: a run that leaves a settled end picks the
/// side, and a reversal mid-run keeps it. A side switch repaints the stages
/// even when the controller's value did not change.
fn latch_phase(controller: &Rc<dyn Animation<f64>>, progress: &Rc<Progress>) -> StatusSubscription {
    let status = controller.status();
    progress.set_phase(if status.is_reverse() {
        Phase::Exit
    } else {
        Phase::Enter
    });
    let last = Cell::new(status);
    let progress = Rc::downgrade(progress);
    controller.observe_status(Rc::new(
        move |status: AnimationStatus, recovery: &mut RecoveryScope<'_>| {
            if !last.replace(status).is_settled() {
                return;
            }
            let side = if status.is_forward() {
                Phase::Enter
            } else if status.is_reverse() {
                Phase::Exit
            } else {
                return;
            };
            if let Some(progress) = progress.upgrade() {
                progress.set_phase_with_recovery(side, recovery);
            }
        },
    ))
}

impl AnimateState {
    fn controller(&self) -> &AnimationController {
        self.driver.controller()
    }

    fn bind(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.driver.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "Animate has no clock");
        }
    }

    /// Play from the start toward the end, entering.
    fn replay(&self) {
        self.progress.set_phase(Phase::Enter);
        admit(self.controller().forward_from(Some(0.0)));
    }

    /// Move toward `position`. Leaving rest picks a transition's side from
    /// the direction; a run already moving keeps its side, so a reversal
    /// plays the same effects back from the displayed pose.
    fn move_to(&self, position: f64) {
        let controller = self.controller();
        if !controller.is_animating() {
            let current = controller.value();
            if position > current {
                self.progress.set_phase(Phase::Enter);
            } else if position < current {
                self.progress.set_phase(Phase::Exit);
            }
        }
        admit(controller.animate_to(position, None));
    }

    /// Hand progress to the caller's `controller`. The owned run stops, so
    /// nothing keeps requesting frames for a progress no stage reads. The
    /// previous controller's subscription retires only once the handoff is
    /// committed.
    fn adopt_controller(&mut self, controller: &Rc<dyn Animation<f64>>) {
        if let Err(error) = self.controller().stop() {
            tracing::warn!(%error, "Animate could not stop its own run");
        }
        let outgoing = self
            .controller_phase
            .replace(latch_phase(controller, &self.progress));
        self.reparent(Rc::clone(controller), outgoing);
    }

    /// Hand progress back from the caller's controller to the owned one,
    /// continuing from the controller's position. The controller's
    /// subscription retires only once the handoff is committed, so a
    /// panicking removal cannot leave progress attached to it.
    fn release_controller(&mut self, controller: &Rc<dyn Animation<f64>>, view: &Animate) {
        let outgoing = self.controller_phase.take();
        let from = controller.value();
        if from.is_finite() {
            self.controller().set_value(from.clamp(0.0, 1.0));
        }
        let to = match view.trigger {
            Trigger::Target(position) => position,
            Trigger::OnMount | Trigger::OnChange(_) | Trigger::Controller(_) => 1.0,
        };
        self.move_to(to);
        self.reparent(own_progress(&self.driver, &view.choreography), outgoing);
    }

    /// Point the stages at `parent`, then remove `outgoing`. Reparenting
    /// commits before the previous parent's subscriptions are removed; a
    /// panicking removal there or in `outgoing` propagates only after both
    /// have been retired.
    fn reparent(&self, parent: Rc<dyn Animation<f64>>, outgoing: Option<StatusSubscription>) {
        let mut recovery = PanicRecovery::new();
        recovery.run_with(|_| self.progress.set_parent(parent));
        if let Some(mut subscription) = outgoing {
            subscription.cancel_with_recovery(&mut recovery.scope());
            recovery.retire(subscription);
        }
        recovery.finish();
    }
}

/// Remove a controller's status subscription; a panicking removal propagates
/// once it has been retired.
fn retire_subscription(subscription: Option<StatusSubscription>) {
    let Some(mut subscription) = subscription else {
        return;
    };
    let mut recovery = PanicRecovery::new();
    subscription.cancel_with_recovery(&mut recovery.scope());
    recovery.retire(subscription);
    recovery.finish();
}

impl ViewState<Animate> for AnimateState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.bind(ctx);
        if std::mem::take(&mut self.plays_on_mount) {
            self.replay();
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.bind(ctx);
    }

    fn build(&self, view: &Animate, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut child = view.child.clone();
        if let Some(sweep) = &self.stages.shimmer {
            child = ShimmerMaskTransition::new(Rc::clone(sweep), child).boxed();
        }
        if let Some(turns) = &self.stages.rotate {
            child = RotationTransition::new(Rc::clone(turns), child).boxed();
        }
        if let Some(scale) = &self.stages.scale {
            child = ScaleTransition::new(Rc::clone(scale), child).boxed();
        }
        if let Some(slide) = &self.stages.slide {
            child = SlideTransition::new(Rc::clone(slide), child).boxed();
        }
        if let Some(opacity) = &self.stages.fade {
            child = FadeTransition::new(Rc::clone(opacity), child).boxed();
        }
        child
    }

    fn did_update_view(&mut self, old: &Animate, new: &Animate) {
        // Every infallible change commits first; each reparent, which can run
        // a caller's subscription removal, comes last and retires through
        // recovery, so an identical rebuild after a contained panic has
        // nothing left to repair.
        let rechoreographed = old.choreography != new.choreography;
        if rechoreographed {
            let duration = new.choreography.duration();
            self.controller().set_duration(duration);
            // A new duration applies to later runs; a choreography with
            // nothing to play settles the one in flight instead of letting it
            // drive frames for its remaining time.
            if duration.is_zero()
                && let Err(error) = self.controller().finish()
            {
                tracing::warn!(%error, "Animate could not settle its run");
            }
            self.stages = StageAnimations::new(&source(&new.choreography, &self.progress));
        }
        match (&old.trigger, &new.trigger) {
            (Trigger::Controller(a), Trigger::Controller(b)) if Rc::ptr_eq(a, b) => {}
            (_, Trigger::Controller(controller)) => self.adopt_controller(controller),
            (Trigger::Controller(controller), _) => self.release_controller(controller, new),
            (old_trigger, new_trigger) => {
                match (old_trigger, new_trigger) {
                    (Trigger::OnChange(a), Trigger::OnChange(b)) if a != b => self.replay(),
                    (_, Trigger::Target(position)) if old_trigger != new_trigger => {
                        self.move_to(*position);
                    }
                    _ => {}
                }
                if rechoreographed {
                    // The curve may have changed with the choreography.
                    self.reparent(own_progress(&self.driver, &new.choreography), None);
                }
            }
        }
    }

    fn dispose(&mut self) {
        let outgoing = self.controller_phase.take();
        self.driver.dispose();
        retire_subscription(outgoing);
    }
}
