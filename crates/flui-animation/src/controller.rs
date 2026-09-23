//! `AnimationController` - The primary animation driver.

use crate::animation::{Animation, AnimationDirection, StatusCallback};
use crate::curve::Curve;
use crate::error::AnimationError;
use crate::simulation::{Simulation, SpringDescription, SpringSimulation, SpringType, Tolerance};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_scheduler::config::time_dilation;
use flui_scheduler::ticker::{TickerCompleter, TickerDelivery, TickerFuture};
use flui_scheduler::{Ticker, UpdateScheduler};
use parking_lot::Mutex;
use smallvec::SmallVec;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Absolute tolerance for "is the value at a bound" comparisons.
const BOUND_EPSILON: f32 = 1e-6;

/// Narrow an f64 time/progress value to the f32 the animation value space uses.
/// Time is accumulated in f64 for frame-coherence, but values and simulations
/// are f32; the sub-microsecond precision lost here is irrelevant to rendering.
#[inline]
fn narrow_f32(x: f64) -> f32 {
    x as f32
}

/// Default spring for fling animations.
fn default_fling_spring() -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)
}

/// Default tolerance for fling animations.
const FLING_TOLERANCE: Tolerance = Tolerance {
    distance: 0.01,
    velocity: f32::INFINITY,
    time: 1e-3,
};

/// Whether `AnimationController::finish` must notify value listeners,
/// alongside status listeners and any pending [`TickerDelivery`]. Most
/// run-starting calls change `status` but not `value` at the call itself
/// (the value only moves once ticks arrive) — but `repeat_with` snaps
/// `value` to the repeat range's lower endpoint and `drive_simulation`
/// snaps it to `simulation.x(0.0)`, and both still pass `Unchanged`: Flutter
/// parity is `_startSimulation` setting `_value` directly, without
/// `notifyListeners()` (`animation_controller.dart:865` @ 3.44.0) — the jump
/// is real but reported on the run's first tick, not synchronously at the
/// call. A settle or a tick always changes both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueChange {
    /// `value` did not change at this call; only status (and delivery) fire.
    Unchanged,
    /// `value` changed; notify value listeners too.
    Notify,
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
    /// Whether the walk should tick this controller: running AND not
    /// disposed. `status` alone cannot tell the two apart — see
    /// [`AnimationController::walk_probe`]'s own doc.
    pub(crate) live_running: bool,
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
    min: f32,
    /// Upper endpoint of the repeat range.
    max: f32,
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
    value: f32,
    /// The leg's direction (`Forward` unless bouncing on an odd-indexed leg).
    direction: AnimationDirection,
    /// This leg's start endpoint (`min` or `max`, by direction).
    start: f32,
    /// This leg's end endpoint (the opposite of `start`).
    target: f32,
}

/// Controls an animation, driving it forward/backward.
///
/// `AnimationController` is a **PERSISTENT OBJECT** that survives widget rebuilds.
/// It must be disposed when no longer needed to prevent resource leaks.
///
/// The controller generates values from `lower_bound` to `upper_bound` (typically 0.0 to 1.0)
/// over the specified duration. It implements `Animation<f32>` and can be used directly,
/// or transformed using `Tween` or `CurvedAnimation`.
///
/// # Time model
///
/// The controller advances on the elapsed time delivered by its [`Ticker`]
/// callback, not on wall-clock reads. Elapsed time is scaled by the global
/// [`time_dilation`] factor, and muting the ticker (e.g. when the view is
/// hidden) freezes progress — so lifecycle gating is handled at the ticker
/// layer rather than re-derived here. `restart_ticker` always begins a
/// fresh run's timeline at zero, so every value/status/direction the
/// controller reports — including a repeat's leg and phase — is a pure
/// function of the elapsed time since the current run started, never of
/// incremental per-cycle bookkeeping (see the private `tick_repeat`).
///
/// # Thread safety
///
/// The controller is `Send + Sync` via `Arc<Mutex<…>>`. Per ADR-0027 the
/// controller is control-plane and would ideally be thread-affine; it remains
/// `Send + Sync` as a recorded, scoped exception until the engine-wide `!Send`
/// flip lands.
/// Status listeners are always fired **after** the inner lock is released, so a
/// status callback may re-enter the controller without deadlocking.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, Animation};
/// use flui_scheduler::UpdateScheduler;
/// use std::time::Duration;
///
/// let scheduler = UpdateScheduler::new();
/// let controller = AnimationController::new(
///     Duration::from_millis(300),
///     &scheduler,
/// );
///
/// // Start animation
/// controller.forward().unwrap();
///
/// // Get current value (0.0 to 1.0)
/// let value = controller.value();
///
/// // Cleanup when done
/// controller.dispose();
/// ```
#[derive(Clone)]
pub struct AnimationController {
    inner: Arc<Mutex<AnimationControllerInner>>,
    notifier: Arc<ChangeNotifier>,
}

struct AnimationControllerInner {
    /// Current value (typically 0.0 to 1.0).
    value: f32,

    /// Animation status.
    status: AnimationStatus,

    /// Duration of forward animation.
    duration: Duration,

    /// Duration of reverse animation (defaults to `duration`).
    reverse_duration: Option<Duration>,

    /// Lower bound (default 0.0).
    lower_bound: f32,

    /// Upper bound (default 1.0).
    upper_bound: f32,

    /// Ticker for frame callbacks (auto-scheduling via the attached `UpdateScheduler`).
    ticker: Option<Ticker>,

    /// Status listeners, in registration order.
    status_listeners: Vec<(ListenerId, StatusCallback)>,

    /// Current run direction.
    direction: AnimationDirection,

    /// Value at the start of the current run (for partial animations).
    start_value: f32,

    /// Target value for the current run.
    target_value: f32,

    /// Most recent raw (pre-dilation) elapsed seconds seen by
    /// [`AnimationController::tick_at`], so `velocity()` can report the
    /// in-progress rate without a fresh tick.
    last_raw_elapsed_secs: f64,

    /// Monotonically increasing counter, bumped once each time a fresh run is
    /// established (every [`restart_ticker`](AnimationController::restart_ticker),
    /// which restarts the [`Ticker`] at elapsed zero). An external frame driver reads
    /// [`AnimationController::run_generation`] to detect "a new run's `t = 0`
    /// was just set" and re-anchor its own per-run epoch — so a controller run
    /// twice (forward → reverse) is ticked from the second run's start instead
    /// of a stale anchor. Never reset; stable across `tick_at`.
    run_generation: u64,

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
    simulation: Option<Box<dyn Simulation>>,

    /// Per-run easing curve for a time-based `animate_to_curved`/
    /// `animate_back_curved` run (`None` = linear). Cleared by
    /// [`clear_run_modes`](AnimationControllerInner::clear_run_modes) so a
    /// later plain `forward`/`reverse`/`animate_to` run does not inherit a
    /// stale curve. Flutter parity: `AnimationController._animateToInternal`
    /// threads `curve` straight into `_InterpolationSimulation`.
    run_curve: Option<Arc<dyn Curve + Send + Sync>>,

    /// Status most recently delivered to status listeners. The emission seam
    /// ([`take_status_change`](AnimationControllerInner::take_status_change))
    /// compares against this before firing, so a call that leaves `status`
    /// unchanged (e.g. `set_value` re-asserting the same directional status
    /// every frame of a gesture drag) does not re-notify. Flutter parity:
    /// `AnimationController._lastReportedStatus` / `_checkStatusChanged`.
    last_reported_status: AnimationStatus,

    /// The write half of the current run's [`TickerFuture`], if a run is
    /// installed. Every run-starting method displaces this (canceling
    /// whatever it held) and installs its own fresh completer; every
    /// run-ending path (`tick_time_based`, `tick_simulation`,
    /// `stop_running`) takes it and completes or cancels it. Never touch
    /// this field with `=` — a direct assignment drops whatever value was
    /// here before, inline, under whatever lock is held at that statement;
    /// always go through `.replace()`/`.take()` and bind the returned
    /// `Option<TickerCompleter>` so its displaced value reaches
    /// [`AnimationController::finish`] instead. `Option<T>` does not
    /// inherit `T`'s `#[must_use]`, so nothing in the compiler catches a
    /// dropped binding here — the backstop is the source-guard test in this
    /// file's `tests` module
    /// (`ticker_completer_resolution_never_bypasses_the_finish_chokepoint`),
    /// checked per statement (comments stripped, rustfmt-wrapped chains
    /// rejoined) for a direct assignment to this field, an unbound
    /// replace-or-take call on it, or any `TickerCompleter`/`TickerDelivery`
    /// call — bare, `let _ =`-discarded, or `drop(..)`-wrapped.
    active_run: Option<TickerCompleter>,

    /// Latches the "received a non-finite value" warning so a poisoned
    /// gesture drag calling [`set_value`](AnimationController::set_value)
    /// every frame — or a simulation whose sample goes non-finite mid-run —
    /// warns once per controller, not once per frame. Never reset: once a
    /// caller has proven it can produce a non-finite value, repeating the
    /// warning on every subsequent occurrence adds noise without adding
    /// information.
    non_finite_warned: bool,
}

impl AnimationController {
    /// Create a new animation controller, auto-scheduled off `scheduler`.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - UpdateScheduler the controller's ticker auto-schedules against
    #[must_use]
    pub fn new(duration: Duration, scheduler: &UpdateScheduler) -> Self {
        // 0.0 < 1.0 always holds, so the default-bounds path cannot fail.
        Self::with_bounds(duration, scheduler, 0.0, 1.0)
            .expect("default bounds (0.0, 1.0) satisfy lower < upper")
    }

    /// Create an animation controller with no ticker at all — bounds `0.0..1.0`.
    ///
    /// This is the shape every FLUI widget-layer controller actually needs:
    /// production widgets never attach a real, pumped `UpdateScheduler` to their
    /// controllers (`Vsync` — see [`crate::vsync`] — is the real widget-layer
    /// clock seam; a plain `AnimationController` advances only through
    /// external [`Self::tick_at`] calls a `Vsync`-driven caller makes). A
    /// controller built this way behaves identically to one built with
    /// [`Self::new`] against a private, never-pumped `UpdateScheduler`: neither
    /// ever auto-ticks, both advance only via `tick_at`. This constructor
    /// simply doesn't allocate the ticker (and the dead scheduler) that
    /// would have gone unused.
    #[must_use]
    pub fn without_ticker(duration: Duration) -> Self {
        Self::with_bounds_inner(duration, None, 0.0, 1.0)
            .expect("default bounds (0.0, 1.0) satisfy lower < upper")
    }

    /// [`Self::without_ticker`] with custom bounds — the shape a fling/
    /// ballistic-simulation controller needs (wide-open
    /// `f32::NEG_INFINITY..f32::INFINITY` bounds so the simulation's own
    /// `is_done` terminates the run instead of the controller clamping it),
    /// while still needing no ticker: the driver is `tick_at`, called
    /// directly by the simulation stepper, never a scheduler.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] unless both bounds are
    /// finite, `lower_bound < upper_bound`, AND `upper_bound - lower_bound`
    /// itself fits in `f32` — two finite endpoints do not by themselves
    /// make a finite range (`(-f32::MAX, f32::MAX)` has a span of
    /// `f32::INFINITY`).
    pub fn without_ticker_bounds(
        duration: Duration,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        Self::with_bounds_inner(duration, None, lower_bound, upper_bound)
    }

    /// Create an animation controller with a real, but permanently detached,
    /// ticker — no `UpdateScheduler` at all, bounds `0.0..1.0`.
    ///
    /// [`Self::without_ticker`] skips the `Ticker` field entirely, and
    /// [`AnimationController::is_animating`] is intentionally ticker-based
    /// (Flutter parity: mirrors `Ticker.isActive`, not this controller's own
    /// status), so a `without_ticker` controller can never report
    /// `is_animating() == true`, even mid-run. Some call sites need
    /// `is_animating()` to report correctly but never actually need a live
    /// scheduler to pump anything (`Vsync` — see [`crate::vsync`] — is the
    /// real widget-layer clock seam; nothing ever calls
    /// `UpdateScheduler::execute_frame` for these controllers' tickers). Before
    /// this constructor existed, those sites built a full, private
    /// `UpdateScheduler::new()` purely so [`Ticker::new_with_scheduler`] had
    /// something to downgrade — a whole `SchedulerInner` allocation whose
    /// `Weak` was already dead by the end of the constructing statement (the
    /// `UpdateScheduler` itself was never bound to anything), labeled with a
    /// misleading "real ticker" comment at each call site.
    ///
    /// [`Ticker::new()`] already builds exactly the needed shape: a
    /// manually-driven ticker with no scheduler attached
    /// (`Ticker`'s own `scheduler: Option<WeakUpdateScheduler>` field takes its
    /// `None` arm). `start_inner` sets [`TickerState::Active`](flui_scheduler::TickerState::Active) before it
    /// ever attempts to upgrade that `None` scheduler to auto-reschedule, so
    /// `is_animating()`/`Ticker::is_active()` report correctly the instant a
    /// run starts — the ticker simply never fires on its own, exactly like
    /// the throwaway-`UpdateScheduler` shape it replaces, but without allocating
    /// the scheduler nobody was ever going to pump.
    #[must_use]
    pub fn with_detached_ticker(duration: Duration) -> Self {
        Self::with_bounds_inner(duration, Some(Ticker::new()), 0.0, 1.0)
            .expect("default bounds (0.0, 1.0) satisfy lower < upper")
    }

    /// [`Self::with_detached_ticker`] with custom bounds.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] unless both bounds are
    /// finite, `lower_bound < upper_bound`, AND `upper_bound - lower_bound`
    /// itself fits in `f32` — two finite endpoints do not by themselves
    /// make a finite range (`(-f32::MAX, f32::MAX)` has a span of
    /// `f32::INFINITY`).
    pub fn with_detached_ticker_bounds(
        duration: Duration,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        Self::with_bounds_inner(duration, Some(Ticker::new()), lower_bound, upper_bound)
    }

    /// Create an animation controller with custom bounds.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - UpdateScheduler the controller's ticker auto-schedules against
    /// * `lower_bound` - Minimum value (default 0.0)
    /// * `upper_bound` - Maximum value (default 1.0)
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] unless both bounds are
    /// finite, `lower_bound < upper_bound`, AND `upper_bound - lower_bound`
    /// itself fits in `f32` — two finite endpoints do not by themselves
    /// make a finite range (`(-f32::MAX, f32::MAX)` has a span of
    /// `f32::INFINITY`).
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), flui_animation::AnimationError> {
    /// use flui_animation::AnimationController;
    /// use flui_scheduler::UpdateScheduler;
    /// use std::time::Duration;
    ///
    /// let scheduler = UpdateScheduler::new();
    /// let controller = AnimationController::with_bounds(
    ///     Duration::from_millis(300),
    ///     &scheduler,
    ///     0.0,
    ///     100.0,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_bounds(
        duration: Duration,
        scheduler: &UpdateScheduler,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        let ticker = Ticker::new_with_scheduler(scheduler);
        Self::with_bounds_inner(duration, Some(ticker), lower_bound, upper_bound)
    }

    /// Shared construction body for [`Self::with_bounds`] and
    /// [`Self::without_ticker`] — `ticker: None` is exactly the shape a
    /// controller ends up in today anyway (a private scheduler's ticker that
    /// nothing ever pumps), just without allocating the unused parts.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::InvalidBounds`] unless both bounds are
    /// finite, `lower_bound < upper_bound`, AND `upper_bound - lower_bound`
    /// itself fits in `f32` — two finite endpoints do not make a finite
    /// RANGE (`(-f32::MAX, f32::MAX)` has a span of `f32::INFINITY`).
    /// Bounded means finite (endpoints AND span); unbounded is
    /// [`Self::unbounded_inner`], not a bound value — a half-open pair (one
    /// finite, one infinite) is rejected the same way, since nothing in
    /// this workspace needs it and the rule would otherwise be incidental
    /// complexity.
    fn with_bounds_inner(
        duration: Duration,
        ticker: Option<Ticker>,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        // `lower_bound >= upper_bound` (not the negated `!(lower < upper)`,
        // which clippy's `neg_cmp_op_on_partial_ord` flags on a
        // `PartialOrd`-only type): NaN makes the two diverge, but NaN is
        // caught by the `is_finite` clauses below regardless of which form
        // this takes. The span check closes every bounded run start's
        // `target - value`/`target - start` arithmetic at once — without
        // it, `without_ticker_bounds(d, -f32::MAX, f32::MAX)` is accepted,
        // `forward()` returns `Ok`, and the first `tick_at` publishes
        // `value = f32::INFINITY`. `drive_to`'s own span-overflow check
        // stays: it is still reachable from an UNBOUNDED controller, whose
        // bounds never pass through this function.
        if lower_bound >= upper_bound
            || !lower_bound.is_finite()
            || !upper_bound.is_finite()
            || !(upper_bound - lower_bound).is_finite()
        {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower_bound}) and upper_bound ({upper_bound}) must both be \
                 finite, with lower_bound < upper_bound, and the range \
                 (upper_bound - lower_bound) must fit in f32"
            )));
        }

        Ok(Self::new_inner(
            duration,
            ticker,
            lower_bound,
            upper_bound,
            lower_bound,
        ))
    }

    /// Shared construction body for [`Self::unbounded`],
    /// [`Self::unbounded_without_ticker`], and
    /// [`Self::unbounded_with_detached_ticker`] — fixed `(-inf, inf)`
    /// bounds, infallible (there is no bound input to reject).
    ///
    /// Initial `value = 0.0` (not `lower_bound`, which would be `-inf` and
    /// leak into [`AnimationController::velocity`]) — Flutter parity:
    /// `AnimationController.unbounded`'s own doc (`animation_controller.dart`
    /// @ 3.44.0) fixes the initial value at `0.0`.
    fn unbounded_inner(duration: Duration, ticker: Option<Ticker>) -> Self {
        Self::new_inner(duration, ticker, f32::NEG_INFINITY, f32::INFINITY, 0.0)
    }

    /// The one place every constructor builds the inner state: `value`,
    /// `start_value`, and `target_value` all start at `initial_value`
    /// (`lower_bound` for a bounded controller, `0.0` for an unbounded one),
    /// and `status`/`last_reported_status` are BOTH set from
    /// [`AnimationControllerInner::settled_status_keep_direction`] at that
    /// value — Flutter parity: `_internalSetValue`'s status rule applies at
    /// construction too, not only to a later `set_value` (a bounded
    /// controller at `lower_bound` stays `Dismissed`; an unbounded one at
    /// `0.0`, direction defaulted `Forward`, reports `Forward` — see
    /// `docs/ARCHITECTURE.md`'s mapping entry for the recorded cost). Both
    /// fields must agree at construction: if `last_reported_status` stayed
    /// hard-coded `Dismissed` while `status` starts `Forward`, the first
    /// [`AnimationController::finish`] call — even one that changes
    /// nothing — would read them as different and fire a spurious status
    /// notification for a change that never happened.
    fn new_inner(
        duration: Duration,
        ticker: Option<Ticker>,
        lower_bound: f32,
        upper_bound: f32,
        initial_value: f32,
    ) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        let mut inner = AnimationControllerInner {
            value: initial_value,
            status: AnimationStatus::Dismissed,
            duration,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            ticker,
            status_listeners: Vec::new(),
            direction: AnimationDirection::Forward,
            start_value: initial_value,
            target_value: initial_value,
            last_raw_elapsed_secs: 0.0,
            run_generation: 0,
            run_duration: None,
            disposed: false,
            next_listener_id: 1,
            repeat: None,
            simulation: None,
            run_curve: None,
            last_reported_status: AnimationStatus::Dismissed,
            active_run: None,
            non_finite_warned: false,
        };
        let initial_status = inner.settled_status_keep_direction();
        inner.status = initial_status;
        inner.last_reported_status = initial_status;

        Self {
            inner: Arc::new(Mutex::new(inner)),
            notifier,
        }
    }

    /// Create an unbounded animation controller, auto-scheduled off
    /// `scheduler`: fixed `(-inf, inf)` bounds, initial `value = 0.0`,
    /// initial [`status`](Animation::status) [`Forward`](AnimationStatus::Forward)
    /// (see [`unbounded_without_ticker`](Self::unbounded_without_ticker)'s
    /// own doc for the full contract — this is the same shape with a real,
    /// scheduled ticker).
    #[must_use]
    pub fn unbounded(duration: Duration, scheduler: &UpdateScheduler) -> Self {
        let ticker = Ticker::new_with_scheduler(scheduler);
        Self::unbounded_inner(duration, Some(ticker))
    }

    /// Create an unbounded animation controller with no ticker at all — the
    /// shape a pixel-space fling/ballistic-simulation controller needs (see
    /// [`Self::without_ticker`] for why "no ticker, driven by `tick_at`" is
    /// the production widget-layer shape).
    ///
    /// Bounds are fixed at `f32::NEG_INFINITY..f32::INFINITY`: unboundedness
    /// is this constructor, not a bound value — [`Self::with_bounds`] and
    /// its siblings reject a non-finite bound. Initial `value = 0.0`
    /// (Flutter parity: `AnimationController.unbounded`'s own doc), and
    /// initial [`status`](Animation::status) is
    /// [`AnimationStatus::Forward`] — [`AnimationDirection`] defaults
    /// `Forward` and `0.0` is not "at a bound" on an infinite range, so the
    /// keep-direction status rule (the same one
    /// [`set_value`](Self::set_value) applies) reports the running
    /// direction rather than `Dismissed`. This means a never-run unbounded
    /// controller reports `status().is_running() == true`; the real "is a
    /// run installed" fact is [`is_animating`](Animation::is_animating), not
    /// `status`.
    ///
    /// Driving this controller with a run that targets a bound —
    /// [`forward`](Self::forward)/[`reverse`](Self::reverse)/
    /// [`fling`](Self::fling) — is refused with
    /// [`AnimationError::NonFiniteTarget`], since there is no finite bound
    /// to run to; drive it with
    /// [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back)
    /// (finite `target`), [`animate_with`](Self::animate_with) (a
    /// [`Simulation`]), [`set_value`](Self::set_value), or
    /// [`repeat_with`](Self::repeat_with) with an explicit finite range
    /// instead. [`reset`](Self::reset) resets to `0.0` (the defined
    /// beginning — flutter/flutter#76014), not `-inf`.
    #[must_use]
    pub fn unbounded_without_ticker(duration: Duration) -> Self {
        Self::unbounded_inner(duration, None)
    }

    /// [`Self::unbounded_without_ticker`] with a real, but permanently
    /// detached, [`Ticker`] — the unbounded twin of
    /// [`Self::with_detached_ticker`]: `is_animating()` reports correctly
    /// mid-run without a live scheduler ever pumping it.
    #[must_use]
    pub fn unbounded_with_detached_ticker(duration: Duration) -> Self {
        Self::unbounded_inner(duration, Some(Ticker::new()))
    }

    /// Set the duration for reverse animation.
    pub fn set_reverse_duration(&self, duration: Duration) {
        let mut inner = self.inner.lock();
        inner.reverse_duration = Some(duration);
    }

    /// Set the base forward duration.
    ///
    /// Mirrors Flutter's `controller.duration = newDuration`, which an
    /// [`ImplicitlyAnimatedWidget`](https://api.flutter.dev/flutter/widgets/ImplicitlyAnimatedWidget-class.html)
    /// assigns on every `didUpdateWidget` so a duration change takes effect on
    /// the *next* run. It does not retime an in-flight run (the active run keeps
    /// the duration it started with, since `tick_at` scales against the
    /// already-captured `run_duration`/`duration`); the new value is read when
    /// the next `forward`/`reverse` begins the run at elapsed zero.
    pub fn set_duration(&self, duration: Duration) {
        let mut inner = self.inner.lock();
        inner.duration = duration;
    }

    /// Start animation forward from current value to upper bound.
    ///
    /// The returned [`TickerFuture`] resolves `Ok(())` when this run finishes
    /// and `Err(TickerCanceled)` if it is superseded (a later run starts
    /// before this one ends) or torn down ([`stop`](Self::stop)/
    /// [`set_value`](Self::set_value)/[`reset`](Self::reset)/
    /// [`dispose`](Self::dispose)). See
    /// [`TickerFuture::when_complete_or_cancel`] for the idiom to react to
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
    /// let controller = AnimationController::without_ticker(Duration::from_millis(100));
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
    /// controller.tick_at(0.1);
    /// let waker = Waker::noop();
    /// let mut cx = Context::from_waker(waker);
    /// assert_eq!(Pin::new(&mut run).poll(&mut cx), Poll::Ready(Ok(())));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward(&self) -> Result<TickerFuture, AnimationError> {
        self.forward_from(None)
    }

    /// Start animation forward from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the forward duration scaled by
    /// `(upper_bound - value) / (upper_bound - lower_bound)` (Flutter
    /// parity — `AnimationController._animateToInternal` scales the
    /// simulation duration by the remaining fraction). A zero DISTANCE
    /// (starting at the upper bound) or a zero DURATION (the scaled run
    /// duration is `Duration::ZERO`) settles SYNCHRONOUSLY, before this
    /// call returns, with [`AnimationStatus::Completed`] and an
    /// already-complete [`TickerFuture`] — Flutter parity:
    /// `_animateToInternal`'s `simulationDuration == Duration.zero` branch
    /// (`animation_controller.dart:674-684` @ 3.44.0). See
    /// [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `from` is `NaN`, if
    /// `from` is infinite toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](Self::unbounded) (`forward`
    /// always targets `upper_bound`, which has no finite value to run to).
    pub fn forward_from(&self, from: Option<f32>) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

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

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Forward;
        inner.start_value = inner.value;
        inner.target_value = inner.upper_bound;
        let distance = (inner.target_value - inner.value).abs();
        let run_duration = inner.scaled_run_duration(inner.duration);
        if distance < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, inner));
        }

        inner.status = AnimationStatus::Forward;
        inner.run_duration = Some(run_duration);
        // `from` may have jumped `value` above without a settle (a real run
        // still starts). Flutter's `forward`'s `if (from != null) { value =
        // from; }` goes through the `value=` setter, which notifies
        // UNCONDITIONALLY — FLUI narrows that to "iff it actually moved"
        // (the same entry-value rule `settle_at_target` uses), so
        // `forward_from(Some(x))` from `x` itself does not fire a spurious
        // notification for a value that never changed.
        let value_change = if (inner.value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        self.finish(
            AnimationStatus::Forward,
            value_change,
            displaced_delivery,
            inner,
        );
        Self::warn_if_no_ticker(has_ticker);
        Ok(future)
    }

    /// Start animation in reverse from current value to lower bound.
    ///
    /// See [`forward`](Self::forward) for the returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse(&self) -> Result<TickerFuture, AnimationError> {
        self.reverse_from(None)
    }

    /// Start animation in reverse from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// The run covers the REMAINING distance at the full-range velocity:
    /// its duration is the reverse duration (falling back to the forward
    /// duration) scaled by `(value - lower_bound) / (upper_bound -
    /// lower_bound)` (Flutter parity — see [`forward_from`](Self::forward_from)).
    /// A zero DISTANCE (starting at the lower bound) or a zero DURATION
    /// settles SYNCHRONOUSLY, before this call returns, with
    /// [`AnimationStatus::Dismissed`] — see [`forward_from`](Self::forward_from)'s
    /// doc for the Flutter citation. See [`forward`](Self::forward) for the
    /// returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] if `from` is `NaN`, if
    /// `from` is infinite toward a bound this controller does not have, or
    /// if this controller is [`unbounded`](Self::unbounded) (`reverse`
    /// always targets `lower_bound`, which has no finite value to run to).
    pub fn reverse_from(&self, from: Option<f32>) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

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

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Reverse;
        inner.start_value = inner.value;
        inner.target_value = inner.lower_bound;
        let distance = (inner.target_value - inner.value).abs();
        let base = inner.reverse_duration.unwrap_or(inner.duration);
        let run_duration = inner.scaled_run_duration(base);
        if distance < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, inner));
        }

        inner.status = AnimationStatus::Reverse;
        inner.run_duration = Some(run_duration);
        // See `forward_from`'s own comment: notify iff `from` actually moved
        // the value, narrower than Flutter's unconditional `value=` notify.
        let value_change = if (inner.value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        self.finish(
            AnimationStatus::Reverse,
            value_change,
            displaced_delivery,
            inner,
        );
        Self::warn_if_no_ticker(has_ticker);
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
    /// Cancels the active run's [`TickerFuture`] with
    /// [`TickerCanceled`](flui_scheduler::ticker::TickerCanceled),
    /// delivered with no controller lock held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        let delivery = inner.stop_running();

        let status = inner.settled_status_directed();
        inner.status = status;
        self.finish(status, ValueChange::Unchanged, delivery, inner);
        Ok(())
    }

    /// Reset to the beginning (lower bound).
    ///
    /// Sets the value to the beginning — `lower_bound` on a bounded
    /// controller, `0.0` on an [`unbounded`](Self::unbounded) one (there is
    /// no `lower_bound` to return to: flutter/flutter#76014 asks for
    /// exactly this defined beginning) — and the status to
    /// [`AnimationStatus::Dismissed`]. Never fails for non-finiteness: a
    /// reset always has a value to land on. Cancels the active run's
    /// [`TickerFuture`] with
    /// [`TickerCanceled`](flui_scheduler::ticker::TickerCanceled), delivered
    /// with no controller lock held.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        let delivery = inner.stop_running();
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
    /// or below the current value**, ending [`AnimationStatus::Completed`] —
    /// Flutter parity: `AnimationController.animateTo`'s own doc
    /// (`animation_controller.dart:574-577` @ 3.44.0). If `target` is
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
    /// or if `target - value` overflows `f32`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_animation::AnimationController;
    /// use std::time::Duration;
    ///
    /// let controller = AnimationController::without_ticker(Duration::from_millis(100));
    /// let run = controller.animate_to(1.0, None).unwrap();
    /// run.when_complete_or_cancel(|end| {
    ///     assert!(end.is_ok(), "a run nothing superseded or stopped must complete");
    /// });
    /// controller.tick_at(0.1);
    /// ```
    pub fn animate_to(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<TickerFuture, AnimationError> {
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
    /// the mirror of [`animate_to`](Self::animate_to)'s own contract
    /// (`animation_controller.dart:611-614` @ 3.44.0).
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, AnimationDirection::Reverse, None)
    }

    /// Like [`animate_to`](Self::animate_to), but eases the run through
    /// `curve` instead of running linearly. Flutter parity:
    /// `AnimationController.animateTo(target, duration: ..., curve: ...)`,
    /// which threads `curve` into `_InterpolationSimulation`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_to_curved(
        &self,
        target: f32,
        duration: Option<Duration>,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, AnimationDirection::Forward, Some(curve))
    }

    /// Like [`animate_back`](Self::animate_back), but eases the run through
    /// `curve` instead of running linearly. Flutter parity:
    /// `AnimationController.animateBack(target, duration: ..., curve: ...)`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::NonFiniteTarget`] under the same conditions
    /// as [`animate_to`](Self::animate_to).
    pub fn animate_back_curved(
        &self,
        target: f32,
        duration: Option<Duration>,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, AnimationDirection::Reverse, Some(curve))
    }

    /// Shared driver for [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back)
    /// and their `_curved` variants: interpolate from the current value to
    /// `target`, easing through `curve` (`None` = linear).
    ///
    /// `direction` is the METHOD's, not derived from `target`'s relation to
    /// the current value — Flutter parity: `AnimationController.animateTo`/
    /// `animateBack` (`animation_controller.dart` @ 3.44.0) assign
    /// `_direction` from which method was called, before `target` is even
    /// looked at. It drives both the run's status (Forward/Reverse while
    /// running, Completed/Dismissed at the end —
    /// [`AnimationDirection::settled_status`]) and, when `duration` is
    /// `None`, which base duration (`self.duration`/`self.reverse_duration`)
    /// the remaining-fraction scaling starts from — again Flutter parity:
    /// `_animateToInternal`'s `directionDuration` local (same file) picks it
    /// off `_direction`, which by that point is already the method's
    /// choice, not a travel comparison.
    fn drive_to(
        &self,
        target: f32,
        duration: Option<Duration>,
        direction: AnimationDirection,
        curve: Option<Arc<dyn Curve + Send + Sync>>,
    ) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;
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
        // `target - value` overflowing f32 (e.g. `set_value(-f32::MAX)` then
        // `animate_to(f32::MAX)`) would make `tick_time_based`'s
        // `start_value + range * eased_t` interior lerp compute `inf * t`,
        // finite but wrong, or — at an already-non-finite `start_value` —
        // `inf * 0.0 = NaN`. Refuse before any mutation rather than let a
        // run install with a span nothing downstream can interpolate.
        let span = target - entry_value;
        if !span.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "{caller}: span ({target} - {entry_value}) overflows f32"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }
        inner.clear_run_modes();
        inner.run_curve = curve;
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
        // — Flutter's `simulationDuration == Duration.zero` gate
        // (`forward_from`'s doc has the citation).
        if (target - inner.value).abs() < BOUND_EPSILON || run_duration.is_zero() {
            return Ok(self.settle_at_target(entry_value, inner));
        }

        inner.status = inner.direction.running_status();
        inner.run_duration = Some(run_duration);
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        let status = inner.status;
        self.finish(status, ValueChange::Unchanged, displaced_delivery, inner);
        Self::warn_if_no_ticker(has_ticker);
        Ok(future)
    }

    /// Settle a run whose distance or duration is trivial: snap the value,
    /// stop the ticker, cancel whatever run this displaced, and report the
    /// run's directed settled status — no transient running status, no
    /// full-duration no-op run. Returns an already-complete [`TickerFuture`]
    /// for the (trivial) run this call represents. The single settle
    /// chokepoint for zero-DISTANCE (`forward()` already at the upper bound)
    /// and zero-DURATION (`forward(..., Some(Duration::ZERO))`) runs alike
    /// (issue #1171) — Flutter parity: `_animateToInternal`'s
    /// `simulationDuration == Duration.zero` branch covers both the same way
    /// (`animation_controller.dart:674-684` @ 3.44.0).
    ///
    /// `entry_value` is the value at the METHOD's entry, before
    /// `forward_from(Some(x))`/`reverse_from(Some(x))` apply `from` —
    /// comparing against the post-`from` value here would miss a jump
    /// (`forward_from(Some(1.0))` from `0.3` would report no value change).
    /// Value listeners fire only when `entry_value` actually differs from
    /// where this settle lands (Flutter: `if (value != target) { …
    /// notifyListeners(); }`, `:675-678`).
    fn settle_at_target(
        &self,
        entry_value: f32,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
    ) -> TickerFuture {
        inner.value = inner.target_value;
        // `is_running()` (Active | Muted), not `can_tick()` (Active only) —
        // the same lesson `restart_ticker`'s doc records: a MUTED ticker
        // still holds its run and would resume scheduling ticks against a
        // settled controller on unmute. `AnimationController` exposes no
        // mute today, so a Muted ticker is unreachable through the public
        // API here and this widening has no red test of its own.
        if let Some(ticker) = &mut inner.ticker
            && ticker.state().is_running()
        {
            ticker.stop();
        }
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(TickerCompleter::cancel);
        let value_change = if (inner.target_value - entry_value).abs() < BOUND_EPSILON {
            ValueChange::Unchanged
        } else {
            ValueChange::Notify
        };
        self.finish(status, value_change, delivery, inner);
        TickerFuture::complete()
    }

    /// Repeat the animation, bouncing if `reverse` is true. Repeats forever.
    ///
    /// An infinite repeat's [`TickerFuture`] resolves only by cancellation —
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
    /// [`unbounded`](Self::unbounded) controller — a default repeat targets
    /// this controller's own (infinite) bounds; use
    /// [`repeat_with`](Self::repeat_with) with an explicit finite range
    /// instead.
    pub fn repeat(&self, reverse: bool) -> Result<TickerFuture, AnimationError> {
        self.repeat_with(None, None, reverse, None, None)
    }

    /// Repeat the animation with full control over range, period, and count.
    ///
    /// The run starts from the CURRENT value, clamped into `[min, max]` —
    /// not from `min` — so a `repeat()` issued every build (a common pattern
    /// for a looping indicator) progresses instead of freezing at the start
    /// each time; to start at `min`, call [`set_value`](Self::set_value)
    /// first (flutter#67507). `value`/`status`/`direction` at any later
    /// [`tick_at`](Self::tick_at) are a pure function of the elapsed time
    /// since this call, the range, `period`, `reverse`, and `count` — the
    /// frame partition never changes the answer. `count` boundaries are
    /// measured from that phase origin, not from a fresh cycle 0: a run
    /// started mid-cycle ends `count` boundaries later, not `count` full
    /// periods (Flutter: `_exitTimeInSeconds = count*period - _initialT`;
    /// Compose: `iterations*duration - initialOffset`), and a finite run
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
    ///   0-duration animator: skip to the end; Compose rejects it, Flutter
    ///   asserts)
    /// * `count` - Number of cycles; `None` repeats indefinitely (see
    ///   [`repeat`](Self::repeat) for what that means for the returned
    ///   future). `Some(0)` also settles SYNCHRONOUSLY at the call, at the
    ///   CURRENT (clamped) value with no landing jump — zero cycles run, so
    ///   there is nothing to land on (Web Animations semantics for an
    ///   empty active interval; Flutter asserts `count > 0`, Compose throws
    ///   for `iterations < 1`)
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    /// Returns [`AnimationError::InvalidBounds`] if `min`/`max` are `NaN` or
    /// describe an inverted or empty range (a range-shape error, on any
    /// controller). Returns [`AnimationError::NonFiniteTarget`] if the
    /// EFFECTIVE range (after defaulting unset endpoints to this
    /// controller's own bounds) is not finite — only reachable on an
    /// [`unbounded`](Self::unbounded) controller with `min`/`max` left
    /// unset (or set on only one side); pass an explicit finite range to
    /// repeat on an unbounded controller.
    pub fn repeat_with(
        &self,
        min: Option<f32>,
        max: Option<f32>,
        reverse: bool,
        period: Option<Duration>,
        count: Option<u32>,
    ) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;
        let entry_value = inner.value;

        // A caller-supplied NaN endpoint is a range-shape error on ANY
        // controller (unguarded, `repeat_with(Some(f32::NAN), ..)` reaches
        // `inner.value.clamp(lo, hi)` below with `lo` itself NaN, which
        // panics inside `f32::clamp`'s own `assert!(min <= max)`) — checked
        // before defaulting/clamping so it can never be confused with the
        // *effective*-range non-finiteness an unbounded controller's own
        // defaulted bound produces below.
        if min.is_some_and(f32::is_nan) || max.is_some_and(f32::is_nan) {
            return Err(AnimationError::InvalidBounds(format!(
                "repeat range endpoints must not be NaN (min={min:?}, max={max:?})"
            )));
        }

        // Clamp the repeat range into the controller's bounds and reject an
        // empty/inverted range, so a repeat run can never start `value` (or its
        // ticks) outside `[lower_bound, upper_bound]` — consistent with
        // [`with_bounds`]'s `InvalidBounds` contract. Flutter permits
        // `min == max`; FLUI does not: a repeat that can structurally never
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
        // simulation) must not shape a following repeat — Flutter's `repeat`
        // applies no curve at all, and `tick_repeat` applies none either.
        // The curve specifically can never leak (`tick_repeat` never reads
        // `run_curve`), but a leftover `simulation`/`run_duration` would
        // still leak into `velocity()`/`current_duration()` without this.
        inner.clear_run_modes();

        // The value at the call is the pure function sampled at elapsed
        // time zero — NOT a bare `lo`: from `value == max` in restart mode
        // that is `lo` (the phase wraps), exactly Flutter's
        // `_startSimulation` setting `_value = x(0.0)`; in bounce mode a
        // value starting at `max` reports the reverse leg. Widen to f64
        // before subtracting — near `max` the f32 difference loses bits,
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
            return Ok(self.settle_at_target(entry_value, inner));
        }

        // Resolved ONCE, not read live on every tick: a later `set_duration`
        // must not retime an active repeat (Flutter parity — `period ??=
        // duration`, captured by the simulation at the call), and one period
        // for both legs of a bounce keeps the modular-nanosecond arithmetic
        // in `tick_repeat` exact.
        let period = period.unwrap_or(inner.duration);
        let period_ns = period.as_nanos();

        if period_ns == 0 {
            // ZERO EFFECTIVE PERIOD, any count: settle SYNCHRONOUSLY at the
            // call instead of installing a run that can never advance —
            // Android's rule ("0 duration animator, ignore the repeat count
            // and skip to the end", `ValueAnimator.animateBasedOnTime`);
            // Compose rejects it, Flutter asserts. An infinite zero-period
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
            return Ok(self.settle_at_target(entry_value, inner));
        }

        // Every degenerate case has returned: a `RepeatRun` is constructed
        // only here, so `period_ns > 0` holds by construction for every live
        // run and nothing downstream needs to guard it again.
        let ratio = (f64::from(v) - f64::from(lo)) / (f64::from(hi) - f64::from(lo));
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
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        // Flutter parity: `_startSimulation` sets `_value` directly, without
        // `notifyListeners()` (`AnimationController._startSimulation` @
        // 3.44.0) — the value-at-the-call jump is real but reported on the
        // run's first tick, not synchronously here.
        let status = inner.status;
        self.finish(status, ValueChange::Unchanged, displaced_delivery, inner);
        Self::warn_if_no_ticker(has_ticker);
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
    /// finite, or if this controller is [`unbounded`](Self::unbounded) in
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
    /// let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
    /// controller.fling(1.0).unwrap(); // Fling forward
    /// ```
    pub fn fling(&self, velocity: f32) -> Result<TickerFuture, AnimationError> {
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
        velocity: f32,
        spring: Option<SpringDescription>,
    ) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

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
        inner.clear_run_modes();
        inner.simulation = Some(Box::new(sim));
        inner.status = inner.direction.running_status();
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        let status = inner.status;
        self.finish(status, ValueChange::Unchanged, displaced_delivery, inner);
        Self::warn_if_no_ticker(has_ticker);
        Ok(future)
    }

    /// Drive the animation according to a custom simulation. Works
    /// unmodified on an [`unbounded`](Self::unbounded) controller — a
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
    /// let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
    /// let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 0.5);
    /// let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
    /// controller.animate_with(sim).unwrap();
    /// ```
    pub fn animate_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<TickerFuture, AnimationError> {
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
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_simulation(Box::new(simulation), AnimationDirection::Reverse)
    }

    fn drive_simulation(
        &self,
        simulation: Box<dyn Simulation>,
        direction: AnimationDirection,
    ) -> Result<TickerFuture, AnimationError> {
        // `Simulation::x` is arbitrary caller code, evaluated BEFORE taking
        // the lock: running it under `inner`'s guard (as a `lock, evaluate,
        // proceed` shape would) lets a caller whose `x` re-enters this same
        // controller — directly, or through anything reachable from it —
        // deadlock on the non-reentrant `parking_lot::Mutex`. A simulation
        // that starts non-finite would otherwise also install a run whose
        // `is_done` may never fire — refused below, before any mutation,
        // exactly like every other non-finite entry point.
        let initial = simulation.x(0.0);

        let mut inner = self.inner.lock();
        // `check_disposed` still runs FIRST, preserving today's error
        // precedence: a disposed controller reports `Disposed` even when
        // `initial` is ALSO non-finite, not `NonFiniteTarget` — evaluating
        // `initial` earlier changes WHEN it runs, not which error wins.
        Self::check_disposed(&inner)?;

        if !initial.is_finite() {
            let err = AnimationError::NonFiniteTarget(format!(
                "simulation.x(0.0) = {initial} is not finite"
            ));
            return Err(Self::warn_non_finite_target(inner, err));
        }

        inner.clear_run_modes();
        inner.direction = direction;
        inner.status = direction.running_status();
        inner.value = initial.clamp(inner.lower_bound, inner.upper_bound);
        inner.simulation = Some(simulation);
        // `restart_ticker` runs BEFORE the completer replaces `active_run` —
        // see its own doc for why the order is load-bearing.
        let has_ticker = self.restart_ticker(&mut inner);
        let (completer, future) = TickerFuture::pending();
        let displaced_delivery = inner
            .active_run
            .replace(completer)
            .map(TickerCompleter::cancel);

        let status = inner.status;
        self.finish(status, ValueChange::Unchanged, displaced_delivery, inner);
        Self::warn_if_no_ticker(has_ticker);
        Ok(future)
    }

    /// Get the current velocity of the animation (0.0 if not running).
    #[must_use]
    pub fn velocity(&self) -> f32 {
        let inner = self.inner.lock();
        // `active_run.is_none()`, not `!status.is_running()`: `active_run`
        // is the actual "is a run installed" fact (see `walk_probe`'s doc
        // for the two ways `status.is_running()` diverges from it — a mid-run
        // `dispose()`, and a mid-run `set_value()`). Gating on a stale
        // running status here let a mid-run `set_value` report the
        // interrupted run's `(target_value - start_value) / duration` rate
        // for a run that no longer exists.
        if inner.active_run.is_none() {
            return 0.0;
        }

        let cycle = inner.cycle_elapsed_secs();
        if let Some(sim) = &inner.simulation {
            return sim.dx(narrow_f32(cycle));
        }

        let duration = inner.current_duration();
        if duration.is_zero() {
            return 0.0;
        }
        let range = inner.target_value - inner.start_value;
        range / duration.as_secs_f32()
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
    /// chokepoint) leave it untouched. It wraps on `u64` overflow — only
    /// its *change* is observed, so the wrap is harmless.
    #[must_use]
    pub fn run_generation(&self) -> u64 {
        self.inner.lock().run_generation
    }

    /// One-lock snapshot for [`Vsync`](crate::vsync::Vsync)'s per-frame walk —
    /// see [`WalkProbe`]'s own doc for the perf rationale.
    ///
    /// `live_running` is `!disposed && active_run.is_some()` — **not**
    /// `status.is_running()`, which is the wrong "is a run installed"
    /// predicate for two independent reasons:
    /// - [`dispose`](Self::dispose) deliberately leaves `status` untouched
    ///   (see its own doc), so a controller disposed mid-run keeps whatever
    ///   running status it had.
    /// - [`set_value`](Self::set_value) reports a *directional* running
    ///   status at an interior value (Flutter parity —
    ///   [`settled_status_keep_direction`](AnimationControllerInner::settled_status_keep_direction))
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
        let inner = self.inner.lock();
        WalkProbe {
            generation: inner.run_generation,
            live_running: !inner.disposed && inner.active_run.is_some(),
        }
    }

    /// Advance the animation using the ticker's most recent elapsed time.
    ///
    /// Normally driven by the ticker callback; exposed for manual stepping.
    pub fn tick(&self) {
        let raw = {
            let inner = self.inner.lock();
            inner.ticker.as_ref().map_or(0.0, Ticker::elapsed_secs)
        };
        self.tick_at(raw);
    }

    /// Advance the animation to `raw_elapsed_secs` seconds (ticker timeline,
    /// pre-dilation) since the ticker started.
    ///
    /// This is the single time-driven entry point: time-based runs interpolate
    /// `start_value -> target_value`, simulations sample `x(t)`, and a repeat
    /// samples its leg/phase/exhaustion as a pure function of `cycle` (see
    /// the private `tick_repeat`). Value and status listeners are fired
    /// only after the inner lock is released.
    pub fn tick_at(&self, raw_elapsed_secs: f64) {
        let mut inner = self.inner.lock();
        // `active_run.is_none()`, not `!status.is_running()`: `active_run`
        // is the actual "is there a run installed to advance" fact (see
        // `walk_probe`'s doc for the two ways `status.is_running()` diverges
        // from it — a mid-run `dispose()`, and a mid-run `set_value()`).
        // Advancing on a stale `status` alone let a `set_value` mid-run be
        // silently overwritten by the run it had just stopped.
        if inner.disposed || inner.active_run.is_none() {
            return;
        }
        inner.last_raw_elapsed_secs = raw_elapsed_secs;
        // `restart_ticker` always begins a fresh run's `Ticker` at elapsed
        // zero, so the dilated elapsed time IS the elapsed time since this
        // run started — no per-run epoch to subtract.
        let cycle = (raw_elapsed_secs / time_dilation().max(f64::MIN_POSITIVE)).max(0.0);

        if let Some(run) = inner.repeat {
            self.tick_repeat(inner, run, cycle);
        } else if inner.simulation.is_some() {
            self.tick_simulation(inner, narrow_f32(cycle));
        } else {
            self.tick_time_based(inner, cycle);
        }
    }

    /// Simulation branch of [`tick_at`](Self::tick_at).
    fn tick_simulation(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        cycle: f32,
    ) {
        // `simulation.is_some()` was checked by the caller.
        let sim = inner
            .simulation
            .as_ref()
            .expect("tick_simulation requires an active simulation");
        let sampled = sim.x(cycle);

        // A user simulation emitting a non-finite sample mid-run must not
        // poison the controller — end the run AT THE LAST FINITE VALUE
        // instead of writing the sample through: a "value unchanged, run
        // continues" no-op would leave `active_run` installed and `Vsync`
        // ticking forever, and a scrollable's `is_scrolling` stuck. The
        // latch is shared with `set_value`'s non-finite canonicalization.
        if !sampled.is_finite() {
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;
            self.end_simulation_run(inner, ValueChange::Unchanged);
            Self::warn_non_finite_value(should_warn, sampled, NonFiniteOutcome::EndedRun);
            return;
        }

        let new_value = sampled.clamp(inner.lower_bound, inner.upper_bound);
        let is_done = sim.is_done(cycle);
        inner.value = new_value;

        if is_done {
            self.end_simulation_run(inner, ValueChange::Notify);
        } else {
            drop(inner);
            self.notifier.notify_listeners();
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
    /// unlocks (Flutter parity: `_tick` completes its `Completer` before
    /// `notifyListeners()`), so a panicking value/status listener still
    /// leaves the run `Ok` — the unwind drops the delivery, which delivers
    /// the already-published outcome. `complete`, not `cancel`: both paths
    /// are the run ending on its own terms, never a cancellation.
    fn end_simulation_run(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        value_change: ValueChange,
    ) {
        inner.simulation = None;
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }
        let status = inner.direction.settled_status();
        inner.status = status;
        let delivery = inner.active_run.take().map(TickerCompleter::complete);
        self.finish(status, value_change, delivery, inner);
    }

    /// Time-based (tween) branch of [`tick_at`](Self::tick_at). Never called
    /// while a repeat is active — [`tick_at`](Self::tick_at) dispatches a
    /// repeat to [`tick_repeat`](Self::tick_repeat) instead.
    fn tick_time_based(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        cycle: f64,
    ) {
        let duration = inner.current_duration();
        let t = if duration.is_zero() {
            1.0
        } else {
            narrow_f32((cycle / duration.as_secs_f64()).clamp(0.0, 1.0))
        };
        // Flutter parity: `_InterpolationSimulation.x` special-cases the
        // endpoints to the exact begin/end value and only runs the curve
        // through the interior, so a curve that overshoots slightly at its
        // bounds (e.g. an elastic curve) never reports outside
        // `[start, target]`. Reading `start_value`/`target_value` directly
        // at the endpoints (rather than `start + range * eased_t` with
        // `eased_t` merely clamped to 0.0/1.0) is structural, not cosmetic:
        // `range` can be `+-inf` in principle (an extreme-bounds
        // configuration; `animate_to`/`animate_back` themselves already
        // refuse an overflowing span before a run ever starts), and
        // `inf * 0.0 = NaN` — no path through this function may ever
        // compute that product.
        let value = if t <= 0.0 {
            inner.start_value
        } else if t >= 1.0 {
            inner.target_value
        } else {
            let eased_t = match &inner.run_curve {
                Some(curve) => curve.transform(t),
                None => t,
            };
            inner.start_value + (inner.target_value - inner.start_value) * eased_t
        };
        inner.value = value;

        if t < 1.0 {
            drop(inner);
            self.notifier.notify_listeners();
            return;
        }
        // `inner.value` is already `target_value` — set above by the
        // `t >= 1.0` arm.

        // Non-repeating completion. Flutter parity: `AnimationController._tick`
        // (`animation_controller.dart` @ 3.44.0) reports the settled status
        // BY DIRECTION — completed after a forward run, dismissed after a
        // reverse one — with no at-a-bound requirement. Keeping the running
        // status for a mid-range stop (the previous behavior) starved every
        // status listener of the run's end: on an unbounded controller (a
        // scrollable's pixel-space fling controller) a driven `animate_to`
        // NEVER lands on a bound, so its completion was silent.
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }
        let status = inner.direction.settled_status();
        inner.status = status;
        // Publish before unlocking — see the simulation branch's own comment
        // for why the order matters to a panicking listener.
        let delivery = inner.active_run.take().map(TickerCompleter::complete);
        self.finish(status, ValueChange::Notify, delivery, inner);
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
    fn tick_repeat(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        run: RepeatRun,
        cycle: f64,
    ) {
        // `tick_at` already clamps `cycle` to `.max(0.0)`, and NaN cannot
        // reach it (`f64::max` returns the non-NaN operand), so the only
        // remaining failure is `Err` on an out-of-range `cycle` (e.g.
        // `f64::INFINITY`, or `raw_elapsed_secs / time_dilation` overflowing
        // under an extreme dilation). That must saturate the SAME direction
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
            if let Some(ticker) = &mut inner.ticker {
                ticker.stop();
            }
            inner.repeat = None;
            let status = direction.settled_status();
            inner.status = status;
            let delivery = inner.active_run.take().map(TickerCompleter::complete);
            self.finish(status, ValueChange::Notify, delivery, inner);
            return;
        }

        let sample = AnimationControllerInner::repeat_sample(&run, total_ns);
        inner.direction = sample.direction;
        inner.start_value = sample.start;
        inner.target_value = sample.target;
        // `take_status_change` dedups repeated same-status writes, so a leg
        // flip fires exactly one status change and an even number of
        // skipped bounce cycles in one long frame fires none. Value
        // listeners fire on every tick, as they do for the time-based and
        // simulation branches (Flutter's `_tick` calls `notifyListeners()`
        // unconditionally): a tick is a frame, and a listener that repaints
        // per frame must not be starved by a sample that happens to repeat
        // the previous value.
        inner.value = sample.value;
        let status = sample.direction.running_status();
        inner.status = status;
        self.finish(status, ValueChange::Notify, None, inner);
    }

    /// Set the value directly without animating; recomputes status and notifies.
    ///
    /// Stops any active run first (Flutter parity: `AnimationController`'s
    /// `value=` setter calls `stop()` before `_internalSetValue`) — otherwise
    /// a live ticker keeps re-registering itself with the scheduler and the
    /// next frame recomputes the value from the stale run's `start_value`/
    /// `target_value`, silently overwriting what was just set.
    ///
    /// A non-finite input is canonicalized on a BOUNDED controller — `NaN`
    /// to the lower bound, `+-inf` to whichever bound it points at — the
    /// same rule `clamp` already applies to a finite input, made total over
    /// `NaN` (which `clamp` alone propagates unchanged and would otherwise
    /// poison every downstream curve/tween evaluation). On an
    /// [`unbounded`](Self::unbounded) controller a non-finite input is
    /// instead a FULL no-op: no active run is stopped, no notification
    /// fires, the value stays exactly what it was — a poisoned gesture drag
    /// or a bad computation must not clobber a live fling or snap a
    /// scrollable to a bound it doesn't have. Either way the warning below
    /// is latched (`non_finite_warned`): it fires once per controller, not
    /// once per frame of a misbehaving caller.
    pub fn set_value(&self, value: f32) {
        let mut inner = self.inner.lock();

        if !value.is_finite() {
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;

            if inner.is_unbounded() {
                drop(inner);
                Self::warn_non_finite_value(should_warn, value, NonFiniteOutcome::Ignored);
                return;
            }

            let delivery = inner.stop_running();
            let canonical = if value.is_nan() {
                inner.lower_bound
            } else {
                value
            };
            inner.value = canonical.clamp(inner.lower_bound, inner.upper_bound);
            let status = inner.settled_status_keep_direction();
            inner.status = status;
            self.finish(status, ValueChange::Notify, delivery, inner);
            Self::warn_non_finite_value(should_warn, value, NonFiniteOutcome::Canonicalized);
            return;
        }

        let delivery = inner.stop_running();
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        self.finish(status, ValueChange::Notify, delivery, inner);
    }

    /// **CRITICAL:** Dispose when done to prevent leaks.
    ///
    /// Stops the animation and clears resources. Idempotent. Cancels the
    /// active run's [`TickerFuture`] with
    /// [`TickerCanceled`](flui_scheduler::ticker::TickerCanceled) —
    /// delivered with no controller lock held — even though `dispose` itself never
    /// changes `status` and so fires no status listener (they are already
    /// cleared by the time delivery runs).
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();
        if inner.disposed {
            return;
        }
        let delivery = inner.active_run.take().map(TickerCompleter::cancel);
        if let Some(mut ticker) = inner.ticker.take() {
            ticker.stop();
        }
        inner.status_listeners.clear();
        inner.disposed = true;
        let status = inner.status;
        self.finish(status, ValueChange::Unchanged, delivery, inner);
    }

    fn check_disposed(inner: &AnimationControllerInner) -> Result<(), AnimationError> {
        if inner.disposed {
            Err(AnimationError::Disposed)
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
    /// bound it points at when that bound is finite — Flutter's own "go to
    /// the end" idiom, e.g. `animate_to(f32::INFINITY)` on a bounded
    /// controller — and is refused when that bound is itself non-finite: an
    /// unbounded controller has no end in that direction to go to.
    fn canonicalize_value_target(
        caller: &str,
        raw: f32,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<f32, AnimationError> {
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
    /// mutation has been applied — it drops the controller's lock before
    /// warning, mirroring [`warn_if_no_ticker`](Self::warn_if_no_ticker), so
    /// the warning's arbitrary subscriber never runs under it.
    fn warn_non_finite_target(
        inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
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
        self.inner.lock().status_listeners.len()
    }

    /// Whether the "received a non-finite value" latch has already fired.
    /// Test-only: pins that the latch is set-once, not a per-call flag —
    /// the warning it gates fires at most once per controller.
    #[cfg(test)]
    pub(crate) fn debug_non_finite_warned(&self) -> bool {
        self.inner.lock().non_finite_warned
    }

    /// Reset run state and (re)start the ticker for a fresh run from epoch 0.
    ///
    /// Returns `false` iff this controller has no ticker at all — the
    /// caller must warn (and only after it has dropped the controller
    /// lock). This function itself never emits, and every run-starting site
    /// calls it **before** creating this run's [`TickerCompleter`] and
    /// displacing the previous one — so no `TickerDelivery` is ever live
    /// while this runs. That ordering is load-bearing, not incidental:
    /// `Ticker::start` reaches the scheduler's `schedule_tick_if_active` →
    /// `request_frame` → the embedder's `on_frame_scheduled` hook, which
    /// *is* foreign code running under this guard (unavoidably — it always
    /// has been, since `Ticker::start` was first called under the
    /// controller's lock). What this ordering removes is only "and a live
    /// delivery drops under the lock if that foreign code panics" — not the
    /// foreign call itself. `replace`/`.map(TickerCompleter::cancel)` at
    /// every call site touch only `active_run`; this function touches only
    /// `last_raw_elapsed_secs`/`run_generation`/`ticker` — disjoint fields,
    /// so reordering the two calls is free.
    #[must_use]
    fn restart_ticker(&self, inner: &mut AnimationControllerInner) -> bool {
        inner.last_raw_elapsed_secs = 0.0;
        // A fresh run's `t = 0` is established here — bump the generation so an
        // external frame driver re-anchors its per-run epoch (see
        // [`run_generation`](Self::run_generation)). `restart_ticker` is the
        // single chokepoint every run-start path funnels through.
        inner.run_generation = inner.run_generation.wrapping_add(1);
        let Some(ticker) = &mut inner.ticker else {
            return false;
        };
        // Restart-safe: `Ticker::start` refuses a start while a run is
        // already installed and silently drops the callback instead, so
        // a live run must be ended here or the restart is silently
        // dropped. `is_running()`, not `can_tick()`: a Muted ticker still
        // holds its run, so the narrower Active-only test skipped the
        // stop and left the animation stuck.
        //
        // The guard stays rather than stopping unconditionally: `stop()` is
        // NOT a no-op on an Idle or Stopped ticker — it clears the callback
        // slot — and the start below is the only thing that reinstalls one.
        // Narrowing the stop to runs that actually exist keeps that
        // coupling out of the picture.
        if ticker.state().is_running() {
            ticker.stop();
        }
        let controller = self.clone();
        ticker.start(move |elapsed| controller.tick_at(elapsed));
        true
    }

    /// Emits the "no ticker" warning `restart_ticker` cannot emit itself —
    /// call only after `finish` has unlocked and delivered, never while
    /// this controller's own lock is still held.
    fn warn_if_no_ticker(has_ticker: bool) {
        if !has_ticker {
            tracing::warn!("AnimationController has no ticker; the animation will not advance");
        }
    }

    /// Emits the "received a non-finite value" warning shared by
    /// [`set_value`](Self::set_value)'s canonicalization and
    /// [`tick_simulation`](Self::tick_simulation)'s mid-run non-finite
    /// sample — call only after `finish` has unlocked and delivered, for
    /// the same reason as [`warn_if_no_ticker`](Self::warn_if_no_ticker).
    /// `should_warn` is the caller's snapshot of the latch
    /// (`!non_finite_warned`, taken before setting it) — this fires at most
    /// once per controller, not once per frame of a poisoned gesture drag
    /// or a misbehaving simulation.
    fn warn_non_finite_value(should_warn: bool, raw: f32, outcome: NonFiniteOutcome) {
        if should_warn {
            tracing::warn!(
                value = raw,
                "received a non-finite value; {} -- drive the controller with finite values",
                outcome.description()
            );
        }
    }

    /// Fire status callbacks. MUST be called with no controller lock held.
    fn fire_status(callbacks: &[StatusCallback], status: AnimationStatus) {
        for cb in callbacks {
            cb(status);
        }
    }

    /// The single chokepoint for the unlock-then-fan-out sequence every
    /// run-ending or run-starting site needs: drop the controller lock,
    /// notify value listeners iff `value_change` says the value changed too
    /// (a run-start changes status but not value; a settle/tick changes
    /// both), fire status listeners for `status`, and — last — deliver a
    /// resolved or displaced run's [`TickerDelivery`] if one is pending.
    ///
    /// `TickerDelivery::deliver` is called from nowhere else in this file:
    /// every site that obtains one from [`TickerCompleter::complete`]/
    /// [`cancel`](TickerCompleter::cancel) hands it here instead of
    /// delivering it itself, which is what lets one source-guard test
    /// (`ticker_completer_resolution_never_bypasses_the_finish_chokepoint`) cover
    /// every call site in this file at once. Status fires BEFORE delivery:
    /// a new run's status is observable before the run it displaced reports
    /// its own cancellation, matching the order a caller sees them settle.
    ///
    /// `delivery` is declared before `inner` on purpose: parameters drop in
    /// reverse declaration order, so if a panic unwinds from between
    /// `take_status_change()` and the explicit `drop(inner)` below,
    /// `inner`'s guard is still released before `delivery` — a delivery
    /// dropped here always runs its fan-out with the controller lock free.
    /// The six run-start callers run `restart_ticker` (which runs the
    /// scheduler's `on_frame_scheduled` hook — genuinely foreign code, under
    /// the guard, same as it always was) *before* creating this run's
    /// completer, so no delivery is ever live across that call either; see
    /// `restart_ticker`'s own doc. The run-ending callers (`stop`, `reset`,
    /// `set_value`, `dispose`, the tick paths) never call it.
    fn finish(
        &self,
        status: AnimationStatus,
        value_change: ValueChange,
        delivery: Option<TickerDelivery>,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
    ) {
        let callbacks = inner.take_status_change();
        drop(inner);
        if value_change == ValueChange::Notify {
            self.notifier.notify_listeners();
        }
        if let Some(callbacks) = callbacks {
            Self::fire_status(&callbacks, status);
        }
        if let Some(delivery) = delivery {
            delivery.deliver();
        }
    }
}

impl AnimationControllerInner {
    /// Clear repeat/simulation/per-run-duration/curve modes (used when a new
    /// explicit run begins).
    fn clear_run_modes(&mut self) {
        self.repeat = None;
        self.run_duration = None;
        self.simulation = None;
        self.run_curve = None;
    }

    /// Halt any active run at the current value: stop the ticker, clear
    /// simulation/repeat/curve state, and cancel the displaced run's
    /// completer — without touching `status` or emitting any notification.
    /// Flutter parity: the raw `AnimationController.stop()` that the
    /// `value=` setter calls before `_internalSetValue` — it only clears
    /// `_simulation`/`_lastElapsedDuration` and stops the ticker, it does
    /// not recompute status (the caller does that separately).
    ///
    /// The returned [`TickerDelivery`] must be handed to
    /// [`AnimationController::finish`]; nothing else in this file may call
    /// `deliver()` on it.
    #[must_use = "a displaced run's TickerDelivery must reach AnimationController::finish"]
    fn stop_running(&mut self) -> Option<TickerDelivery> {
        self.clear_run_modes();
        if let Some(ticker) = &mut self.ticker {
            ticker.stop();
        }
        self.active_run.take().map(TickerCompleter::cancel)
    }

    /// Snapshot the callbacks to fire **iff** `self.status` differs from the
    /// last-reported status, updating the marker so a later same-status
    /// emission is suppressed. Every status-emission site in this file
    /// funnels through this seam — Flutter parity: `_checkStatusChanged`.
    fn take_status_change(&mut self) -> Option<SmallVec<[StatusCallback; 4]>> {
        if self.status == self.last_reported_status {
            return None;
        }
        self.last_reported_status = self.status;
        Some(
            self.status_listeners
                .iter()
                .map(|(_, cb)| Arc::clone(cb))
                .collect(),
        )
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
    /// Flutter parity: `AnimationController._animateToInternal` scales the
    /// simulation duration by the remaining fraction, so a mid-flight
    /// `forward()`/`reverse()` keeps constant velocity instead of stretching
    /// the leftover distance over the full duration. Degenerate ranges
    /// (zero, non-finite) fall back to the unscaled base, like Flutter's
    /// `range.isFinite ? ... : 1.0`.
    fn scaled_run_duration(&self, base: Duration) -> Duration {
        let range = self.upper_bound - self.lower_bound;
        if !range.is_finite() || range <= 0.0 {
            return base;
        }
        let fraction =
            f64::from(((self.target_value - self.start_value).abs() / range).clamp(0.0, 1.0));
        if !fraction.is_finite() {
            return base;
        }
        base.mul_f64(fraction)
    }

    /// Dilated elapsed time since the current run started, from the last
    /// observed tick (`restart_ticker` always begins a fresh run at elapsed
    /// zero, so there is no per-run epoch left to subtract).
    fn cycle_elapsed_secs(&self) -> f64 {
        (self.last_raw_elapsed_secs / time_dilation().max(f64::MIN_POSITIVE)).max(0.0)
    }

    /// Whether this controller was built by one of the `unbounded*`
    /// constructors. Both bounds are fixed `+-inf` TOGETHER there — FLUI
    /// never constructs a half-open pair (one finite, one infinite); see
    /// [`AnimationController::with_bounds_inner`]'s own doc — so checking
    /// `lower_bound` alone detects it.
    fn is_unbounded(&self) -> bool {
        !self.lower_bound.is_finite()
    }

    /// Whether the current value is at (or indistinguishable from) the upper bound.
    ///
    /// Uses exact equality for infinite bounds to avoid the `INFINITY - INFINITY = NaN`
    /// pitfall that breaks the epsilon comparison on an
    /// [`unbounded`](AnimationController::unbounded)-family controller,
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
    /// frame-driver contract, not a Flutter one (Flutter's `stop()` changes
    /// no status at all): a bound reached mid-frame must report the bound it
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
    fn repeat_landing(reverse: bool, min: f32, max: f32, index: u128) -> (f32, AnimationDirection) {
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
        let value = start + (target - start) * narrow_f32(phase);
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
    /// Flutter parity: `AnimationController._tick` reports
    /// `completed`/`dismissed` purely by `_direction`
    /// (`animation_controller.dart:948-950` @ 3.44.0) — so
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

impl Animation<f32> for AnimationController {
    #[inline]
    fn value(&self) -> f32 {
        self.inner.lock().value
    }

    #[inline]
    fn status(&self) -> AnimationStatus {
        self.inner.lock().status
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        let mut inner = self.inner.lock();
        let id = ListenerId::new(inner.next_listener_id);
        inner.next_listener_id += 1;
        inner.status_listeners.push((id, callback));
        id
    }

    fn remove_status_listener(&self, id: ListenerId) {
        let mut inner = self.inner.lock();
        inner
            .status_listeners
            .retain(|(listener_id, _)| *listener_id != id);
    }

    /// Whether the controller is currently driving a run.
    ///
    /// Flutter parity: `AnimationController.isAnimating` is ticker-based
    /// (`_ticker!.isActive`), not status-based. `set_value` at an interior
    /// value reports a directional status (per `_internalSetValue`) while the
    /// controller is stopped, so the trait's status-derived default would
    /// wrongly report `true` there. A muted ticker still counts as animating,
    /// matching `Ticker.isActive`.
    #[inline]
    fn is_animating(&self) -> bool {
        self.inner
            .lock()
            .ticker
            .as_ref()
            .is_some_and(Ticker::is_running)
    }
}

impl Listenable for AnimationController {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

impl fmt::Debug for AnimationController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock();
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
