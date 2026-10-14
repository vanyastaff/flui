//! The controller's run: one value, so a run's mode, its completer and how it
//! settles are installed and retired together and cannot disagree.

use std::rc::Rc;
use std::time::Duration;

use super::policy::MotionRunState;
use super::{
    AnimationController, AnimationControllerInner, ClockBinding, ControllerDelivery, Opaque,
    RetiredSources, RunStart,
};
use crate::AnimationRunFuture;
use crate::animation::AnimationDirection;
use crate::curve::Curve;
use crate::retarget::{Reversal, Segment};
use crate::run_future::{RunCompleter, RunDelivery};
use crate::simulation::Simulation;

/// The run installed on a controller.
pub(super) struct ActiveRun {
    /// The write half of the run's future. Taken, never overwritten, so the
    /// displaced completion reaches transition delivery.
    pub(super) completer: RunCompleter,
    pub(super) mode: RunMode,
    pub(super) settle: Settle,
}

/// What a run samples each frame. A repeat cannot also be a simulation.
pub(super) enum RunMode {
    /// From the start to the target value over `duration`, eased by `curve`
    /// (linear when `None`).
    Timed {
        curve: Option<Rc<dyn Curve>>,
        duration: Duration,
    },
    /// A repeat, sampled as a pure function of its elapsed time.
    Repeat(RepeatRun),
    /// A physical or generated simulation.
    Simulation(SimulationRun),
}

/// Whether a run waits for frames or settles on its next delivery.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Settle {
    /// It advances as frames sample it.
    Sampled,
    /// It settles on its next delivery: a motion already at rest, or a run
    /// admitted while its controller delivers.
    Immediate,
}

impl ActiveRun {
    /// Hand the mode's opaque sources to `retired` and return the completer
    /// for the caller to complete or cancel.
    pub(super) fn retire(self, retired: &mut RetiredSources) -> RunCompleter {
        match self.mode {
            RunMode::Timed {
                curve: Some(curve), ..
            } => retired.curve = Some(Opaque::new(curve)),
            RunMode::Simulation(simulation) => retired.simulation = Some(Opaque::new(simulation)),
            RunMode::Timed { curve: None, .. } | RunMode::Repeat(_) => {}
        }
        self.completer
    }
}

#[derive(Clone)]
pub(super) enum SimulationRun {
    Custom(Rc<dyn Simulation>),
    Motion {
        source: Rc<Segment>,
        /// Animation seconds already consumed from this physical trajectory
        /// before the current run's seam. Future replacement does not restart
        /// an unchanged spring's analytic or exact-rest suffix.
        elapsed_offset: f64,
        /// What the next retarget needs to reverse this segment faster.
        reversal: Reversal<f64>,
    },
    Value(Rc<dyn crate::animated_value::ValueMotion>),
    Fling {
        source: Rc<dyn Simulation>,
        bound: f64,
        direction: AnimationDirection,
    },
}

impl SimulationRun {
    fn source(&self) -> &dyn Simulation {
        match self {
            Self::Custom(source) | Self::Fling { source, .. } => source.as_ref(),
            Self::Motion { source, .. } => source.as_ref(),
            Self::Value(source) => source.as_ref(),
        }
    }

    pub(super) fn reached_bound(&self, sample: f64) -> bool {
        match self {
            Self::Custom(_) | Self::Motion { .. } | Self::Value(_) => false,
            Self::Fling {
                bound, direction, ..
            } => match direction {
                AnimationDirection::Forward => sample >= *bound,
                AnimationDirection::Reverse => sample <= *bound,
            },
        }
    }
}

impl Simulation for SimulationRun {
    fn x(&self, time: f64) -> f64 {
        self.source().x(self.source_time(time))
    }

    fn dx(&self, time: f64) -> f64 {
        self.source().dx(self.source_time(time))
    }

    fn is_done(&self, time: f64) -> bool {
        self.source().is_done(self.source_time(time))
    }
}

impl SimulationRun {
    pub(super) fn source_time(&self, time: f64) -> f64 {
        match self {
            Self::Motion { elapsed_offset, .. } => time + elapsed_offset,
            _ => time,
        }
    }
}

/// The configuration a live `repeat` run needs to sample its
/// value/status/direction as a pure function of elapsed time — see
/// [`AnimationControllerInner::repeat_sample`]. Constructed only after
/// `repeat`'s zero-period and zero-count degenerate cases have already
/// settled synchronously and returned, so `period_ns` is **always nonzero
/// here** — no live `RepeatRun` ever needs a zero-period guard downstream.
#[derive(Debug, Clone, Copy)]
pub(super) struct RepeatRun {
    /// Bounce back and forth (`true`) instead of restarting each cycle.
    pub(super) reverse: bool,
    /// Lower endpoint of the repeat range.
    pub(super) min: f64,
    /// Upper endpoint of the repeat range.
    pub(super) max: f64,
    /// Per-cycle duration, resolved ONCE at the call
    /// (`period.unwrap_or(duration)`) — never re-read from the live
    /// `duration`/`reverse_duration`, so `set_duration` mid-repeat and a
    /// bounce's reverse leg both leave it alone. Never zero: the zero-period
    /// case settles at the call before a `RepeatRun` is constructed.
    pub(super) period: Duration,
    /// `period.as_nanos()`, cached because every sample divides by it.
    /// Always `> 0` (see `period`).
    pub(super) period_ns: u128,
    /// Number of cycles to run; `None` repeats indefinitely. `Some(0)`
    /// (zero cycles) is handled by `repeat` before a `RepeatRun` is
    /// ever constructed — see that method's own doc.
    pub(super) count: Option<u32>,
    /// Phase offset, in nanoseconds into the period, that the value at the
    /// call sits at: `total_ns = elapsed_ns_since_the_run_started +
    /// initial_ns` is what every leg/phase/exhaustion computation samples,
    /// so a long frame that spans several cycles at once needs no
    /// incremental bookkeeping.
    pub(super) initial_ns: u128,
}

impl RepeatRun {
    /// Where this run lands once its iterations run out; `None` when it
    /// repeats forever.
    pub(super) fn landing(&self) -> Option<(f64, AnimationDirection)> {
        self.count.map(|count| {
            AnimationControllerInner::repeat_landing(self.reverse, self.min, self.max, count)
        })
    }
}

/// A repeat's value/direction/leg-endpoints at one instant, sampled by
/// [`AnimationControllerInner::repeat_sample`] — the one place this parity
/// math lives, called from both `repeat`'s at-call state and
/// `AnimationController::tick_repeat`'s running state.
pub(super) struct RepeatSample {
    /// The interpolated value at this instant.
    pub(super) value: f64,
    /// The leg's direction (`Forward` unless bouncing on an odd-indexed leg).
    pub(super) direction: AnimationDirection,
    /// This leg's start endpoint (`min` or `max`, by direction).
    pub(super) start: f64,
    /// This leg's end endpoint (the opposite of `start`).
    pub(super) target: f64,
}

impl AnimationController {
    /// Establish a fresh run epoch and install a run of `mode`. The displaced
    /// run's sources join `retired`; its cancellation is returned with the new
    /// run's future, for the caller to deliver with the transition once the
    /// state is committed.
    pub(super) fn install_run(
        inner: &mut AnimationControllerInner,
        mode: RunMode,
        settle: Settle,
        retired: &mut RetiredSources,
    ) -> (AnimationRunFuture, Option<RunDelivery>) {
        inner.motion_state = MotionRunState::Active;
        inner.run_start = RunStart::Fresh;
        inner.last_elapsed = Duration::ZERO;
        inner.local_elapsed = Duration::ZERO;
        inner.rate_epoch_elapsed = Duration::ZERO;
        inner.rate_epoch_local = Duration::ZERO;
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        {
            inner.never_settles_warned = false;
        }
        inner.run_generation = inner
            .run_generation
            .checked_add(1)
            .expect("BUG: run admission reserves an available generation");
        inner.settle_pending =
            inner.clock_binding == ClockBinding::Missing || settle == Settle::Immediate;
        inner
            .pending_delivery
            .push_back(ControllerDelivery::RequestFrame);
        let (completer, future) = AnimationRunFuture::pending();
        let run = ActiveRun {
            completer,
            mode,
            settle,
        };
        let displaced = inner
            .run
            .replace(run)
            .map(|displaced| displaced.retire(retired).cancel());
        (future, displaced)
    }
}

impl AnimationControllerInner {
    /// Take the installed run, retiring its sources into `retired`, and
    /// return its completer for the caller to complete or cancel.
    pub(super) fn take_run(&mut self, retired: &mut RetiredSources) -> Option<RunCompleter> {
        self.motion_state = MotionRunState::Active;
        self.run.take().map(|run| run.retire(retired))
    }

    /// Halt any active run at the current value: clear
    /// simulation/repeat/curve state, and cancel the displaced run's
    /// completer — without touching `status` or emitting any notification.
    /// This is the raw halt that `set_value` performs first; it does not
    /// recompute status (the caller does that separately).
    ///
    /// The returned [`RunDelivery`] must be handed to
    /// transition delivery; nothing else in this file may call
    /// `deliver()` on it.
    #[must_use = "a displaced run's RunDelivery must reach transition delivery"]
    pub(super) fn stop_running(&mut self, retired: &mut RetiredSources) -> Option<RunDelivery> {
        self.take_run(retired).map(RunCompleter::cancel)
    }

    /// The direction of repeat cycle `index` (0-based): `Forward` unless the
    /// repeat bounces and `index` is odd. The single owner of this parity —
    /// every leg/landing computation in this file
    /// ([`repeat_sample`](Self::repeat_sample),
    /// [`repeat_landing`](Self::repeat_landing)) calls this instead of
    /// hand-copying the arithmetic; two copies is how one path ships wrong.
    fn repeat_leg(reverse: bool, index: u128) -> AnimationDirection {
        if reverse && index % 2 == 1 {
            AnimationDirection::Reverse
        } else {
            AnimationDirection::Forward
        }
    }

    /// Where a repeat of `count` iterations lands: the end of its last one,
    /// `min` after a `Reverse` leg and `max` after a `Forward` one. The
    /// single landing rule for exhaustion, `finish` and the zero-period
    /// settle. Takes the range as scalars rather than a [`RepeatRun`]
    /// because the zero-period settle runs BEFORE any `RepeatRun` exists;
    /// [`RepeatRun::landing`] applies it to a live run.
    pub(super) fn repeat_landing(
        reverse: bool,
        min: f64,
        max: f64,
        count: u32,
    ) -> (f64, AnimationDirection) {
        let direction = Self::repeat_leg(reverse, u128::from(count.saturating_sub(1)));
        let value = match direction {
            AnimationDirection::Reverse => min,
            AnimationDirection::Forward => max,
        };
        (value, direction)
    }

    /// The value/direction/leg-endpoints at `total_ns` nanoseconds since a
    /// repeat's run started — the ONE place the running-leg math lives,
    /// called by both `repeat`'s at-call sample (`total_ns =
    /// run.initial_ns`) and [`AnimationController::tick_repeat`]'s running
    /// sample (`total_ns = elapsed_ns + run.initial_ns`), replacing the
    /// `Forward => (min, max) / Reverse => (max, min)` match that used to
    /// exist at both call sites independently. Takes `run` by reference for
    /// the same reason [`repeat_landing`](Self::repeat_landing) does.
    pub(super) fn repeat_sample(run: &RepeatRun, total_ns: u128) -> RepeatSample {
        let index = total_ns / run.period_ns;
        let phase = (total_ns % run.period_ns) as f64 / run.period_ns as f64;
        let direction = Self::repeat_leg(run.reverse, index);
        let (start, target) = match direction {
            AnimationDirection::Forward => (run.min, run.max),
            AnimationDirection::Reverse => (run.max, run.min),
        };
        let value = start + (target - start) * phase;
        RepeatSample {
            value,
            direction,
            start,
            target,
        }
    }
}
