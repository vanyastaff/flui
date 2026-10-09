//! `AnimationController` - The primary animation driver.

mod motion;
mod sample;

use sample::{SampleIdentity, SampleTime};

use crate::AnimationRunFuture;
use crate::PlaybackRate;
use crate::animation::StatusObserver;
use crate::animation::{Animation, AnimationDirection, Retirement, StatusCallback, Terminal};
use crate::curve::Curve;
use crate::error::AnimationError;
use crate::retarget::Segment;
use crate::run_future::{RunCompleter, RunDelivery};
use crate::simulation::{Simulation, SpringDescription, SpringSimulation, SpringType, Tolerance};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

/// Absolute tolerance for "is the value at a bound" comparisons.
const BOUND_EPSILON: f64 = 1e-6;

/// Default spring for fling animations.
fn default_fling_spring() -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)
}

/// Default tolerance for fling animations.
const FLING_TOLERANCE: Tolerance = Tolerance {
    distance: 0.01,
    velocity: f64::INFINITY,
    time: 1e-3,
};

/// Whether `AnimationController::finish` must notify value listeners,
/// alongside status listeners and any pending [`RunDelivery`]. Most
/// run-starting calls change `status` but not `value` at the call itself
/// (the value only moves once ticks arrive) — but `repeat_with` snaps
/// `value` to the repeat range's lower endpoint and `drive_simulation`
/// snaps it to `simulation.x(0.0)`, and both still pass `Unchanged`: the run
/// start sets the value directly, without notifying listeners — the jump
/// is real but reported on the run's first tick, not synchronously at the
/// call. A settle or a tick always changes both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueChange {
    /// `value` did not change at this call; only status (and delivery) fire.
    Unchanged,
    /// `value` changed; notify value listeners too.
    Notify,
}

/// Caller-owned envelopes retire normally only when no failure is unwinding.
/// A retained envelope may be the last reference to arbitrary user captures.
struct Opaque<T>(Option<T>);

impl<T> Opaque<T> {
    fn new(value: T) -> Self {
        Self(Some(value))
    }
    fn get(&self) -> &T {
        self.0
            .as_ref()
            .expect("BUG: an opaque envelope owns its value until retirement")
    }
    fn take(&mut self) -> T {
        self.0
            .take()
            .expect("BUG: an opaque envelope is consumed only once")
    }
}

impl<T> Drop for Opaque<T> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            std::mem::forget(self.0.take());
        }
        // On an ordinary return the Option retires normally. If that
        // destructor panics, other envelopes see unwind and retain theirs.
    }
}

#[derive(Default)]
struct RetiredSources {
    simulation: Option<Opaque<SimulationRun>>,
    curve: Option<Opaque<Rc<dyn Curve>>>,
    callbacks: SmallVec<[Opaque<StatusListener>; 4]>,
    value_callbacks:
        SmallVec<[Opaque<Rc<flui_foundation::notifier_generic::NotificationCallback<()>>>; 4]>,
}

impl RetiredSources {
    fn new() -> Self {
        Self::default()
    }
}

/// Accepted transitions and run outcomes share one controller delivery order.
/// Snapshot ownership keeps removed callbacks alive until they can retire outside
/// the state guard, with the round's first failure still authoritative.
enum ControllerDelivery {
    RequestFrame,
    SettleRun(u64),
    Status(
        AnimationStatus,
        SmallVec<[(ListenerId, Terminal<StatusListener>); 4]>,
    ),
    Run(Terminal<RunDelivery>),
    Retire(RetiredSources),
}

enum TickSource {
    Repeat(RepeatRun),
    Simulation(SimulationRun),
    Time {
        curve: Option<Rc<dyn Curve>>,
        duration: Duration,
        start: f64,
        target: f64,
    },
}

#[derive(Clone)]
enum SimulationRun {
    Custom(Rc<dyn Simulation>),
    Motion(Rc<Segment>),
    Fling {
        source: Rc<dyn Simulation>,
        bound: f64,
        direction: AnimationDirection,
    },
}

#[derive(Clone)]
enum StatusListener {
    User(StatusCallback),
    Relay(StatusObserver),
}

impl StatusListener {
    fn invoke(&self, status: AnimationStatus, recovery: &mut Retirement) {
        match self {
            Self::User(callback) => callback(status),
            Self::Relay(callback) => callback(status, recovery),
        }
    }
}

impl SimulationRun {
    fn source(&self) -> &dyn Simulation {
        match self {
            Self::Custom(source) | Self::Fling { source, .. } => source.as_ref(),
            Self::Motion(source) => source.as_ref(),
        }
    }

    fn reached_bound(&self, sample: f64) -> bool {
        match self {
            Self::Custom(_) | Self::Motion(_) => false,
            Self::Fling {
                bound, direction, ..
            } => match direction {
                AnimationDirection::Forward => sample >= *bound,
                AnimationDirection::Reverse => sample <= *bound,
            },
        }
    }
}

/// What actually happened to a non-finite value `set_value` or a running
/// simulation received — named so
/// [`AnimationController::warn_non_finite_value`]'s message states the real
/// outcome instead of a single "canonicalized or ignored" that was wrong
/// for two of its three call sites (a simulation sample going non-finite
/// mid-run ends the run; it is neither canonicalized nor ignored).
#[derive(Debug, Clone, Copy)]
enum NonFiniteOutcome {
    /// Bounded `set_value`: canonicalized to the bound it points at
    /// (`NaN` -> lower bound, `+-inf` -> whichever bound it points at).
    Canonicalized,
    /// Unbounded `set_value`: a full no-op — value untouched.
    Ignored,
    /// A running simulation's sample went non-finite: the run ended at its
    /// last finite value.
    EndedRun,
}

impl NonFiniteOutcome {
    const fn description(self) -> &'static str {
        match self {
            Self::Canonicalized => "canonicalized to the nearest bound",
            Self::Ignored => {
                "ignored (this controller is unbounded, so there is no bound to canonicalize toward)"
            }
            Self::EndedRun => "ended the run at its last finite value",
        }
    }
}

/// Single-lock snapshot for [`Vsync`](crate::vsync::Vsync)'s per-frame walk:
/// the run-generation counter and whether this controller currently holds
/// the frame loop open, both read under ONE controller lock by
/// [`AnimationController::walk_probe`]. See `docs/PERFORMANCE.md`'s Vsync
/// section for the measured per-controller cost.
pub(crate) struct WalkProbe {
    /// The controller's [`AnimationController::run_generation`] at the time
    /// of the probe.
    pub(crate) generation: u64,
    pub(crate) start: RunStart,
    /// Whether the walk should tick this controller: running AND not
    /// disposed. `status` alone cannot tell the two apart — see
    /// [`AnimationController::walk_probe`]'s own doc.
    pub(crate) live_running: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum RunStart {
    Fresh,
    /// The published seam in the source run's raw elapsed coordinates.
    Continue {
        generation: u64,
        elapsed: Duration,
    },
}

/// The configuration a live `repeat`/`repeat_with` run needs to sample its
/// value/status/direction as a pure function of elapsed time — see
/// [`AnimationControllerInner::repeat_sample`]. Constructed only after
/// `repeat_with`'s zero-period and zero-count degenerate cases have already
/// settled synchronously and returned, so `period_ns` is **always nonzero
/// here** — no live `RepeatRun` ever needs a zero-period guard downstream.
#[derive(Debug, Clone, Copy)]
struct RepeatRun {
    /// Bounce back and forth (`true`) instead of restarting each cycle.
    reverse: bool,
    /// Lower endpoint of the repeat range.
    min: f64,
    /// Upper endpoint of the repeat range.
    max: f64,
    /// Per-cycle duration, resolved ONCE at the call
    /// (`period.unwrap_or(duration)`) — never re-read from the live
    /// `duration`/`reverse_duration`, so `set_duration` mid-repeat and a
    /// bounce's reverse leg both leave it alone. Never zero: the zero-period
    /// case settles at the call before a `RepeatRun` is constructed.
    period: Duration,
    /// `period.as_nanos()`, cached because every sample divides by it.
    /// Always `> 0` (see `period`).
    period_ns: u128,
    /// Number of cycles to run; `None` repeats indefinitely. `Some(0)`
    /// (zero cycles) is handled by `repeat_with` before a `RepeatRun` is
    /// ever constructed — see that method's own doc.
    count: Option<u32>,
    /// Phase offset, in nanoseconds into the period, that the value at the
    /// call sits at: `total_ns = elapsed_ns_since_the_run_started +
    /// initial_ns` is what every leg/phase/exhaustion computation samples,
    /// so a long frame that spans several cycles at once needs no
    /// incremental bookkeeping.
    initial_ns: u128,
}

/// A repeat's value/direction/leg-endpoints at one instant, sampled by
/// [`AnimationControllerInner::repeat_sample`] — the one place this parity
/// math lives, called from both `repeat_with`'s at-call state and
/// `AnimationController::tick_repeat`'s running state.
struct RepeatSample {
    /// The interpolated value at this instant.
    value: f64,
    /// The leg's direction (`Forward` unless bouncing on an odd-indexed leg).
    direction: AnimationDirection,
    /// This leg's start endpoint (`min` or `max`, by direction).
    start: f64,
    /// This leg's end endpoint (the opposite of `start`).
    target: f64,
}

/// Controls an animation, driving it forward/backward.
///
/// `AnimationController` is a **PERSISTENT OBJECT** that survives widget rebuilds.
/// It must be disposed when no longer needed to prevent resource leaks.
///
/// The controller generates values from `lower_bound` to `upper_bound` (typically 0.0 to 1.0)
/// over the specified duration. It implements `Animation<f64>` and can be used directly,
/// or transformed using `Tween` or `CurvedAnimation`.
///
/// # Time model
///
/// Manual controllers accept elapsed [`Duration`] through [`Self::tick_at`].
/// A [`DrivenController`](crate::DrivenController) receives samples from its
/// presentation's [`Vsync`](crate::Vsync) and [`MotionClock`](crate::MotionClock).
/// Playback-rate changes preserve sampled local time. Each fresh run starts
/// its own timeline at zero; repeat phase derives from elapsed time rather
/// than incremental cycle bookkeeping.
///
/// # Ownership
///
/// Controllers and callbacks belong to their UI owner thread. Clones share
/// `Rc` state; every callback runs after state borrows have been released.
/// Reentrant transitions join the active delivery queue (ADR-0177, ADR-0178).
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, Animation};
/// use flui_scheduler::UpdateScheduler;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
///
/// // Start animation
/// controller.forward().unwrap();
///
/// // Get current value (0.0 to 1.0)
/// let value = controller.value();
///
/// // Cleanup when done
/// drop(controller);
/// ```
#[derive(Clone)]
pub struct AnimationController {
    inner: Rc<RefCell<AnimationControllerInner>>,
    notifier: Rc<ChangeNotifier>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClockBinding {
    Manual,
    Bound,
    Missing,
}

struct AnimationControllerInner {
    /// Current value (typically 0.0 to 1.0).
    value: f64,

    /// Animation status.
    status: AnimationStatus,

    /// Duration of forward animation.
    duration: Duration,

    /// Duration of reverse animation (defaults to `duration`).
    reverse_duration: Option<Duration>,

    /// Lower bound (default 0.0).
    lower_bound: f64,

    /// Upper bound (default 1.0).
    upper_bound: f64,

    /// Status listeners, in registration order.
    status_listeners: Vec<(ListenerId, StatusListener)>,

    pending_delivery: VecDeque<ControllerDelivery>,
    delivering: bool,
    frame_routes: Vec<crate::VsyncRegistration>,

    /// Current run direction.
    direction: AnimationDirection,

    /// Value at the start of the current run (for partial animations).
    start_value: f64,

    /// Target value for the current run.
    target_value: f64,

    /// Raw and local time of the last published position sample. A rejected
    /// or panicking source leaves both times and its applied rate unchanged.
    last_elapsed: Duration,
    local_elapsed: Duration,
    playback_rate: PlaybackRate,
    pending_rate: Option<PlaybackRate>,
    rate_epoch_elapsed: Duration,
    rate_epoch_local: Duration,
    clock_binding: ClockBinding,
    settle_pending: bool,
    missing_clock_warned: bool,

    /// Monotonically increasing counter, bumped once each time a fresh run is
    /// established. An external frame driver reads
    /// [`AnimationController::run_generation`] to detect "a new run's `t = 0`
    /// was just set" and re-anchor its own per-run epoch — so a controller run
    /// twice (forward → reverse) is ticked from the second run's start instead
    /// of a stale anchor. Never reset; stable across `tick_at`.
    run_generation: u64,
    run_start: RunStart,

    /// Invalidates outer samples when the same run is ticked reentrantly.
    sample_epoch: u64,

    /// Per-run duration override (used by `animate_to`/`animate_back`); does NOT
    /// clobber the controller's base `duration`.
    run_duration: Option<Duration>,

    /// Is disposed?
    disposed: bool,

    /// Next status-listener ID.
    next_listener_id: usize,

    /// The active repeat, if any. `None` outside a `repeat`/`repeat_with`
    /// run, so "repeating with no configuration" is unrepresentable. Every
    /// field a repeat needs to sample its value as a pure function of
    /// elapsed time lives on [`RepeatRun`] — see that type's own doc.
    repeat: Option<RepeatRun>,

    /// Active physics simulation (if using fling/animate_with).
    simulation: Option<SimulationRun>,

    /// Per-run easing curve for a time-based `animate_to_curved`/
    /// `animate_back_curved` run (`None` = linear). Cleared by
    /// [`clear_run_modes`](AnimationControllerInner::clear_run_modes) so a
    /// later plain `forward`/`reverse`/`animate_to` run does not inherit a
    /// stale curve.
    run_curve: Option<Rc<dyn Curve>>,

    /// Status most recently committed for delivery. The emission seam
    /// ([`enqueue_status_change`](AnimationControllerInner::enqueue_status_change))
    /// compares against this before firing, so a call that leaves `status`
    /// unchanged (e.g. `set_value` re-asserting the same directional status
    /// every frame of a gesture drag) does not re-notify.
    last_committed_status: AnimationStatus,

    /// The write half of the current run's [`AnimationRunFuture`], if a run is
    /// installed. Every run-starting method displaces this (canceling
    /// whatever it held) and installs its own fresh completer; every
    /// run-ending path (`tick_time_based`, `tick_simulation`,
    /// `stop_running`) takes it and completes or cancels it. Never touch
    /// this field with `=` — a direct assignment drops whatever value was
    /// here before, inline, under whatever lock is held at that statement;
    /// always go through `.replace()`/`.take()` and bind the returned
    /// `Option<RunCompleter>` so its displaced value reaches
    /// [`AnimationController::finish`] instead. `Option<T>` does not
    /// inherit `T`'s `#[must_use]`, so nothing in the compiler catches a
    /// dropped binding here.
    active_run: Option<RunCompleter>,

    /// Latches the "received a non-finite value" warning so a poisoned
    /// gesture drag calling [`set_value`](AnimationController::set_value)
    /// every frame — or a simulation whose sample goes non-finite mid-run —
    /// warns once per controller, not once per frame. Never reset: once a
    /// caller has proven it can produce a non-finite value, repeating the
    /// warning on every subsequent occurrence adds noise without adding
    /// information.
    non_finite_warned: bool,
}

impl Drop for AnimationControllerInner {
    fn drop(&mut self) {
        self.disposed = true;
        self.repeat = None;
        self.run_duration = None;
        let run = self.active_run.take();
        let simulation = Terminal::new(self.simulation.take());
        let curve = Terminal::new(self.run_curve.take());
        let callbacks: Vec<_> = std::mem::take(&mut self.status_listeners)
            .into_iter()
            .map(|(_, callback)| Terminal::new(callback))
            .collect();
        // All physical custody is withdrawn before cancellation or user Drop.
        let mut retirement = Retirement::new();
        let delivery = run.map(RunCompleter::cancel);
        if let Some(delivery) = delivery {
            retirement.run(|| delivery.deliver());
        }
        retirement.retire(simulation);
        retirement.retire(curve);
        for callback in callbacks {
            retirement.retire(callback);
        }
        retirement.finish();
    }
}

impl AnimationController {
    pub(crate) fn from_config(
        duration: Duration,
        bounds: Option<crate::ValueRange>,
        initial: Option<f64>,
    ) -> Self {
        let (lower, upper, default) = bounds
            .map_or((f64::NEG_INFINITY, f64::INFINITY, 0.0), |range| {
                (range.lower(), range.upper(), range.lower())
            });
        let value = initial
            .filter(|value| value.is_finite())
            .unwrap_or(default)
            .clamp(lower, upper);
        Self::new_inner(duration, lower, upper, value)
    }

    pub(crate) fn last_elapsed(&self) -> Duration {
        self.inner.borrow().last_elapsed
    }

    pub(crate) fn add_frame_route(&self, route: crate::VsyncRegistration) {
        let mut inner = self.inner.borrow_mut();
        inner
            .frame_routes
            .retain(crate::VsyncRegistration::owner_is_alive);
        inner.frame_routes.push(route);
    }

    pub(crate) fn remove_frame_route(&self, route: &crate::VsyncRegistration) {
        self.inner
            .borrow_mut()
            .frame_routes
            .retain(|candidate| candidate != route);
    }

    pub(crate) fn set_clock_bound(&self, bound: bool, retirement: &mut Retirement) {
        let mut inner = self.inner.borrow_mut();
        inner.clock_binding = if bound {
            ClockBinding::Bound
        } else {
            ClockBinding::Missing
        };
        inner.settle_pending = !bound && inner.active_run.is_some();
        if bound && inner.active_run.is_some() {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::RequestFrame);
        }
        let status = inner.status;
        self.finish_with_retirement(
            status,
            ValueChange::Unchanged,
            None,
            RetiredSources::new(),
            inner,
            retirement,
        );
    }

    /// The one place every constructor builds the inner state: `value`,
    /// `start_value`, and `target_value` all start at `initial_value`
    /// (`lower_bound` for a bounded controller, `0.0` for an unbounded one),
    /// and `status`/`last_committed_status` are BOTH set from
    /// [`AnimationControllerInner::settled_status_keep_direction`] at that
    /// value — the status rule applies at construction too, not only to a
    /// later `set_value` (a bounded
    /// controller at `lower_bound` stays `Dismissed`; an unbounded one at
    /// `0.0`, direction defaulted `Forward`, reports `Forward` — see
    /// `docs/ARCHITECTURE.md`'s mapping entry for the recorded cost). Both
    /// fields must agree at construction: if `last_committed_status` stayed
    /// hard-coded `Dismissed` while `status` starts `Forward`, the first
    /// [`AnimationController::finish`] call — even one that changes
    /// nothing — would read them as different and fire a spurious status
    /// notification for a change that never happened.
    fn new_inner(
        duration: Duration,
        lower_bound: f64,
        upper_bound: f64,
        initial_value: f64,
    ) -> Self {
        let notifier = Rc::new(ChangeNotifier::new());

        let mut inner = AnimationControllerInner {
            value: initial_value,
            status: AnimationStatus::Dismissed,
            duration,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            status_listeners: Vec::new(),
            pending_delivery: VecDeque::new(),
            frame_routes: Vec::new(),
            delivering: false,
            direction: AnimationDirection::Forward,
            start_value: initial_value,
            target_value: initial_value,
            last_elapsed: Duration::ZERO,
            local_elapsed: Duration::ZERO,
            playback_rate: PlaybackRate::NORMAL,
            pending_rate: None,
            rate_epoch_elapsed: Duration::ZERO,
            rate_epoch_local: Duration::ZERO,
            clock_binding: ClockBinding::Manual,
            settle_pending: false,
            missing_clock_warned: false,
            run_generation: 0,
            run_start: RunStart::Fresh,
            sample_epoch: 0,
            run_duration: None,
            disposed: false,
            next_listener_id: 1,
            repeat: None,
            simulation: None,
            run_curve: None,
            last_committed_status: AnimationStatus::Dismissed,
            active_run: None,
            non_finite_warned: false,
        };
        let initial_status = inner.settled_status_keep_direction();
        inner.status = initial_status;
        inner.last_committed_status = initial_status;

        Self {
            inner: Rc::new(RefCell::new(inner)),
            notifier,
        }
    }

    /// Set the duration for reverse animation.
    pub fn set_reverse_duration(&self, duration: Duration) {
        let mut inner = self.inner.borrow_mut();
        inner.reverse_duration = Some(duration);
    }

    /// Set the base forward duration.
    ///
    /// An implicitly animated widget assigns this on every update so a
    /// duration change takes effect on the *next* run. It does not retime an in-flight run (the active run keeps
    /// the duration it started with, since `tick_at` scales against the
    /// already-captured `run_duration`/`duration`); the new value is read when
    /// the next `forward`/`reverse` begins the run at elapsed zero.
    pub fn set_duration(&self, duration: Duration) {
        let mut inner = self.inner.borrow_mut();
        inner.duration = duration;
    }

    /// Start animation forward from current value to upper bound.
    ///
    /// The returned [`AnimationRunFuture`] resolves `Ok(())` when this run finishes
    /// and `Err(RunCanceled)` if it is superseded (a later run starts
    /// before this one ends) or torn down ([`stop`](Self::stop)/
    /// [`set_value`](Self::set_value)/[`reset`](Self::reset)/
    /// [owning disposal](crate::DrivenController::dispose)). See
    /// [`AnimationRunFuture::when_complete_or_cancel`] for the idiom to react to
    /// either outcome without matching on it, and this method's own
    /// `# Awaiting a run` section below for the `async`/`await` route.
    ///
    /// # Awaiting a run
    ///
    /// ```rust
    /// use flui_animation::AnimationController;
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use std::task::{Context, Poll, Waker};
    /// use std::time::Duration;
    ///
    /// let controller = AnimationController::builder(Duration::from_millis(100)).build();
    /// let mut run = controller.forward().unwrap();
    ///
    /// // A caller that only cares "how did it end", not "did it end yet",
    /// // reacts to either outcome the same way:
    /// run.when_complete_or_cancel(|end| {
    ///     println!("run ended: {end:?}");
    /// });
    ///
    /// // Or await it directly. `tick_at` is normally driven by a real
    /// // ticker/Vsync; this drives it to completion by hand for the example.
    /// controller.tick_at(Duration::from_millis(100));
    /// let waker = Waker::noop();
    /// let mut cx = Context::from_waker(waker);
    /// assert_eq!(Pin::new(&mut run).poll(&mut cx), Poll::Ready(Ok(())));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward(&self) -> Result<AnimationRunFuture, AnimationError> {
        self.forward_from(None)
    }

    /// Start animation forward from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the forward duration scaled by
    /// `(upper_bound - value) / (upper_bound - lower_bound)`. A zero DISTANCE
    /// (starting at the upper bound) or a zero DURATION (the scaled run
    /// duration is `Duration::ZERO`) settles SYNCHRONOUSLY, before this
    /// call returns, with [`AnimationStatus::Completed`] and an
    /// already-complete [`AnimationRunFuture`]. See
    /// [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `from` is `NaN`, if
    /// `from` is infinite toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](crate::AnimationControllerBuilder::unbounded) (`forward`
    /// always targets `upper_bound`, which has no finite value to run to).
    pub fn forward_from(&self, from: Option<f64>) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;

        // `forward` always targets `upper_bound` — refuse before touching
        // `from`, so `forward()` (no `from` at all) is refused on an
        // unbounded controller too, not only an explicit non-finite `from`.
        if !inner.upper_bound.is_finite() {
            let err = AnimationError::NonFiniteTarget(
                "forward/forward_from targets the upper bound, which is not finite on an \
                 unbounded controller"
                    .to_string(),
            );
            return Err(Self::warn_non_finite_target(inner, err));
        }
        let entry_value = inner.value;

        let from = match from {
            Some(raw) => {
                match Self::canonicalize_value_target(
                    "forward_from's from",
                    raw,
                    inner.lower_bound,
                    inner.upper_bound,
                ) {
                    Ok(canonical) => Some(canonical),
                    Err(err) => return Err(Self::warn_non_finite_target(inner, err)),
                }
            }
            None => None,
        };
        if let Some(start) = from {
            inner.value = start;
        }

        inner.clear_run_modes(&mut retired);
        inner.direction = AnimationDirection::Forward;
        inner.start_value = inner.value;
        inner.target_value = inner.upper_bound;
        let distance = (inner.target_value - inner.value).abs();
        let run_duration = inner.scaled_run_duration(inner.duration);
        if distance < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, retired, inner));
        }

        inner.status = AnimationStatus::Forward;
        inner.run_duration = Some(run_duration);
        // `from` may have jumped `value` above without a settle (a real run
        // still starts). Notification fires "iff it actually moved" (the
        // same entry-value rule `settle_at_target` uses), so
        // `forward_from(Some(x))` from `x` itself does not fire a spurious
        // notification for a value that never changed.
        let value_change = if (inner.value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        self.finish(
            AnimationStatus::Forward,
            value_change,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Start animation in reverse from current value to lower bound.
    ///
    /// See [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse(&self) -> Result<AnimationRunFuture, AnimationError> {
        self.reverse_from(None)
    }

    /// Start animation in reverse from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the reverse duration (falling back to the forward
    /// duration) scaled by `(value - lower_bound) / (upper_bound -
    /// lower_bound)`. A zero DISTANCE (starting at the lower bound) or a zero DURATION
    /// settles SYNCHRONOUSLY, before this call returns, with
    /// [`AnimationStatus::Dismissed`] — see [`forward_from`](Self::forward_from).
    /// See [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `from` is `NaN`, if
    /// `from` is infinite toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](crate::AnimationControllerBuilder::unbounded) (`reverse`
    /// always targets `lower_bound`, which has no finite value to run to).
    pub fn reverse_from(&self, from: Option<f64>) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;

        // See `forward_from`'s own comment: refuse before touching `from`,
        // so `reverse()` (no `from` at all) is refused too.
        if !inner.lower_bound.is_finite() {
            let err = AnimationError::NonFiniteTarget(
                "reverse/reverse_from targets the lower bound, which is not finite on an \
                 unbounded controller"
                    .to_string(),
            );
            return Err(Self::warn_non_finite_target(inner, err));
        }
        let entry_value = inner.value;

        let from = match from {
            Some(raw) => {
                match Self::canonicalize_value_target(
                    "reverse_from's from",
                    raw,
                    inner.lower_bound,
                    inner.upper_bound,
                ) {
                    Ok(canonical) => Some(canonical),
                    Err(err) => return Err(Self::warn_non_finite_target(inner, err)),
                }
            }
            None => None,
        };
        if let Some(start) = from {
            inner.value = start;
        }

        inner.clear_run_modes(&mut retired);
        inner.direction = AnimationDirection::Reverse;
        inner.start_value = inner.value;
        inner.target_value = inner.lower_bound;
        let distance = (inner.target_value - inner.value).abs();
        let base = inner.reverse_duration.unwrap_or(inner.duration);
        let run_duration = inner.scaled_run_duration(base);
        if distance < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, retired, inner));
        }

        inner.status = AnimationStatus::Reverse;
        inner.run_duration = Some(run_duration);
        // See `forward_from`'s own comment: notify iff `from` actually moved
        // the value.
        let value_change = if (inner.value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        self.finish(
            AnimationStatus::Reverse,
            value_change,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Stop the animation at its current value.
    ///
    /// The status transitions to a **non-running** settled status:
    /// - [`AnimationStatus::Completed`] at (or above) the upper bound, or when
    ///   the active run direction was [`Forward`](AnimationDirection::Forward)
    /// - [`AnimationStatus::Dismissed`] at (or below) the lower bound, or when
    ///   the active run direction was [`Reverse`](AnimationDirection::Reverse)
    ///
    /// Using a non-running status is critical for frame drivers that poll
    /// `status().is_running()` (e.g. `Vsync::tick_all`) — a running status
    /// after `stop()` would allow the driver to continue ticking a stale or
    /// cleared simulation and produce non-finite pixel values.
    ///
    /// Cancels the active run's [`AnimationRunFuture`] with
    /// [`RunCanceled`](crate::RunCanceled),
    /// delivered with no controller lock held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_disposed(&inner)?;

        let delivery = inner.stop_running(&mut retired);

        let status = inner.settled_status_directed();
        inner.status = status;
        self.finish(status, ValueChange::Unchanged, delivery, retired, inner);
        Ok(())
    }

    /// Reset to the beginning (lower bound).
    ///
    /// Sets the value to the beginning — `lower_bound` on a bounded
    /// controller, `0.0` on an [`unbounded`](crate::AnimationControllerBuilder::unbounded) one (there is
    /// no `lower_bound` to return to) — and the status to
    /// [`AnimationStatus::Dismissed`]. Never fails for non-finiteness: a
    /// reset always has a value to land on. Cancels the active run's
    /// [`AnimationRunFuture`] with
    /// [`RunCanceled`](crate::RunCanceled), delivered
    /// with no controller lock held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_disposed(&inner)?;

        let delivery = inner.stop_running(&mut retired);
        inner.value = if inner.is_unbounded() {
            0.0
        } else {
            inner.lower_bound
        };
        inner.status = AnimationStatus::Dismissed;
        self.finish(
            AnimationStatus::Dismissed,
            ValueChange::Notify,
            delivery,
            retired,
            inner,
        );
        Ok(())
    }

    /// Animate to a specific value over `duration` (or the controller's forward
    /// duration when `None`, scaled by the remaining fraction of the range —
    /// see [`forward_from`](Self::forward_from)).
    ///
    /// The per-run `duration` override applies to **this run only** and does not
    /// modify the controller's base duration. An explicit `duration` is used
    /// as-is, without remaining-fraction scaling. See [`forward`](Self::forward)
    /// for the returned future's contract and the `when_complete_or_cancel`
    /// idiom.
    ///
    /// [`status`](Animation::status) is reported as
    /// [`AnimationStatus::Forward`] **regardless of whether `target` is above
    /// or below the current value**, ending [`AnimationStatus::Completed`].
    /// If `target` is
    /// already the current value (or `duration` resolves to
    /// `Duration::ZERO`), this settles synchronously; see
    /// [`forward_from`](Self::forward_from)'s doc.
    ///
    /// # Arguments
    ///
    /// * `target` - The target value (clamped to bounds)
    /// * `duration` - Optional per-run duration override
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `target` is `NaN`, if
    /// `target` is infinite toward a bound this controller does not have,
    /// or if `target - value` overflows `f64`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_animation::AnimationController;
    /// use std::time::Duration;
    ///
    /// let controller = AnimationController::builder(Duration::from_millis(100)).build();
    /// let run = controller.animate_to(1.0, None).unwrap();
    /// run.when_complete_or_cancel(|end| {
    ///     assert!(end.is_ok(), "a run nothing superseded or stopped must complete");
    /// });
    /// controller.tick_at(Duration::from_millis(100));
    /// ```
    pub fn animate_to(
        &self,
        target: f64,
        duration: Option<Duration>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_to(target, duration, AnimationDirection::Forward, None)
    }

    /// Animate back to a specific value, defaulting to the reverse duration.
    ///
    /// Like [`animate_to`](Self::animate_to) but, when `duration` is `None`,
    /// defaults to the configured reverse duration (then the base duration),
    /// scaled by the remaining fraction of the range.
    ///
    /// [`status`](Animation::status) is reported as
    /// [`AnimationStatus::Reverse`] regardless of whether `target` is above
    /// or below the current value, ending [`AnimationStatus::Dismissed`] —
    /// the mirror of [`animate_to`](Self::animate_to)'s own contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back(
        &self,
        target: f64,
        duration: Option<Duration>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_to(target, duration, AnimationDirection::Reverse, None)
    }

    /// Like [`animate_to`](Self::animate_to), but eases the run through
    /// `curve` instead of running linearly.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_to_curved(
        &self,
        target: f64,
        duration: Option<Duration>,
        curve: impl Curve + 'static,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_to(
            target,
            duration,
            AnimationDirection::Forward,
            Some(Rc::new(curve)),
        )
    }

    /// Like [`animate_back`](Self::animate_back), but eases the run through
    /// `curve` instead of running linearly.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back_curved(
        &self,
        target: f64,
        duration: Option<Duration>,
        curve: impl Curve + 'static,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_to(
            target,
            duration,
            AnimationDirection::Reverse,
            Some(Rc::new(curve)),
        )
    }

    /// Shared driver for [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back)
    /// and their `_curved` variants: interpolate from the current value to
    /// `target`, easing through `curve` (`None` = linear).
    ///
    /// `direction` is the METHOD's, not derived from `target`'s relation to
    /// the current value: it is fixed by which method was called, before
    /// `target` is even looked at. It drives both the run's status (Forward/Reverse while
    /// running, Completed/Dismissed at the end —
    /// [`AnimationDirection::settled_status`]) and, when `duration` is
    /// `None`, which base duration (`self.duration`/`self.reverse_duration`)
    /// the remaining-fraction scaling starts from — again the method's
    /// choice, not a travel comparison.
    fn drive_to(
        &self,
        target: f64,
        duration: Option<Duration>,
        direction: AnimationDirection,
        curve: Option<Rc<dyn Curve>>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut curve = Opaque::new(curve);
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;
        let entry_value = inner.value;

        // `drive_to` is shared by both directions -- name the actual method
        // the caller invoked (`animate_to`/`animate_back`, curved or not
        // share the same target argument), not the shared internal name.
        let caller = match direction {
            AnimationDirection::Forward => "animate_to's target",
            AnimationDirection::Reverse => "animate_back's target",
        };
        let target = match Self::canonicalize_value_target(
            caller,
            target,
            inner.lower_bound,
            inner.upper_bound,
        ) {
            Ok(target) => target,
            Err(err) => return Err(Self::warn_non_finite_target(inner, err)),
        };
        // `target - value` overflowing f64 (e.g. `set_value(-f64::MAX)` then
        // `animate_to(f64::MAX)`) would make `tick_time_based`'s
        // `start_value + range * eased_t` interior lerp compute `inf * t`,
        // finite but wrong, or — at an already-non-finite `start_value` —
        // `inf * 0.0 = NaN`. Refuse before any mutation rather than let a
        // run install with a span nothing downstream can interpolate.
        let span = target - entry_value;
        if !span.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "{caller}: span ({target} - {entry_value}) overflows f64"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }
        inner.clear_run_modes(&mut retired);
        inner.run_curve = curve.take();
        inner.start_value = inner.value;
        inner.target_value = target;
        inner.direction = direction;

        // Per-run override only — never clobber `inner.duration`. Without an
        // explicit duration, the direction's base duration is scaled by the
        // remaining fraction so partial runs keep the full-range velocity.
        let run_duration = duration.unwrap_or_else(|| {
            let base = match inner.direction {
                AnimationDirection::Forward => inner.duration,
                AnimationDirection::Reverse => inner.reverse_duration.unwrap_or(inner.duration),
            };
            inner.scaled_run_duration(base)
        });
        // No-op fast path: already at the target, or the run duration is
        // zero. Starting the ticker would run for the full duration,
        // re-notifying value listeners every frame while the value never
        // changes, so settle immediately with a single notification instead
        // (see `forward_from`).
        if (target - inner.value).abs() < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, retired, inner));
        }

        inner.status = inner.direction.running_status();
        inner.run_duration = Some(run_duration);
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        let status = inner.status;
        self.finish(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Settle a run whose distance or duration is trivial: snap the value,
    /// stop the ticker, cancel whatever run this displaced, and report the
    /// run's directed settled status — no transient running status, no
    /// full-duration no-op run. Returns an already-complete [`AnimationRunFuture`]
    /// for the (trivial) run this call represents. The single settle
    /// chokepoint for zero-DISTANCE (`forward()` already at the upper bound)
    /// and zero-DURATION (`forward(..., Some(Duration::ZERO))`) runs alike
    /// (issue #1171).
    ///
    /// `entry_value` is the value at the METHOD's entry, before
    /// `forward_from(Some(x))`/`reverse_from(Some(x))` apply `from` —
    /// comparing against the post-`from` value here would miss a jump
    /// (`forward_from(Some(1.0))` from `0.3` would report no value change).
    /// Value listeners fire only when `entry_value` actually differs from
    /// where this settle lands.
    fn settle_at_target(
        &self,
        entry_value: f64,
        retired: RetiredSources,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) -> AnimationRunFuture {
        if inner.delivering {
            inner.start_value = inner.value;
            inner.run_duration = Some(Duration::ZERO);
            inner.status = inner.direction.running_status();
            Self::begin_run(&mut inner);
            inner.settle_pending = true;
            let (completer, future) = AnimationRunFuture::pending();
            let displaced = inner
                .active_run
                .replace(completer)
                .map(RunCompleter::cancel);
            let status = inner.status;
            let change = if (inner.value - entry_value).abs() < BOUND_EPSILON {
                ValueChange::Unchanged
            } else {
                ValueChange::Notify
            };
            self.finish(status, change, displaced, retired, inner);
            return future;
        }
        inner.value = inner.target_value;
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(RunCompleter::cancel);
        let value_change = if (inner.target_value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        self.finish(status, value_change, delivery, retired, inner);
        AnimationRunFuture::complete()
    }

    /// Repeat the animation, bouncing if `reverse` is true. Repeats forever.
    ///
    /// An infinite repeat's [`AnimationRunFuture`] resolves only by cancellation —
    /// it has no natural end — EXCEPT that a zero effective period settles
    /// synchronously at the call with an already-complete future; see
    /// [`repeat_with`](Self::repeat_with)'s `period` bullet. A finite
    /// [`repeat_with`](Self::repeat_with) count completes normally once
    /// exhausted. See [`forward`](Self::forward) for the returned future's
    /// general contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] on an
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller — a default repeat targets
    /// this controller's own (infinite) bounds; use
    /// [`repeat_with`](Self::repeat_with) with an explicit finite range
    /// instead.
    pub fn repeat(&self, reverse: bool) -> Result<AnimationRunFuture, AnimationError> {
        self.repeat_with(None, None, reverse, None, None)
    }

    /// Repeat the animation with full control over range, period, and count.
    ///
    /// The run starts from the CURRENT value, clamped into `[min, max]` —
    /// not from `min` — so a `repeat()` issued every build (a common pattern
    /// for a looping indicator) progresses instead of freezing at the start
    /// each time; to start at `min`, call [`set_value`](Self::set_value)
    /// first. `value`/`status`/`direction` at any later
    /// [`tick_at`](Self::tick_at) are a pure function of the elapsed time
    /// since this call, the range, `period`, `reverse`, and `count` — the
    /// frame partition never changes the answer. `count` boundaries are
    /// measured from that phase origin, not from a fresh cycle 0: a run
    /// started mid-cycle ends `count` boundaries later, not `count` full
    /// periods (`count*period - initial_offset`; Compose:
    /// `iterations*duration - initialOffset`), and a finite run
    /// lands on the END of its last cycle.
    ///
    /// # Arguments
    ///
    /// * `min` - Lower endpoint of the repeat range (defaults to `lower_bound`)
    /// * `max` - Upper endpoint of the repeat range (defaults to `upper_bound`)
    /// * `reverse` - Bounce back and forth instead of restarting each cycle
    /// * `period` - Per-cycle duration (defaults to the forward duration);
    ///   resolved once at this call — a later [`set_duration`](Self::set_duration)
    ///   does not retime an active repeat, and both legs of a bounce share it.
    ///   A zero EFFECTIVE period (an explicit [`Duration::ZERO`], or a
    ///   defaulted zero `duration`) settles SYNCHRONOUSLY at the call instead
    ///   of installing a run that could never advance (Android's rule for a
    ///   0-duration animator: skip to the end; Compose rejects it)
    /// * `count` - Number of cycles; `None` repeats indefinitely (see
    ///   [`repeat`](Self::repeat) for what that means for the returned
    ///   future). `Some(0)` also settles SYNCHRONOUSLY at the call, at the
    ///   CURRENT (clamped) value with no landing jump — zero cycles run, so
    ///   there is nothing to land on (Web Animations semantics for an
    ///   empty active interval; Compose throws for `iterations < 1`)
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::InvalidBounds`] if `min`/`max` are `NaN` or
    /// describe an inverted or empty range (a range-shape error, on any
    /// controller). Returns [`AnimationError::NonFiniteTarget`] if the
    /// EFFECTIVE range (after defaulting unset endpoints to this
    /// controller's own bounds) is not finite — only reachable on an
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller with `min`/`max` left
    /// unset (or set on only one side); pass an explicit finite range to
    /// repeat on an unbounded controller.
    pub fn repeat_with(
        &self,
        min: Option<f64>,
        max: Option<f64>,
        reverse: bool,
        period: Option<Duration>,
        count: Option<u32>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;
        let entry_value = inner.value;

        // A caller-supplied NaN endpoint is a range-shape error on ANY
        // controller (unguarded, `repeat_with(Some(f64::NAN), ..)` reaches
        // `inner.value.clamp(lo, hi)` below with `lo` itself NaN, which
        // panics inside `f64::clamp`'s own `assert!(min <= max)`) — checked
        // before defaulting/clamping so it can never be confused with the
        // *effective*-range non-finiteness an unbounded controller's own
        // defaulted bound produces below.
        if min.is_some_and(f64::is_nan) || max.is_some_and(f64::is_nan) {
            return Err(AnimationError::InvalidBounds(format!(
                "repeat range endpoints must not be NaN (min={min:?}, max={max:?})"
            )));
        }

        // Clamp the repeat range into the controller's bounds and reject an
        // empty/inverted range, so a repeat run can never start `value` (or its
        // ticks) outside `[lower_bound, upper_bound]` — consistent with
        // [`with_bounds`]'s `InvalidBounds` contract.
        // `min == max` is refused: a repeat that can structurally never
        // change value is a caller error that would hold the frame loop open
        // doing nothing, the same contract `with_bounds` already applies to
        // an empty range.
        let lo = min
            .unwrap_or(inner.lower_bound)
            .clamp(inner.lower_bound, inner.upper_bound);
        let hi = max
            .unwrap_or(inner.upper_bound)
            .clamp(inner.lower_bound, inner.upper_bound);
        if lo >= hi {
            return Err(AnimationError::InvalidBounds(format!(
                "repeat min ({lo}) must be less than max ({hi}) within bounds [{}, {}]",
                inner.lower_bound, inner.upper_bound
            )));
        }
        // The range wasn't inverted, but a side that defaulted from an
        // unbounded controller's own +-inf bound leaked through: there is no
        // finite range to repeat inside. `repeat()`/`repeat_with(None,
        // None, ..)` on an unbounded controller is refused this way; an
        // explicit finite range on both sides never reaches here.
        if !lo.is_finite() || !hi.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "repeat range [{lo}, {hi}] is not finite; this controller is unbounded in \
                 that direction -- pass an explicit finite range"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }

        // A leftover per-run mode (an `animate_to_curved` curve, a fling
        // simulation) must not shape a following repeat — a repeat
        // applies no curve at all, and `tick_repeat` applies none either.
        // The curve specifically can never leak (`tick_repeat` never reads
        // `run_curve`), but a leftover `simulation`/`run_duration` would
        // still leak into `velocity()`/`current_duration()` without this.
        inner.clear_run_modes(&mut retired);

        // The value at the call is the pure function sampled at elapsed
        // time zero — NOT a bare `lo`: from `value == max` in restart mode
        // that is `lo` (the phase wraps); in bounce mode a
        // value starting at `max` reports the reverse leg. Widen to f64
        // before subtracting — near `max` the f64 difference loses bits,
        // ~60ns of quantization at a 1s period, harmless to the phase this
        // computes.
        let v = inner.value.clamp(lo, hi);

        // `count: Some(0)` is a degenerate case distinct from a zero
        // period: zero cycles run AT ALL, regardless of period, so the
        // value at the call is the plain clamp with no landing jump —
        // there is no cycle to land on. Checked before `RepeatRun` even
        // exists; the run never reaches `tick_repeat`.
        if count == Some(0) {
            inner.direction = AnimationDirection::Forward;
            inner.target_value = v;
            return Ok(self.settle_at_target(entry_value, retired, inner));
        }

        // Resolved ONCE, not read live on every tick: a later `set_duration`
        // must not retime an active repeat (the period is captured at the
        // call), and one period
        // for both legs of a bounce keeps the modular-nanosecond arithmetic
        // in `tick_repeat` exact.
        let period = period.unwrap_or(inner.duration);
        let period_ns = period.as_nanos();

        if period_ns == 0 {
            // ZERO EFFECTIVE PERIOD, any count: settle SYNCHRONOUSLY at the
            // call instead of installing a run that can never advance —
            // Android's rule ("0 duration animator, ignore the repeat count
            // and skip to the end", `ValueAnimator.animateBasedOnTime`);
            // Compose rejects it. An infinite zero-period
            // repeat ticking once per frame would hold the frame loop open
            // forever doing nothing, so it settles instead — a documented
            // exception to "an infinite repeat's future resolves only by
            // cancellation". `count.saturating_sub(1)` treats an explicit
            // `Some(0)` the same as `Some(1)` (`Some(0)` alone is already
            // handled above and never reaches here); landing on cycle 0's
            // end rather than underflowing.
            let landing_index = u128::from(count.map_or(0, |c| c.saturating_sub(1)));
            let (value, direction) =
                AnimationControllerInner::repeat_landing(reverse, lo, hi, landing_index);
            inner.direction = direction;
            inner.target_value = value;
            return Ok(self.settle_at_target(entry_value, retired, inner));
        }

        // Every degenerate case has returned: a `RepeatRun` is constructed
        // only here, so `period_ns > 0` holds by construction for every live
        // run and nothing downstream needs to guard it again.
        let ratio = (v - lo) / (hi - lo);
        let initial_ns = (ratio * period_ns as f64).round() as u128;
        let run = RepeatRun {
            reverse,
            min: lo,
            max: hi,
            period,
            period_ns,
            count,
            initial_ns,
        };
        let sample = AnimationControllerInner::repeat_sample(&run, initial_ns);
        inner.repeat = Some(run);
        inner.direction = sample.direction;
        inner.status = sample.direction.running_status();
        inner.start_value = sample.start;
        inner.target_value = sample.target;
        inner.value = sample.value;
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        // The value is set directly, without notifying listeners — the
        // value-at-the-call jump is real but reported on the run's first
        // tick, not synchronously here.
        let status = inner.status;
        self.finish(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Drive the animation with a spring (fling) and initial velocity.
    ///
    /// Positive velocity drives toward the upper bound; negative toward the lower.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `velocity` is not
    /// finite, or if this controller is [`unbounded`](crate::AnimationControllerBuilder::unbounded) in
    /// the direction `velocity` drives toward (a fling has no finite end to
    /// spring at).
    /// Returns [`AnimationError::InvalidSpring`] if the spring is underdamped
    /// (would oscillate).
    ///
    /// # Example
    ///
    /// ```
    /// # use flui_animation::AnimationController;
    /// # use flui_scheduler::UpdateScheduler;
    /// # use std::time::Duration;
    /// # let scheduler = UpdateScheduler::new();
    /// let controller = AnimationController::builder(Duration::from_millis(300)).build();
    /// controller.fling(1.0).unwrap(); // Fling forward
    /// ```
    pub fn fling(&self, velocity: f64) -> Result<AnimationRunFuture, AnimationError> {
        self.fling_with(velocity, None)
    }

    /// Drive the animation with a custom spring and initial velocity.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`fling`](Self::fling).
    /// Returns [`AnimationError::InvalidSpring`] if the spring is underdamped.
    pub fn fling_with(
        &self,
        velocity: f64,
        spring: Option<SpringDescription>,
    ) -> Result<AnimationRunFuture, AnimationError> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        Self::check_run_admission(&inner)?;

        if !velocity.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "fling velocity {velocity} must be finite"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }

        // Compute into locals; every refusal below must leave `inner`
        // untouched, so `direction` is assigned to the controller only
        // after the InvalidSpring check too -- assigning it any earlier
        // would corrupt `direction` on a refused fling, observable only via
        // a later `stop()`.
        let direction = if velocity < 0.0 {
            AnimationDirection::Reverse
        } else {
            AnimationDirection::Forward
        };
        let target = if velocity < 0.0 {
            inner.lower_bound - FLING_TOLERANCE.distance
        } else {
            inner.upper_bound + FLING_TOLERANCE.distance
        };
        if !target.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "fling target {target} is not finite; this controller is unbounded in the \
                 direction velocity {velocity} drives toward"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }

        let spring = spring.unwrap_or_else(default_fling_spring);
        let sim =
            SpringSimulation::new(spring, inner.value, target, velocity).with_snap_to_end(true);
        if sim.spring_type() == SpringType::Underdamped {
            return Err(AnimationError::InvalidSpring(
                "Underdamped springs oscillate and cannot be used for fling. \
                 Use animate_with() for oscillating springs."
                    .to_string(),
            ));
        }

        inner.direction = direction;
        inner.clear_run_modes(&mut retired);
        inner.simulation = Some(SimulationRun::Fling {
            source: Rc::new(sim),
            bound: match direction {
                AnimationDirection::Forward => inner.upper_bound,
                AnimationDirection::Reverse => inner.lower_bound,
            },
            direction,
        });
        inner.status = inner.direction.running_status();
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        let status = inner.status;
        self.finish(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Drive the animation according to a custom simulation. Works
    /// unmodified on an [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller — a
    /// simulation is not refused the way a bound-targeting run is, since it
    /// carries its own `is_done` termination rather than running toward
    /// `lower_bound`/`upper_bound`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `simulation.x(0.0)` is
    /// not finite.
    ///
    /// # Example
    ///
    /// ```
    /// # use flui_animation::{AnimationController, simulation::SpringSimulation};
    /// # use flui_animation::simulation::SpringDescription;
    /// # use flui_scheduler::UpdateScheduler;
    /// # use std::time::Duration;
    /// # let scheduler = UpdateScheduler::new();
    /// let controller = AnimationController::builder(Duration::from_millis(300)).build();
    /// let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 0.5);
    /// let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
    /// controller.animate_with(sim).unwrap();
    /// ```
    pub fn animate_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_simulation(Box::new(simulation), AnimationDirection::Forward)
    }

    /// Drive the animation according to a custom simulation, reporting reverse.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same condition
    /// as [`animate_with`](Self::animate_with).
    pub fn animate_back_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<AnimationRunFuture, AnimationError> {
        self.drive_simulation(Box::new(simulation), AnimationDirection::Reverse)
    }

    fn drive_simulation(
        &self,
        simulation: Box<dyn Simulation>,
        direction: AnimationDirection,
    ) -> Result<AnimationRunFuture, AnimationError> {
        // `Simulation::x` is arbitrary caller code, evaluated BEFORE taking
        // the lock: running it under `inner`'s guard (as a `lock, evaluate,
        // proceed` shape would) lets a caller whose `x` re-enters this same
        // controller — directly, or through anything reachable from it —
        // deadlock on the non-reentrant `parking_lot::Mutex`. A simulation
        // that starts non-finite would otherwise also install a run whose
        // `is_done` may never fire — refused below, before any mutation,
        // exactly like every other non-finite entry point.
        let mut simulation = Opaque::new(Rc::<dyn Simulation>::from(simulation));
        let initial = simulation.get().x(0.0);

        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        // Initial sampling has already run and may have panicked or changed
        // the controller. Of the returned validation errors, Disposed wins
        // over a non-finite initial value; a sampling panic propagates first.
        Self::check_run_admission(&inner)?;

        if !initial.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "simulation.x(0.0) = {initial} is not finite"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }

        inner.clear_run_modes(&mut retired);
        inner.direction = direction;
        inner.status = direction.running_status();
        inner.value = initial.clamp(inner.lower_bound, inner.upper_bound);
        inner.simulation = Some(SimulationRun::Custom(simulation.take()));
        // Queue the restart; finish commits the run before invoking the wake hook.
        Self::begin_run(&mut inner);
        let (completer, future) = AnimationRunFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(RunCompleter::cancel);

        let status = inner.status;
        self.finish(
            status,
            ValueChange::Unchanged,
            displaced_delivery,
            retired,
            inner,
        );
        Ok(future)
    }

    /// Apply `rate` at the next sample, preserving the local run time there.
    ///
    /// A paused run stays installed and keeps its completion future. A bound
    /// controller requests the sample that applies this change.
    pub fn set_playback_rate(&self, rate: PlaybackRate) {
        let mut inner = self.inner.borrow_mut();
        if inner.disposed || inner.pending_rate.unwrap_or(inner.playback_rate) == rate {
            return;
        }
        inner.pending_rate = Some(rate);
        inner
            .pending_delivery
            .push_back(ControllerDelivery::RequestFrame);
        let status = inner.status;
        self.finish(
            status,
            ValueChange::Unchanged,
            None,
            RetiredSources::new(),
            inner,
        );
    }

    /// Rate currently applied to samples; a pending change takes effect next tick.
    #[must_use]
    pub fn playback_rate(&self) -> PlaybackRate {
        self.inner.borrow().playback_rate
    }

    /// A monotonically increasing run-generation counter, bumped once each time
    /// a fresh run begins — every `forward`/`reverse`/`animate_to`/`animate_back`/
    /// `repeat`/`fling`/`animate_with` (i.e. every internal `restart_ticker`
    /// that begins the run at elapsed zero).
    ///
    /// An external, deterministic frame driver (e.g. `flui-testing`'s
    /// `HeadlessBinding`) reads this to detect that a new run's `t = 0` was just
    /// established and re-anchor the virtual instant it feeds to
    /// [`tick_at`](Self::tick_at). Without it, a controller run a *second* time
    /// (forward to completion, then reverse) would be ticked from the first
    /// run's stale anchor and snap straight to its target on the first frame.
    ///
    /// The counter is **stable across [`tick_at`](Self::tick_at)** (ticking
    /// advances a run, it does not start one), and the settle-without-restart
    /// paths (`stop`/`reset`/`set_value`; `forward`/`reverse`/`animate_to`/
    /// `animate_back` whose distance or duration is trivial; and
    /// `repeat_with`'s own zero-effective-period and zero-`count` settles —
    /// all of which settle through the private `settle_at_target`
    /// chokepoint) leave it untouched. Exhaustion permanently refuses new runs;
    /// an already issued generation is never reused.
    #[must_use]
    pub fn run_generation(&self) -> u64 {
        self.inner.borrow().run_generation
    }

    /// One-lock snapshot for [`Vsync`](crate::vsync::Vsync)'s per-frame walk —
    /// see [`WalkProbe`]'s own doc for the perf rationale.
    ///
    /// `live_running` is `!disposed && active_run.is_some()` — **not**
    /// `status.is_running()`, which is the wrong "is a run installed"
    /// predicate for two independent reasons:
    /// - [Owning disposal](crate::DrivenController::dispose) leaves `status` untouched,
    ///   so a controller disposed mid-run keeps whatever
    ///   running status it had.
    /// - [`set_value`](Self::set_value) reports a *directional* running
    ///   status at an interior value
    ///   ([`settled_status_keep_direction`](AnimationControllerInner::settled_status_keep_direction))
    ///   even though it already called `stop_running()` and cleared
    ///   `active_run`. A `Vsync`-driven controller that receives a
    ///   `set_value` mid-run would otherwise still read `status.is_running()
    ///   == true`, and the NEXT `tick_all` would recompute its value from
    ///   the stale, already-stopped run's `start_value`/`target_value`,
    ///   silently overwriting what `set_value` had just set.
    ///
    /// `active_run.is_some()` is the actual, always-consistent fact: every
    /// run-starting method installs it in the same locked region it sets a
    /// running status in, and every run-ending path (`stop_running` for
    /// `stop`/`set_value`/`reset`, and `dispose`) clears it. Folding in
    /// `!disposed` this way is also what keeps a disposed-but-not-yet-
    /// unregistered controller from ticking (via
    /// [`tick_at`](Self::tick_at)) or keeping
    /// [`Vsync::has_running`](crate::vsync::Vsync::has_running) reporting
    /// `true`, holding the frame loop open forever.
    #[must_use]
    pub(crate) fn walk_probe(&self) -> WalkProbe {
        let inner = self.inner.borrow();
        WalkProbe {
            generation: inner.run_generation,
            start: inner.run_start,
            live_running: !inner.disposed
                && inner.active_run.is_some()
                && (!inner.playback_rate.is_paused() || inner.pending_rate.is_some()),
        }
    }

    /// Advance the animation to `raw_elapsed_secs` seconds (ticker timeline,
    /// pre-dilation) since the ticker started.
    ///
    /// This is the single time-driven entry point: time-based runs interpolate
    /// `start_value -> target_value`, simulations sample `x(t)`, and a repeat
    /// samples its leg/phase/exhaustion as a pure function of `cycle` (see
    /// the private `tick_repeat`). Value and status listeners are fired
    /// only after the inner lock is released.
    pub fn tick_at(&self, elapsed: Duration) {
        let mut retirement = Retirement::new();
        retirement.run_with(|retirement| self.tick_at_with_retirement(elapsed, retirement));
        retirement.finish();
    }

    pub(crate) fn tick_at_with_retirement(&self, elapsed: Duration, retirement: &mut Retirement) {
        let source;
        let identity;
        let cycle;
        let time;
        {
            let mut inner = self.inner.borrow_mut();
            if inner.disposed || inner.active_run.is_none() || elapsed < inner.last_elapsed {
                return;
            }
            let Some(epoch) = inner.sample_epoch.checked_add(1) else {
                let mut retired = RetiredSources::new();
                let delivery = inner.stop_running(&mut retired);
                let status = inner.settled_status_directed();
                inner.status = status;
                self.finish_with_retirement(
                    status,
                    ValueChange::Unchanged,
                    delivery,
                    retired,
                    inner,
                    retirement,
                );
                retirement.run(|| {
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
        retirement.run_with(|retirement| match source.get() {
            TickSource::Repeat(run) => {
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                time.commit(&mut inner);
                self.tick_repeat(inner, *run, cycle, retirement);
            }
            TickSource::Simulation(simulation) => {
                let sampled = simulation.source().x(cycle);
                // A position callback may stop or replace the run. Do not call
                // another method on its stale source after that decision.
                let current = { self.inner.borrow_mut().matches_sample(&identity) };
                if !current {
                    return;
                }
                let is_done = sampled.is_finite()
                    && (simulation.reached_bound(sampled) || simulation.source().is_done(cycle));
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if sampled.is_finite() {
                    time.commit(&mut inner);
                }
                self.tick_simulation(inner, sampled, is_done, retirement);
            }
            TickSource::Time {
                curve,
                duration,
                start,
                target,
            } => {
                let t = if duration.is_zero() {
                    1.0
                } else {
                    (cycle / duration.as_secs_f64()).clamp(0.0, 1.0)
                };
                let value = if t <= 0.0 {
                    *start
                } else if t >= 1.0 {
                    *target
                } else {
                    let eased = curve.as_ref().map_or(t, |curve| curve.transform(t));
                    start + (target - start) * eased
                };
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if value.is_finite() {
                    time.commit(&mut inner);
                }
                self.tick_time_based(inner, t, value, retirement);
            }
        });
        retirement.retire(source);
    }

    /// Settle an admitted run without reading a wall clock or invoking its curve.
    /// Reentrant replacement invalidates the source before the next callout.
    fn settle_run(&self, generation: u64, recovery: &mut Retirement) -> bool {
        let (source, identity, warn) = {
            let mut inner = self.inner.borrow_mut();
            if inner.disposed
                || inner.active_run.is_none()
                || inner.run_generation != generation
                || (inner.clock_binding != ClockBinding::Missing
                    && !inner.current_duration().is_zero())
            {
                return false;
            }
            let warn = inner.clock_binding == ClockBinding::Missing && !inner.missing_clock_warned;
            inner.missing_clock_warned |= warn;
            (
                Opaque::new(inner.tick_source()),
                SampleIdentity {
                    generation,
                    epoch: inner.sample_epoch,
                },
                warn,
            )
        };
        if warn {
            recovery
                .run(|| tracing::warn!("animation has no clock; finite work settles immediately"));
        }
        recovery.run_with(|recovery| match source.get() {
            TickSource::Time { target, .. } => {
                let inner = self.inner.borrow_mut();
                if inner.matches_sample(&identity) {
                    self.tick_time_based(inner, 1.0, *target, recovery);
                }
            }
            TickSource::Repeat(run) => {
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if let Some(count) = run.count {
                    self.finish_repeat(inner, run, count, recovery);
                } else {
                    let mut parked = *run;
                    parked.initial_ns = 0;
                    inner.repeat = Some(parked);
                    inner.value = run.min;
                    inner.start_value = run.min;
                    inner.target_value = run.max;
                    inner.direction = AnimationDirection::Forward;
                    inner.status = AnimationStatus::Forward;
                    inner.last_elapsed = Duration::ZERO;
                    inner.local_elapsed = Duration::ZERO;
                    inner.rate_epoch_elapsed = Duration::ZERO;
                    inner.rate_epoch_local = Duration::ZERO;
                    self.finish_with_retirement(
                        AnimationStatus::Forward,
                        ValueChange::Notify,
                        None,
                        RetiredSources::new(),
                        inner,
                        recovery,
                    );
                }
            }
            TickSource::Simulation(simulation) => {
                if matches!(simulation, SimulationRun::Motion(_)) {
                    let inner = self.inner.borrow_mut();
                    if inner.matches_sample(&identity) {
                        let target = inner.target_value;
                        self.tick_simulation(inner, target, true, recovery);
                    }
                    return;
                }
                let mut last_finite = self.inner.borrow().value;
                for exponent in 0..=8 {
                    let time = 0.25 * f64::from(1u32 << exponent);
                    let sample = simulation.source().x(time);
                    if !self.inner.borrow().matches_sample(&identity) {
                        return;
                    }
                    if sample.is_finite() {
                        last_finite = sample;
                        let done =
                            simulation.reached_bound(sample) || simulation.source().is_done(time);
                        if !self.inner.borrow().matches_sample(&identity) {
                            return;
                        }
                        if done {
                            break;
                        }
                    }
                }
                let inner = self.inner.borrow_mut();
                if inner.matches_sample(&identity) {
                    self.tick_simulation(inner, last_finite, true, recovery);
                }
            }
        });
        recovery.retire(source);
        true
    }

    /// Commit a caller sample only after its run and tick identity survived.
    fn tick_simulation(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        sampled: f64,
        is_done: bool,
        retirement: &mut Retirement,
    ) {
        if !sampled.is_finite() {
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;
            self.end_simulation_run(inner, ValueChange::Unchanged, retirement);
            Self::warn_non_finite_value(should_warn, sampled, NonFiniteOutcome::EndedRun);
            return;
        }
        inner.value = sampled.clamp(inner.lower_bound, inner.upper_bound);
        if is_done {
            self.end_simulation_run(inner, ValueChange::Notify, retirement);
        } else {
            let status = inner.status;
            self.finish_with_retirement(
                status,
                ValueChange::Notify,
                None,
                RetiredSources::new(),
                inner,
                retirement,
            );
        }
    }

    /// End a simulation run at its current value — the one place
    /// [`tick_simulation`](Self::tick_simulation)'s two ending paths agree:
    /// a normal `is_done` completion (`ValueChange::Notify`, since `value`
    /// was just written to the final sample) and a non-finite sample
    /// (`ValueChange::Unchanged`, since `value` is deliberately left at its
    /// last finite point — the caller already decided not to write the bad
    /// sample through). Clears `simulation`, stops the ticker, settles
    /// `status` by direction, and publishes the completion BEFORE `finish`
    /// unlocks (the completer resolves before listeners are notified), so a panicking value/status listener still
    /// leaves the run `Ok` — the admitted delivery preserves the
    /// already-published outcome. `complete`, not `cancel`: both paths
    /// are the run ending on its own terms, never a cancellation.
    fn end_simulation_run(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        value_change: ValueChange,
        retirement: &mut Retirement,
    ) {
        let retired = RetiredSources {
            simulation: inner.simulation.take().map(Opaque::new),
            curve: None,
            callbacks: SmallVec::new(),
            value_callbacks: SmallVec::new(),
        };
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(RunCompleter::complete);
        self.finish_with_retirement(status, value_change, delivery, retired, inner, retirement);
    }

    /// Time-based (tween) branch of [`tick_at`](Self::tick_at). Never called
    /// while a repeat is active — [`tick_at`](Self::tick_at) dispatches a
    /// repeat to [`tick_repeat`](Self::tick_repeat) instead.
    fn tick_time_based(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        t: f64,
        value: f64,
        retirement: &mut Retirement,
    ) {
        if !value.is_finite() {
            return;
        }
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        if t < 1.0 {
            let status = inner.status;
            self.finish_with_retirement(
                status,
                ValueChange::Notify,
                None,
                RetiredSources::new(),
                inner,
                retirement,
            );
            return;
        }
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(RunCompleter::complete);
        self.finish_with_retirement(
            status,
            ValueChange::Notify,
            delivery,
            RetiredSources::new(),
            inner,
            retirement,
        );
    }

    /// Repeat branch of [`tick_at`](Self::tick_at). `run` is a snapshot of
    /// `inner.repeat` taken by the caller's `if let Some(run) = inner.repeat`
    /// match — [`RepeatRun`] is `Copy`, so this never needs to re-read (or
    /// `expect`) the `Option` field itself.
    ///
    /// `value`/`status`/`direction` are a pure function of `cycle` (elapsed
    /// time since the run started) and `run` — the frame partition never
    /// changes the answer: `tick_at(1.25)` gives the same result whether or
    /// not an intervening `tick_at(1.0)` happened, for any number of cycles
    /// a long frame spans. All arithmetic is integer nanoseconds —
    /// Compose's `VectorizedRepeatableSpec` and GPUI both reduce modulo the
    /// period in nanos before any float conversion, because the f64
    /// predicate `total >= count * period` is unsound at an exact boundary
    /// (`0.3 >= 3.0 * 0.1` is `false`), which would land an exhaustion a
    /// frame late.
    ///
    /// [`AnimationControllerInner::repeat_leg`]/`repeat_landing`/
    /// `repeat_sample` are the only place the leg/landing/phase parity math
    /// lives; this function calls them instead of hand-copying it.
    fn finish_repeat(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        run: &RepeatRun,
        count: u32,
        recovery: &mut Retirement,
    ) {
        let (value, direction) = AnimationControllerInner::repeat_landing(
            run.reverse,
            run.min,
            run.max,
            u128::from(count.saturating_sub(1)),
        );
        inner.value = value;
        inner.start_value = value;
        inner.target_value = value;
        inner.direction = direction;
        inner.repeat = None;
        let status = direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(RunCompleter::complete);
        self.finish_with_retirement(
            status,
            ValueChange::Notify,
            delivery,
            RetiredSources::new(),
            inner,
            recovery,
        );
    }

    fn tick_repeat(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        run: RepeatRun,
        cycle: f64,
        retirement: &mut Retirement,
    ) {
        // `tick_at` already clamps `cycle` to `.max(0.0)`, and NaN cannot
        // reach it (`f64::max` returns the non-NaN operand), so the only
        // remaining failure is `Err` on an out-of-range `cycle` (e.g.
        // rounding `Duration::MAX` past its admitted range). That must saturate the SAME direction
        // real time does — to a very large elapsed time, not to zero: a
        // zero-rewind would let a pathological input never exhaust a finite
        // repeat while `forward()` on the same input completes normally.
        // `Duration::MAX`'s nanoseconds (~1.8e28) plus `run.initial_ns`
        // (bounded by `run.period_ns`, itself at most `Duration::MAX`'s
        // nanoseconds) is at most ~3.7e28, well inside `u128`.
        let elapsed_ns = Duration::try_from_secs_f64(cycle)
            .unwrap_or(Duration::MAX)
            .as_nanos();
        let total_ns = elapsed_ns + run.initial_ns;

        // `saturating_sub(1)` treats a degenerate `count == 0` the same as
        // `count == 1` (lands on cycle 0's end) rather than underflowing a
        // u128 index — `repeat_with` already settles a true `Some(0)`
        // synchronously at the call, so this is defense in depth, not a
        // reachable path today.
        if let Some(count) = run.count
            && total_ns >= u128::from(count) * run.period_ns
        {
            self.finish_repeat(inner, &run, count, retirement);
            return;
        }

        let sample = AnimationControllerInner::repeat_sample(&run, total_ns);
        inner.direction = sample.direction;
        inner.start_value = sample.start;
        inner.target_value = sample.target;
        // `enqueue_status_change` dedups repeated same-status writes, so a leg
        // flip fires exactly one status change and an even number of
        // skipped bounce cycles in one long frame fires none. Value
        // listeners fire on every tick, as they do for the time-based and
        // simulation branches (unconditionally): a tick is a frame, and a listener that repaints
        // per frame must not be starved by a sample that happens to repeat
        // the previous value.
        inner.value = sample.value;
        let status = sample.direction.running_status();
        inner.status = status;
        self.finish_with_retirement(
            status,
            ValueChange::Notify,
            None,
            RetiredSources::new(),
            inner,
            retirement,
        );
    }

    /// Set the value directly without animating; recomputes status and notifies.
    ///
    /// Stops any active run first — otherwise
    /// a live ticker keeps re-registering itself with the scheduler and the
    /// next frame recomputes the value from the stale run's `start_value`/
    /// `target_value`, silently overwriting what was just set.
    ///
    /// A non-finite input is canonicalized on a BOUNDED controller — `NaN`
    /// to the lower bound, `+-inf` to whichever bound it points at — the
    /// same rule `clamp` already applies to a finite input, made total over
    /// `NaN` (which `clamp` alone propagates unchanged and would otherwise
    /// poison every downstream curve/tween evaluation). On an
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller a non-finite input is
    /// instead a FULL no-op: no active run is stopped, no notification
    /// fires, the value stays exactly what it was — a poisoned gesture drag
    /// or a bad computation must not clobber a live fling or snap a
    /// scrollable to a bound it doesn't have. Either way the warning below
    /// is latched (`non_finite_warned`): it fires once per controller, not
    /// once per frame of a misbehaving caller.
    pub fn set_value(&self, value: f64) {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        if inner.disposed {
            return;
        }

        if !value.is_finite() {
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;

            if inner.is_unbounded() {
                drop(inner);
                Self::warn_non_finite_value(should_warn, value, NonFiniteOutcome::Ignored);
                return;
            }

            let delivery = inner.stop_running(&mut retired);
            let canonical = if value.is_nan() {
                inner.lower_bound
            } else {
                value
            };
            inner.value = canonical.clamp(inner.lower_bound, inner.upper_bound);
            let status = inner.settled_status_keep_direction();
            inner.status = status;
            self.finish(status, ValueChange::Notify, delivery, retired, inner);
            Self::warn_non_finite_value(should_warn, value, NonFiniteOutcome::Canonicalized);
            return;
        }

        let delivery = inner.stop_running(&mut retired);
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        self.finish(status, ValueChange::Notify, delivery, retired, inner);
    }

    /// Close the kernel and cancel its run under the owning lifecycle's recovery.
    /// The owner withdraws its seat before entering this idempotent drain.
    pub(crate) fn dispose(&self, retirement: &mut Retirement) {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        if inner.disposed {
            return;
        }
        let delivery = inner.active_run.take().map(RunCompleter::cancel);
        inner.clear_run_modes(&mut retired);
        retired.callbacks.extend(
            inner
                .status_listeners
                .drain(..)
                .map(|(_, callback)| Opaque::new(callback)),
        );
        inner.disposed = true;
        retired.value_callbacks.extend(
            self.notifier
                .dispose_and_take_listeners()
                .into_iter()
                .map(Opaque::new),
        );
        let status = inner.status;
        self.finish_with_retirement(
            status,
            ValueChange::Unchanged,
            delivery,
            retired,
            inner,
            retirement,
        );
    }

    fn check_disposed(inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        if inner.disposed {
            Err(AnimationError::Disposed)
        } else {
            Ok(())
        }
    }

    fn check_run_admission(inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        Self::check_disposed(inner)?;
        if inner.run_generation == u64::MAX || inner.sample_epoch == u64::MAX {
            Err(AnimationError::IdentityExhausted)
        } else {
            Ok(())
        }
    }

    /// Canonicalize a caller-supplied value-space input — `animate_to`/
    /// `animate_back`'s `target`, `forward_from`/`reverse_from`'s `from` —
    /// against this controller's bounds. `caller` names the method and
    /// parameter (e.g. `"animate_to's target"`) so the `NonFiniteTarget`
    /// message identifies which call and argument is at fault — the only
    /// place that identity is visible, since production callers of these
    /// methods discard the `Result` and read only the `tracing::warn!` this
    /// error also reaches.
    ///
    /// `NaN` is always refused: there is no finite value to repair toward,
    /// and letting it through would poison every downstream curve/tween
    /// evaluation for the rest of the run (`clamp` propagates `NaN`
    /// unchanged rather than rejecting it). `+-inf` clamps to whichever
    /// bound it points at when that bound is finite — the "go to
    /// the end" idiom, e.g. `animate_to(f64::INFINITY)` on a bounded
    /// controller — and is refused when that bound is itself non-finite: an
    /// unbounded controller has no end in that direction to go to.
    fn canonicalize_value_target(
        caller: &str,
        raw: f64,
        lower_bound: f64,
        upper_bound: f64,
    ) -> Result<f64, AnimationError> {
        if raw.is_nan() {
            return Err(AnimationError::NonFiniteTarget(format!(
                "{caller} must be finite, got NaN"
            )));
        }
        let clamped = raw.clamp(lower_bound, upper_bound);
        if !clamped.is_finite() {
            return Err(AnimationError::NonFiniteTarget(format!(
                "{caller} = {raw} has no finite bound to run to in that direction \
                 (bounds are [{lower_bound}, {upper_bound}])"
            )));
        }
        Ok(clamped)
    }

    /// Emit the "refused for a non-finite input" warning every
    /// [`AnimationError::NonFiniteTarget`]-returning call site needs:
    /// production callers of these methods discard the `Result` (the three
    /// workspace fling controllers spell `let _ = fling.animate_to_curved(..)`
    /// and friends), so this is the only place the refusal becomes
    /// observable. Call only once the refusal is fully decided and no
    /// mutation has been applied — it releases the controller's borrow before
    /// warning, so an arbitrary subscriber can reenter the controller.
    fn warn_non_finite_target(
        inner: std::cell::RefMut<'_, AnimationControllerInner>,
        err: AnimationError,
    ) -> AnimationError {
        drop(inner);
        tracing::warn!("{err}");
        err
    }

    /// Number of registered value listeners. Test-only: lets combinator tests
    /// assert that a parent subscription is added on construct and removed on drop.
    #[cfg(test)]
    pub(crate) fn debug_value_listener_count(&self) -> usize {
        self.notifier.len()
    }

    /// Number of registered status listeners. Test-only, for the same reason:
    /// `AnimationSwitch::dispose` must detach both of the listeners it attached.
    #[cfg(test)]
    pub(crate) fn debug_status_listener_count(&self) -> usize {
        self.inner.borrow_mut().status_listeners.len()
    }

    /// Establish a fresh run epoch before publishing its callbacks.
    fn begin_run(inner: &mut AnimationControllerInner) {
        inner.run_start = RunStart::Fresh;
        inner.last_elapsed = Duration::ZERO;
        inner.local_elapsed = Duration::ZERO;
        inner.rate_epoch_elapsed = Duration::ZERO;
        inner.rate_epoch_local = Duration::ZERO;
        inner.run_generation = inner
            .run_generation
            .checked_add(1)
            .expect("BUG: run admission reserves an available generation");
        inner.settle_pending = inner.clock_binding == ClockBinding::Missing;
        inner
            .pending_delivery
            .push_back(ControllerDelivery::RequestFrame);
    }

    fn retire_value_callbacks(
        &self,
        callbacks: impl IntoIterator<
            Item = Rc<flui_foundation::notifier_generic::NotificationCallback<()>>,
        >,
    ) {
        let mut retired = RetiredSources::new();
        retired
            .value_callbacks
            .extend(callbacks.into_iter().map(Opaque::new));
        if retired.value_callbacks.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        if inner.delivering {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::Retire(retired));
            return;
        }
        drop(inner);
        let mut retirement = Retirement::new();
        retirement.retire(retired);
        retirement.finish();
    }

    /// Emits the "received a non-finite value" warning shared by
    /// [`set_value`](Self::set_value)'s canonicalization and
    /// [`tick_simulation`](Self::tick_simulation)'s mid-run non-finite
    /// sample — call only after `finish` has released its borrow and delivered,
    /// so an arbitrary subscriber can reenter the controller.
    /// `should_warn` is the caller's snapshot of the latch
    /// (`!non_finite_warned`, taken before setting it) — this fires at most
    /// once per controller, not once per frame of a poisoned gesture drag
    /// or a misbehaving simulation.
    fn warn_non_finite_value(should_warn: bool, raw: f64, outcome: NonFiniteOutcome) {
        if should_warn {
            tracing::warn!(
                value = raw,
                "received a non-finite value; {} -- drive the controller with finite values",
                outcome.description()
            );
        }
    }

    /// The outermost caller drains accepted work; reentry only appends to it.
    fn drain_delivery(&self, retirement: &mut Retirement) {
        let mut settled = false;
        loop {
            let delivery = {
                let mut inner = self.inner.borrow_mut();
                if settled
                    && matches!(
                        inner.pending_delivery.front(),
                        Some(ControllerDelivery::SettleRun(_))
                    )
                {
                    inner.delivering = false;
                    return;
                }
                let Some(delivery) = inner.pending_delivery.pop_front() else {
                    inner.delivering = false;
                    return;
                };
                delivery
            };
            match delivery {
                ControllerDelivery::RequestFrame => {
                    if self.walk_probe().live_running {
                        let routes = self.inner.borrow().frame_routes.clone();
                        for route in routes {
                            route.request_frame(retirement);
                        }
                    }
                }
                ControllerDelivery::SettleRun(generation) => {
                    retirement.run_with(|retirement| {
                        settled |= self.settle_run(generation, retirement);
                    });
                }
                ControllerDelivery::Status(status, listeners) => {
                    for (id, callback) in &listeners {
                        let live = {
                            let inner = self.inner.borrow_mut();
                            !inner.disposed
                                && inner
                                    .status_listeners
                                    .iter()
                                    .any(|(candidate, _)| candidate == id)
                        };
                        if live {
                            retirement.run_with(|recovery| callback.get().invoke(status, recovery));
                        }
                    }
                    for (_, callback) in listeners {
                        retirement.retire(callback);
                    }
                }
                ControllerDelivery::Run(delivery) => {
                    let retain = retirement.has_failure();
                    retirement.run(|| {
                        let delivery = delivery.into_inner();
                        if retain {
                            delivery.deliver_after_failure();
                        } else {
                            delivery.deliver();
                        }
                    });
                }
                ControllerDelivery::Retire(retired) => retirement.retire(retired),
            }
        }
    }

    /// Commit status, run delivery and outgoing custody before unlocking.
    /// The outermost finish owns the FIFO drain, including work admitted by
    /// value listeners, status listeners, continuations and destructors.
    /// Status precedes the corresponding run delivery (ADR-0064); accepted
    /// transitions do not coalesce. A caught failure does not discard the tail
    /// or change a published run outcome. Retirement resumes the first failure
    /// only after the queue is empty and the delivery latch is released.
    fn finish(
        &self,
        status: AnimationStatus,
        value_change: ValueChange,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) {
        let mut retirement = Retirement::new();
        self.finish_with_retirement(
            status,
            value_change,
            delivery,
            retired,
            inner,
            &mut retirement,
        );
        retirement.finish();
    }

    fn finish_with_retirement(
        &self,
        status: AnimationStatus,
        value_change: ValueChange,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        retirement: &mut Retirement,
    ) {
        inner.enqueue_status_change(status);
        if let Some(delivery) = delivery {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::Run(Terminal::new(delivery)));
        }
        inner
            .pending_delivery
            .push_back(ControllerDelivery::Retire(retired));
        if inner.settle_pending {
            inner.settle_pending = false;
            let generation = inner.run_generation;
            inner
                .pending_delivery
                .push_back(ControllerDelivery::SettleRun(generation));
        }
        let drain = !inner.delivering;
        inner.delivering = true;
        drop(inner);
        if value_change == ValueChange::Notify {
            retirement
                .run_with(|retirement| self.notifier.notify_listeners_with_recovery(retirement));
        }
        if drain {
            self.drain_delivery(retirement);
        }
    }
}

impl AnimationControllerInner {
    fn tick_source(&self) -> TickSource {
        if let Some(run) = self.repeat {
            TickSource::Repeat(run)
        } else if let Some(simulation) = &self.simulation {
            TickSource::Simulation(simulation.clone())
        } else {
            TickSource::Time {
                curve: self.run_curve.as_ref().map(Rc::clone),
                duration: self.current_duration(),
                start: self.start_value,
                target: self.target_value,
            }
        }
    }

    fn matches_sample(&self, identity: &SampleIdentity) -> bool {
        !self.disposed
            && self.active_run.is_some()
            && self.run_generation == identity.generation
            && self.sample_epoch == identity.epoch
    }

    /// Clear repeat/simulation/per-run-duration/curve modes (used when a new
    /// explicit run begins).
    fn clear_run_modes(&mut self, retired: &mut RetiredSources) {
        self.repeat = None;
        self.run_duration = None;
        if let Some(simulation) = self.simulation.take() {
            retired.simulation = Some(Opaque::new(simulation));
        }
        if let Some(curve) = self.run_curve.take() {
            retired.curve = Some(Opaque::new(curve));
        }
    }

    /// Halt any active run at the current value: stop the ticker, clear
    /// simulation/repeat/curve state, and cancel the displaced run's
    /// completer — without touching `status` or emitting any notification.
    /// This is the raw halt that `set_value` performs first; it does not
    /// recompute status (the caller does that separately).
    ///
    /// The returned [`RunDelivery`] must be handed to
    /// [`AnimationController::finish`]; nothing else in this file may call
    /// `deliver()` on it.
    #[must_use = "a displaced run's RunDelivery must reach AnimationController::finish"]
    fn stop_running(&mut self, retired: &mut RetiredSources) -> Option<RunDelivery> {
        self.clear_run_modes(retired);
        self.active_run.take().map(RunCompleter::cancel)
    }

    /// Admit each distinct committed status with its subscription snapshot.
    /// Equal adjacent commits are silent; returning to an earlier status after
    /// an intervening transition still admits that transition.
    fn enqueue_status_change(&mut self, status: AnimationStatus) {
        if self.disposed || status == self.last_committed_status {
            return;
        }
        self.last_committed_status = status;
        let listeners = self
            .status_listeners
            .iter()
            .map(|(id, callback)| (*id, Terminal::new(callback.clone())))
            .collect();
        self.pending_delivery
            .push_back(ControllerDelivery::Status(status, listeners));
    }

    /// Effective duration for the current run: per-run override, else repeat
    /// period (when repeating), else the direction's base duration.
    fn current_duration(&self) -> Duration {
        if let Some(run) = self.run_duration {
            return run;
        }
        if let Some(repeat) = &self.repeat {
            // Resolved once at the call and never zero (see `RepeatRun`).
            return repeat.period;
        }
        match self.direction {
            AnimationDirection::Forward => self.duration,
            AnimationDirection::Reverse => self.reverse_duration.unwrap_or(self.duration),
        }
    }

    /// Duration for a run covering `|target_value - start_value|` of the
    /// range at the full-range velocity implied by `base`.
    ///
    /// The duration is scaled by the remaining fraction, so a mid-flight
    /// `forward()`/`reverse()` keeps constant velocity instead of stretching
    /// the leftover distance over the full duration. Degenerate ranges
    /// (zero, non-finite) fall back to the unscaled base.
    fn scaled_run_duration(&self, base: Duration) -> Duration {
        let range = self.upper_bound - self.lower_bound;
        if !range.is_finite() || range <= 0.0 {
            return base;
        }
        let fraction = ((self.target_value - self.start_value).abs() / range).clamp(0.0, 1.0);
        if !fraction.is_finite() {
            return base;
        }
        // The float representation of Duration::MAX rounds beyond its
        // admitted range. Preserve a full-range duration without conversion.
        if fraction == 1.0 {
            base
        } else {
            base.mul_f64(fraction)
        }
    }

    /// Dilated elapsed time since the current run started, from the last
    /// observed tick (`restart_ticker` always begins a fresh run at elapsed
    /// zero, so there is no per-run epoch left to subtract).
    fn cycle_elapsed_secs(&self) -> f64 {
        self.local_elapsed.as_secs_f64()
    }

    /// Unbounded builders store both infinite bounds; a validated value range
    /// stores two finite endpoints. Checking `lower_bound` alone distinguishes
    /// those configurations because a half-open pair cannot be constructed.
    fn is_unbounded(&self) -> bool {
        !self.lower_bound.is_finite()
    }

    /// Whether the current value is at (or indistinguishable from) the upper bound.
    ///
    /// Uses exact equality for infinite bounds to avoid the `INFINITY - INFINITY = NaN`
    /// pitfall that breaks the epsilon comparison on an
    /// [`unbounded`](crate::AnimationControllerBuilder::unbounded) controller,
    /// whose bounds are fixed at `(NEG_INFINITY, INFINITY)`.
    fn is_at_upper_bound(&self) -> bool {
        self.value == self.upper_bound
            || (!self.upper_bound.is_infinite()
                && (self.value - self.upper_bound).abs() < BOUND_EPSILON)
    }

    /// Whether the current value is at (or indistinguishable from) the lower bound.
    ///
    /// Uses exact equality for infinite bounds — see [`is_at_upper_bound`](Self::is_at_upper_bound).
    fn is_at_lower_bound(&self) -> bool {
        self.value == self.lower_bound
            || (!self.lower_bound.is_infinite()
                && (self.value - self.lower_bound).abs() < BOUND_EPSILON)
    }

    /// Status at a settled value, mapping non-bound stops by direction.
    ///
    /// Used only by [`stop`](AnimationController::stop) — FLUI's own
    /// frame-driver contract: a bound reached mid-frame must report the bound it
    /// actually reached, not the run's nominal direction, so a driver
    /// polling `status().is_running()` sees a real settle rather than a
    /// direction that never touched the value. Every RUN END instead uses
    /// [`AnimationDirection::settled_status`], which has no bound check —
    /// see that method's own doc.
    fn settled_status_directed(&self) -> AnimationStatus {
        if self.is_at_upper_bound() {
            AnimationStatus::Completed
        } else if self.is_at_lower_bound() {
            AnimationStatus::Dismissed
        } else {
            match self.direction {
                AnimationDirection::Forward => AnimationStatus::Completed,
                AnimationDirection::Reverse => AnimationStatus::Dismissed,
            }
        }
    }

    /// Status at a settled value, keeping the running status for non-bound stops.
    ///
    /// Used only by [`set_value`](AnimationController::set_value), for the
    /// same frame-driver reason as
    /// [`settled_status_directed`](Self::settled_status_directed): a 120Hz
    /// gesture drag calling `set_value` every frame must report the bound it
    /// is actually at, not a manufactured settle for an interior value still
    /// under a live gesture. This is also exactly why a *running* status
    /// (`AnimationStatus::is_running`) is not proof that a run is
    /// installed: `set_value` reaches this branch at an interior value
    /// AFTER `stop_running()` has already cleared `active_run` — see
    /// [`walk_probe`](AnimationController::walk_probe)'s doc.
    fn settled_status_keep_direction(&self) -> AnimationStatus {
        if self.is_at_upper_bound() {
            AnimationStatus::Completed
        } else if self.is_at_lower_bound() {
            AnimationStatus::Dismissed
        } else {
            self.direction.running_status()
        }
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

    /// The value/direction at the END of repeat cycle `index` (0-based): a
    /// `Reverse` leg lands on `min`, a `Forward` leg on `max`. The single
    /// landing rule every exhaustion (finite count run out) and zero-period
    /// settle reports. Takes the range as scalars rather than a
    /// [`RepeatRun`] because `repeat_with`'s zero-period settle runs BEFORE
    /// any `RepeatRun` exists — a `RepeatRun` is never constructed with a
    /// zero period.
    fn repeat_landing(reverse: bool, min: f64, max: f64, index: u128) -> (f64, AnimationDirection) {
        let direction = Self::repeat_leg(reverse, index);
        let value = match direction {
            AnimationDirection::Reverse => min,
            AnimationDirection::Forward => max,
        };
        (value, direction)
    }

    /// The value/direction/leg-endpoints at `total_ns` nanoseconds since a
    /// repeat's run started — the ONE place the running-leg math lives,
    /// called by both `repeat_with`'s at-call sample (`total_ns =
    /// run.initial_ns`) and [`AnimationController::tick_repeat`]'s running
    /// sample (`total_ns = elapsed_ns + run.initial_ns`), replacing the
    /// `Forward => (min, max) / Reverse => (max, min)` match that used to
    /// exist at both call sites independently. Takes `run` by reference for
    /// the same reason [`repeat_landing`](Self::repeat_landing) does.
    fn repeat_sample(run: &RepeatRun, total_ns: u128) -> RepeatSample {
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

impl AnimationDirection {
    /// The running status for this direction.
    const fn running_status(self) -> AnimationStatus {
        match self {
            AnimationDirection::Forward => AnimationStatus::Forward,
            AnimationDirection::Reverse => AnimationStatus::Reverse,
        }
    }

    /// The status a run in this direction ends at, with **no bound check**.
    ///
    /// The end status follows purely from the run's direction, so
    /// `animate_to(lower_bound)` from mid-range ends `Completed`, not
    /// `Dismissed`. Used at every RUN END: `tick_time_based`'s non-repeat
    /// and repeat-exhaustion ends, `tick_simulation`'s `is_done`, and
    /// `settle_at_target`. `stop()`/`set_value` do **not** use this — they
    /// keep the bounds-first
    /// [`settled_status_directed`](AnimationControllerInner::settled_status_directed)/
    /// [`settled_status_keep_direction`](AnimationControllerInner::settled_status_keep_direction),
    /// FLUI's own frame-driver contract (see those methods' docs).
    const fn settled_status(self) -> AnimationStatus {
        match self {
            AnimationDirection::Forward => AnimationStatus::Completed,
            AnimationDirection::Reverse => AnimationStatus::Dismissed,
        }
    }
}

impl Animation<f64> for AnimationController {
    #[inline]
    fn value(&self) -> f64 {
        self.inner.borrow_mut().value
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.inner.borrow_mut().status
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        self.register_status_listener(StatusListener::User(callback))
    }

    fn add_status_observer(&self, observer: StatusObserver) -> ListenerId {
        self.register_status_listener(StatusListener::Relay(observer))
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.unregister_status_listener(id);
    }

    /// Whether the controller is currently driving a run.
    #[inline]
    fn is_animating(&self) -> bool {
        let inner = self.inner.borrow();
        !inner.disposed && inner.active_run.is_some()
    }
}

impl AnimationController {
    fn register_status_listener(&self, callback: StatusListener) -> ListenerId {
        let mut callback = Opaque::new(callback);
        let mut inner = self.inner.borrow_mut();
        let id = ListenerId::new(inner.next_listener_id);
        inner.next_listener_id = inner
            .next_listener_id
            .checked_add(1)
            .expect("BUG: listener identities exhausted");
        if inner.disposed {
            if inner.delivering {
                let mut retired = RetiredSources::new();
                retired.callbacks.push(callback);
                inner
                    .pending_delivery
                    .push_back(ControllerDelivery::Retire(retired));
            } else {
                drop(inner);
                drop(callback);
            }
            return id;
        }
        inner.status_listeners.push((id, callback.take()));
        id
    }

    fn unregister_status_listener(&self, id: ListenerId) {
        let mut retired;
        {
            let mut inner = self.inner.borrow_mut();
            retired = inner
                .status_listeners
                .iter()
                .position(|(candidate, _)| *candidate == id)
                .map(|index| Opaque::new(inner.status_listeners.remove(index).1));
            if inner.delivering
                && let Some(callback) = retired.take()
            {
                let mut sources = RetiredSources::new();
                sources.callbacks.push(callback);
                inner
                    .pending_delivery
                    .push_back(ControllerDelivery::Retire(sources));
            }
        }
        drop(retired);
    }
}

impl Listenable for AnimationController {
    fn add_observer(&self, observer: flui_foundation::notifier::ListenerObserver) -> ListenerId {
        let mut inner = self.inner.borrow_mut();
        if inner.disposed {
            let id = ListenerId::new(inner.next_listener_id);
            inner.next_listener_id = inner
                .next_listener_id
                .checked_add(1)
                .expect("BUG: listener identities exhausted");
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::relay(Rc::new(
                    move |(): &(), recovery| observer(recovery),
                )),
            ));
            if inner.delivering {
                inner
                    .pending_delivery
                    .push_back(ControllerDelivery::Retire(retired));
                return id;
            }
            drop(inner);
            drop(retired);
            return id;
        }
        drop(inner);
        self.notifier.add_observer(observer)
    }
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        let mut inner = self.inner.borrow_mut();
        if inner.disposed {
            let id = ListenerId::new(inner.next_listener_id);
            inner.next_listener_id = inner
                .next_listener_id
                .checked_add(1)
                .expect("BUG: listener identities exhausted");
            let mut retired = RetiredSources::new();
            retired.value_callbacks.push(Opaque::new(
                flui_foundation::notifier_generic::NotificationCallback::user(Rc::new(
                    move |(): &()| callback(),
                )),
            ));
            if inner.delivering {
                inner
                    .pending_delivery
                    .push_back(ControllerDelivery::Retire(retired));
                return id;
            }
            drop(inner);
            drop(retired);
            return id;
        }
        drop(inner);
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        let callback = self.notifier.take_listener(id);
        self.retire_value_callbacks(callback);
    }

    fn remove_all_listeners(&self) {
        self.retire_value_callbacks(self.notifier.take_listeners());
    }
}

impl fmt::Debug for AnimationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow_mut();
        f.debug_struct("AnimationController")
            .field("value", &inner.value)
            .field("status", &inner.status)
            .field("direction", &inner.direction)
            .field("lower_bound", &inner.lower_bound)
            .field("upper_bound", &inner.upper_bound)
            .field("disposed", &inner.disposed)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
