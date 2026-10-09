//! One owning frame registration for a vector of interruptible components.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use smallvec::SmallVec;

use super::{AnimationVector, TwoWayConverter, ValueMotion};
use crate::animation::{Retirement, StatusObserver, Terminal};
use crate::curve::Curve;
use crate::retarget::Segment;
use crate::simulation::Simulation;
use crate::{
    Animation, AnimationController, AnimationError, AnimationRunFuture, AnimationStatus, ArcCurve,
    Curves, DrivenController, MotionSpec, StatusCallback, Vsync, VsyncRegistrationError,
};

#[derive(Clone, Copy)]
struct Sample<V> {
    value: V,
    velocity: V,
    elapsed: Duration,
    done: bool,
}

struct ComponentRun<V: AnimationVector> {
    segments: SmallVec<[Segment; 4]>,
    goal: V,
    published: Rc<Cell<Sample<V>>>,
    staged: Cell<Sample<V>>,
}

impl<V: AnimationVector> Simulation for ComponentRun<V> {
    fn x(&self, time: f64) -> f64 {
        time
    }

    fn dx(&self, _time: f64) -> f64 {
        1.0
    }

    fn is_done(&self, _time: f64) -> bool {
        self.staged.get().done
    }
}

impl<V: AnimationVector> super::sealed::GeneratedMotion for ComponentRun<V> {}

impl<V: AnimationVector> ValueMotion for ComponentRun<V> {
    fn stage_sample(&self, time: f64, current: &dyn Fn() -> bool) -> bool {
        let mut next = self.published.get();
        next.done = true;
        for ((position, velocity), segment) in next
            .value
            .as_mut()
            .iter_mut()
            .zip(next.velocity.as_mut())
            .zip(&self.segments)
        {
            *position = segment.x(time);
            if !current() {
                return false;
            }
            *velocity = segment.dx(time);
            if !current() {
                return false;
            }
            next.done &= segment.is_done(time);
        }
        self.staged.set(next);
        true
    }
    fn commit_sample(&self, elapsed: Duration) {
        let mut sample = self.staged.get();
        sample.elapsed = elapsed;
        self.published.set(sample);
    }

    fn settle(&self) {
        self.published.set(Sample {
            value: self.goal,
            velocity: self.goal.zero(),
            elapsed: Duration::ZERO,
            done: true,
        });
    }
}

#[derive(Clone, Copy)]
struct Reversal<V> {
    adjusted_start: V,
    shortening: f64,
}

#[derive(Clone, Copy)]
enum Change {
    Animate,
    Snap,
}

struct Prepared<V: AnimationVector> {
    seam: crate::controller::ValueSeam,
    goal: V,
    run: Option<PreparedRun<V>>,
}

struct PreparedRun<V: AnimationVector> {
    source: Terminal<Rc<dyn ValueMotion>>,
    sample: Sample<V>,
    deadline: Option<Duration>,
    reversal: Reversal<V>,
}

/// An interruptible value owned by one frame registration.
///
/// Every component inherits its last published position and velocity. The
/// owner controls the run and its registry seat; observer clones retain only
/// the published value. Dropping the owner cancels and unregisters the run.
///
/// ```
/// use flui_animation::{AnimatedValue, ArcCurve, Curves, MotionClock, MotionSpec, Vsync};
/// use std::time::Duration;
///
/// let registry = Vsync::new();
/// let mut clock = MotionClock::new();
/// let motion = MotionSpec::Curve {
///     duration: Duration::from_millis(200),
///     curve: ArcCurve::new(Curves::EaseInOut),
/// };
/// let mut value = AnimatedValue::new(0.0_f64, motion, Some(&registry)).expect("finite value");
/// let old = value.animate_to(1.0).expect("finite target");
/// registry.tick_all(&clock.frame(Duration::ZERO));
/// registry.tick_all(&clock.frame(Duration::from_millis(50)));
/// let seam = (value.value(), value.velocity());
/// let next = value.animate_to(-1.0).expect("finite target");
/// assert!(old.is_canceled() && next.is_pending());
/// assert_eq!((value.value(), value.velocity()), seam);
/// ```
#[must_use = "dropping an AnimatedValue cancels its motion"]
pub struct AnimatedValue<T: TwoWayConverter + 'static> {
    driven: Terminal<DrivenController>,
    motion: Terminal<MotionSpec>,
    target: Terminal<Rc<T>>,
    target_vector: T::Vector,
    visible_target: Rc<RefCell<Terminal<Rc<T>>>>,
    published: Rc<Cell<Sample<T::Vector>>>,
    deadline: Option<Duration>,
    reversal: Reversal<T::Vector>,
}

/// A cloneable view of an owning animated value.
///
/// Observers cannot move, register or dispose its controller. After owner
/// teardown, they keep the last published value and report zero velocity.
pub struct AnimatedValueView<T: TwoWayConverter + 'static> {
    controller: AnimationController,
    target: Rc<RefCell<Terminal<Rc<T>>>>,
    published: Rc<Cell<Sample<T::Vector>>>,
}

impl<T: TwoWayConverter + 'static> Clone for AnimatedValueView<T> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
            target: Rc::clone(&self.target),
            published: Rc::clone(&self.published),
        }
    }
}

impl<T: TwoWayConverter + 'static> AnimatedValue<T> {
    /// Create a resting value, with the given motion and optional frame registry.
    /// An unbound owner settles newly admitted motion synchronously.
    ///
    /// # Errors
    /// Refuses a non-finite initial component before constructing the owner.
    pub fn new(
        initial: T,
        motion: MotionSpec,
        vsync: Option<&Vsync>,
    ) -> Result<Self, AnimationError> {
        let initial = Terminal::new(Rc::new(initial));
        let motion = Terminal::new(motion);
        let vector = initial.to_vector();
        validate(&vector)?;
        let published = Rc::new(Cell::new(Sample {
            value: vector,
            velocity: vector.zero(),
            elapsed: Duration::ZERO,
            done: true,
        }));
        let visible_target = Rc::new(RefCell::new(Terminal::new(Rc::clone(&initial))));
        let driven = AnimationController::builder(Duration::from_secs(1))
            .unbounded()
            .build_on(vsync);
        Ok(Self {
            driven: Terminal::new(driven),
            motion,
            target: initial,
            target_vector: vector,
            visible_target,
            published,
            deadline: None,
            reversal: Reversal {
                adjusted_start: vector,
                shortening: 1.0,
            },
        })
    }

    /// Retarget while retaining the last published component velocities.
    /// Repeating the same goal leaves the current run and deadline intact.
    ///
    /// # Errors
    /// Refuses non-finite inputs or unrepresentable motion before mutation.
    /// Repeated invalidation during preparation returns `ReentrantMotion`.
    pub fn animate_to(&mut self, target: T) -> Result<AnimationRunFuture, AnimationError> {
        self.retarget(target, self.motion.get().clone())
    }

    /// Change the target and motion in one admission, before canceling the
    /// displaced run or notifying observers. Curve-only changes preserve the
    /// remaining deadline; a changed duration configures the new segment.
    ///
    /// # Errors
    /// Refuses invalid inputs or repeated preparation reentry before mutation.
    pub fn retarget(
        &mut self,
        target: T,
        motion: MotionSpec,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.replace_motion(
            Terminal::new(Rc::new(target)),
            Terminal::new(motion),
            Change::Animate,
        )
    }

    /// Change the motion while retaining the current value and velocity.
    /// A curve-only change keeps the old run's remaining deadline.
    ///
    /// # Errors
    /// Refuses unrepresentable motion without replacing the current run.
    pub fn set_motion(&mut self, motion: MotionSpec) -> Result<AnimationRunFuture, AnimationError> {
        self.replace_motion(
            Terminal::new(Rc::clone(&self.target)),
            Terminal::new(motion),
            Change::Animate,
        )
    }

    /// Set an exact value immediately and cancel the displaced run.
    ///
    /// # Errors
    /// Refuses non-finite components without changing the published value.
    pub fn snap_to(&mut self, value: T) -> Result<(), AnimationError> {
        self.replace_motion(
            Terminal::new(Rc::new(value)),
            Terminal::new(self.motion.get().clone()),
            Change::Snap,
        )
        .map(|_| ())
    }

    /// Move this owner to another registry, preserving the published sample time.
    ///
    /// # Errors
    /// Returns registration exhaustion after releasing the old seat.
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError> {
        self.driven.get_mut().rebind(vsync)
    }

    /// Release the registry seat and cancel the run. Idempotent.
    pub fn dispose(&mut self) {
        self.driven.get_mut().dispose();
    }

    /// The current published value.
    #[must_use]
    pub fn value(&self) -> T {
        read_value(self.published.get(), &self.visible_target)
    }

    /// Published component velocities per animation second, zero after stopping.
    #[must_use]
    pub fn velocity(&self) -> T::Vector {
        let sample = self.published.get();
        if self.is_settled() {
            sample.velocity.zero()
        } else {
            sample.velocity
        }
    }

    /// The current exact target.
    #[must_use]
    pub fn target(&self) -> &T {
        self.target.get()
    }

    /// Whether the owning controller has stopped.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        !self.driven.controller().is_animating()
    }

    /// Observe this value without acquiring its logical ownership.
    #[must_use]
    pub fn animation(&self) -> AnimatedValueView<T> {
        AnimatedValueView {
            controller: self.driven.controller().clone(),
            target: Rc::clone(&self.visible_target),
            published: Rc::clone(&self.published),
        }
    }

    fn replace_motion(
        &mut self,
        mut target: Terminal<Rc<T>>,
        mut motion: Terminal<MotionSpec>,
        change: Change,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut recovery = Retirement::new();
        let mut result = Err(AnimationError::ReentrantMotion);
        recovery.run_with(|recovery| {
            result = self.prepare_and_install(&mut target, &mut motion, change, recovery);
        });
        recovery.retire(target);
        recovery.retire(motion);
        recovery.finish();
        result
    }

    fn prepare_and_install(
        &mut self,
        target: &mut Terminal<Rc<T>>,
        motion: &mut Terminal<MotionSpec>,
        change: Change,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        for _ in 0..2 {
            let prepared = self.prepare(target, motion, change, recovery)?;
            match self.install(prepared, target, motion, recovery) {
                Err(AnimationError::ReentrantMotion) => {}
                result => return result,
            }
        }
        Err(AnimationError::ReentrantMotion)
    }

    fn prepare(
        &self,
        target: &Terminal<Rc<T>>,
        motion: &Terminal<MotionSpec>,
        change: Change,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<Prepared<T::Vector>, AnimationError> {
        let controller = self.driven.controller().clone();
        let seam = controller.value_seam()?;
        let goal = target.to_vector();
        validate(&goal)?;
        let target_changed = goal.as_ref() != self.target_vector.as_ref();
        if !matches!(change, Change::Snap)
            && !target_changed
            && (motion.get() == self.motion.get() || self.is_settled())
        {
            return Ok(Prepared {
                seam,
                goal,
                run: None,
            });
        }
        let sample = self.published.get();
        let velocity = if self.is_settled() {
            sample.velocity.zero()
        } else {
            sample.velocity
        };
        let mut shortening = 1.0;
        let mut reversal = Reversal {
            adjusted_start: sample.value,
            shortening: 1.0,
        };
        let mut segment_motion = Terminal::new(motion.get().clone());
        if matches!(change, Change::Snap) {
            segment_motion = Terminal::new(MotionSpec::Curve {
                duration: Duration::ZERO,
                curve: ArcCurve::new(Curves::Linear),
            });
        } else if !target_changed {
            if let (
                MotionSpec::Curve { duration: new, .. },
                MotionSpec::Curve { duration: old, .. },
            ) = (segment_motion.get(), self.motion.get())
                && new == old
                && !self.is_settled()
                && let Some(deadline) = self.deadline
                && let MotionSpec::Curve { duration, .. } = segment_motion.get_mut()
            {
                *duration = deadline.saturating_sub(sample.elapsed);
            }
        } else if let MotionSpec::Curve { curve, .. } = self.motion.get()
            && !self.is_settled()
            && goal.as_ref() == self.reversal.adjusted_start.as_ref()
        {
            let old = self.reversal.shortening;
            let duration = self.deadline.unwrap_or(Duration::ZERO).as_secs_f64();
            if duration > 0.0 {
                shortening = (curve
                    .transform((sample.elapsed.as_secs_f64() / duration).clamp(0.0, 1.0))
                    * old
                    + (1.0 - old))
                    .abs()
                    .clamp(0.0, 1.0);
                reversal = Reversal {
                    adjusted_start: self.target_vector,
                    shortening,
                };
            }
        }
        let components = sample
            .value
            .as_ref()
            .iter()
            .zip(velocity.as_ref())
            .zip(goal.as_ref())
            .map(|((&position, &velocity), &target)| {
                Segment::start(position, velocity, target, &segment_motion, shortening)
            })
            .collect::<Result<SmallVec<[Segment; 4]>, _>>()
            .map_err(|error| {
                AnimationError::NonFiniteTarget(format!("animated value motion: {error}"))
            })?;
        recovery.retire(segment_motion);
        let deadline = components.iter().filter_map(Segment::curve_duration).max();
        let immediate = components.iter().all(|component| component.is_done(0.0));
        let prepared = Sample {
            value: if immediate { goal } else { sample.value },
            velocity: if immediate { goal.zero() } else { velocity },
            elapsed: Duration::ZERO,
            done: immediate,
        };
        let run = Terminal::new(Rc::new(ComponentRun {
            segments: components,
            goal,
            published: Rc::clone(&self.published),
            staged: Cell::new(prepared),
        }));
        let source = Rc::clone(run.get()) as Rc<dyn ValueMotion>;
        recovery.retire(run);
        Ok(Prepared {
            seam,
            goal,
            run: Some(PreparedRun {
                source: Terminal::new(source),
                sample: prepared,
                deadline,
                reversal,
            }),
        })
    }

    fn install(
        &mut self,
        prepared: Prepared<T::Vector>,
        target: &mut Terminal<Rc<T>>,
        motion: &mut Terminal<MotionSpec>,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let controller = self.driven.controller().clone();
        controller.validate_value_seam(&prepared.seam)?;
        let Some(mut run) = prepared.run else {
            let old_target = self.target.withdraw();
            let old_motion = self.motion.withdraw();
            self.target = target.withdraw();
            self.motion = motion.withdraw();
            let old_visible = self
                .visible_target
                .replace(Terminal::new(Rc::clone(&self.target)));
            // Equal vectors preserve the run while replacing the exact target.
            controller.publish_value_metadata(recovery);
            recovery.retire(old_visible);
            recovery.retire(old_target);
            recovery.retire(old_motion);
            return Ok(controller.value_run_future());
        };
        let admission = controller.start_value_motion(
            &prepared.seam,
            run.source.withdraw().into_inner(),
            run.sample.done,
            || {
                let old_target = self.target.withdraw();
                let old_motion = self.motion.withdraw();
                self.reversal = run.reversal;
                self.target_vector = prepared.goal;
                self.target = target.withdraw();
                self.motion = motion.withdraw();
                self.deadline = run.deadline;
                let old_visible = self
                    .visible_target
                    .replace(Terminal::new(Rc::clone(&self.target)));
                self.published.set(run.sample);
                (old_target, old_motion, old_visible)
            },
            recovery,
        );
        match admission {
            Ok((future, (old_target, old_motion, old_visible))) => {
                recovery.retire(old_visible);
                recovery.retire(old_motion);
                recovery.retire(old_target);
                Ok(future)
            }
            Err(error) => Err(error),
        }
    }
}

impl<T: TwoWayConverter + 'static> AnimatedValueView<T> {
    /// The velocity of the last published sample, or zero after owner teardown.
    #[must_use]
    pub fn velocity(&self) -> T::Vector {
        let sample = self.published.get();
        if self.controller.is_animating() {
            sample.velocity
        } else {
            sample.velocity.zero()
        }
    }
}

impl<T: TwoWayConverter + 'static> Animation<T> for AnimatedValueView<T> {
    fn value(&self) -> T {
        read_value(self.published.get(), &self.target)
    }
    fn status(&self) -> AnimationStatus {
        self.controller.status()
    }
    fn is_animating(&self) -> bool {
        self.controller.is_animating()
    }

    fn subscribe_status(&self, callback: StatusCallback) -> crate::StatusSubscription {
        self.controller.subscribe_status(callback)
    }
    fn subscribe_status_observer(&self, observer: StatusObserver) -> crate::StatusSubscription {
        self.controller.subscribe_status_observer(observer)
    }
}

impl<T: TwoWayConverter + 'static> Listenable for AnimatedValueView<T> {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.controller.add_listener(callback)
    }
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        self.controller.add_observer(observer)
    }
    fn remove_listener(&self, id: ListenerId) {
        self.controller.remove_listener(id);
    }
    fn remove_all_listeners(&self) {
        self.controller.remove_all_listeners();
    }
}

impl<T: TwoWayConverter + 'static> fmt::Debug for AnimatedValueView<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimatedValueView")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl<T: TwoWayConverter + 'static> fmt::Debug for AnimatedValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimatedValue")
            .field("motion", self.motion.get())
            .field("settled", &self.is_settled())
            .finish_non_exhaustive()
    }
}

impl<T: TwoWayConverter + 'static> Drop for AnimatedValue<T> {
    fn drop(&mut self) {
        let mut driven = self.driven.withdraw();
        let target = self.target.withdraw();
        let motion = self.motion.withdraw();
        let mut recovery = Retirement::new();
        recovery.run(|| driven.get_mut().dispose());
        recovery.retire(motion);
        recovery.retire(target);
        recovery.retire(driven);
        recovery.finish();
    }
}

fn validate(vector: &impl AsRef<[f64]>) -> Result<(), AnimationError> {
    if vector
        .as_ref()
        .iter()
        .all(|component| component.is_finite())
    {
        Ok(())
    } else {
        Err(AnimationError::NonFiniteTarget(
            "animated value components must be finite".into(),
        ))
    }
}

fn read_value<T: TwoWayConverter>(
    sample: Sample<T::Vector>,
    target: &RefCell<Terminal<Rc<T>>>,
) -> T {
    if sample.done {
        let target = Rc::clone(target.borrow().get());
        target.as_ref().clone()
    } else {
        T::from_vector(sample.value)
    }
}
