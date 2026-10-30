//! Interruptible value motion with one owning frame registration.
//!
//! [`AnimatedValue`] owns a coordinated curve or spring run over fixed-width
//! components. [`AnimatedValueView`] observes published samples without extending
//! the owner's lifetime. Motion changes and registry changes commit through the
//! corresponding update types before delivering notifications.

mod motion_update;
pub use motion_update::MotionUpdate;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::RecoveryScope;
use flui_foundation::{Listenable, ListenerCallback, ListenerId};
use smallvec::SmallVec;

use crate::animation::{Retirement, StatusObserver, Terminal};
use crate::retarget::{Reversal, Segment, eased_progress};
use crate::simulation::Simulation;
use crate::value_conversion::{AnimationVector, TwoWayConverter};
use crate::{
    Animation, AnimationController, AnimationError, AnimationRunFuture, AnimationStatus,
    DrivenController, MotionSpec, SharedCurve, Vsync, VsyncRegistrationError, curves,
};

mod sealed {
    pub trait GeneratedMotion {}
}

/// Only generated component runs can publish vector state inside a sample
/// commit. Staging invokes curves outside controller borrows; publication and
/// settlement only move Copy arrays.
pub(crate) trait ValueMotion: sealed::GeneratedMotion + Simulation {
    fn stage_sample(&self, time: f64, current: &dyn Fn() -> bool) -> bool;
    fn commit_sample(&self, elapsed: Duration);
    fn settle(&self);
}

#[derive(Clone, Copy)]
struct Sample<V> {
    value: V,
    velocity: V,
    elapsed: Duration,
    done: bool,
    representation: Representation,
}

#[derive(Clone, Copy)]
enum Representation {
    Origin,
    Vector,
    Target,
}

// Endpoint values need not survive a lossy component round trip. Retaining
// them until replacement also keeps user destruction out of sample commits.
struct ExactValues<T> {
    origin: Terminal<Rc<T>>,
    target: Terminal<Rc<T>>,
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

impl<V: AnimationVector> sealed::GeneratedMotion for ComponentRun<V> {}

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
        sample.representation = if sample.done {
            Representation::Target
        } else if elapsed.is_zero() {
            Representation::Origin
        } else {
            Representation::Vector
        };
        self.published.set(sample);
    }

    fn settle(&self) {
        self.published.set(Sample {
            value: self.goal,
            velocity: V::ZERO,
            elapsed: Duration::ZERO,
            done: true,
            representation: Representation::Target,
        });
    }
}

#[derive(Clone, Copy)]
enum Change {
    Animate,
    Snap,
}

struct Prepared<T: TwoWayConverter> {
    seam: crate::controller::ValueSeam,
    goal: T::Vector,
    run: Option<PreparedRun<T>>,
}

struct PreparedRun<T: TwoWayConverter> {
    source: Terminal<Rc<dyn ValueMotion>>,
    origin: Terminal<Rc<T>>,
    sample: Sample<T::Vector>,
    deadline: Option<Duration>,
    reversal: Reversal<T::Vector>,
}

struct Admission<T> {
    future: AnimationRunFuture,
    publication: crate::controller::ValuePublication,
    old_target: Terminal<Rc<T>>,
    old_motion: Terminal<MotionSpec>,
    old_visible: Terminal<ExactValues<T>>,
}

impl<T> Admission<T> {
    fn publish(self, recovery: &mut RecoveryScope<'_>) -> AnimationRunFuture {
        self.publication.publish(recovery);
        recovery.retire(self.old_visible);
        recovery.retire(self.old_motion);
        recovery.retire(self.old_target);
        self.future
    }
}

/// An interruptible value owned by one frame registration.
///
/// Every component inherits its last published position and velocity. The
/// owner controls the run and its registry seat; observer clones retain only
/// the published sample and its exact endpoints. Dropping the owner cancels
/// and unregisters the run.
///
/// ```
/// use flui_animation::{AnimatedValue, SharedCurve, curves, MotionClock, MotionSpec, Vsync};
/// use std::time::Duration;
///
/// let registry = Vsync::new();
/// let mut clock = MotionClock::new();
/// let motion = MotionSpec::Curve {
///     duration: Duration::from_millis(200),
///     curve: SharedCurve::new(curves::EASE_IN_OUT),
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
    exact_values: Terminal<Rc<RefCell<Terminal<ExactValues<T>>>>>,
    published: Rc<Cell<Sample<T::Vector>>>,
    deadline: Option<Duration>,
    reversal: Reversal<T::Vector>,
}

/// A cloneable view of an owning animated value.
///
/// Observers cannot move, register or dispose its controller. After owner
/// teardown, they keep the last published sample and its exact endpoints,
/// and report zero velocity.
pub struct AnimatedValueView<T: TwoWayConverter + 'static> {
    controller: AnimationController,
    exact_values: Rc<RefCell<Terminal<ExactValues<T>>>>,
    published: Rc<Cell<Sample<T::Vector>>>,
}

impl<T: TwoWayConverter + 'static> Clone for AnimatedValueView<T> {
    fn clone(&self) -> Self {
        Self {
            controller: self.controller.clone(),
            exact_values: Rc::clone(&self.exact_values),
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
    #[track_caller]
    pub fn new(
        initial: T,
        motion: MotionSpec,
        vsync: Option<&Vsync>,
    ) -> Result<Self, AnimationError> {
        Self::construct(None, initial, motion, vsync)
    }

    /// Like [`new`](Self::new), naming the owner's controller `label` in
    /// diagnostics (see
    /// [`AnimationControllerBuilder::debug_label`](crate::AnimationControllerBuilder::debug_label)).
    ///
    /// # Errors
    /// Refuses a non-finite initial component before constructing the owner.
    #[track_caller]
    pub fn with_debug_label(
        label: impl Into<std::borrow::Cow<'static, str>>,
        initial: T,
        motion: MotionSpec,
        vsync: Option<&Vsync>,
    ) -> Result<Self, AnimationError> {
        Self::construct(Some(label.into()), initial, motion, vsync)
    }

    #[track_caller]
    fn construct(
        label: Option<std::borrow::Cow<'static, str>>,
        initial: T,
        motion: MotionSpec,
        vsync: Option<&Vsync>,
    ) -> Result<Self, AnimationError> {
        let initial = Terminal::new(Rc::new(initial));
        let motion = Terminal::new(motion);
        let vector = initial.to_vector();
        crate::refusal::report(
            "AnimatedValue::new",
            || None,
            validate(&vector, crate::AnimationInput::InitialValue),
        )?;
        let published = Rc::new(Cell::new(Sample {
            value: vector,
            velocity: T::Vector::ZERO,
            elapsed: Duration::ZERO,
            done: true,
            representation: Representation::Target,
        }));
        let exact_values = Rc::new(RefCell::new(Terminal::new(ExactValues {
            origin: Terminal::new(Rc::clone(&initial)),
            target: Terminal::new(Rc::clone(&initial)),
        })));
        let mut builder = AnimationController::builder(Duration::from_secs(1)).unbounded();
        if let Some(label) = label {
            builder = builder.debug_label(label);
        }
        let driven = builder.build_on(vsync);
        Ok(Self {
            driven: Terminal::new(driven),
            motion,
            target: initial,
            target_vector: vector,
            exact_values: Terminal::new(exact_values),
            published,
            deadline: None,
            reversal: Reversal::from_start(vector),
        })
    }

    /// Retarget while retaining the last published component velocities.
    /// Repeating the same goal leaves the current run and deadline intact.
    ///
    /// # Errors
    /// Refuses non-finite inputs or unrepresentable motion before mutation.
    /// Repeated invalidation during preparation returns `ReentrantMotion`.
    pub fn animate_to(&mut self, target: T) -> Result<AnimationRunFuture, AnimationError> {
        let motion = Terminal::new(self.motion.get().clone());
        let admitted = self.replace_motion(Terminal::new(Rc::new(target)), motion, Change::Animate);
        self.driven
            .get()
            .controller()
            .reported("AnimatedValue::animate_to", admitted)
    }

    /// Change the target and motion in one admission, before canceling the
    /// displaced run or notifying observers. Curve-only changes preserve the
    /// remaining deadline; a changed duration configures the new segment.
    ///
    /// # Errors
    /// Refuses invalid inputs or repeated preparation reentry before mutation.
    /// A spring converter's invalid rest distance returns
    /// [`AnimationError::ComponentSimulation`] with the component index and
    /// the original [`crate::SimulationError`]
    /// without cancelling the previous run.
    pub fn retarget(
        &mut self,
        target: T,
        motion: MotionSpec,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let admitted = self.replace_motion(
            Terminal::new(Rc::new(target)),
            Terminal::new(motion),
            Change::Animate,
        );
        self.driven
            .get()
            .controller()
            .reported("AnimatedValue::retarget", admitted)
    }

    /// Change the motion while retaining the current value and velocity.
    /// A curve-only change keeps the old run's remaining deadline.
    ///
    /// # Errors
    /// Refuses unrepresentable motion without replacing the current run.
    pub fn set_motion(&mut self, motion: MotionSpec) -> Result<AnimationRunFuture, AnimationError> {
        let admitted = self.replace_motion(
            Terminal::new(Rc::clone(&self.target)),
            Terminal::new(motion),
            Change::Animate,
        );
        self.driven
            .get()
            .controller()
            .reported("AnimatedValue::set_motion", admitted)
    }

    /// Set an exact value immediately and cancel the displaced run.
    ///
    /// # Errors
    /// Refuses non-finite components without changing the published value.
    pub fn snap_to(&mut self, value: T) -> Result<(), AnimationError> {
        let admitted = self
            .replace_motion(
                Terminal::new(Rc::new(value)),
                Terminal::new(self.motion.get().clone()),
                Change::Snap,
            )
            .map(|_| ());
        self.driven
            .get()
            .controller()
            .reported("AnimatedValue::snap_to", admitted)
    }

    /// Move this owner to another registry, preserving the published sample time.
    ///
    /// # Errors
    /// Returns registration exhaustion after releasing the old seat.
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError> {
        self.driven.get_mut().rebind(vsync)
    }

    /// The owner of this value's controller and registry seat.
    pub(crate) fn driven_mut(&mut self) -> &mut DrivenController {
        self.driven.get_mut()
    }

    /// Release the registry seat and cancel the run. Idempotent.
    pub fn dispose(&mut self) {
        self.driven.get_mut().dispose();
    }

    fn dispose_with_recovery(&mut self, recovery: &mut RecoveryScope<'_>) {
        self.driven.get_mut().dispose_with_recovery(recovery);
    }

    /// The current published value.
    #[must_use]
    pub fn value(&self) -> T {
        read_value(self.published.get(), &self.exact_values)
    }

    /// Published component velocities per animation second, zero after stopping.
    #[must_use]
    pub fn velocity(&self) -> T::Vector {
        let sample = self.published.get();
        if self.is_settled() {
            T::Vector::ZERO
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
            exact_values: Rc::clone(&self.exact_values),
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
            let prepared = self.prepare(target, motion, change)?;
            match self.install(prepared, target, motion, recovery) {
                Err(AnimationError::ReentrantMotion) => {}
                Ok(admission) => return Ok(admission.publish(recovery)),
                Err(error) => return Err(error),
            }
        }
        Err(AnimationError::ReentrantMotion)
    }

    fn prepare(
        &self,
        target: &Terminal<Rc<T>>,
        motion: &Terminal<MotionSpec>,
        change: Change,
    ) -> Result<Prepared<T>, AnimationError> {
        self.prepare_from(target, motion, change, None)
    }

    fn prepare_from(
        &self,
        target: &Terminal<Rc<T>>,
        motion: &Terminal<MotionSpec>,
        change: Change,
        origin: Option<&Rc<T>>,
    ) -> Result<Prepared<T>, AnimationError> {
        let controller = self.driven.controller().clone();
        let seam = controller.value_seam()?;
        let goal = target.to_vector();
        validate(&goal, crate::AnimationInput::Target)?;
        let origin_vector = origin.map(|origin| origin.to_vector());
        if let Some(origin) = &origin_vector {
            validate(origin, crate::AnimationInput::From)?;
        }
        let target_changed = origin.is_some() || goal.as_ref() != self.target_vector.as_ref();
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
        let sample = origin_vector.map_or_else(
            || self.published.get(),
            |value| Sample {
                value,
                velocity: T::Vector::ZERO,
                elapsed: Duration::ZERO,
                done: false,
                representation: Representation::Origin,
            },
        );
        let velocity = if self.is_settled() || origin.is_some() {
            T::Vector::ZERO
        } else {
            sample.velocity
        };
        let mut reversal = Reversal::from_start(sample.value);
        let mut segment_motion = Terminal::new(motion.get().clone());
        if matches!(change, Change::Snap) {
            segment_motion = Terminal::new(MotionSpec::Curve {
                duration: Duration::ZERO,
                curve: SharedCurve::new(curves::LINEAR),
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
            && origin.is_none()
            && !self.is_settled()
        {
            let duration = self.deadline.unwrap_or(Duration::ZERO).as_secs_f64();
            reversal = self
                .reversal
                .interrupt(sample.value, goal, segment_motion.get(), || {
                    eased_progress(curve, sample.elapsed.as_secs_f64(), duration)
                        .map(|eased| (self.target_vector, eased))
                });
        }
        let thresholds = if matches!(segment_motion.get(), MotionSpec::Spring(_)) {
            T::rest_thresholds()
        } else {
            T::Vector::ZERO
        };
        let mut components = sample
            .value
            .as_ref()
            .iter()
            .zip(velocity.as_ref())
            .zip(goal.as_ref())
            .zip(thresholds.as_ref())
            .enumerate()
            .map(
                |(component, (((&position, &velocity), &target), &threshold))| {
                    let tolerance = if matches!(segment_motion.get(), MotionSpec::Spring(_)) {
                        crate::simulation::Tolerance::new(threshold, f64::INFINITY).map_err(
                            |error| AnimationError::ComponentSimulation {
                                component,
                                source: error,
                            },
                        )?
                    } else {
                        crate::simulation::Tolerance::DEFAULT
                    };
                    Segment::start(
                        position,
                        velocity,
                        target,
                        &segment_motion,
                        reversal.shortening(),
                        tolerance,
                    )
                    .map_err(|source| AnimationError::ComponentSimulation { component, source })
                },
            )
            .collect::<Result<SmallVec<[Segment; 4]>, _>>()?;
        if let Some(start) = components
            .iter()
            .filter_map(Segment::rest_start)
            .reduce(f64::max)
        {
            let duration = components
                .iter()
                .filter_map(|component| component.rest_duration_at(start))
                .reduce(f64::min)
                .expect("BUG: a spring rest start has a duration");
            for (index, component) in components.iter_mut().enumerate() {
                component
                    .synchronize_rest(start, duration)
                    .map_err(|source| AnimationError::ComponentSimulation {
                        component: index,
                        source,
                    })?;
            }
        }
        drop(segment_motion);
        let deadline = components.iter().filter_map(Segment::curve_duration).max();
        let immediate = components.iter().all(|component| component.is_done(0.0));
        let exact_origin = Terminal::new(if immediate {
            Rc::clone(target)
        } else {
            origin.map_or_else(|| exact_value(sample, &self.exact_values), Rc::clone)
        });
        let prepared = Sample {
            value: if immediate { goal } else { sample.value },
            velocity: if immediate { T::Vector::ZERO } else { velocity },
            elapsed: Duration::ZERO,
            done: immediate,
            representation: if immediate {
                Representation::Target
            } else {
                Representation::Origin
            },
        };
        let run = Terminal::new(Rc::new(ComponentRun {
            segments: components,
            goal,
            published: Rc::clone(&self.published),
            staged: Cell::new(prepared),
        }));
        let source = Rc::clone(run.get()) as Rc<dyn ValueMotion>;
        drop(run);
        Ok(Prepared {
            seam,
            goal,
            run: Some(PreparedRun {
                source: Terminal::new(source),
                origin: exact_origin,
                sample: prepared,
                deadline,
                reversal,
            }),
        })
    }

    fn install(
        &mut self,
        prepared: Prepared<T>,
        target: &mut Terminal<Rc<T>>,
        motion: &mut Terminal<MotionSpec>,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<Admission<T>, AnimationError> {
        let controller = self.driven.controller().clone();
        controller.validate_value_seam(&prepared.seam)?;
        let Some(mut run) = prepared.run else {
            let old_target = self.target.withdraw();
            let old_motion = self.motion.withdraw();
            self.target = target.withdraw();
            self.motion = motion.withdraw();
            let origin = Rc::clone(&self.exact_values.borrow().origin);
            let old_visible = self.exact_values.replace(Terminal::new(ExactValues {
                origin: Terminal::new(origin),
                target: Terminal::new(Rc::clone(&self.target)),
            }));
            // Equal vectors preserve the run while replacing the exact target.
            return Ok(Admission {
                future: controller.value_run_future(),
                publication: controller.admit_value_metadata(),
                old_target,
                old_motion,
                old_visible,
            });
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
                let old_visible = self.exact_values.replace(Terminal::new(ExactValues {
                    origin: run.origin.withdraw(),
                    target: Terminal::new(Rc::clone(&self.target)),
                }));
                self.published.set(run.sample);
                (old_target, old_motion, old_visible)
            },
            recovery,
        );
        match admission {
            Ok((future, (old_target, old_motion, old_visible), publication)) => Ok(Admission {
                future,
                publication,
                old_target,
                old_motion,
                old_visible,
            }),
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
            T::Vector::ZERO
        }
    }
}

impl<T: TwoWayConverter + 'static> Animation<T> for AnimatedValueView<T> {
    fn value(&self) -> T {
        read_value(self.published.get(), &self.exact_values)
    }
    fn status(&self) -> AnimationStatus {
        self.controller.status()
    }
    fn is_animating(&self) -> bool {
        self.controller.is_animating()
    }

    fn observe_status(&self, observer: StatusObserver) -> crate::StatusSubscription {
        self.controller.observe_status(observer)
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
        let controller = self.driven.controller().clone();
        let mut driven = self.driven.withdraw();
        let target = self.target.withdraw();
        let motion = self.motion.withdraw();
        let exact_values = self.exact_values.withdraw();
        let publication = driven.get_mut().prepare_dispose();
        let mut recovery = Retirement::new();
        controller.retire_with_delivery(
            move |scope| {
                scope.retire(motion);
                scope.retire(target);
                scope.retire(exact_values);
                scope.retire(driven);
            },
            &mut recovery.scope(),
        );
        publication.publish(&mut recovery.scope());
        recovery.finish();
    }
}

fn validate(
    vector: &impl AsRef<[f64]>,
    input: crate::AnimationInput,
) -> Result<(), AnimationError> {
    if let Some((component, &value)) = vector
        .as_ref()
        .iter()
        .enumerate()
        .find(|(_, value)| !value.is_finite())
    {
        Err(AnimationError::NonFiniteComponent {
            input,
            component,
            value,
        })
    } else {
        Ok(())
    }
}

fn read_value<T: TwoWayConverter>(
    sample: Sample<T::Vector>,
    values: &RefCell<Terminal<ExactValues<T>>>,
) -> T {
    match sample.representation {
        Representation::Vector => T::from_vector(sample.value),
        Representation::Origin | Representation::Target => {
            let exact = exact_value(sample, values);
            exact.as_ref().clone()
        }
    }
}

fn exact_value<T: TwoWayConverter>(
    sample: Sample<T::Vector>,
    values: &RefCell<Terminal<ExactValues<T>>>,
) -> Rc<T> {
    match sample.representation {
        Representation::Origin => Rc::clone(&values.borrow().origin),
        Representation::Target => Rc::clone(&values.borrow().target),
        Representation::Vector => Rc::new(T::from_vector(sample.value)),
    }
}
