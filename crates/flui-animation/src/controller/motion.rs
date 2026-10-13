//! Interruptible runs owned by the controller.

use flui_foundation::panic::RecoveryScope;
use std::rc::Rc;
use std::time::Duration;

use super::{
    AnimationController, AnimationControllerInner, AnimationDirection, Opaque, RetiredSources,
    Retirement, RunStart, SampleIdentity, SimulationRun, TickSource, ValueChange,
};
use crate::retarget::{Reversal, Segment};
use crate::run_future::RunCompleter;
use crate::{AnimationError, AnimationInput, AnimationRunFuture, MotionSpec, Simulation};

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

/// Delivery committed by typed motion admission. Publishing is deliberately
/// separate so several owners can be installed before the first callout.
#[must_use = "admitted motion must publish its delivery"]
pub(crate) struct ValuePublication {
    controller: AnimationController,
    drain: bool,
    value_change: ValueChange,
}

impl ValuePublication {
    pub(crate) fn publish(self, recovery: &mut RecoveryScope<'_>) {
        self.controller
            .publish_delivery(self.value_change, self.drain, recovery);
    }

    pub(super) fn unchanged(controller: AnimationController, drain: bool) -> Self {
        Self {
            controller,
            drain,
            value_change: ValueChange::Unchanged,
        }
    }
}

impl ValueSeam {
    fn matches(&self, inner: &AnimationControllerInner) -> bool {
        !inner.disposed
            && inner.run_generation == self.generation
            && inner.sample_epoch == self.epoch
    }
}

/// The derivative of `source` `cycle` run-local seconds in, in value units
/// per second of a clock running `scale` times run-local time. Calls user
/// curves and simulations, so it runs outside the state borrow. An
/// unrepresentable derivative stays non-finite: the retarget seam refuses
/// it and [`AnimationController::velocity`] reports zero.
fn run_velocity(source: &TickSource, cycle: f64, span: f64, duration: Duration, scale: f64) -> f64 {
    let seconds = duration.as_secs_f64();
    match source {
        TickSource::Simulation(simulation) => simulation.dx(cycle) * scale,
        TickSource::Repeat(_) => crate::retarget::rate(span, scale, seconds),
        TickSource::Time { .. } if duration.is_zero() => 0.0,
        TickSource::Time { curve, .. } => {
            let progress = (cycle / seconds).clamp(0.0, 1.0);
            let slope = curve.as_ref().map_or(1.0, |curve| curve.slope(progress));
            crate::retarget::scaled_rate(span, slope, seconds, scale)
        }
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
    fn continuing_spring(&self, target: f64, motion: &MotionSpec) -> Option<SimulationRun> {
        let MotionSpec::Spring(description) = motion else {
            return None;
        };
        let Some(TickSource::Simulation(
            run @ SimulationRun::Motion {
                source,
                elapsed_offset,
                reversal,
            },
        )) = &self.source
        else {
            return None;
        };
        let elapsed_offset = elapsed_offset + self.cycle;
        if !elapsed_offset.is_finite()
            || !source.matches_spring(target, *description)
            || run.x(self.cycle) != self.value
        {
            return None;
        }
        Some(SimulationRun::Motion {
            source: Rc::clone(source),
            elapsed_offset,
            reversal: *reversal,
        })
    }

    /// The reversal state of a segment from this seam toward `goal`. Reads
    /// the interrupted curve, so it runs outside the state borrow.
    fn reversal(&self, goal: f64, motion: &MotionSpec) -> Reversal<f64> {
        match &self.source {
            Some(TickSource::Simulation(
                run @ SimulationRun::Motion {
                    source, reversal, ..
                },
            )) => reversal.interrupt(self.value, goal, motion, || {
                source.eased_progress(run.source_time(self.cycle))
            }),
            _ => Reversal::from_start(self.value),
        }
    }

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

    /// The velocity the replacement segment starts with. The segment runs on
    /// the same scaled clock, so it inherits the run-local derivative.
    fn velocity(&self) -> Result<f64, AnimationError> {
        let velocity = match &self.source {
            Some(source) if self.playback != 0.0 => {
                run_velocity(source, self.cycle, self.span, self.duration, 1.0)
            }
            _ => 0.0,
        };
        if velocity.is_finite() {
            Ok(velocity)
        } else {
            Err(AnimationError::NonFiniteInput {
                input: AnimationInput::InheritedVelocity,
                value: velocity,
            })
        }
    }
}

impl AnimationController {
    pub(crate) fn admit_value_metadata(&self) -> ValuePublication {
        let inner = self.inner.borrow_mut();
        let status = inner.status;
        let drain = Self::enqueue_delivery(status, None, RetiredSources::new(), inner);
        ValuePublication {
            controller: self.clone(),
            drain,
            value_change: ValueChange::Notify,
        }
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

    pub(crate) fn validate_value_seam(&self, seam: &ValueSeam) -> Result<(), AnimationError> {
        let inner = self.inner.borrow();
        Self::check_run_admission(&inner)?;
        if seam.matches(&inner) {
            Ok(())
        } else {
            Err(AnimationError::ReentrantMotion)
        }
    }

    /// The closure only moves prepared framework fields; it must not call or
    /// drop user code. Returned outgoing ownership retires after unlocking.
    pub(crate) fn start_value_motion<R>(
        &self,
        seam: &ValueSeam,
        source: Rc<dyn crate::animated_value::ValueMotion>,
        immediate: bool,
        commit: impl FnOnce() -> R,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<(AnimationRunFuture, R, ValuePublication), AnimationError> {
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
        let (future, displaced) = Self::install_run(&mut inner);
        inner.run_start = seam.origin;
        if immediate {
            inner.run_duration = Some(Duration::ZERO);
            inner.settle_pending = true;
        }
        let outgoing = commit();
        let status = inner.status;
        let drain = Self::enqueue_delivery(status, displaced, retired, inner);
        Ok((
            future,
            outgoing,
            ValuePublication {
                controller: self.clone(),
                drain,
                value_change: ValueChange::Notify,
            },
        ))
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
        recovery.run(|| velocity = run_velocity(source.get(), cycle, span, duration, playback));
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
                AnimationError::NonFiniteInput { .. }
                    | AnimationError::SpanOverflow { .. }
                    | AnimationError::Simulation(_)
                    | AnimationError::ReentrantMotion
            )
        {
            recovery.run(|| tracing::warn!("retarget: {error}"));
        }
        recovery.finish();
        result
    }

    fn retarget_with_recovery(
        &self,
        target: f64,
        motion: &MotionSpec,
        recovery: &mut RecoveryScope<'_>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        for _ in 0..2 {
            let (seam, target) = {
                let inner = self.inner.borrow();
                Self::check_run_admission(&inner)?;
                if !target.is_finite() {
                    return Err(AnimationError::NonFiniteInput {
                        input: AnimationInput::Target,
                        value: target,
                    });
                }
                let target = target.clamp(inner.lower_bound, inner.upper_bound);
                if !(target - inner.value).is_finite() {
                    return Err(AnimationError::SpanOverflow {
                        from: inner.value,
                        to: target,
                    });
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
            // The interrupted curve is user code too: sample it only from a
            // seam still current after the velocity callout, and see a
            // replacement it made before the new motion's curve runs.
            let reversal = seam.get().reversal(target, motion);
            let current = {
                let inner = self.inner.borrow();
                Self::check_run_admission(&inner)?;
                seam.get().matches(&inner)
            };
            if !current {
                recovery.retire(seam);
                continue;
            }
            let mut next = Opaque::new(
                if let Some(continuation) = seam.get().continuing_spring(target, motion) {
                    continuation
                } else {
                    let source = Segment::start(
                        seam.get().value,
                        velocity,
                        target,
                        motion,
                        reversal.shortening(),
                        crate::simulation::Tolerance::DEFAULT,
                    )
                    .map_err(AnimationError::Simulation)?;
                    SimulationRun::Motion {
                        source: Rc::new(source),
                        elapsed_offset: 0.0,
                        reversal,
                    }
                },
            );
            let mut inner = self.inner.borrow_mut();
            Self::check_run_admission(&inner)?;
            if !seam.get().matches(&inner) {
                drop(inner);
                recovery.retire(next);
                recovery.retire(seam);
                continue;
            }
            let immediate = matches!(next.get(), SimulationRun::Motion { source, .. }
                if matches!(source.as_ref(), Segment::Rest(_)));
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
            inner.simulation = Some(next.take());
            let (future, displaced) = Self::install_run(&mut inner);
            inner.run_start = seam.get().origin;
            if immediate {
                inner.run_duration = Some(Duration::ZERO);
                inner.settle_pending = true;
            }
            let status = inner.status;
            self.deliver_transition_with_recovery(
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
