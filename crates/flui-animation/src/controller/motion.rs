//! Interruptible runs owned by the controller.

use std::rc::Rc;
use std::time::Duration;

use super::{
    AnimationController, AnimationControllerInner, AnimationDirection, Opaque, RetiredSources,
    Retirement, RunStart, SampleIdentity, SimulationRun, TickSource, ValueChange,
};
use crate::retarget::Segment;
use crate::run_future::RunCompleter;
use crate::{AnimationError, AnimationRunFuture, MotionSpec};

struct Seam {
    value: f64,
    generation: u64,
    epoch: u64,
    source: Option<TickSource>,
    cycle: f64,
    playback: f64,
    span: f64,
    duration: Duration,
    origin: RunStart,
}

/// Identity and origin of a typed value's published sample. Preparation may
/// invoke a converter or curve, so admission must still match this seam.
pub(crate) struct ValueSeam {
    generation: u64,
    epoch: u64,
    origin: RunStart,
}

impl ValueSeam {
    fn matches(&self, inner: &AnimationControllerInner) -> bool {
        !inner.disposed
            && inner.run_generation == self.generation
            && inner.sample_epoch == self.epoch
    }
}

fn continuation(inner: &AnimationControllerInner) -> RunStart {
    match inner.run_start {
        origin @ RunStart::Continue { .. } if inner.last_elapsed.is_zero() => origin,
        _ => RunStart::Continue {
            generation: inner.run_generation,
            elapsed: inner.last_elapsed,
        },
    }
}

impl Seam {
    fn capture(inner: &AnimationControllerInner) -> Self {
        Self {
            value: inner.value,
            generation: inner.run_generation,
            epoch: inner.sample_epoch,
            source: inner.active_run.as_ref().map(|_| inner.tick_source()),
            cycle: inner.cycle_elapsed_secs(),
            playback: inner.playback_rate.get(),
            span: inner.target_value - inner.start_value,
            duration: inner.current_duration(),
            origin: continuation(inner),
        }
    }

    fn matches(&self, inner: &AnimationControllerInner) -> bool {
        !inner.disposed
            && inner.run_generation == self.generation
            && inner.sample_epoch == self.epoch
            && inner.value == self.value
            && inner.active_run.is_some() == self.source.is_some()
    }

    fn velocity(&self) -> Result<f64, AnimationError> {
        if self.playback == 0.0 {
            return Ok(0.0);
        }
        let velocity = match &self.source {
            None => 0.0,
            Some(TickSource::Simulation(source)) => source.source().dx(self.cycle),
            Some(TickSource::Repeat(_)) => self.span / self.duration.as_secs_f64(),
            Some(TickSource::Time { curve, .. }) => {
                if self.duration.is_zero() {
                    0.0
                } else {
                    let progress = (self.cycle / self.duration.as_secs_f64()).clamp(0.0, 1.0);
                    let slope = curve.as_ref().map_or(1.0, |curve| curve.slope(progress));
                    crate::retarget::rate(self.span, slope, self.duration.as_secs_f64())
                }
            }
        };
        if velocity.is_finite() {
            Ok(velocity)
        } else {
            Err(AnimationError::NonFiniteTarget(format!(
                "retarget inherited velocity {velocity} is not finite"
            )))
        }
    }
}

impl AnimationController {
    pub(crate) fn publish_value_metadata(&self, recovery: &mut Retirement) {
        let inner = self.inner.borrow_mut();
        let status = inner.status;
        self.finish_with_retirement(
            status,
            ValueChange::Notify,
            None,
            RetiredSources::new(),
            inner,
            recovery,
        );
    }

    pub(crate) fn value_run_future(&self) -> AnimationRunFuture {
        self.inner
            .borrow()
            .active_run
            .as_ref()
            .map_or_else(AnimationRunFuture::complete, RunCompleter::future)
    }

    pub(crate) fn value_seam(&self) -> Result<ValueSeam, AnimationError> {
        let inner = self.inner.borrow();
        Self::check_run_admission(&inner)?;
        Ok(ValueSeam {
            generation: inner.run_generation,
            epoch: inner.sample_epoch,
            origin: continuation(&inner),
        })
    }

    /// The closure only moves prepared framework fields; it must not call or
    /// drop user code. Returned outgoing ownership retires after unlocking.
    pub(crate) fn start_value_motion<R>(
        &self,
        seam: &ValueSeam,
        source: Rc<dyn crate::spring::ValueMotion>,
        immediate: bool,
        commit: impl FnOnce() -> R,
        recovery: &mut Retirement,
    ) -> Result<(AnimationRunFuture, R), AnimationError> {
        let mut source = Opaque::new(source);
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;
        if !seam.matches(&inner) {
            drop(inner);
            recovery.retire(source);
            return Err(AnimationError::ReentrantMotion);
        }
        let mut retired = RetiredSources::new();
        inner.clear_run_modes(&mut retired);
        inner.value = 0.0;
        inner.start_value = 0.0;
        inner.target_value = 0.0;
        inner.direction = AnimationDirection::Forward;
        inner.status = inner.direction.running_status();
        inner.simulation = Some(SimulationRun::Value(source.take()));
        Self::begin_run(&mut inner);
        inner.run_start = seam.origin;
        if immediate {
            inner.run_duration = Some(Duration::ZERO);
            inner.settle_pending = true;
        }
        let outgoing = commit();
        let (completer, future) = AnimationRunFuture::pending();
        let displaced = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);
        let status = inner.status;
        self.finish_with_retirement(
            status,
            ValueChange::Notify,
            displaced,
            retired,
            inner,
            recovery,
        );
        Ok((future, outgoing))
    }

    /// Velocity at the last sampled run time, in value units per second.
    ///
    /// Curved runs use the curve's derivative, including the applied playback
    /// rate. Stopped and paused runs report zero. User curves and simulations
    /// run outside the state borrow; if they invalidate the sampled run, this
    /// read reports zero instead of publishing a stale velocity. A derivative
    /// that cannot be represented as a finite value also reports zero.
    #[must_use]
    pub fn velocity(&self) -> f64 {
        let (source, identity, cycle, playback, span, duration) = {
            let inner = self.inner.borrow();
            if inner.active_run.is_none() || inner.playback_rate.is_paused() {
                return 0.0;
            }
            (
                Opaque::new(inner.tick_source()),
                SampleIdentity {
                    generation: inner.run_generation,
                    epoch: inner.sample_epoch,
                },
                inner.cycle_elapsed_secs(),
                inner.playback_rate.get(),
                inner.target_value - inner.start_value,
                inner.current_duration(),
            )
        };
        let mut recovery = Retirement::new();
        let mut velocity = 0.0;
        recovery.run(|| {
            velocity = match source.get() {
                TickSource::Simulation(simulation) => simulation.source().dx(cycle) * playback,
                TickSource::Repeat(_) => {
                    crate::retarget::rate(span, playback, duration.as_secs_f64())
                }
                TickSource::Time { curve, .. } => {
                    if duration.is_zero() {
                        0.0
                    } else {
                        let progress = (cycle / duration.as_secs_f64()).clamp(0.0, 1.0);
                        let slope = curve.as_ref().map_or(1.0, |curve| curve.slope(progress));
                        crate::retarget::scaled_rate(span, slope, duration.as_secs_f64(), playback)
                    }
                }
            };
        });
        recovery.retire(source);
        recovery.finish();
        if velocity.is_finite() && self.inner.borrow().matches_sample(&identity) {
            velocity
        } else {
            0.0
        }
    }

    /// Replace the run with motion from its last sampled value and velocity.
    /// Cancels the displaced future without an intermediate settled status.
    /// Manual drivers supply elapsed time measured from this seam.
    ///
    /// # Errors
    /// Refuses non-finite targets, spans or velocities before mutation. Motion
    /// callbacks run outside borrows; one retry admits a changed sample, and
    /// repeated reentry returns [`AnimationError::ReentrantMotion`].
    pub fn retarget(
        &self,
        target: f64,
        motion: &MotionSpec,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut recovery = Retirement::new();
        let mut result = Err(AnimationError::ReentrantMotion);
        recovery.run_with(|recovery| {
            result = self.retarget_with_recovery(target, motion, recovery);
        });
        if let Err(error) = &result
            && matches!(
                error,
                AnimationError::NonFiniteTarget(_) | AnimationError::ReentrantMotion
            )
        {
            recovery.run(|| tracing::warn!("{error}"));
        }
        recovery.finish();
        result
    }

    fn retarget_with_recovery(
        &self,
        target: f64,
        motion: &MotionSpec,
        recovery: &mut Retirement,
    ) -> Result<AnimationRunFuture, AnimationError> {
        for _ in 0..2 {
            let (seam, target) = {
                let inner = self.inner.borrow();
                Self::check_run_admission(&inner)?;
                if !target.is_finite() {
                    return Err(AnimationError::NonFiniteTarget(format!(
                        "retarget target {target} is not finite"
                    )));
                }
                let target = target.clamp(inner.lower_bound, inner.upper_bound);
                if !(target - inner.value).is_finite() {
                    return Err(AnimationError::NonFiniteTarget(
                        "retarget span overflows f64".into(),
                    ));
                }
                (Opaque::new(Seam::capture(&inner)), target)
            };
            let velocity = seam.get().velocity();
            let current = {
                let inner = self.inner.borrow();
                Self::check_run_admission(&inner)?;
                seam.get().matches(&inner)
            };
            if !current {
                // Even an invalid derivative belongs to the displaced source.
                // Retry before calling into the replacement motion's curve.
                recovery.retire(seam);
                continue;
            }
            let velocity = velocity?;
            let mut next = Opaque::new(
                Segment::start(seam.get().value, velocity, target, motion, 1.0).map_err(
                    |error| AnimationError::NonFiniteTarget(format!("retarget motion: {error}")),
                )?,
            );
            let mut inner = self.inner.borrow_mut();
            Self::check_run_admission(&inner)?;
            if !seam.get().matches(&inner) {
                drop(inner);
                recovery.retire(next);
                recovery.retire(seam);
                continue;
            }
            let immediate = matches!(next.get(), Segment::Rest(_));
            let mut retired = RetiredSources::new();
            inner.clear_run_modes(&mut retired);
            inner.start_value = seam.get().value;
            inner.target_value = target;
            inner.direction = if target < inner.value {
                AnimationDirection::Reverse
            } else {
                AnimationDirection::Forward
            };
            inner.status = inner.direction.running_status();
            inner.simulation = Some(SimulationRun::Motion(Rc::new(next.take())));
            Self::begin_run(&mut inner);
            inner.run_start = seam.get().origin;
            if immediate {
                inner.run_duration = Some(Duration::ZERO);
                inner.settle_pending = true;
            }
            let (completer, future) = AnimationRunFuture::pending();
            let displaced = inner
                .active_run
                .replace(completer)
                .map(RunCompleter::cancel);
            let status = inner.status;
            self.finish_with_retirement(
                status,
                ValueChange::Unchanged,
                displaced,
                retired,
                inner,
                recovery,
            );
            recovery.retire(next);
            recovery.retire(seam);
            return Ok(future);
        }
        Err(AnimationError::ReentrantMotion)
    }
}
