//! `AnimationController` - The primary animation driver.

mod admission;
mod commands;
mod completion;
mod delivery;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod listeners;
mod motion;
mod policy;
mod run;
mod sample;

pub(crate) use motion::{ValuePublication, ValueSeam};
use policy::MotionRunState;
pub(crate) use policy::SettleReason;
use run::{ActiveRun, RepeatRun, RunMode, Settle, SimulationRun};

/// Run-local time after which a simulation that has not settled is reported:
/// far beyond any fling or spring a person waits for.
#[cfg(any(debug_assertions, feature = "diagnostics"))]
const NEVER_SETTLES_AFTER: Duration = Duration::from_secs(60);
use sample::SampleIdentity;

use crate::PlaybackRate;
use crate::StatusObserver;
use crate::animation::{Animation, AnimationDirection, Retirement, Terminal};
use crate::curve::Curve;
use crate::error::AnimationError;
use crate::origin::ControllerOrigin;
use crate::run_future::{RunCompleter, RunDelivery};
use crate::simulation::{Simulation, SpringDescription, Tolerance};
use crate::status::AnimationStatus;
use flui_foundation::panic::{FailureClass, RecoveryScope};
use flui_foundation::{ChangeNotifier, ListenerId};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

/// Default spring for fling animations.
fn default_fling_spring() -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)
        .expect("BUG: default fling spring is in the admitted domain")
}

/// Default tolerance for fling animations.
const FLING_TOLERANCE: Tolerance = Tolerance {
    distance: 0.01,
    velocity: f64::INFINITY,
};

/// Whether transition delivery must notify value listeners,
/// alongside status listeners and any pending [`RunDelivery`]. Most
/// run-starting calls change `status` but not `value` at the call itself
/// (the value only moves once ticks arrive) — but `repeat` snaps
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
    callbacks: SmallVec<[Opaque<StatusObserver>; 4]>,
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
    #[cfg(feature = "diagnostics")]
    Diagnostic(diagnostics::Transition),
    RequestFrame,
    SettleRun(u64),
    Status(
        AnimationStatus,
        SmallVec<[(ListenerId, Terminal<StatusObserver>); 4]>,
    ),
    Run(Terminal<RunDelivery>),
    Retire(RetiredSources),
    RetireRegistry(Terminal<crate::Vsync>),
    RetireOwned(Terminal<RetirementAction>),
}

type RetirementAction = Box<dyn FnOnce(&mut RecoveryScope<'_>)>;

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

/// What actually happened to a non-finite value `set_value` or a running
/// simulation received — named so
/// [`AnimationController::warn_non_finite_value`]'s message states the real
/// outcome instead of a single "canonicalized or ignored" that was wrong
/// for two of its three call sites (a simulation sample going non-finite
/// mid-run ends the run; it is neither canonicalized nor ignored).
#[derive(Debug, Clone, Copy)]
enum NonFiniteOutcome {
    /// Bounded initialization or `set_value`: canonicalized to the bound it points at
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

/// Snapshot for [`Vsync`](crate::vsync::Vsync)'s per-frame walk, collected
/// under one shared controller borrow. Run identity, playback and motion
/// policy are inspected together before the registry chooses a sample.
pub(crate) struct WalkProbe {
    /// The controller's [`AnimationController::run_generation`] at the time
    /// of the probe.
    pub(crate) generation: u64,
    pub(crate) start: RunStart,
    /// Whether the walk should tick this controller: running AND not
    /// disposed. `status` alone cannot tell the two apart — see
    /// [`AnimationController::walk_probe`]'s own doc.
    pub(crate) live_running: bool,
    pub(crate) behavior: crate::AnimationBehavior,
    pub(crate) has_run: bool,
    pub(crate) parked: bool,
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
/// use std::time::Duration;
///
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
    /// The debug label and creation site, fixed at construction; read by
    /// diagnostics without borrowing the state.
    origin: Rc<ControllerOrigin>,
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
    behavior: crate::AnimationBehavior,
    motion_state: MotionRunState,

    /// Duration of reverse animation (defaults to `duration`).
    reverse_duration: Option<Duration>,

    /// Lower bound (default 0.0).
    lower_bound: f64,

    /// Upper bound (default 1.0).
    upper_bound: f64,

    /// Status listeners, in registration order.
    status_listeners: Vec<(ListenerId, StatusObserver)>,

    pending_delivery: VecDeque<ControllerDelivery>,
    delivering: bool,
    #[cfg(feature = "diagnostics")]
    diagnostics: diagnostics::ControllerDiagnostics,
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

    /// Is disposed?
    disposed: bool,

    /// Next status-listener ID.
    next_listener_id: usize,

    /// Status most recently committed for delivery. The emission seam
    /// ([`enqueue_status_change`](AnimationControllerInner::enqueue_status_change))
    /// compares against this before firing, so a call that leaves `status`
    /// unchanged (e.g. `set_value` re-asserting the same directional status
    /// every frame of a gesture drag) does not re-notify.
    last_committed_status: AnimationStatus,

    /// The installed run, if any: its completer, what it samples and how it
    /// settles. Every run-starting method replaces it through
    /// [`install_run`](AnimationController::install_run), cancelling the
    /// displaced completer and retiring its mode; every run-ending path takes
    /// it. Never assign it with `=`: a direct assignment drops the displaced
    /// completer inline, under whatever borrow is held, instead of handing its
    /// delivery to transition delivery.
    run: Option<ActiveRun>,

    /// Latches the "received a non-finite value" warning so a poisoned
    /// gesture drag calling [`set_value`](AnimationController::set_value)
    /// every frame — or a simulation whose sample goes non-finite mid-run —
    /// warns once per controller, not once per frame. Never reset: once a
    /// caller has proven it can produce a non-finite value, repeating the
    /// warning on every subsequent occurrence adds noise without adding
    /// information.
    non_finite_warned: bool,
    /// Whether this run's watchdog already reported it never settles.
    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    never_settles_warned: bool,
}

impl Drop for AnimationControllerInner {
    fn drop(&mut self) {
        self.disposed = true;
        let mut sources = RetiredSources::new();
        let run = self.run.take().map(|run| run.retire(&mut sources));
        let simulation = Terminal::new(sources.simulation.take());
        let curve = Terminal::new(sources.curve.take());
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
        behavior: crate::AnimationBehavior,
        origin: ControllerOrigin,
    ) -> Self {
        let (lower, upper, default) = bounds
            .map_or((f64::NEG_INFINITY, f64::INFINITY, 0.0), |range| {
                (range.lower(), range.upper(), range.lower())
            });
        let raw = initial.unwrap_or(default);
        let canonical = Self::canonicalize_direct_value(raw, lower, upper);
        let controller = Self::new_inner(
            duration,
            lower,
            upper,
            canonical.unwrap_or(default),
            behavior,
            origin,
        );
        if !raw.is_finite() {
            controller.inner.borrow_mut().non_finite_warned = true;
            controller.warn_non_finite_value(
                true,
                raw,
                if canonical.is_some() {
                    NonFiniteOutcome::Canonicalized
                } else {
                    NonFiniteOutcome::Ignored
                },
            );
        }
        controller
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

    pub(crate) fn prepare_clock_bound(&self, bound: bool) -> ValuePublication {
        let mut inner = self.inner.borrow_mut();
        inner.clock_binding = if bound {
            ClockBinding::Bound
        } else {
            ClockBinding::Missing
        };
        inner.settle_pending = !bound && inner.run.is_some();
        if bound && inner.run.is_some() {
            inner
                .pending_delivery
                .push_back(ControllerDelivery::RequestFrame);
        }
        let status = inner.status;
        let drain = Self::enqueue_delivery(status, None, RetiredSources::new(), inner);
        ValuePublication::unchanged(self.clone(), drain)
    }

    /// The one place every constructor builds the inner state: `value`,
    /// `start_value`, and `target_value` all start at `initial_value`
    /// (`lower_bound` for a bounded controller, `0.0` for an unbounded one),
    /// and `status`/`last_committed_status` are BOTH set from
    /// [`AnimationControllerInner::settled_status_keep_direction`] at that
    /// value — the status rule applies at construction too, not only to a
    /// later `set_value` (a bounded
    /// controller at `lower_bound` stays `ReverseSettled`; an unbounded one at
    /// `0.0`, direction defaulted `Forward`, reports `Forward` — see
    /// `docs/ARCHITECTURE.md`'s mapping entry for the recorded cost). Both
    /// fields must agree at construction: if `last_committed_status` stayed
    /// hard-coded `ReverseSettled` while `status` starts `Forward`, the first
    /// transition delivery call — even one that changes
    /// nothing — would read them as different and fire a spurious status
    /// notification for a change that never happened.
    fn new_inner(
        duration: Duration,
        lower_bound: f64,
        upper_bound: f64,
        initial_value: f64,
        behavior: crate::AnimationBehavior,
        origin: ControllerOrigin,
    ) -> Self {
        let notifier = Rc::new(ChangeNotifier::new());

        let mut inner = AnimationControllerInner {
            value: initial_value,
            status: AnimationStatus::ReverseSettled,
            duration,
            behavior,
            motion_state: MotionRunState::Active,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            status_listeners: Vec::new(),
            pending_delivery: VecDeque::new(),
            frame_routes: Vec::new(),
            delivering: false,
            #[cfg(feature = "diagnostics")]
            diagnostics: diagnostics::ControllerDiagnostics::new(&origin),
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
            disposed: false,
            next_listener_id: 1,
            last_committed_status: AnimationStatus::ReverseSettled,
            run: None,
            non_finite_warned: false,
            #[cfg(any(debug_assertions, feature = "diagnostics"))]
            never_settles_warned: false,
        };
        let initial_status = inner.settled_status_keep_direction();
        inner.status = initial_status;
        inner.last_committed_status = initial_status;
        #[cfg(feature = "diagnostics")]
        diagnostics::ControllerDiagnostics::initialize(&mut inner);

        Self {
            inner: Rc::new(RefCell::new(inner)),
            notifier,
            origin: Rc::new(origin),
        }
    }

    /// Apply `rate` at the next sample, preserving the local run time there.
    ///
    /// A paused run stays installed and keeps its completion future. A bound
    /// controller requests the sample that applies this change.
    ///
    /// `controller_rate_preserves_elapsed_and_paused_delivery` covers tween,
    /// curve, repeat and spring seams. The property contract
    /// `controller_local_time_is_the_integral_of_its_rate` compares samples
    /// against an independent piecewise integral, including repeated time.
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
        self.deliver_transition(
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
    /// `repeat`/`fling`/`animate_with` that begins the run at elapsed zero.
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
    /// `repeat`'s own zero-effective-period and zero-iteration settles —
    /// all of which settle through the private `settle_at_target`
    /// chokepoint) leave it untouched. Exhaustion permanently refuses new runs;
    /// an already issued generation is never reused.
    #[must_use]
    pub fn run_generation(&self) -> u64 {
        self.inner.borrow().run_generation
    }

    /// One-borrow snapshot for [`Vsync`](crate::vsync::Vsync)'s per-frame walk —
    /// see [`WalkProbe`]'s own doc for the perf rationale.
    ///
    /// `live_running` is `!disposed && run.is_some()` — **not**
    /// `status.is_directional()`, which is the wrong "is a run installed"
    /// predicate for two independent reasons:
    /// - [Owning disposal](crate::DrivenController::dispose) leaves `status` untouched,
    ///   so a controller disposed mid-run keeps whatever
    ///   running status it had.
    /// - [`set_value`](Self::set_value) reports a *directional* running
    ///   status at an interior value
    ///   ([`settled_status_keep_direction`](AnimationControllerInner::settled_status_keep_direction))
    ///   even though it already called `stop_running()` and cleared
    ///   `run`. A `Vsync`-driven controller that receives a
    ///   `set_value` mid-run would otherwise still read `status.is_directional()
    ///   == true`, and the NEXT `tick_all` would recompute its value from
    ///   the stale, already-stopped run's `start_value`/`target_value`,
    ///   silently overwriting what `set_value` had just set.
    ///
    /// `run.is_some()` is the actual, always-consistent fact: every
    /// run-starting method installs it in the same mutable borrow it sets a
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
            behavior: inner.behavior,
            has_run: !inner.disposed && inner.run.is_some(),
            parked: inner.motion_state == MotionRunState::Parked,
            live_running: !inner.disposed
                && inner.run.is_some()
                && inner.motion_state == MotionRunState::Active
                && (!inner.playback_rate.is_paused() || inner.pending_rate.is_some()),
        }
    }

    /// Settle an admitted run without reading a wall clock or invoking its curve.
    /// Reentrant replacement invalidates the source before the next callout.
    pub(crate) fn settle_run(
        &self,
        generation: u64,
        reason: SettleReason,
        recovery: &mut RecoveryScope<'_>,
    ) -> bool {
        let (source, identity, warn) = {
            let mut inner = self.inner.borrow_mut();
            if !inner.accepts_settlement(generation, &reason) {
                return false;
            }
            inner.note_settle_reason(reason.diagnostic_name());
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
            recovery.run_classified(FailureClass::Delivery, |_| {
                tracing::warn!("animation has no clock; finite work settles immediately");
            });
        }
        let mut committed = false;
        recovery.run_with(|recovery| match source.get() {
            TickSource::Time { target, .. } => {
                let inner = self.inner.borrow_mut();
                if inner.matches_sample(&identity) {
                    let direction = inner.direction;
                    self.complete_finite_run(inner, (*target, direction), recovery);
                    committed = true;
                }
            }
            TickSource::Repeat(run) => {
                let mut inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if let Some(endpoint) = run.landing() {
                    self.complete_finite_run(inner, endpoint, recovery);
                } else {
                    let mut parked = *run;
                    parked.initial_ns = 0;
                    if let Some(active) = inner.run.as_mut() {
                        active.mode = RunMode::Repeat(parked);
                    }
                    inner.value = run.min;
                    inner.start_value = run.min;
                    inner.target_value = run.max;
                    inner.direction = AnimationDirection::Forward;
                    inner.status = AnimationStatus::Forward;
                    inner.last_elapsed = Duration::ZERO;
                    inner.local_elapsed = Duration::ZERO;
                    inner.rate_epoch_elapsed = Duration::ZERO;
                    inner.rate_epoch_local = Duration::ZERO;
                    if matches!(
                        reason,
                        SettleReason::ReducedMotion | SettleReason::ExhaustedClock
                    ) {
                        inner.motion_state = MotionRunState::Parked;
                        inner.run_start = RunStart::Fresh;
                    }
                    self.deliver_transition_with_recovery(
                        AnimationStatus::Forward,
                        ValueChange::Notify,
                        None,
                        RetiredSources::new(),
                        inner,
                        recovery,
                    );
                }
                committed = true;
            }
            TickSource::Simulation(simulation) => {
                let inner = self.inner.borrow_mut();
                if !inner.matches_sample(&identity) {
                    return;
                }
                if let Ok(endpoint) = inner.completion_endpoint() {
                    self.complete_finite_run(inner, endpoint, recovery);
                    committed = true;
                    return;
                }
                drop(inner);
                let mut prepared = None;
                recovery.run_classified(FailureClass::Computation, |_| {
                    let mut last_finite = self.inner.borrow().value;
                    for exponent in 0..=8 {
                        let time = 0.25 * f64::from(1u32 << exponent);
                        let sample = simulation.x(time);
                        if !self.inner.borrow().matches_sample(&identity) {
                            return;
                        }
                        if sample.is_finite() {
                            last_finite = sample;
                            let done = simulation.reached_bound(sample) || simulation.is_done(time);
                            if !self.inner.borrow().matches_sample(&identity) {
                                return;
                            }
                            if done {
                                break;
                            }
                        }
                    }
                    prepared = Some(last_finite);
                });
                let Some(last_finite) = prepared else { return };
                let inner = self.inner.borrow_mut();
                if inner.matches_sample(&identity) {
                    self.tick_simulation(inner, last_finite, true, recovery);
                    committed = true;
                }
            }
        });
        recovery.retire_classified(
            if committed {
                FailureClass::Delivery
            } else {
                FailureClass::Unknown
            },
            source,
        );
        true
    }

    /// Commit a caller sample only after its run and tick identity survived.
    fn tick_simulation(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        sampled: f64,
        is_done: bool,
        retirement: &mut RecoveryScope<'_>,
    ) {
        if !sampled.is_finite() {
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;
            self.end_simulation_run(inner, ValueChange::Unchanged, retirement);
            retirement.run_classified(FailureClass::Delivery, |_| {
                self.warn_non_finite_value(should_warn, sampled, NonFiniteOutcome::EndedRun);
            });
            return;
        }
        inner.value = sampled.clamp(inner.lower_bound, inner.upper_bound);
        if is_done {
            self.end_simulation_run(inner, ValueChange::Notify, retirement);
        } else {
            let overdue = inner.overdue_simulation();
            let status = inner.status;
            self.deliver_transition_with_recovery(
                status,
                ValueChange::Notify,
                None,
                RetiredSources::new(),
                inner,
                retirement,
            );
            if let Some(elapsed) = overdue {
                retirement.run_classified(FailureClass::Delivery, |_| {
                    self.warn_never_settles(elapsed);
                });
            }
        }
    }

    /// End a simulation run at its current value — the one place
    /// [`tick_simulation`](Self::tick_simulation)'s two ending paths agree:
    /// a normal `is_done` completion (`ValueChange::Notify`, since `value`
    /// was just written to the final sample) and a non-finite sample
    /// (`ValueChange::Unchanged`, since `value` is deliberately left at its
    /// last finite point — the caller already decided not to write the bad
    /// sample through). Retires the run, settles
    /// `status` by direction, and publishes the completion before transition
    /// delivery releases its state borrow (the completer resolves before listeners are notified), so a panicking value/status listener still
    /// leaves the run `Ok` — the admitted delivery preserves the
    /// already-published outcome. `complete`, not `cancel`: both paths
    /// are the run ending on its own terms, never a cancellation.
    fn end_simulation_run(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        value_change: ValueChange,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let mut retired = RetiredSources::new();
        let delivery = inner.take_run(&mut retired).map(RunCompleter::complete);
        let status = inner.direction.settled_status();
        inner.status = status;
        self.deliver_transition_with_recovery(
            status,
            value_change,
            delivery,
            retired,
            inner,
            retirement,
        );
    }

    /// Time-based (tween) branch of [`tick_at`](Self::tick_at). Never called
    /// while a repeat is active — [`tick_at`](Self::tick_at) dispatches a
    /// repeat to [`tick_repeat`](Self::tick_repeat) instead.
    fn tick_time_based(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        t: f64,
        value: f64,
        retirement: &mut RecoveryScope<'_>,
    ) {
        if !value.is_finite() {
            return;
        }
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        if t < 1.0 {
            let status = inner.status;
            self.deliver_transition_with_recovery(
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
        let mut retired = RetiredSources::new();
        let delivery = inner.take_run(&mut retired).map(RunCompleter::complete);
        self.deliver_transition_with_recovery(
            status,
            ValueChange::Notify,
            delivery,
            retired,
            inner,
            retirement,
        );
    }

    /// Apply a repeat proposal sampled outside the controller borrow. Integer
    /// nanosecond parity and exhaustion are independent of frame partitioning.
    fn tick_repeat(
        &self,
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        sample: sample::RepeatFrame,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let sample = match sample {
            sample::RepeatFrame::Complete(endpoint) => {
                self.complete_finite_run(inner, endpoint, retirement);
                return;
            }
            sample::RepeatFrame::Running(sample) => sample,
        };
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
        self.deliver_transition_with_recovery(
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
    /// the next manual or presentation sample recomputes the value from the stale run's `start_value`/
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

        let should_warn = !value.is_finite() && !inner.non_finite_warned;
        if !value.is_finite() {
            inner.non_finite_warned = true;
        }
        let Some(canonical) =
            Self::canonicalize_direct_value(value, inner.lower_bound, inner.upper_bound)
        else {
            drop(inner);
            self.warn_non_finite_value(should_warn, value, NonFiniteOutcome::Ignored);
            return;
        };

        let delivery = inner.stop_running(&mut retired);
        inner.value = canonical;
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        self.deliver_transition(status, ValueChange::Notify, delivery, retired, inner);
        self.warn_non_finite_value(should_warn, value, NonFiniteOutcome::Canonicalized);
    }

    /// Close the kernel and cancel its run under the owning lifecycle's recovery.
    /// The owner withdraws its seat before entering this idempotent drain.
    pub(crate) fn prepare_dispose(&self) -> Option<ValuePublication> {
        let mut retired = RetiredSources::new();
        let mut inner = self.inner.borrow_mut();
        if inner.disposed {
            return None;
        }
        let delivery = inner.take_run(&mut retired).map(RunCompleter::cancel);
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
        let drain = Self::enqueue_delivery(status, delivery, retired, inner);
        Some(ValuePublication::unchanged(self.clone(), drain))
    }

    pub(crate) fn is_disposed(&self) -> bool {
        self.inner.borrow().disposed
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
        Self::retire_after(self.inner.borrow_mut(), retired);
    }

    /// Retire `retired` outside the state borrow: after the delivery in
    /// progress, which retires its queue in order, or now, contained.
    fn retire_after(
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
        retired: RetiredSources,
    ) {
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

    /// A fresh listener identity. Exhaustion is refused permanently, with
    /// the borrow released before the panic unwinds through user code.
    fn mint_listener_id(
        mut inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) -> (ListenerId, std::cell::RefMut<'_, AnimationControllerInner>) {
        let Some(next_id) = inner.next_listener_id.checked_add(1) else {
            drop(inner);
            panic!("BUG: listener identities exhausted");
        };
        let id = ListenerId::new(inner.next_listener_id);
        inner.next_listener_id = next_id;
        (id, inner)
    }

    /// Report a simulation run that has not settled after `elapsed` run-local
    /// seconds, once per run, on [`NEVER_SETTLES_TARGET`](crate::NEVER_SETTLES_TARGET).
    fn warn_never_settles(&self, elapsed: f64) {
        let subject = self.subject();
        let label = subject.label();
        let created_at = subject.created_at().map(tracing::field::display);
        if let Some(parent) = subject.parent.clone() {
            tracing::warn!(
                target: crate::NEVER_SETTLES_TARGET,
                parent: parent,
                elapsed_seconds = elapsed,
                label,
                created_at,
                "a simulation has run without settling; it may never finish",
            );
        } else {
            tracing::warn!(
                target: crate::NEVER_SETTLES_TARGET,
                elapsed_seconds = elapsed,
                label,
                created_at,
                "a simulation has run without settling; it may never finish",
            );
        }
    }

    /// Emits the "received a non-finite value" warning shared by initialization,
    /// [`set_value`](Self::set_value)'s canonicalization and
    /// [`tick_simulation`](Self::tick_simulation)'s mid-run non-finite
    /// sample — call only after releasing all controller borrows,
    /// so an arbitrary subscriber can reenter the controller.
    /// `should_warn` is the caller's snapshot of the latch
    /// (`!non_finite_warned`, taken before setting it) — this fires at most
    /// once per controller, not once per frame of a poisoned gesture drag
    /// or a misbehaving simulation.
    fn warn_non_finite_value(&self, should_warn: bool, raw: f64, outcome: NonFiniteOutcome) {
        if !should_warn {
            return;
        }
        let subject = self.subject();
        let label = subject.label();
        let created_at = subject.created_at().map(tracing::field::display);
        if let Some(parent) = subject.parent.clone() {
            tracing::warn!(
                parent: parent,
                value = raw,
                label,
                created_at,
                "received a non-finite value; {} -- drive the controller with finite values",
                outcome.description()
            );
        } else {
            tracing::warn!(
                value = raw,
                label,
                created_at,
                "received a non-finite value; {} -- drive the controller with finite values",
                outcome.description()
            );
        }
    }

    /// Commit status, run delivery and outgoing custody before releasing the borrow.
    /// The outermost delivery owns the FIFO drain, including work admitted by
    /// value listeners, status listeners, continuations and destructors.
    /// Status precedes the corresponding run delivery (ADR-0064); accepted
    /// transitions do not coalesce. A caught failure does not discard the tail
    /// or change a published run outcome. Retirement resumes the first failure
    /// only after the queue is empty and the delivery latch is released.
    fn deliver_transition(
        &self,
        status: AnimationStatus,
        value_change: ValueChange,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        inner: std::cell::RefMut<'_, AnimationControllerInner>,
    ) {
        let mut retirement = Retirement::new();
        self.deliver_transition_with_recovery(
            status,
            value_change,
            delivery,
            retired,
            inner,
            &mut retirement.scope(),
        );
        crate::refusal::finish_reporting(retirement, "controller delivery", self.debug_label());
    }

    /// The debug label this controller was built with, if any.
    pub(crate) fn debug_label(&self) -> Option<&str> {
        self.origin.label.as_deref()
    }

    fn deliver_transition_with_recovery(
        &self,
        status: AnimationStatus,
        value_change: ValueChange,
        delivery: Option<RunDelivery>,
        retired: RetiredSources,
        inner: std::cell::RefMut<'_, AnimationControllerInner>,
        retirement: &mut RecoveryScope<'_>,
    ) {
        let drain = Self::enqueue_delivery(status, delivery, retired, inner);
        self.publish_delivery(value_change, drain, retirement);
    }
}

impl AnimationControllerInner {
    /// Run-local seconds when this simulation run has gone far past any
    /// motion that settles ([`NEVER_SETTLES_AFTER`]) and the watchdog has
    /// not reported it yet; reporting it is left to the caller.
    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    fn overdue_simulation(&mut self) -> Option<f64> {
        if self.never_settles_warned || self.local_elapsed < NEVER_SETTLES_AFTER {
            return None;
        }
        self.never_settles_warned = true;
        Some(self.local_elapsed.as_secs_f64())
    }

    #[cfg(not(any(debug_assertions, feature = "diagnostics")))]
    #[expect(
        clippy::unused_self,
        reason = "the watchdog exists only with debug assertions or `diagnostics`"
    )]
    fn overdue_simulation(&mut self) -> Option<f64> {
        None
    }

    /// Record why the run about to complete settles, for its transition receipt.
    #[cfg(feature = "diagnostics")]
    fn note_settle_reason(&mut self, reason: &'static str) {
        self.diagnostics.settle_reason = Some(reason);
    }

    #[cfg(not(feature = "diagnostics"))]
    #[expect(
        clippy::unused_self,
        reason = "receipts, and the reason they report, exist only with `diagnostics`"
    )]
    fn note_settle_reason(&mut self, _: &'static str) {}

    fn accepts_settlement(&self, generation: u64, reason: &SettleReason) -> bool {
        let Some(run) = &self.run else {
            return false;
        };
        !self.disposed
            && self.run_generation == generation
            && match reason {
                SettleReason::Clock => {
                    self.clock_binding == ClockBinding::Missing || run.settle == Settle::Immediate
                }
                SettleReason::ReducedMotion => self.behavior == crate::AnimationBehavior::Normal,
                SettleReason::ExhaustedClock => true,
            }
    }

    fn tick_source(&self) -> TickSource {
        match self.run.as_ref().map(|run| &run.mode) {
            Some(RunMode::Repeat(run)) => TickSource::Repeat(*run),
            Some(RunMode::Simulation(simulation)) => TickSource::Simulation(simulation.clone()),
            Some(RunMode::Timed { curve, .. }) => TickSource::Time {
                curve: curve.as_ref().map(Rc::clone),
                duration: self.current_duration(),
                start: self.start_value,
                target: self.target_value,
            },
            None => TickSource::Time {
                curve: None,
                duration: self.current_duration(),
                start: self.start_value,
                target: self.target_value,
            },
        }
    }

    fn matches_sample(&self, identity: &SampleIdentity) -> bool {
        !self.disposed
            && self.run.is_some()
            && self.run_generation == identity.generation
            && self.sample_epoch == identity.epoch
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

    /// Effective duration for the current run: a timed run's own duration, a
    /// repeat's period, else the direction's base duration.
    fn current_duration(&self) -> Duration {
        match self.run.as_ref().map(|run| &run.mode) {
            Some(RunMode::Timed { duration, .. }) => return *duration,
            // Resolved once at the call and never zero (see `RepeatRun`).
            Some(RunMode::Repeat(repeat)) => return repeat.period,
            Some(RunMode::Simulation(_)) | None => {}
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

    /// Local playback time of the current run, from its last published sample.
    fn cycle_elapsed_secs(&self) -> f64 {
        self.local_elapsed.as_secs_f64()
    }

    /// Unbounded builders store both infinite bounds; a validated value range
    /// stores two finite endpoints. Checking `lower_bound` alone distinguishes
    /// those configurations because a half-open pair cannot be constructed.
    fn is_unbounded(&self) -> bool {
        !self.lower_bound.is_finite()
    }

    /// Whether the current value is exactly at the upper bound.
    fn is_at_upper_bound(&self) -> bool {
        self.value == self.upper_bound
    }

    /// Whether the current value is exactly at the lower bound.
    fn is_at_lower_bound(&self) -> bool {
        self.value == self.lower_bound
    }

    /// Status at a settled value, mapping non-bound stops by direction.
    ///
    /// Used only by [`stop`](AnimationController::stop): reached bounds choose
    /// their corresponding phase; an interior stop uses the run's direction.
    /// Every run end instead uses
    /// [`AnimationDirection::settled_status`], which has no bound check —
    /// see that method's own doc.
    fn settled_status_directed(&self) -> AnimationStatus {
        if self.is_at_upper_bound() {
            AnimationStatus::ForwardSettled
        } else if self.is_at_lower_bound() {
            AnimationStatus::ReverseSettled
        } else {
            match self.direction {
                AnimationDirection::Forward => AnimationStatus::ForwardSettled,
                AnimationDirection::Reverse => AnimationStatus::ReverseSettled,
            }
        }
    }

    /// Status at a settled value, keeping the running status for non-bound stops.
    ///
    /// Used only by [`set_value`](AnimationController::set_value): direct
    /// manipulation preserves direction at an interior value and selects the
    /// corresponding phase at a bound. Consumers can distinguish a drag from
    /// a settled phase without requiring a run. A directional phase
    /// (`AnimationStatus::is_directional`) is not proof that a run is
    /// installed: `set_value` reaches this branch at an interior value
    /// AFTER `stop_running()` has already cleared `run` — see
    /// [`walk_probe`](AnimationController::walk_probe)'s doc.
    fn settled_status_keep_direction(&self) -> AnimationStatus {
        if self.is_at_upper_bound() {
            AnimationStatus::ForwardSettled
        } else if self.is_at_lower_bound() {
            AnimationStatus::ReverseSettled
        } else {
            self.direction.running_status()
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
    /// `animate_to(lower_bound)` from mid-range ends `ForwardSettled`, not
    /// `ReverseSettled`. Used at every RUN END: `tick_time_based`'s non-repeat
    /// and repeat-exhaustion ends, `tick_simulation`'s `is_done`, and
    /// `settle_at_target`. `stop()`/`set_value` do **not** use this — they
    /// keep the bounds-first
    /// [`settled_status_directed`](AnimationControllerInner::settled_status_directed)/
    /// [`settled_status_keep_direction`](AnimationControllerInner::settled_status_keep_direction),
    /// FLUI's own frame-driver contract (see those methods' docs).
    const fn settled_status(self) -> AnimationStatus {
        match self {
            AnimationDirection::Forward => AnimationStatus::ForwardSettled,
            AnimationDirection::Reverse => AnimationStatus::ReverseSettled,
        }
    }
}

impl Animation<f64> for AnimationController {
    #[inline]
    fn value(&self) -> f64 {
        self.inner.borrow().value
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.inner.borrow().status
    }

    fn observe_status(&self, observer: StatusObserver) -> crate::StatusSubscription {
        self.register_status_listener(observer)
            .map_or_else(crate::StatusSubscription::default, |id| {
                crate::StatusSubscription::new(&self.inner, id, Self::withdraw_status_listener)
            })
    }

    /// Whether the controller is currently driving a run.
    #[inline]
    fn is_animating(&self) -> bool {
        let inner = self.inner.borrow();
        !inner.disposed && inner.run.is_some()
    }
}

impl fmt::Debug for AnimationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.borrow();
        let mut debug = f.debug_struct("AnimationController");
        if let Some(label) = &self.origin.label {
            debug.field("label", label);
        }
        if let Some(site) = self.origin.site.location() {
            debug.field("created_at", &format_args!("{site}"));
        }
        debug
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
