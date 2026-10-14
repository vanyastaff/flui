//! Timing proposals commit only with the position they describe.

use std::time::Duration;

use flui_foundation::panic::{FailureClass, RecoveryScope};

use super::run::{RepeatRun, RepeatSample, SimulationRun};
use super::{
    AnimationController, AnimationControllerInner, AnimationDirection, Opaque, RetiredSources,
    Retirement, Simulation, TickSource, ValueChange,
};
use crate::PlaybackRate;

impl AnimationController {
    /// Sample the run at `elapsed`, measured from its own time origin.
    /// Registry-driven calls use the presentation clock's projected timeline;
    /// explicit callers supply their own run timeline. The controller applies
    /// its local playback rate to this elapsed time.
    ///
    /// This is the single time-driven entry point: time-based runs interpolate
    /// `start_value -> target_value`, simulations sample `x(t)`, and a repeat
    /// samples its leg/phase/exhaustion as a pure function of `cycle`.
    /// Value and status listeners fire only after the state borrow is released.
    pub fn tick_at(&self, elapsed: Duration) {
        let mut retirement = Retirement::new();
        retirement.run_with(|retirement| self.tick_at_with_retirement(elapsed, retirement));
        retirement.finish();
    }

    pub(crate) fn tick_at_with_retirement(
        &self,
        elapsed: Duration,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let source;
        let identity;
        let cycle;
        let time;
        {
            let mut inner = self.inner.borrow_mut();
            if inner.disposed || inner.run.is_none() || elapsed < inner.last_elapsed {
                return;
            }
            let Some(epoch) = inner.sample_epoch.checked_add(1) else {
                let mut retired = RetiredSources::new();
                let delivery = inner.stop_running(&mut retired);
                let status = inner.settled_status_directed();
                inner.status = status;
                self.deliver_transition_with_recovery(
                    status,
                    ValueChange::Unchanged,
                    delivery,
                    retired,
                    inner,
                    retirement,
                );
                retirement.run_classified(FailureClass::Delivery, |_| {
                    tracing::warn!(
                        "animation sample identities exhausted; the accepted run was cancelled"
                    );
                });
                return;
            };
            time = SampleTime::capture(&inner, elapsed);
            cycle = time.local_elapsed.as_secs_f64();
            inner.sample_epoch = epoch;
            identity = SampleIdentity {
                generation: inner.run_generation,
                epoch: inner.sample_epoch,
            };
            source = Opaque::new(inner.tick_source());
        }
        let mut committed = false;
        retirement.run_with(|retirement| match source.get() {
            TickSource::Repeat(run) => {
                let profile = crate::observability::ProfileSpan::sampling();
                let sample = run.sample(cycle);
                profile.close(retirement);
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                time.commit(&mut inner);
                self.tick_repeat(inner, sample, retirement);
                committed = true;
            }
            TickSource::Simulation(simulation) => {
                let mut prepared = None;
                let profile = crate::observability::ProfileSpan::sampling();
                retirement.run_classified(FailureClass::Computation, |_| {
                    // Creating/entering the span can withdraw this captured source.
                    if !self.inner.borrow().matches_sample(&identity) {
                        return;
                    }
                    if let SimulationRun::Value(source) = simulation
                        && !source
                            .stage_sample(cycle, &|| self.inner.borrow().matches_sample(&identity))
                    {
                        return;
                    }
                    let sampled = simulation.x(cycle);
                    // A reentrant position call may replace the source.
                    if !self.inner.borrow().matches_sample(&identity) {
                        return;
                    }
                    let done = sampled.is_finite()
                        && (simulation.reached_bound(sampled) || simulation.is_done(cycle));
                    prepared = Some((sampled, done));
                });
                profile.close(retirement);
                let Some((sampled, done)) = prepared else {
                    return;
                };
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if sampled.is_finite() {
                    time.commit(&mut inner);
                    if let SimulationRun::Value(source) = simulation {
                        source.commit_sample(inner.local_elapsed);
                    }
                }
                self.tick_simulation(inner, sampled, done, retirement);
                committed = true;
            }
            TickSource::Time {
                curve,
                duration,
                start,
                target,
            } => {
                let profile = crate::observability::ProfileSpan::sampling();
                let t = if duration.is_zero() {
                    1.0
                } else {
                    (cycle / duration.as_secs_f64()).clamp(0.0, 1.0)
                };
                let mut prepared = None;
                retirement.run_classified(FailureClass::Computation, |_| {
                    if !self.inner.borrow().matches_sample(&identity) {
                        return;
                    }
                    let value = if t <= 0.0 {
                        *start
                    } else if t >= 1.0 {
                        *target
                    } else {
                        let eased = curve.as_ref().map_or(t, |curve| curve.transform(t));
                        start + (target - start) * eased
                    };
                    prepared = Some(value);
                });
                profile.close(retirement);
                let Some(value) = prepared else {
                    return;
                };
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if value.is_finite() {
                    time.commit(&mut inner);
                }
                self.tick_time_based(inner, t, value, retirement);
                committed = value.is_finite();
            }
        });
        retirement.retire_classified(
            if committed {
                FailureClass::Delivery
            } else {
                FailureClass::Unknown
            },
            source,
        );
    }
}

pub(super) enum RepeatFrame {
    Complete((f64, AnimationDirection)),
    Running(RepeatSample),
}

impl RepeatRun {
    fn sample(&self, cycle: f64) -> RepeatFrame {
        // A rounded Duration::MAX must saturate forward, never rewind a finite
        // repeat. Adding its initial phase remains bounded by twice that
        // duration's nanoseconds, well inside u128.
        let elapsed_ns = Duration::try_from_secs_f64(cycle)
            .unwrap_or(Duration::MAX)
            .as_nanos();
        let total_ns = elapsed_ns + self.initial_ns;
        if let Some(count) = self.count
            && total_ns >= u128::from(count) * self.period_ns
            && let Some(landing) = self.landing()
        {
            return RepeatFrame::Complete(landing);
        }
        RepeatFrame::Running(AnimationControllerInner::repeat_sample(self, total_ns))
    }
}

pub(super) struct SampleIdentity {
    pub(super) generation: u64,
    pub(super) epoch: u64,
}

/// Proposed timing belongs to a position sample until that sample commits.
/// A failed source cannot advance the derivative of the published value.
pub(super) struct SampleTime {
    elapsed: Duration,
    pub(super) local_elapsed: Duration,
    pending_rate: Option<PlaybackRate>,
}

impl SampleTime {
    pub(super) fn capture(inner: &AnimationControllerInner, elapsed: Duration) -> Self {
        let span = elapsed.saturating_sub(inner.rate_epoch_elapsed);
        let scaled = if inner.playback_rate == PlaybackRate::NORMAL {
            span
        } else if inner.playback_rate.is_paused() {
            Duration::ZERO
        } else {
            Duration::try_from_secs_f64(span.as_secs_f64() * inner.playback_rate.get())
                .unwrap_or(Duration::MAX)
        };
        Self {
            elapsed,
            local_elapsed: inner.rate_epoch_local.saturating_add(scaled),
            pending_rate: inner.pending_rate,
        }
    }

    pub(super) fn commit(&self, inner: &mut AnimationControllerInner) {
        inner.last_elapsed = self.elapsed;
        inner.local_elapsed = self.local_elapsed;
        if let Some(rate) = self.pending_rate {
            inner.rate_epoch_elapsed = self.elapsed;
            inner.rate_epoch_local = self.local_elapsed;
            inner.playback_rate = rate;
            // A source may have requested a newer rate while it was sampled.
            if inner.pending_rate == self.pending_rate {
                inner.pending_rate = None;
            }
        }
    }
}
