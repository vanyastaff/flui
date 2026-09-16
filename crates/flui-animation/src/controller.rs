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
/// The controller is `Send + Sync` via `Arc<Mutex<…>>`. Per ADR-0002 the
/// controller is control-plane and would ideally be thread-affine; it remains
/// `Send + Sync` as a recorded, scoped exception until the engine-wide `!Send`
/// flip lands (see `docs/adr/ADR-0002-engine-wide-threading-architecture.md`).
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
    /// Returns [`AnimationError::InvalidBounds`] if `lower_bound >= upper_bound`.
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
    /// Returns [`AnimationError::InvalidBounds`] if `lower_bound >= upper_bound`.
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
    /// Returns [`AnimationError::InvalidBounds`] if `lower_bound >= upper_bound`.
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
    /// finite and `lower_bound < upper_bound`. Bounded means finite;
    /// unbounded is [`Self::unbounded_inner`], not a bound value — a
    /// half-open pair (one finite, one infinite) is rejected the same way,
    /// since nothing in this workspace needs it and the rule would
    /// otherwise be incidental complexity.
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
        // this takes.
        if lower_bound >= upper_bound || !lower_bound.is_finite() || !upper_bound.is_finite() {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower_bound}) and upper_bound ({upper_bound}) must both be \
                 finite, with lower_bound < upper_bound"
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
                match Self::canonicalize_value_target(raw, inner.lower_bound, inner.upper_bound) {
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
                match Self::canonicalize_value_target(raw, inner.lower_bound, inner.upper_bound) {
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

        let target =
            match Self::canonicalize_value_target(target, inner.lower_bound, inner.upper_bound) {
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
                "animate_to/animate_back span ({target} - {entry_value}) overflows f32"
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
        // controller (today `repeat_with(Some(f32::NAN), ..)` installs a NaN
        // run) — checked before defaulting/clamping so it can never be
        // confused with the *effective*-range non-finiteness an unbounded
        // controller's own defaulted bound produces below.
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
        // finite range to repeat inside (Decision 2 — `repeat()`/
        // `repeat_with(None, None, ..)` on an unbounded controller is
        // refused this way; an explicit finite range on both sides never
        // reaches here).
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
        // after the InvalidSpring check too (fixing the pre-existing bug
        // where a refused fling still corrupted `direction`, observable
        // via a later `stop()`).
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
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        // A simulation that starts non-finite would otherwise install a run
        // whose `is_done` may never fire (Decision 4/v3 delta 1) — refuse
        // before any mutation, exactly like every other non-finite entry
        // point.
        let initial = simulation.x(0.0);
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
        if !inner.status.is_running() {
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
        // instead of writing the sample through (v3 delta 1): a
        // "value unchanged, run continues" no-op would leave `active_run`
        // installed and `Vsync` ticking forever, and a scrollable's
        // `is_scrolling` stuck. The latch is shared with
        // `set_value`'s non-finite canonicalization.
        if !sampled.is_finite() {
            inner.simulation = None;
            if let Some(ticker) = &mut inner.ticker {
                ticker.stop();
            }
            let status = inner.direction.settled_status();
            inner.status = status;
            // Publish before unlocking — see below for why the order
            // matters to a panicking listener. `complete`, not `cancel`:
            // this is the run ending on its own terms, same as a normal
            // `is_done`.
            let delivery = inner.active_run.take().map(TickerCompleter::complete);
            let should_warn = !inner.non_finite_warned;
            inner.non_finite_warned = true;
            self.finish(status, ValueChange::Unchanged, delivery, inner);
            Self::warn_non_finite_value(should_warn, sampled);
            return;
        }

        let new_value = sampled.clamp(inner.lower_bound, inner.upper_bound);
        let is_done = sim.is_done(cycle);
        inner.value = new_value;

        if is_done {
            inner.simulation = None;
            if let Some(ticker) = &mut inner.ticker {
                ticker.stop();
            }
            let status = inner.direction.settled_status();
            inner.status = status;
            // Publish the completion BEFORE unlocking (Flutter parity: `_tick`
            // completes the run's `Completer` before `notifyListeners()`), so
            // a panicking value/status listener leaves the run `Ok` — the
            // unwind drops the delivery, which delivers the already-published
            // outcome.
            let delivery = inner.active_run.take().map(TickerCompleter::complete);
            self.finish(status, ValueChange::Notify, delivery, inner);
        } else {
            drop(inner);
            self.notifier.notify_listeners();
        }
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
                Self::warn_non_finite_value(should_warn, value);
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
            Self::warn_non_finite_value(should_warn, value);
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
    /// against this controller's bounds.
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
        raw: f32,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<f32, AnimationError> {
        if raw.is_nan() {
            return Err(AnimationError::NonFiniteTarget(
                "value must be finite, got NaN".to_string(),
            ));
        }
        let clamped = raw.clamp(lower_bound, upper_bound);
        if !clamped.is_finite() {
            return Err(AnimationError::NonFiniteTarget(format!(
                "value {raw} has no finite bound to run to in that direction \
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
    fn warn_non_finite_value(should_warn: bool, raw: f32) {
        if should_warn {
            tracing::warn!(
                value = raw,
                "received a non-finite value; canonicalized or ignored -- drive the \
                 controller with finite values"
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
    /// pitfall that breaks the epsilon comparison when the fling controller is created
    /// with `(NEG_INFINITY, INFINITY)` bounds.
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
mod tests {
    use super::*;
    use flui_scheduler::UpdateScheduler;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Several tests assert exact per-tick progress, and `time_dilation_scales_progress`
    // mutates the *global* `time_dilation`. Serialize all controller tests so the
    // dilation mutation can never corrupt a sibling's progress assertions under a
    // parallel `cargo test` run.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> parking_lot::MutexGuard<'static, ()> {
        SERIAL.lock()
    }

    fn controller(ms: u64) -> AnimationController {
        let scheduler = UpdateScheduler::new();
        AnimationController::new(Duration::from_millis(ms), &scheduler)
    }

    /// Migration-premise pin: every production
    /// `AnimationController` today is built with a private, never-pumped
    /// `Arc::new(UpdateScheduler::new())` — the "wall-clock fallback" comments in
    /// `flui-widgets` (`scrollable.rs`, `navigator/binding.rs`) claim the
    /// ticker on that private scheduler fires on its own. It cannot: nothing
    /// ever calls `handle_begin_frame`/`execute_frame` on a scheduler no one
    /// else holds, so the ticker's transient callback registers and then
    /// simply never runs.
    ///
    /// This test proves that premise BEFORE `without_ticker` (a controller
    /// with no scheduler at all) replaces those throwaway-scheduler call
    /// sites: it drives a *different*, actually-pumped scheduler for many
    /// frames and asserts the controller's own private scheduler never once
    /// produced a tick. If this assertion ever fails, some path pumps a
    /// controller's private scheduler after all, `without_ticker` would be a
    /// real behavior change, and the widget migration must stop and be
    /// re-examined — not proceed on this premise.
    #[test]
    fn a_controllers_private_unpumped_scheduler_never_advances_without_vsync() {
        let _serial = serial();
        let private_scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::from_millis(100), &private_scheduler);
        c.forward().unwrap();

        // Drive an UNRELATED, actually-pumped scheduler for many frames —
        // this is what a real event loop's realm-owned `UpdateScheduler` does
        // every frame. It must have zero effect on `c`, which is wired to
        // `private_scheduler` alone.
        let other_scheduler = UpdateScheduler::new();
        for _ in 0..120 {
            other_scheduler.execute_frame();
        }
        std::thread::sleep(Duration::from_millis(20));

        assert_eq!(
            c.value(),
            0.0,
            "a controller wired to a private, never-pumped UpdateScheduler must not \
             advance no matter how many frames an unrelated scheduler runs — \
             the ticker's callback is registered on `private_scheduler`'s own \
             transient queue, which nothing ever drains"
        );
        assert_eq!(
            private_scheduler.frame_count(),
            0,
            "precondition: the controller's own private scheduler was never pumped"
        );
        c.dispose();
    }

    #[test]
    fn creation_starts_dismissed_at_lower_bound() {
        let _serial = serial();
        let c = controller(100);
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    // ---- remaining-fraction duration scaling (Flutter `_animateToInternal`) ----

    #[test]
    fn reverse_mid_flight_keeps_full_range_velocity() {
        let _serial = serial();
        let c = controller(1000);
        c.forward().unwrap();
        c.tick_at(0.6);
        assert!((c.value() - 0.6).abs() < 1e-4);

        // reverse() restarts the ticker (elapsed re-zeroes) and the leg
        // covers 0.6 of the range in 0.6s — NOT the full second. Pre-fix
        // the lerp ran start->target over the full duration, so the same
        // distance took longer and velocity dropped at the turn.
        c.reverse().unwrap();
        c.tick_at(0.3);
        assert!(
            (c.value() - 0.3).abs() < 1e-3,
            "0.3s into the 0.6s reverse leg must sit at 0.3, got {}",
            c.value()
        );
        // Past the scaled leg's end (0.6s + float headroom) → dismissed.
        c.tick_at(0.7);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    #[test]
    fn forward_from_mid_scales_run_duration() {
        let _serial = serial();
        let c = controller(100);
        c.forward_from(Some(0.5)).unwrap();
        // Half the range remains -> 50ms run. 25ms in = halfway -> 0.75.
        c.tick_at(0.025);
        assert!(
            (c.value() - 0.75).abs() < 1e-3,
            "constant velocity from 0.5: got {}",
            c.value()
        );
        c.tick_at(0.05);
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    #[test]
    fn forward_at_upper_bound_settles_immediately() {
        let _serial = serial();
        let c = controller(100);
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&statuses);
        let _id = c.add_status_listener(Arc::new(move |s| s2.lock().push(s)));

        c.forward_from(Some(1.0)).unwrap();
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert!(!c.is_animating(), "no ticker run for a zero-distance leg");
        assert_eq!(
            statuses.lock().as_slice(),
            &[AnimationStatus::Completed],
            "settles with the final status only — no transient Forward",
        );
        c.dispose();
    }

    #[test]
    fn reverse_at_lower_bound_settles_immediately() {
        let _serial = serial();
        let c = controller(100);
        c.reverse().unwrap();
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        assert!(!c.is_animating());
        c.dispose();
    }

    #[test]
    fn explicit_animate_to_duration_is_not_scaled() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);
        // Explicit 100ms run from 0.5 -> 1.0: 50ms in = halfway -> 0.75.
        c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
        c.tick_at(0.05);
        assert!(
            (c.value() - 0.75).abs() < 1e-3,
            "an explicit per-run duration is used as-is, got {}",
            c.value()
        );
        c.dispose();
    }

    #[test]
    fn forward_sets_running_status() {
        let _serial = serial();
        let c = controller(100);
        c.forward().unwrap();
        assert_eq!(c.status(), AnimationStatus::Forward);
        c.dispose();
    }

    #[test]
    fn is_animating_is_ticker_based_not_status_based() {
        let _serial = serial();
        let c = controller(100);
        // Flutter `_internalSetValue` parity: an interior set_value reports a
        // directional status, but a stopped controller must not claim to be
        // animating (Flutter's isAnimating is ticker-based).
        c.set_value(0.5);
        assert_eq!(c.status(), AnimationStatus::Forward);
        assert!(!c.is_animating(), "stopped controller must not animate");

        c.forward().unwrap();
        assert!(c.is_animating(), "running controller must animate");

        c.stop().unwrap();
        assert!(!c.is_animating(), "stop() must end animating");
        c.dispose();
    }

    #[test]
    fn set_value_nan_is_canonicalized() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);
        c.set_value(f32::NAN);
        assert_eq!(
            c.value(),
            0.0,
            "NaN must canonicalize to the lower bound, not poison the value"
        );
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    #[test]
    fn reset_returns_to_lower_bound() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);
        assert_eq!(c.value(), 0.5);
        c.reset().unwrap();
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    #[test]
    fn custom_bounds_clamp() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c =
            AnimationController::with_bounds(Duration::from_millis(100), &scheduler, 10.0, 20.0)
                .unwrap();
        assert_eq!(c.value(), 10.0);
        c.set_value(15.0);
        assert_eq!(c.value(), 15.0);
        c.set_value(100.0);
        assert_eq!(c.value(), 20.0);
        c.dispose();
    }

    #[test]
    fn invalid_bounds_rejected() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let r =
            AnimationController::with_bounds(Duration::from_millis(100), &scheduler, 20.0, 10.0);
        assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    }

    /// `without_ticker` builds a controller with no scheduler attached at
    /// all — it must still advance via `tick_at` (the widget-layer `Vsync`
    /// driving path), exactly like a controller built against a private,
    /// never-pumped `UpdateScheduler`.
    #[test]
    fn without_ticker_advances_via_tick_at_only() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        assert_eq!(c.value(), 0.0);

        c.forward().unwrap();
        c.tick_at(0.05);
        assert!((c.value() - 0.5).abs() < 1e-4);

        c.dispose();
    }

    /// `without_ticker_bounds` is a BOUNDED constructor: bounded means
    /// finite (#1183). This test used to assert the OPPOSITE — that a
    /// wide-open `(NEG_INFINITY, INFINITY)` pair was ACCEPTED, landing
    /// `value() == NEG_INFINITY` — that assertion is deliberately inverted
    /// here, not preserved: unboundedness is now a constructor fact
    /// ([`AnimationController::unbounded_without_ticker`]), not a bound
    /// value, so the same wide-open pair this test used to accept must now
    /// be rejected, and the unbounded shape starts at `0.0`, never `-inf`.
    #[test]
    fn without_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones() {
        let _serial = serial();
        let rejected =
            AnimationController::without_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
        assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

        let wide_open = AnimationController::without_ticker_bounds(
            Duration::from_millis(1),
            f32::NEG_INFINITY,
            f32::INFINITY,
        );
        assert!(
            matches!(wide_open, Err(AnimationError::InvalidBounds(_))),
            "a wide-open pair is unboundedness spelled as bounds -- reject it; \
             use unbounded_without_ticker instead"
        );

        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        assert_eq!(
            c.value(),
            0.0,
            "unbounded_without_ticker starts at 0.0, never -inf"
        );
        c.dispose();
    }

    /// `with_detached_ticker` is the shape the throwaway-`UpdateScheduler` sites
    /// migrated to: unlike `without_ticker`, this controller has a REAL
    /// `Ticker`, so `is_animating()` reports `true` mid-run exactly as it
    /// would with a live (but never-pumped) scheduler attached — while still
    /// advancing only via `tick_at`, never on its own, because the ticker's
    /// scheduler is `None`.
    #[test]
    fn with_detached_ticker_reports_animating_and_advances_via_tick_at_only() {
        let _serial = serial();
        let c = AnimationController::with_detached_ticker(Duration::from_millis(100));
        assert_eq!(c.value(), 0.0);
        assert!(
            !c.is_animating(),
            "a freshly built, un-started controller must not report animating"
        );

        c.forward().unwrap();
        assert!(
            c.is_animating(),
            "a detached ticker must still report is_animating() == true once started -- \
             this is exactly the behavior without_ticker cannot provide"
        );
        c.tick_at(0.05);
        assert!((c.value() - 0.5).abs() < 1e-4);

        c.stop().unwrap();
        assert!(
            !c.is_animating(),
            "stop() must end animating for a detached ticker just as it does for a scheduled one"
        );
        c.dispose();
    }

    /// `with_detached_ticker_bounds` is the same shape with custom bounds --
    /// mirrors `without_ticker_bounds`'s coverage of bounds validation. See
    /// that test's own doc for why the wide-open-pair assertion below is a
    /// deliberate inversion of what this test used to check, not a
    /// preserved pin.
    #[test]
    fn with_detached_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones() {
        let _serial = serial();
        let rejected =
            AnimationController::with_detached_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
        assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

        let wide_open = AnimationController::with_detached_ticker_bounds(
            Duration::from_millis(1),
            f32::NEG_INFINITY,
            f32::INFINITY,
        );
        assert!(
            matches!(wide_open, Err(AnimationError::InvalidBounds(_))),
            "a wide-open pair is unboundedness spelled as bounds -- reject it; \
             use unbounded_with_detached_ticker instead"
        );

        let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1));
        assert_eq!(
            c.value(),
            0.0,
            "unbounded_with_detached_ticker starts at 0.0, never -inf"
        );
        c.dispose();
    }

    #[test]
    fn disposed_controller_rejects_forward() {
        let _serial = serial();
        let c = controller(100);
        c.dispose();
        assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
    }

    // ---- #1183: unboundedness is a constructor fact ----------------------

    /// Bounded means finite: every bounded constructor (and `builder.rs`'s
    /// `.bounds()`, tested separately in that module) must reject any bound
    /// that is `NaN`, infinite, or a half-open pair (one finite, one
    /// infinite) -- unboundedness is `unbounded*`, not a bound value.
    #[test]
    fn bounds_constructors_reject_non_finite_bounds() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let cases: &[(f32, f32)] = &[
            (f32::NAN, 1.0),
            (0.0, f32::NAN),
            (f32::NEG_INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, 5.0),
            (5.0, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
        ];
        for &(lower, upper) in cases {
            assert!(
                matches!(
                    AnimationController::with_bounds(
                        Duration::from_millis(1),
                        &scheduler,
                        lower,
                        upper
                    ),
                    Err(AnimationError::InvalidBounds(_))
                ),
                "with_bounds({lower}, {upper}) must be rejected"
            );
            assert!(
                matches!(
                    AnimationController::without_ticker_bounds(
                        Duration::from_millis(1),
                        lower,
                        upper
                    ),
                    Err(AnimationError::InvalidBounds(_))
                ),
                "without_ticker_bounds({lower}, {upper}) must be rejected"
            );
            assert!(
                matches!(
                    AnimationController::with_detached_ticker_bounds(
                        Duration::from_millis(1),
                        lower,
                        upper
                    ),
                    Err(AnimationError::InvalidBounds(_))
                ),
                "with_detached_ticker_bounds({lower}, {upper}) must be rejected"
            );
        }
    }

    /// Pin: a finite, merely non-default range must still be accepted and
    /// start at its own `lower_bound` -- the tightened validation rejects
    /// non-finite bounds, not merely-unusual finite ones.
    #[test]
    fn bounds_constructors_still_accept_a_finite_non_default_range() {
        let _serial = serial();
        let c = AnimationController::without_ticker_bounds(Duration::from_millis(1), -1.0, 3.0)
            .expect("finite (-1, 3) satisfies lower < upper");
        assert_eq!(c.value(), -1.0);
        c.dispose();
    }

    /// `unbounded_without_ticker`'s documented contract: `value() == 0.0`,
    /// initial status `Forward` (Flutter's `_internalSetValue` rule at
    /// `0.0`, direction defaulted `Forward` -- see the mapping entry for the
    /// recorded cost), zero velocity, and not animating until driven.
    #[test]
    fn unbounded_without_ticker_starts_at_zero_forward_and_idle() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Forward);
        assert_eq!(c.velocity(), 0.0);
        assert!(!c.is_animating());
        c.dispose();
    }

    /// `set_value` at a non-bound value on an unbounded controller keeps the
    /// keep-direction status (`Forward`, unchanged from construction) and
    /// must fire no status listener -- status equality alone is vacuous
    /// under `take_status_change`'s dedup, so this counts callbacks (v3
    /// delta 5).
    #[test]
    fn unbounded_set_value_at_a_non_bound_value_fires_no_status_change() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        let status_changes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&status_changes);
        let _id = c.add_status_listener(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        c.set_value(123.0);
        assert_eq!(c.status(), AnimationStatus::Forward);
        assert_eq!(
            status_changes.load(Ordering::SeqCst),
            0,
            "status was already Forward at construction (v3 delta 5's last_reported_status fix); \
             set_value at a non-bound value must not re-fire it"
        );
        c.dispose();
    }

    /// v3 delta 7: `set_value` with a non-finite input on an UNBOUNDED
    /// controller is a FULL no-op -- not merely "unchanged value" but no
    /// `stop_running` and no notification either: a poisoned drag's
    /// `set_value(NaN)` must not cancel a live fling out from under it.
    /// Pinned alongside the BOUNDED canonicalization (`NaN` -> lower bound,
    /// `+inf` -> upper bound) it does not disturb, and the shared latch
    /// (`non_finite_warned`) firing once across three non-finite calls.
    #[test]
    fn set_value_non_finite_is_a_full_no_op_on_unbounded_but_canonicalizes_on_bounded() {
        let _serial = serial();

        // Bounded: pin -- `+inf` clamps to the upper bound (mirrors the
        // existing `set_value_nan_is_canonicalized` pin for `NaN`).
        let c = controller(100);
        c.set_value(f32::INFINITY);
        assert_eq!(
            c.value(),
            1.0,
            "set_value(+inf) on a bounded controller clamps to upper"
        );
        c.dispose();

        // Unbounded: a live run must survive set_value(NaN) untouched --
        // the run is still installed and still ticking afterward.
        let c = AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));
        c.animate_to(500.0, None).unwrap();
        assert!(c.is_animating(), "precondition: a live run is installed");
        let generation_before = c.run_generation();

        c.set_value(f32::NAN);
        assert_eq!(
            c.value(),
            0.0,
            "set_value(NaN) on an unbounded controller must not move value"
        );
        assert!(
            c.is_animating(),
            "set_value(NaN) on an unbounded controller must not stop the live run"
        );
        assert_eq!(c.run_generation(), generation_before);

        c.set_value(f32::INFINITY);
        assert_eq!(
            c.value(),
            0.0,
            "set_value(+inf) on an unbounded controller must not move value"
        );
        c.set_value(f32::NEG_INFINITY);
        assert_eq!(
            c.value(),
            0.0,
            "set_value(-inf) on an unbounded controller must not move value"
        );
        assert!(
            c.is_animating(),
            "none of the three non-finite calls may have stopped the run"
        );
        c.dispose();
    }

    /// Decision 2: every bound-targeting run start is refused on an
    /// unbounded controller (there is no finite bound to run to), and the
    /// refusal is a NO-OP in every observable respect -- value, status,
    /// direction (probed via `stop()`'s reported status against a fresh
    /// twin that never received the refused call), `run_generation`,
    /// `is_animating`, and both listener kinds, plus a concurrently live
    /// `animate_to` run's own future staying pending throughout.
    #[test]
    fn unbounded_refusals_leave_the_controller_completely_untouched() {
        let _serial = serial();

        type Attempt = Box<dyn Fn(&AnimationController) -> Result<TickerFuture, AnimationError>>;
        let attempts: Vec<(&str, Attempt)> = vec![
            ("forward()", Box::new(AnimationController::forward)),
            (
                "forward_from(Some(0.5))",
                Box::new(|c: &AnimationController| c.forward_from(Some(0.5))),
            ),
            ("reverse()", Box::new(AnimationController::reverse)),
            (
                "fling(1.0)",
                Box::new(|c: &AnimationController| c.fling(1.0)),
            ),
            (
                "fling(-1.0)",
                Box::new(|c: &AnimationController| c.fling(-1.0)),
            ),
            (
                "repeat(false)",
                Box::new(|c: &AnimationController| c.repeat(false)),
            ),
            (
                "repeat_with(None, Some(1.0), ..)",
                Box::new(|c: &AnimationController| {
                    c.repeat_with(None, Some(1.0), false, None, None)
                }),
            ),
        ];

        for (name, attempt) in attempts {
            let c =
                AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));
            let twin =
                AnimationController::unbounded_with_detached_ticker(Duration::from_millis(1000));

            // An identical live run on both: a corrupted `direction` on `c`
            // (the `fling_with` ordering bug this plan also fixes) shows up
            // as a divergent `stop()` status against `twin`, which never
            // sees the refused call.
            let live = c.animate_to(200.0, None).unwrap();
            twin.animate_to(200.0, None).unwrap();
            assert!(c.is_animating(), "precondition: a live run is installed");

            let value_hits = Arc::new(AtomicUsize::new(0));
            let vh = Arc::clone(&value_hits);
            let _vid = c.add_listener(Arc::new(move || {
                vh.fetch_add(1, Ordering::SeqCst);
            }));
            let status_hits = Arc::new(AtomicUsize::new(0));
            let sh = Arc::clone(&status_hits);
            let _sid = c.add_status_listener(Arc::new(move |_| {
                sh.fetch_add(1, Ordering::SeqCst);
            }));

            let value_before = c.value();
            let status_before = c.status();
            let generation_before = c.run_generation();

            let result = attempt(&c);
            assert!(
                matches!(result, Err(AnimationError::NonFiniteTarget(_))),
                "{name} must be refused with NonFiniteTarget on an unbounded controller, got {result:?}"
            );

            assert_eq!(c.value(), value_before, "{name}: value must be untouched");
            assert_eq!(
                c.status(),
                status_before,
                "{name}: status must be untouched"
            );
            assert_eq!(
                c.run_generation(),
                generation_before,
                "{name}: run_generation must be untouched"
            );
            assert!(
                c.is_animating(),
                "{name}: the live run must still be installed and ticking"
            );
            assert_eq!(
                value_hits.load(Ordering::SeqCst),
                0,
                "{name}: a refused call must fire no value listener"
            );
            assert_eq!(
                status_hits.load(Ordering::SeqCst),
                0,
                "{name}: a refused call must fire no status listener"
            );
            assert!(
                live.is_pending(),
                "{name}: the live animate_to(200.0) run's future must still be pending"
            );

            c.stop().unwrap();
            twin.stop().unwrap();
            assert_eq!(
                c.status(),
                twin.status(),
                "{name}: a refused call must not corrupt direction -- stop() on the refused \
                 controller must report the same status a twin that never saw it reports"
            );
        }
    }

    /// On a BOUNDED controller, a non-finite `target`/`from` is refused too
    /// (Decision 2's declared change: today `clamp` passes `NaN` straight
    /// through) -- nothing is mutated by the refusal.
    #[test]
    fn bounded_controller_refuses_non_finite_target_and_from() {
        let _serial = serial();

        let c = controller(100);
        assert!(matches!(
            c.animate_to(f32::NAN, None),
            Err(AnimationError::NonFiniteTarget(_))
        ));
        assert_eq!(
            c.value(),
            0.0,
            "a refused animate_to(NaN) must not move value"
        );

        let c = controller(100);
        assert!(matches!(
            c.forward_from(Some(f32::NAN)),
            Err(AnimationError::NonFiniteTarget(_))
        ));
        assert_eq!(c.value(), 0.0);

        let c = controller(100);
        c.set_value(0.5);
        assert!(matches!(
            c.reverse_from(Some(f32::NAN)),
            Err(AnimationError::NonFiniteTarget(_))
        ));
        assert_eq!(
            c.value(),
            0.5,
            "a refused reverse_from(NaN) must not move value"
        );
    }

    /// v3 delta 2: `+-inf` still CLAMPS on a bounded controller (today's
    /// "go to the end" idiom) -- only a bound-less direction refuses.
    #[test]
    fn bounded_controller_clamps_infinite_target_and_from_to_the_pointed_at_bound() {
        let _serial = serial();

        // `target` clamps to the finite upper bound and the run proceeds
        // normally from there (clamping the target does not mean an
        // instant settle: `value` still starts at the entry value and
        // reaches `1.0` only once the run completes).
        let c = controller(100);
        c.animate_to(f32::INFINITY, None).unwrap();
        c.tick_at(0.1);
        assert_eq!(
            c.value(),
            1.0,
            "animate_to(+inf) clamps to the finite upper bound"
        );
        c.dispose();

        // `from` clamping to the upper bound makes `forward_from`'s own
        // target (also `upper_bound`) already reached -- this settles
        // SYNCHRONOUSLY (zero distance), unlike the `animate_to` case above.
        let c = controller(100);
        c.forward_from(Some(f32::INFINITY)).unwrap();
        assert_eq!(
            c.value(),
            1.0,
            "forward_from(Some(+inf)) clamps to the finite upper bound"
        );
        c.dispose();
    }

    /// On unbounded: `animate_to`/`animate_with`/`repeat_with` with an
    /// explicit finite range all still run (pin) -- the refusal is about
    /// bound-*targeting*, not about the controller being unbounded per se.
    #[test]
    fn unbounded_controller_runs_finite_targeted_operations() {
        let _serial = serial();

        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        c.animate_to(200.0, None).unwrap();
        c.tick_at(0.1);
        assert_eq!(
            c.value(),
            200.0,
            "animate_to still runs on an unbounded controller"
        );
        c.dispose();

        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 1.0);
        let sim = SpringSimulation::new(spring, 0.0, 50.0, 10.0);
        c.animate_with(sim).unwrap();
        assert!(
            c.value().is_finite(),
            "animate_with still runs on an unbounded controller"
        );
        c.dispose();

        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        c.repeat_with(Some(0.0), Some(1.0), false, None, None)
            .expect("an explicit finite range must run on an unbounded controller");
        c.dispose();
    }

    /// v3 delta 2's own wording: "on an unbounded one all three are
    /// refused" -- `animate_to` with an INFINITE `target` on an unbounded
    /// controller must be refused exactly like a `NaN` one (both clamp to a
    /// non-finite result, since neither bound is finite). Unlike
    /// `forward`/`reverse` (blanket-refused on unbounded regardless of
    /// their argument -- see the battery test above), `animate_to` is only
    /// refused when `target` ITSELF is non-finite, so this is the
    /// distinguishing case for that half of the rule.
    #[test]
    fn unbounded_controller_refuses_an_infinite_animate_to_target_too() {
        let _serial = serial();

        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        assert!(matches!(
            c.animate_to(f32::INFINITY, None),
            Err(AnimationError::NonFiniteTarget(_))
        ));
        assert_eq!(c.value(), 0.0, "a refused animate_to must not move value");
    }

    /// Decision 1/flutter#76014: `reset()` on an unbounded controller lands
    /// on `0.0` (the defined beginning), never `-inf` -- and never fails for
    /// non-finiteness.
    #[test]
    fn unbounded_reset_lands_on_zero_not_negative_infinity() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        c.set_value(500.0);
        c.reset().unwrap();
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    /// v3 delta 6: the FIRST `stop()` on a fresh, never-run unbounded
    /// controller reports `Completed` -- direction defaults `Forward`, and
    /// `settled_status_directed` falls to the direction-only branch because
    /// neither infinite bound is ever "at". Every `jump_to` on a fresh
    /// `Scrollable` triggers exactly this event through the stop hook.
    #[test]
    fn unbounded_first_stop_on_a_fresh_controller_reports_completed() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        c.stop().unwrap();
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    // ---- #1183: repeat_with's effective-range check ------------------------

    /// v3 delta 3: a caller-supplied NaN endpoint is an any-controller
    /// range-shape error -- red today: the OLD `lo >= hi` check does not
    /// catch NaN (`NaN >= hi` is `false`), so `repeat_with(Some(NaN), ..)`
    /// installed a NaN-poisoned run.
    #[test]
    fn repeat_with_rejects_a_nan_endpoint_on_a_bounded_controller() {
        let _serial = serial();
        let c = controller(100);
        let r = c.repeat_with(Some(f32::NAN), Some(1.0), false, None, None);
        assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
        assert_eq!(c.value(), 0.0, "a refused repeat_with must not move value");
    }

    /// Decision 2: `repeat`/`repeat_with` whose EFFECTIVE range defaults
    /// through an unbounded controller's own infinite bound is refused with
    /// `NonFiniteTarget`, distinct from `InvalidBounds`'s range-SHAPE errors
    /// above -- a partially-specified range (`Some(0.0), None`) hits the
    /// same refusal on the still-infinite side.
    #[test]
    fn repeat_with_refuses_a_non_finite_effective_range_on_unbounded() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(1));
        let r = c.repeat_with(Some(0.0), None, false, None, None);
        assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
        assert_eq!(c.value(), 0.0);
    }

    // ---- #1183: span overflow -----------------------------------------------

    /// v3 delta 4: a `target - value` span overflowing `f32` is refused at
    /// the call rather than let `tick_time_based` interpolate an infinite
    /// range -- red today: `set_value(-f32::MAX)` then `animate_to(f32::MAX)`
    /// installs a run whose `range` is `+inf`.
    #[test]
    fn animate_to_refuses_a_span_that_overflows_f32() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        c.set_value(-f32::MAX);
        let r = c.animate_to(f32::MAX, None);
        assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
        assert_eq!(
            c.value(),
            -f32::MAX,
            "a refused animate_to must not move value"
        );
        c.dispose();
    }

    // ---- #1183: fling_with's ordering and non-finite refusals ---------------

    /// v3 delta 1: `fling_with` refuses a non-finite `velocity` before ANY
    /// mutation -- red today: `NaN < 0.0` is `false`, so a NaN velocity took
    /// the Forward branch and built a spring whose `SpringSimulation` never
    /// reaches `is_done` for a NaN target.
    #[test]
    fn fling_refuses_a_non_finite_velocity() {
        let _serial = serial();
        let c = controller(100);
        let r = c.fling(f32::NAN);
        assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
        assert!(!c.is_animating());
    }

    /// Red today (the plan's own repro): `fling_with` wrote `inner.direction`
    /// BEFORE its `InvalidSpring` check, so a refused fling still corrupted
    /// direction, observable only via a LATER `stop()` (an interior value
    /// keeps the running status via direction, not a bound). Fixed by
    /// computing `direction` into a local and assigning it only after every
    /// check passes.
    #[test]
    fn fling_with_refused_for_invalid_spring_leaves_direction_untouched() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5); // interior value: settled_status_directed falls to direction
        let underdamped = SpringDescription::with_damping_ratio(1.0, 500.0, 0.5);

        let r = c.fling_with(-1.0, Some(underdamped));
        assert!(matches!(r, Err(AnimationError::InvalidSpring(_))));

        c.stop().unwrap();
        assert_eq!(
            c.status(),
            AnimationStatus::Completed,
            "direction must still be the controller's original Forward -- a corrupted \
             Reverse would report Dismissed instead"
        );
    }

    // ---- #1183: no path reads NaN -- simulation and tick endpoints ---------

    /// A scratch [`Simulation`] whose `x()` is finite at `t = 0` (so
    /// `drive_simulation` accepts it) but returns NaN once mid-run.
    struct GoesNanMidRun {
        nan_at: f32,
    }

    impl Simulation for GoesNanMidRun {
        fn x(&self, time: f32) -> f32 {
            if time >= self.nan_at { f32::NAN } else { time }
        }
        fn dx(&self, _time: f32) -> f32 {
            1.0
        }
        fn is_done(&self, _time: f32) -> bool {
            false
        }
        fn tolerance(&self) -> Tolerance {
            Tolerance::DEFAULT
        }
    }

    /// v3 delta 1: a mid-run non-finite sample ENDS the run at the last
    /// finite value -- settled status by direction, the future resolves
    /// `Ok`, the ticker stops, and the run no longer holds `Vsync` open
    /// (checked via a real registration's `has_running()`). NOT
    /// "value unchanged, run continues": that would leave `active_run`
    /// installed forever.
    #[test]
    fn a_simulation_that_turns_non_finite_mid_run_ends_the_run_at_the_last_finite_value() {
        let _serial = serial();
        let c = AnimationController::unbounded_without_ticker(Duration::from_millis(100));
        let vsync = crate::vsync::Vsync::new();
        let _reg = vsync.register(c.clone());

        let mut future = c.animate_with(GoesNanMidRun { nan_at: 1.0 }).unwrap();
        // `Vsync` anchors a run's `t = 0` on the FIRST `tick_all` it sees
        // after the run starts (registration alone reads no clock) -- so
        // the first call establishes the anchor at elapsed 0, exactly like
        // a real frame loop's first pumped frame after `animate_with`.
        vsync.tick_all(0.0);
        vsync.tick_all(0.5);
        assert_eq!(
            c.value(),
            0.5,
            "precondition: the run is progressing normally"
        );
        assert!(vsync.has_running());

        vsync.tick_all(1.0); // sim.x(1.0) is NaN
        assert_eq!(
            c.value(),
            0.5,
            "the run must end AT THE LAST FINITE VALUE, not read the NaN sample through"
        );
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert!(
            !vsync.has_running(),
            "the run must not hold the frame loop open forever"
        );

        use std::future::Future;
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        assert_eq!(
            std::pin::Pin::new(&mut future).poll(&mut cx),
            std::task::Poll::Ready(Ok(())),
            "the future must resolve Ok -- the run ended on its own terms, not by cancellation"
        );
        c.dispose();
    }

    /// v3 delta 1: `drive_simulation` refuses a simulation whose FIRST
    /// sample (`x(0.0)`) is already non-finite, before any mutation.
    #[test]
    fn drive_simulation_refuses_a_non_finite_initial_sample() {
        let _serial = serial();
        let c = controller(100);
        let r = c.animate_with(GoesNanMidRun { nan_at: 0.0 });
        assert!(matches!(r, Err(AnimationError::NonFiniteTarget(_))));
        assert_eq!(c.value(), 0.0);
    }

    /// Decision 4: `tick_time_based` reads `start_value`/`target_value`
    /// directly at the exact endpoints rather than computing
    /// `start + range * eased_t` there -- a curve that is not the identity
    /// at its own endpoints (this uses a curve whose `transform` shifts
    /// every input) must still land EXACTLY on `start_value` at `t == 0`,
    /// matching Flutter's `_InterpolationSimulation.x` parity.
    #[test]
    fn tick_time_based_reads_start_value_exactly_at_t_zero() {
        let _serial = serial();
        let c = AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0)
            .unwrap();
        c.set_value(-1.0);
        c.animate_to_curved(3.0, None, Arc::new(NotIdentityAtEndpoints))
            .unwrap();
        c.tick_at(0.0);
        assert_eq!(
            c.value(),
            -1.0,
            "t == 0 must read start_value exactly, regardless of the curve"
        );
    }

    /// A curve whose `transform` is NOT the identity at `0.0`/`1.0` --
    /// `tick_time_based`'s endpoint special-case must still land exactly on
    /// `start_value`/`target_value` there despite this.
    #[derive(Debug)]
    struct NotIdentityAtEndpoints;

    impl Curve for NotIdentityAtEndpoints {
        fn transform(&self, t: f32) -> f32 {
            0.1 + t * 0.8
        }
    }

    // ---- run-generation: bumps per run-start, stable across ticks ----

    #[test]
    fn run_generation_bumps_per_run_not_per_tick() {
        let _serial = serial();
        let c = controller(100);
        let g0 = c.run_generation();

        c.forward().unwrap();
        let g1 = c.run_generation();
        assert_eq!(g1, g0 + 1, "forward() establishes a new run");

        // Ticking advances the SAME run — the generation must not move, so the
        // binding does not spuriously re-anchor mid-run.
        c.tick_at(0.02);
        c.tick_at(0.05);
        assert_eq!(c.run_generation(), g1, "tick_at must not start a new run");

        c.reverse().unwrap();
        assert_eq!(
            c.run_generation(),
            g1 + 1,
            "reverse() establishes a new run"
        );

        // A settle-only path (reset) does NOT bump — the controller is not
        // running afterwards, so there is no run to anchor.
        c.reset().unwrap();
        assert_eq!(c.run_generation(), g1 + 1, "reset() does not start a run");

        c.animate_to(1.0, Some(Duration::from_millis(50))).unwrap();
        assert_eq!(c.run_generation(), g1 + 2, "animate_to starts a new run");

        c.reset().unwrap();
        c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
            .unwrap();
        assert_eq!(c.run_generation(), g1 + 3, "repeat starts a new run");

        c.fling(1.0).unwrap();
        assert_eq!(c.run_generation(), g1 + 4, "fling starts a new run");

        c.dispose();
    }

    // ---- B1: animate_to actually advances + does not clobber base duration ----

    #[test]
    fn animate_to_advances_value_across_ticks() {
        let _serial = serial();
        let c = controller(100); // base 100ms
        c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
        assert_eq!(c.value(), 0.0);
        c.tick_at(0.05); // 50ms of 100ms -> ~0.5
        assert!((c.value() - 0.5).abs() < 1e-3, "value={}", c.value());
        c.tick_at(0.10); // 100ms -> complete
        assert_eq!(c.value(), 1.0);
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    #[test]
    fn animate_to_does_not_clobber_base_duration() {
        let _serial = serial();
        let c = controller(100); // base 100ms
        c.animate_to(1.0, Some(Duration::from_millis(20))).unwrap();
        c.tick_at(0.02); // completes the 20ms run
        assert_eq!(c.value(), 1.0);
        // Base duration must be intact: a fresh forward run still takes 100ms.
        c.reset().unwrap();
        c.forward().unwrap();
        c.tick_at(0.05); // 50ms of the BASE 100ms -> ~0.5, not already complete
        assert!(
            (c.value() - 0.5).abs() < 1e-3,
            "base duration was clobbered: value={}",
            c.value()
        );
        c.dispose();
    }

    // ---- B1c: status listener may re-enter the controller without deadlock ----

    #[test]
    fn status_callback_can_reenter_controller_without_deadlock() {
        let _serial = serial();
        let c = controller(100);
        let reentered = Arc::new(AtomicUsize::new(0));
        let c2 = c.clone();
        let r2 = Arc::clone(&reentered);
        c.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                // Re-enter: read + mutate the controller from within the status
                // callback. Under the old notify-under-lock code this deadlocked.
                let _ = c2.value();
                let _ = c2.reverse();
                r2.fetch_add(1, Ordering::SeqCst);
            }
        }));
        c.forward().unwrap();
        c.tick_at(0.10); // complete -> fires Completed -> callback re-enters
        assert_eq!(reentered.load(Ordering::SeqCst), 1);
        c.dispose();
    }

    // ---- value listeners fire on tick (regression for the dead ticker) ----

    #[test]
    fn value_listeners_fire_on_tick() {
        let _serial = serial();
        let c = controller(100);
        let ticks = Arc::new(AtomicUsize::new(0));
        let t2 = Arc::clone(&ticks);
        c.add_listener(Arc::new(move || {
            t2.fetch_add(1, Ordering::SeqCst);
        }));
        c.forward().unwrap();
        c.tick_at(0.05);
        c.tick_at(0.08);
        assert!(ticks.load(Ordering::SeqCst) >= 2);
        c.dispose();
    }

    /// `forward_from(Some(x))` jumps `value` to `x` before starting a REAL
    /// (non-settling) run — that jump must notify exactly once, at the
    /// call, same as Flutter's `forward(from:)` going through the `value=`
    /// setter. Plain `forward()` (no `from`) does not jump the value at all,
    /// so it must not notify at the call — only later ticks do.
    ///
    /// Red-check: pass `ValueChange::Unchanged` unconditionally at
    /// `forward_from`'s real-run `finish` call (its pre-fix shape) — the
    /// first assertion reads `0`, not `1`.
    #[test]
    fn forward_from_notifies_once_at_the_call_iff_from_moved_the_value() {
        let _serial = serial();
        let c = controller(100);
        let fires = Arc::new(AtomicUsize::new(0));
        let f2 = Arc::clone(&fires);
        c.add_listener(Arc::new(move || {
            f2.fetch_add(1, Ordering::SeqCst);
        }));

        c.forward_from(Some(0.5)).unwrap();
        assert_eq!(
            fires.load(Ordering::SeqCst),
            1,
            "the from-jump (0.0 -> 0.5) must notify exactly once at the call"
        );
        assert!((c.value() - 0.5).abs() < 1e-6);
        c.dispose();

        let c2 = controller(100);
        let fires2 = Arc::new(AtomicUsize::new(0));
        let f2b = Arc::clone(&fires2);
        c2.add_listener(Arc::new(move || {
            f2b.fetch_add(1, Ordering::SeqCst);
        }));
        c2.forward().unwrap();
        assert_eq!(
            fires2.load(Ordering::SeqCst),
            0,
            "forward() with no `from` does not jump the value, so it must not \
             notify at the call — only a later tick does"
        );
        c2.dispose();
    }

    // ---- repeat with a finite count stops + completes ----

    #[test]
    fn repeat_with_finite_count_stops() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
            .unwrap();
        assert_eq!(c.status(), AnimationStatus::Forward);
        c.tick_at(0.010); // cycle 1 boundary -> restart
        assert_eq!(c.status(), AnimationStatus::Forward);
        c.tick_at(0.020); // cycle 2 boundary -> count reached -> stop
        assert_eq!(c.status(), AnimationStatus::Completed);
        // Further ticks do not advance a stopped controller.
        let v = c.value();
        c.tick_at(0.030);
        assert_eq!(c.value(), v);
        c.dispose();
    }

    #[test]
    fn repeat_consumes_all_cycles_in_one_long_frame() {
        let _serial = serial();
        let c = controller(100);
        // count = 4, period = 10ms. A single 45ms frame (a dropped-frame
        // catch-up) spans 4 whole cycles, so the repeat must already be
        // exhausted — not still Forward as the old one-cycle-per-tick path left
        // it after the first boundary.
        c.repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(4))
            .unwrap();
        assert_eq!(c.status(), AnimationStatus::Forward);
        c.tick_at(0.045); // 4.5 cycles elapsed in one frame
        assert_eq!(
            c.status(),
            AnimationStatus::Completed,
            "all four cycles retired in one long frame -> exhausted"
        );
        c.dispose();
    }

    /// A bounce repeat over a CUSTOM interior range (not the controller's
    /// true bounds) must exhaust on the FINAL retired leg's direction, not
    /// whichever leg was active when the exhausting tick began.
    /// `settled_status` has no bound check, so the old bounds-first fallback
    /// that used to mask a stale `inner.direction` here no longer does.
    ///
    /// `repeat_with(0.2, 0.8, reverse: true, period: 100ms, count: 2)`
    /// starts at `0.2`, direction `Forward` (leg 1: `0.2 -> 0.8`). A single
    /// 250ms tick spans both retired cycles at once (leg 1 forward, leg 2
    /// reverse), landing on leg 2's endpoint `0.2` — so the run's direction
    /// at exhaustion is `Reverse`, ending `Dismissed`.
    ///
    /// Red-check: make `repeat_landing` ignore parity (e.g. always return
    /// the `Forward` leg's `max` regardless of `index`) — status reads
    /// `Completed`, not `Dismissed`.
    #[test]
    fn bounce_repeat_over_an_interior_range_exhausts_on_the_final_legs_status() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(
            Some(0.2),
            Some(0.8),
            true,
            Some(Duration::from_millis(100)),
            Some(2),
        )
        .unwrap();
        assert_eq!(
            c.status(),
            AnimationStatus::Forward,
            "sanity: leg 1 starts forward"
        );

        c.tick_at(0.25);

        assert!(
            (c.value() - 0.2).abs() < 1e-6,
            "value should land on leg 2's endpoint (repeat_min), got {}",
            c.value()
        );
        assert_eq!(
            c.status(),
            AnimationStatus::Dismissed,
            "the exhausting leg (leg 2) ran Reverse, so the run must end Dismissed"
        );
        c.dispose();
    }

    #[test]
    fn animate_to_current_value_settles_immediately() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);
        // Animating to the value we are already at must settle at once instead
        // of running the ticker for `duration` re-notifying an unchanged value.
        c.animate_to(0.5, Some(Duration::from_millis(100))).unwrap();
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
        c.dispose();
    }

    #[test]
    fn repeat_with_rejects_inverted_range() {
        let _serial = serial();
        let c = controller(100);
        // min >= max (within bounds) is rejected like `with_bounds` does.
        let r = c.repeat_with(Some(0.8), Some(0.2), false, None, None);
        assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
        c.dispose();
    }

    /// The `min == max` equality case, distinct from `repeat_with_rejects_inverted_range`'s
    /// `min > max` — Flutter permits this degenerate range (its own dropped
    /// `min: 1.0, max: 1.0` oracle sub-case, `animation_controller_test.dart`
    /// "calling repeat with specified min and max values" @ 3.44.0); FLUI
    /// rejects it, the mapping entry's rationale.
    #[test]
    fn repeat_with_rejects_equal_min_and_max() {
        let _serial = serial();
        let c = controller(100);
        let r = c.repeat_with(Some(0.5), Some(0.5), false, None, None);
        assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
        c.dispose();
    }

    #[test]
    fn repeat_with_clamps_range_into_bounds() {
        let _serial = serial();
        let c = controller(100);
        // Out-of-bounds min/max are clamped into [0, 1]; the run never
        // leaves the controller bounds.
        c.repeat_with(
            Some(-5.0),
            Some(5.0),
            false,
            Some(Duration::from_millis(10)),
            None,
        )
        .unwrap();
        // The run starts at the CURRENT value (0.0, this controller's
        // default) clamped into the range — not "at the clamped min",
        // which only coincides here because the current value already is
        // the lower bound.
        assert_eq!(
            c.value(),
            0.0,
            "current value 0.0 clamped into [0, 1] is still 0.0"
        );
        c.tick_at(0.005); // mid-cycle
        assert!(
            c.value() >= 0.0 && c.value() <= 1.0,
            "stays within bounds: {}",
            c.value()
        );
        c.dispose();
    }

    // ---- repeat sampling is a pure function of elapsed time (#1078) ----

    /// The issue's own reproduction: a repeating run's value/status at any
    /// `tick_at(t)` depends only on the elapsed time since the run started,
    /// never on how many intervening ticks partitioned the way there —
    /// `tick_at(1.25)` must equal `tick_at(1.0); tick_at(1.25)`, for both
    /// restart and bounce.
    #[test]
    fn repeat_value_is_partition_invariant_across_a_skipped_cycle() {
        let _serial = serial();

        // Restart mode: skip cycle 0's boundary tick entirely.
        let direct = AnimationController::without_ticker(Duration::from_secs(1));
        direct
            .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
            .unwrap();
        direct.tick_at(1.25);
        let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
        partitioned
            .repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
            .unwrap();
        partitioned.tick_at(1.0);
        partitioned.tick_at(1.25);
        assert!(
            (direct.value() - 0.25).abs() < 1e-6,
            "value={}",
            direct.value()
        );
        assert_eq!(direct.value(), partitioned.value());
        assert_eq!(direct.status(), partitioned.status());
        direct.dispose();
        partitioned.dispose();

        // Bounce mode: same elapsed time, opposite leg (cycle index 1 is
        // the reverse leg).
        let direct = AnimationController::without_ticker(Duration::from_secs(1));
        direct
            .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        direct.tick_at(1.25);
        let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
        partitioned
            .repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        partitioned.tick_at(1.0);
        partitioned.tick_at(1.25);
        assert!(
            (direct.value() - 0.75).abs() < 1e-6,
            "value={}",
            direct.value()
        );
        assert_eq!(direct.value(), partitioned.value());
        assert_eq!(direct.status(), partitioned.status());
        direct.dispose();
        partitioned.dispose();
    }

    /// Partition invariance holds for any number of skipped cycles (odd and
    /// even), and over a custom `min`/`max` range, not only the
    /// controller's own bounds.
    #[test]
    fn repeat_value_is_partition_invariant_over_custom_bounds_and_multiple_skipped_cycles() {
        let _serial = serial();
        for &t in &[1.25_f64, 2.25, 3.25] {
            let direct = AnimationController::without_ticker(Duration::from_secs(1));
            direct
                .repeat_with(
                    Some(0.2),
                    Some(0.8),
                    true,
                    Some(Duration::from_secs(1)),
                    None,
                )
                .unwrap();
            direct.tick_at(t);

            let partitioned = AnimationController::without_ticker(Duration::from_secs(1));
            partitioned
                .repeat_with(
                    Some(0.2),
                    Some(0.8),
                    true,
                    Some(Duration::from_secs(1)),
                    None,
                )
                .unwrap();
            let mut elapsed = 0.0_f64;
            while elapsed < t {
                elapsed = (elapsed + 1.0).min(t);
                partitioned.tick_at(elapsed);
            }

            assert!(
                (direct.value() - partitioned.value()).abs() < 1e-6,
                "t={t}: direct={} partitioned={}",
                direct.value(),
                partitioned.value()
            );
            assert_eq!(direct.status(), partitioned.status(), "t={t}");
            direct.dispose();
            partitioned.dispose();
        }
    }

    /// Sampling the same elapsed time twice is idempotent.
    #[test]
    fn repeat_tick_at_the_same_time_twice_is_idempotent() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_secs(1));
        c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        c.tick_at(2.25);
        let (value, status) = (c.value(), c.status());
        c.tick_at(2.25);
        assert_eq!(c.value(), value);
        assert_eq!(c.status(), status);
        c.dispose();
    }

    /// The at-call value for a RESTART repeat starting exactly at `max`
    /// must be the pure function sampled at elapsed time zero (the phase
    /// wraps to `min`, exactly Flutter's `_startSimulation` setting
    /// `_value = x(0.0)`), not the bare clamped current value. Storing `v`
    /// directly contradicted the model, its own comment, AND Flutter: it
    /// read `1.0` at the call, then `tick_at(0.0)` — the very first tick,
    /// no time elapsed — recomputed via the sampler and got `0.0`, a
    /// one-frame discontinuity the whole change exists to remove. The tick
    /// still notifies value listeners (every tick does, as in Flutter's
    /// `_tick`); what it must not do is move the value.
    #[test]
    fn repeat_restart_at_call_value_from_max_has_no_discontinuity_at_the_first_tick() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(1.0);
        c.repeat(false).unwrap();
        assert!(
            (c.value() - 0.0).abs() < 1e-6,
            "the phase wraps at the call itself: value={}",
            c.value()
        );
        assert_eq!(c.status(), AnimationStatus::Forward);

        let value_fires = Arc::new(AtomicUsize::new(0));
        let vf = Arc::clone(&value_fires);
        c.add_listener(Arc::new(move || {
            vf.fetch_add(1, Ordering::SeqCst);
        }));
        c.tick_at(0.0);
        assert!(
            (c.value() - 0.0).abs() < 1e-6,
            "no discontinuity: the first tick must agree with the at-call value"
        );
        assert_eq!(
            value_fires.load(Ordering::SeqCst),
            1,
            "a tick is a frame: value listeners fire once per tick even when the sample repeats"
        );
        c.dispose();
    }

    /// The value/status at the call are the pure function sampled at
    /// elapsed time zero, computed BEFORE any tick — a bounce starting
    /// exactly at `max` reports the reverse leg immediately (unaffected by
    /// the restart-mode fix above: bounce mode's phase wrap lands back on
    /// `max`, not `min`). Flutter parity: `_startSimulation` runs `x(0.0)`
    /// (which flips direction) before `_status` is computed
    /// (`AnimationController._startSimulation` @ 3.44.0).
    #[test]
    fn repeat_bounce_from_max_reports_reverse_status_at_the_call() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(1.0);
        c.repeat(true).unwrap();
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        assert_eq!(c.status(), AnimationStatus::Reverse);
        c.dispose();
    }

    /// Flutter oracle: `animation_controller_test.dart` "calling repeat
    /// with reverse set to true makes the animation alternate between
    /// lowerBound and upperBound values on each repeat" (@ 3.44.0) — the
    /// two sub-cases that start from a boundary/interior value (the
    /// `value == 0.0` sub-case is a restart-equivalent already covered by
    /// `repeat_value_is_partition_invariant_across_a_skipped_cycle`).
    #[test]
    fn repeat_bounce_flutter_oracle_reverse_from_max_and_mid() {
        let _serial = serial();

        // value == max at the call reports the reverse leg immediately.
        let c = controller(100);
        c.set_value(1.0);
        c.repeat(true).unwrap();
        c.tick_at(0.025);
        assert!((c.value() - 0.75).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.125);
        assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
        c.dispose();

        // value == 0.5 (mid-range) at the call.
        let c = controller(100);
        c.set_value(0.5);
        c.repeat(true).unwrap();
        c.tick_at(0.05);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.15);
        assert!((c.value() - 0.0).abs() < 1e-6, "value={}", c.value());
        c.dispose();
    }

    /// Flutter oracle: `animation_controller_test.dart` "calling repeat
    /// with specified min and max values between 0 and 1..." (@ 3.44.0) —
    /// the `min == max` degenerate sub-case is skipped: FLUI rejects it
    /// (`repeat_with_rejects_inverted_range`), where Flutter permits it.
    #[test]
    fn repeat_bounce_flutter_oracle_interior_range() {
        let _serial = serial();

        // value 0.0 is below `min` at the call — the silent clamp lands on
        // 0.5 (Flutter's `x(0.0)` parity), not a rejection.
        let c = controller(100);
        c.repeat_with(Some(0.5), Some(1.0), true, None, None)
            .unwrap();
        assert!(
            (c.value() - 0.5).abs() < 1e-6,
            "silent clamp: value={}",
            c.value()
        );
        c.tick_at(0.05);
        assert!((c.value() - 0.75).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.10);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.20);
        assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
        c.dispose();

        // The same 200ms checkpoint, sampled in a SINGLE tick from the call
        // with no intervening boundary ticks: partition invariance must
        // give the identical answer the sequential port above established.
        // A two-cycle skip in one frame is exactly where the old
        // incremental model (advance-by-one-cycle-endpoint) diverged from
        // pure sampling.
        let jumped = controller(100);
        jumped
            .repeat_with(Some(0.5), Some(1.0), true, None, None)
            .unwrap();
        jumped.tick_at(0.20);
        assert!(
            (jumped.value() - 0.5).abs() < 1e-6,
            "value={}",
            jumped.value()
        );
        jumped.dispose();

        let c = controller(100);
        c.set_value(0.2);
        c.repeat_with(Some(0.2), Some(0.6), true, None, None)
            .unwrap();
        c.tick_at(0.05);
        assert!((c.value() - 0.4).abs() < 1e-6, "value={}", c.value());
        c.dispose();
    }

    /// Flutter oracle: `animation_controller_test.dart` "calling repeat
    /// with negative min value and positive max value..." (@ 3.44.0) — a
    /// repeat range that does not start at the controller's own bounds, on
    /// a controller whose own bounds are not `[0, 1]`.
    #[test]
    fn repeat_restart_flutter_oracle_custom_controller_bounds() {
        let _serial = serial();
        let c = AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0)
            .unwrap();
        c.set_value(1.0);
        c.repeat_with(Some(1.0), Some(3.0), false, None, None)
            .unwrap();
        assert!(
            (c.value() - 1.0).abs() < 1e-6,
            "value at call={}",
            c.value()
        );
        c.tick_at(0.05);
        assert!((c.value() - 2.0).abs() < 1e-6, "value={}", c.value());
        c.dispose();

        let c = AnimationController::without_ticker_bounds(Duration::from_millis(100), -1.0, 3.0)
            .unwrap();
        c.set_value(0.0);
        c.repeat_with(Some(-1.0), Some(3.0), false, None, None)
            .unwrap();
        assert!(
            (c.value() - 0.0).abs() < 1e-6,
            "value at call={}",
            c.value()
        );
        c.tick_at(0.025);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        c.dispose();
    }

    /// A finite count is measured from the phase origin, not from a fresh
    /// cycle 0: a run started mid-cycle (value 0.5, half a period's phase)
    /// exhausts `count` boundaries later — half a period after the call,
    /// not a full period later (Flutter: `_exitTimeInSeconds = count*period
    /// - _initialT`). Lands on the leg's own endpoint, `Completed` — the
    /// improved replacement for Flutter's `% 1.0`-wrapped oracle (see
    /// `docs/ARCHITECTURE.md`'s "Repeat sampling" mapping entry).
    #[test]
    fn repeat_restart_finite_count_exhausts_from_the_phase_origin() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);
        c.repeat_with(None, None, false, None, Some(1)).unwrap();
        c.tick_at(0.05);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    /// Flutter oracle: `animation_controller_test.dart` "calling repeat by
    /// setting count as valid with reverse as true..." (@ 3.44.0). Its
    /// harness ticks are ABSOLUTE frame timestamps
    /// (`scheduler_tester.dart`'s `tick` calls `handleBeginFrame` with the
    /// argument directly), so `tick(100ms)` then `tick(60ms)` REWINDS the
    /// clock to 60ms — the oracle's `0.6` sample is elapsed 60ms, not a
    /// cumulative 160ms. Pure sampling makes that rewind exact, with no
    /// `toStringAsFixed` rounding needed. The exhaustion assertion is
    /// FLUI's own addition — the oracle never ticks that far.
    #[test]
    fn repeat_bounce_flutter_oracle_finite_count_and_absolute_time_rewind() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(None, None, true, None, Some(4)).unwrap();
        c.tick_at(0.025);
        assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.05);
        assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
        c.tick_at(0.099);
        assert!((c.value() - 0.99).abs() < 1e-3, "value={}", c.value());
        c.tick_at(0.10);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        // The harness's absolute-time rewind: elapsed 60ms, not a
        // cumulative 160ms.
        c.tick_at(0.06);
        assert!((c.value() - 0.6).abs() < 1e-6, "value={}", c.value());
        c.dispose();

        // The non-rewound interpretation, for contrast: elapsed 160ms lands
        // on the reverse leg's 0.4, not the forward leg's 0.6.
        let c2 = controller(100);
        c2.repeat_with(None, None, true, None, Some(4)).unwrap();
        c2.tick_at(0.16);
        assert!((c2.value() - 0.4).abs() < 1e-6, "value={}", c2.value());
        c2.dispose();

        // Exhaustion at exactly 400ms lands on the 4th (odd-indexed)
        // cycle's reverse-leg endpoint.
        let c3 = controller(100);
        c3.repeat_with(None, None, true, None, Some(4)).unwrap();
        c3.tick_at(0.4);
        assert!((c3.value() - 0.0).abs() < 1e-6, "value={}", c3.value());
        assert_eq!(c3.status(), AnimationStatus::Dismissed);
        c3.dispose();
    }

    /// The exhaustion boundary is checked in integer nanoseconds, not f64:
    /// `0.3 >= 3.0 * 0.1` is `false` in f64 arithmetic, which would leave a
    /// 3-count 100ms repeat `Forward` one frame past the boundary it
    /// should have exhausted at.
    #[test]
    fn repeat_exhaustion_boundary_is_exact_at_a_float_unsafe_ratio() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(None, None, false, None, Some(3)).unwrap();
        c.tick_at(0.3);
        assert_eq!(
            c.status(),
            AnimationStatus::Completed,
            "3 * 100ms sampled at exactly 300ms must already be exhausted"
        );
        c.dispose();
    }

    /// `Duration::try_from_secs_f64` returning `Err` (an out-of-range
    /// `cycle`, e.g. `tick_at(f64::INFINITY)` or an extreme `time_dilation`
    /// overflowing the division) must saturate to a very large elapsed
    /// time, never to zero — a zero-rewind would let a pathological input
    /// never exhaust a finite repeat while `forward()` on the same input
    /// completes normally. Red-check: `.map_or(0, |d| d.as_nanos())`
    /// instead of `.unwrap_or(Duration::MAX)` — this repeat never exhausts.
    #[test]
    fn repeat_tick_at_infinity_exhausts_a_finite_count_instead_of_rewinding() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(None, None, false, Some(Duration::from_millis(100)), Some(2))
            .unwrap();
        c.tick_at(f64::INFINITY);
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        c.dispose();
    }

    /// A zero effective period settles SYNCHRONOUSLY at the call — Android's
    /// rule ("0 duration animator, ignore the repeat count and skip to the
    /// end"); Compose rejects it, Flutter asserts. A later `tick_at`
    /// changes nothing (no run was ever installed), and `run_generation` is
    /// untouched.
    #[test]
    fn repeat_with_zero_period_settles_synchronously_at_the_call() {
        let _serial = serial();

        // Finite count: lands on the count-th cycle's end (count=3, bounce,
        // from 0: cycle index 2 is even -> Forward -> max).
        let c = controller(100);
        let generation_before = c.run_generation();
        let future = c
            .repeat_with(None, None, true, Some(Duration::ZERO), Some(3))
            .unwrap();
        assert!(future.is_complete());
        assert!((c.value() - 1.0).abs() < 1e-6, "value={}", c.value());
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert_eq!(c.run_generation(), generation_before);
        c.tick_at(1.0);
        assert!(
            (c.value() - 1.0).abs() < 1e-6,
            "a later tick must change nothing"
        );
        c.dispose();

        // Infinite count: lands on cycle 0's end (Android's skip-to-end) —
        // a documented exception to "an infinite repeat's future resolves
        // only by cancellation".
        let c2 = controller(100);
        let future2 = c2
            .repeat_with(None, None, false, Some(Duration::ZERO), None)
            .unwrap();
        assert!(future2.is_complete());
        assert!((c2.value() - 1.0).abs() < 1e-6, "value={}", c2.value());
        assert_eq!(c2.status(), AnimationStatus::Completed);
        c2.dispose();
    }

    /// `count: Some(0)` is a degenerate case distinct from a zero
    /// PERIOD: zero cycles run AT ALL, regardless of period, so there is no
    /// cycle to land on — the value at the call is the CLAMPED CURRENT
    /// value, unchanged, not a landing jump. Web Animations semantics (an
    /// empty active interval finishes at once); Flutter asserts
    /// `count > 0`, Compose throws for `iterations < 1`. Repair, not
    /// reject, is the house rule.
    #[test]
    fn repeat_with_zero_count_settles_at_the_current_value_with_no_landing_jump() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.3);
        let generation_before = c.run_generation();
        let future = c
            .repeat_with(None, None, true, Some(Duration::from_millis(100)), Some(0))
            .unwrap();
        assert!(future.is_complete());
        assert!(
            (c.value() - 0.3).abs() < 1e-6,
            "zero cycles must not jump the value: {}",
            c.value()
        );
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert_eq!(c.run_generation(), generation_before);
        c.tick_at(1.0);
        assert!(
            (c.value() - 0.3).abs() < 1e-6,
            "a later tick must change nothing"
        );
        c.dispose();
    }

    /// `tick_repeat` NEVER reads `run_curve` at all — this pins that a
    /// repeat interpolates linearly regardless of what a prior
    /// `animate_to_curved` leaves behind, not that `repeat_with`'s
    /// `clear_run_modes()` call is what protects it (that call stays green
    /// even with the call deleted, since the curve field is structurally
    /// unreachable from `tick_repeat`; see
    /// `a_leftover_fling_simulation_does_not_leak_into_a_following_repeats_velocity`
    /// below for what `clear_run_modes()` actually protects).
    #[test]
    fn a_leftover_curve_does_not_shape_a_following_repeat() {
        use crate::curve::Curves;
        let _serial = serial();
        let c = controller(100);
        // A zero-distance curved run settles synchronously but still
        // installs the curve in `run_curve` — exactly the leftover state a
        // real `animate_to_curved` interruption would leave behind.
        c.animate_to_curved(
            0.0,
            Some(Duration::from_millis(100)),
            Arc::new(Curves::EaseInQuint),
        )
        .unwrap();

        c.repeat_with(None, None, false, Some(Duration::from_millis(100)), None)
            .unwrap();
        c.tick_at(0.05);
        assert!(
            (c.value() - 0.5).abs() < 1e-6,
            "a repeat interpolates linearly, no curve — value={}",
            c.value()
        );
        c.dispose();
    }

    /// `repeat_with`'s `clear_run_modes()` call IS pinned by this: a
    /// leftover `simulation` from a prior `fling` would short-circuit
    /// `velocity()` (`sim.dx(cycle)` instead of `range / duration`) if it
    /// survived into the repeat. Red-check: delete the `clear_run_modes()`
    /// call in `repeat_with` — `velocity()` reads the stale fling spring's
    /// `dx` instead of the repeat's own `range / period`.
    #[test]
    fn a_leftover_fling_simulation_does_not_leak_into_a_following_repeats_velocity() {
        let _serial = serial();
        let c = controller(100);
        c.fling(1.0).unwrap(); // installs `simulation`
        c.repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
            .unwrap();
        c.tick_at(0.25);
        assert!((c.value() - 0.25).abs() < 1e-6, "value={}", c.value());
        assert!(
            (c.velocity() - 1.0).abs() < 1e-6,
            "a leftover fling simulation must not leak into the repeat's velocity: {}",
            c.velocity()
        );
        c.dispose();
    }

    /// `set_duration` must not retime an ACTIVE repeat: the period is
    /// resolved ONCE at `repeat_with` (`period.unwrap_or(duration)`), not
    /// read live on every tick — Flutter parity (`period ??= duration`,
    /// captured by the simulation at the call).
    #[test]
    fn set_duration_during_an_active_repeat_leaves_the_running_period_unchanged() {
        let _serial = serial();
        let c = controller(100);
        // Period defaults from `duration` (no explicit `period` argument) —
        // the shape that used to be re-read live.
        c.repeat_with(None, None, false, None, None).unwrap();
        c.set_duration(Duration::from_millis(200));
        c.tick_at(0.05); // half of the ORIGINAL 100ms period
        assert!(
            (c.value() - 0.5).abs() < 1e-6,
            "set_duration mid-repeat must not retime the running period: value={}",
            c.value()
        );
        c.dispose();
    }

    /// `velocity()` on a reverse leg is SIGNED (a deliberate divergence
    /// from Flutter's `_RepeatingSimulation.dx`, which is always positive)
    /// — see `docs/ARCHITECTURE.md`'s "Repeat sampling" mapping entry, (g).
    /// Previously uncited/untested: the mapping entry's citation of
    /// `reverse_mid_flight_keeps_full_range_velocity` as this behavior's
    /// "sibling repeat coverage" named a test that has no `.velocity()`
    /// call at all.
    #[test]
    fn repeat_reverse_leg_velocity_is_negative() {
        let _serial = serial();
        let c = controller(100);
        c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        c.tick_at(1.5); // cycle index 1 (odd) -> reverse leg, mid-cycle
        assert!(
            (c.value() - 0.5).abs() < 1e-6,
            "sanity: reverse leg mid-cycle value={}",
            c.value()
        );
        assert!(
            (c.velocity() - (-1.0)).abs() < 1e-6,
            "a reverse leg's velocity must be negative: {}",
            c.velocity()
        );
        c.dispose();
    }

    /// A frame that spans several repeat cycles at once fires status
    /// listeners by PARITY, not once per retired cycle: an even number of
    /// skipped bounce legs cancels out (no net direction flip), an odd
    /// number fires exactly one status change, and a restart repeat's
    /// cycle boundary never changes status at all (`take_status_change`
    /// dedups the repeated `Forward` write). A tick that advances a live
    /// run fires the value listener exactly once, regardless of how many
    /// cycles it retired.
    #[test]
    fn repeat_multi_cycle_frame_notification_counts() {
        let _serial = serial();

        // Restart: a single tick spanning 3.5 cycles never changes status
        // and fires exactly one value notification.
        let c = controller(100);
        c.repeat_with(None, None, false, Some(Duration::from_secs(1)), None)
            .unwrap();
        let status_fires = Arc::new(AtomicUsize::new(0));
        let sf = Arc::clone(&status_fires);
        c.add_status_listener(Arc::new(move |_| {
            sf.fetch_add(1, Ordering::SeqCst);
        }));
        let value_fires = Arc::new(AtomicUsize::new(0));
        let vf = Arc::clone(&value_fires);
        c.add_listener(Arc::new(move || {
            vf.fetch_add(1, Ordering::SeqCst);
        }));
        c.tick_at(3.5);
        assert!((c.value() - 0.5).abs() < 1e-6, "value={}", c.value());
        assert_eq!(
            status_fires.load(Ordering::SeqCst),
            0,
            "restart never changes status mid-repeat"
        );
        assert_eq!(
            value_fires.load(Ordering::SeqCst),
            1,
            "one tick, one value notification"
        );
        c.dispose();

        // Bounce, EVEN number of retired cycles (2): direction is back to
        // Forward, so no net status change.
        let c = controller(100);
        c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        let status_fires = Arc::new(AtomicUsize::new(0));
        let sf = Arc::clone(&status_fires);
        c.add_status_listener(Arc::new(move |_| {
            sf.fetch_add(1, Ordering::SeqCst);
        }));
        c.tick_at(2.5);
        assert_eq!(
            status_fires.load(Ordering::SeqCst),
            0,
            "an even number of skipped bounce cycles fires no status change"
        );
        c.dispose();

        // Bounce, ODD number of retired cycles (3): exactly one status
        // change.
        let c = controller(100);
        c.repeat_with(None, None, true, Some(Duration::from_secs(1)), None)
            .unwrap();
        let status_fires = Arc::new(AtomicUsize::new(0));
        let sf = Arc::clone(&status_fires);
        c.add_status_listener(Arc::new(move |_| {
            sf.fetch_add(1, Ordering::SeqCst);
        }));
        c.tick_at(3.5);
        assert_eq!(
            status_fires.load(Ordering::SeqCst),
            1,
            "an odd number of skipped bounce cycles fires exactly one status change"
        );
        c.dispose();
    }

    // ---- set_value recomputes status at the bounds ----

    #[test]
    fn set_value_recomputes_status() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(1.0);
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.set_value(0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    // ---- time dilation slows progress ----

    /// Restores the global time dilation on drop so a failed assertion cannot
    /// leak a non-default dilation into sibling tests.
    struct DilationRestore(f64);
    impl Drop for DilationRestore {
        fn drop(&mut self) {
            let _ = flui_scheduler::config::set_time_dilation(self.0);
        }
    }

    #[test]
    fn time_dilation_scales_progress() {
        use flui_scheduler::config::{set_time_dilation, time_dilation};
        let _serial = serial();
        let _restore = DilationRestore(time_dilation());
        set_time_dilation(2.0).unwrap(); // half speed
        let c = controller(100); // 100ms
        c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
        c.tick_at(0.10); // 100ms raw -> dilated 50ms -> ~0.5, NOT complete
        let value = c.value();
        c.dispose();
        assert!((value - 0.5).abs() < 1e-3, "value={value}");
    }

    // ---- set_value parity: stops an active run, change-detects status ----

    #[test]
    fn set_value_stops_the_ticker_so_a_later_frame_does_not_clobber_it() {
        // Flutter parity: `AnimationController`'s `value=` setter calls
        // `stop()` before `_internalSetValue`. Drive a REAL frame through the
        // scheduler (not a direct `tick_at` call) so this exercises the same
        // path production code does: an auto-scheduling ticker re-registers
        // itself with the scheduler every frame while active, and only
        // `ticker.stop()` deregisters it.
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::from_millis(100), &scheduler);

        c.forward().unwrap();
        scheduler.execute_frame();
        assert!(
            c.value() > 0.0,
            "sanity: the ticker actually drove a frame, got value={}",
            c.value()
        );

        c.set_value(0.5);
        assert!(
            !c.is_animating(),
            "set_value must stop the ticker like Flutter's value= setter"
        );

        // The "next vsync": if the ticker were still registered, this frame
        // would recompute the value from the stale run and clobber 0.5.
        scheduler.execute_frame();
        assert_eq!(
            c.value(),
            0.5,
            "a frame after set_value must not overwrite the value that was just set"
        );
        c.dispose();
    }

    #[test]
    fn set_value_reports_completed_status_at_upper_bound() {
        let _serial = serial();
        let c = controller(100);
        c.forward().unwrap();
        c.set_value(1.0);
        assert_eq!(c.value(), 1.0);
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert!(!c.is_animating(), "set_value stops the run before settling");
        c.dispose();
    }

    #[test]
    fn status_listener_is_not_refired_for_an_unchanged_status() {
        // Flutter parity: `AnimationController._checkStatusChanged` only
        // notifies status listeners when `status` actually differs from
        // `_lastReportedStatus`. Without this, a 120Hz gesture-driven
        // `set_value` loop (or an interior set_value immediately followed by
        // `forward()` in the same direction) would re-fire `Forward` every
        // frame and a one-shot status listener would misfire repeatedly.
        let _serial = serial();
        let c = controller(100);
        let fire_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&fire_count);
        c.add_status_listener(Arc::new(move |_status| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        // set_value(0.5) is the FIRST transition (Dismissed -> Forward): fires once.
        c.set_value(0.5);
        assert_eq!(fire_count.load(Ordering::SeqCst), 1);

        // forward() keeps the same Forward status (direction was already
        // Forward, value already interior) — must NOT re-fire.
        c.forward().unwrap();
        assert_eq!(
            fire_count.load(Ordering::SeqCst),
            1,
            "forward() after an already-Forward set_value must not re-fire the same status"
        );

        // A second set_value that keeps the status Forward must also not re-fire.
        c.set_value(0.6);
        assert_eq!(
            fire_count.load(Ordering::SeqCst),
            1,
            "a same-status set_value must not re-fire (mirrors a 120Hz gesture drag)"
        );

        // A genuine status change (interior -> Completed) DOES fire.
        c.set_value(1.0);
        assert_eq!(fire_count.load(Ordering::SeqCst), 2);
        c.dispose();
    }

    // ---- animate_to_curved / animate_back_curved thread a curve through the run ----

    #[test]
    fn animate_to_curved_eases_through_the_given_curve() {
        use crate::curve::Curves;
        let _serial = serial();
        let c = controller(100);
        c.animate_to_curved(
            1.0,
            Some(Duration::from_millis(100)),
            Arc::new(Curves::EaseInQuint),
        )
        .unwrap();
        c.tick_at(0.05); // t=0.5 raw
        let expected = Curves::EaseInQuint.transform(0.5);
        assert!(
            (c.value() - expected).abs() < 1e-3,
            "expected the curve applied at t=0.5: got {}, want {expected}",
            c.value()
        );
        c.tick_at(0.10);
        assert_eq!(
            c.value(),
            1.0,
            "the curve must land exactly on the target at t=1.0"
        );
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    #[test]
    fn plain_animate_to_after_a_curved_run_is_linear_again() {
        // The per-run curve must not leak into a later plain (linear) run.
        use crate::curve::Curves;
        let _serial = serial();
        let c = controller(100);
        c.animate_to_curved(
            1.0,
            Some(Duration::from_millis(100)),
            Arc::new(Curves::EaseInQuint),
        )
        .unwrap();
        c.tick_at(0.10);
        c.reset().unwrap();

        c.animate_to(1.0, Some(Duration::from_millis(100))).unwrap();
        c.tick_at(0.05);
        assert!(
            (c.value() - 0.5).abs() < 1e-3,
            "a plain animate_to after a curved run must be linear again: got {}",
            c.value()
        );
        c.dispose();
    }

    #[test]
    fn animate_back_curved_eases_toward_the_lower_bound() {
        use crate::curve::Curves;
        let _serial = serial();
        let c = controller(100);
        c.set_value(1.0);
        c.animate_back_curved(
            0.0,
            Some(Duration::from_millis(100)),
            Arc::new(Curves::EaseInQuint),
        )
        .unwrap();
        c.tick_at(0.05);
        let expected = 1.0 - Curves::EaseInQuint.transform(0.5);
        assert!(
            (c.value() - expected).abs() < 1e-3,
            "got {}, want {expected}",
            c.value()
        );
        c.dispose();
    }

    // ---- reentrant ticker restart via a status listener (issue #1059) ----

    /// The canonical "chain the next animation" idiom: a status listener
    /// calls `forward()` again once the run it is reacting to completes.
    /// The controller starts at the UPPER bound and runs `reverse()` first —
    /// calling `forward()` immediately after a forward run lands exactly on
    /// the upper bound already (a real, zero-distance settle, Flutter
    /// parity: see `forward_at_upper_bound_settles_immediately`), which
    /// would never reach `restart_ticker` at all. `restart_ticker`'s
    /// `ticker.start(new_callback)` then runs while the SAME ticker's own
    /// callback is still the one dispatching — this run's own — tick
    /// (`tick_time_based` already stopped the ticker before firing status,
    /// so `restart_ticker`'s own `ticker.stop()` is a no-op, but
    /// `ticker.start` is not: it installs the chained run's callback into a
    /// slot a `TickerLease` still holds checked out). This is the SAME
    /// "restart inside tick" scenario `flui-scheduler`'s own
    /// `restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick`
    /// pins directly on `Ticker`, reached here through the real production
    /// call chain instead of a hand-rolled reentrant probe.
    #[test]
    fn status_listener_chaining_forward_ticks_once_per_frame_and_stop_fully_stops_it() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::from_millis(1), &scheduler);
        c.set_value(1.0);

        let tick_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&tick_count);
        c.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        let restarted = Arc::new(AtomicUsize::new(0));
        let restart_flag = Arc::clone(&restarted);
        let chained = c.clone();
        c.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Dismissed
                && restart_flag.fetch_add(1, Ordering::SeqCst) == 0
            {
                // A LONG chained run on purpose: the point of the second
                // half of this test is that `stop()` cancels a chain that
                // is still in flight. A chained run short enough to finish
                // on the next frame stops itself (`tick_time_based` calls
                // `ticker.stop()` before firing its status), which would
                // leave nothing for `stop()` to cancel and make every
                // assertion below hold with or without the fix.
                chained
                    .animate_to(1.0, Some(Duration::from_secs(10)))
                    .unwrap();
            }
        }));

        c.reverse().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        scheduler.execute_frame();

        assert_eq!(
            restarted.load(Ordering::SeqCst),
            1,
            "sanity: the first run must complete and the status listener \
             must have chained a restart"
        );
        assert_eq!(
            scheduler.transient_callback_count(),
            1,
            "the chained restart must leave exactly ONE live tick \
             registration — not zero (the new run's callback lost) and not \
             two (the old run's registration orphaned alongside it)"
        );

        std::thread::sleep(Duration::from_millis(5));
        scheduler.execute_frame();

        assert_eq!(
            tick_count.load(Ordering::SeqCst),
            2,
            "exactly one value notification per frame across both runs — a \
             duplicated tick chain would notify twice on the second frame"
        );
        assert_eq!(
            scheduler.transient_callback_count(),
            1,
            "the chained run is still in flight, so there is exactly one \
             live registration for `stop()` to cancel below"
        );

        c.stop().unwrap();
        assert_eq!(
            scheduler.transient_callback_count(),
            0,
            "stop() must fully cancel the single live tick chain, not just \
             one half of a duplicated pair"
        );
        assert!(!c.is_animating());

        // A surviving orphaned registration from a duplicated chain would
        // still fire here even after `stop()`.
        scheduler.execute_frame();
        assert_eq!(tick_count.load(Ordering::SeqCst), 2);

        c.dispose();
    }

    // ---- controller-owned run futures (issue #1161 / ADR-0064) ----

    #[test]
    fn forward_future_resolves_ok_when_the_run_completes_via_tick_at() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();
        assert!(future.is_pending());

        c.tick_at(0.1);

        assert!(
            future.is_complete(),
            "a run that reaches its target normally must resolve Ok, not \
             stay pending"
        );
        c.dispose();
    }

    #[test]
    fn zero_duration_forward_completes_before_forward_returns() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::ZERO, &scheduler);

        let value_fires = Arc::new(AtomicUsize::new(0));
        let vf = Arc::clone(&value_fires);
        c.add_listener(Arc::new(move || {
            vf.fetch_add(1, Ordering::SeqCst);
        }));
        let status_fires = Arc::new(AtomicUsize::new(0));
        let sf = Arc::clone(&status_fires);
        c.add_status_listener(Arc::new(move |_status| {
            sf.fetch_add(1, Ordering::SeqCst);
        }));
        let generation_before = c.run_generation();

        let future = c.forward().unwrap();

        assert!(
            future.is_complete(),
            "a zero-duration forward() must return an already-complete \
             future — no ticker run to wait on"
        );
        assert_eq!(c.value(), 1.0, "value snaps to the upper bound at the call");
        assert_eq!(c.status(), AnimationStatus::Completed);
        assert_eq!(
            value_fires.load(Ordering::SeqCst),
            1,
            "the value moved (0.0 -> 1.0), so the value listener fires once"
        );
        assert_eq!(status_fires.load(Ordering::SeqCst), 1);
        assert_eq!(
            c.run_generation(),
            generation_before,
            "a synchronous settle installs no run and must not bump run_generation"
        );

        scheduler.execute_frame();
        assert_eq!(
            (
                value_fires.load(Ordering::SeqCst),
                status_fires.load(Ordering::SeqCst)
            ),
            (1, 1),
            "no ticker was ever installed, so a later frame changes nothing"
        );
        c.dispose();
    }

    #[test]
    fn zero_duration_reverse_settles_dismissed_at_the_call() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::ZERO, &scheduler);
        c.set_value(1.0);

        let future = c.reverse().unwrap();

        assert!(future.is_complete());
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    /// `animate_to(x, Some(Duration::ZERO))` / `animate_back(x, Some(Duration::ZERO))`
    /// are the documented "set a value with a direction" (flutter#158233's
    /// accepted workaround): the METHOD picks the end status, not the
    /// travel — `animate_to` toward a SMALLER value still ends `Completed`,
    /// `animate_back` toward a LARGER one still ends `Dismissed`.
    #[test]
    fn animate_to_with_zero_duration_is_a_directional_set() {
        let _serial = serial();
        let c = controller(100);

        c.set_value(0.7);
        c.animate_to(0.3, Some(Duration::ZERO)).unwrap();
        assert_eq!(c.value(), 0.3);
        assert_eq!(
            c.status(),
            AnimationStatus::Completed,
            "animate_to toward a SMALLER value still ends Completed"
        );

        c.set_value(0.1);
        c.animate_back(0.3, Some(Duration::ZERO)).unwrap();
        assert_eq!(c.value(), 0.3);
        assert_eq!(
            c.status(),
            AnimationStatus::Dismissed,
            "animate_back toward a LARGER value still ends Dismissed"
        );
        c.dispose();
    }

    /// `animate_to`'s status is `Forward` regardless of whether `target` is
    /// above or below the current value — a REAL (non-settling) run, so the
    /// transient running status is observable before the run completes.
    #[test]
    fn animate_to_below_the_current_value_runs_forward() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.7);

        c.animate_to(0.3, Some(Duration::from_millis(100))).unwrap();
        assert_eq!(
            c.status(),
            AnimationStatus::Forward,
            "animate_to is Forward regardless of travel direction"
        );

        c.tick_at(0.1);
        assert_eq!(c.value(), 0.3);
        assert_eq!(c.status(), AnimationStatus::Completed);
        c.dispose();
    }

    /// The mirror of [`animate_to_below_the_current_value_runs_forward`] for
    /// `animate_back`.
    #[test]
    fn animate_back_above_the_current_value_runs_reverse() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.1);

        c.animate_back(0.3, Some(Duration::from_millis(100)))
            .unwrap();
        assert_eq!(
            c.status(),
            AnimationStatus::Reverse,
            "animate_back is Reverse regardless of travel direction"
        );

        c.tick_at(0.1);
        assert_eq!(c.value(), 0.3);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
        c.dispose();
    }

    /// Run-end status is the run's direction, with **no bound check**:
    /// `animate_to(lower_bound)` from mid-range still ends `Completed`.
    /// Flutter's `_tick` rule (`animation_controller.dart:948-950` @ 3.44.0).
    #[test]
    fn animate_to_the_lower_bound_ends_completed() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(0.5);

        c.animate_to(0.0, Some(Duration::from_millis(100))).unwrap();
        c.tick_at(0.1);

        assert_eq!(c.value(), 0.0);
        assert_eq!(
            c.status(),
            AnimationStatus::Completed,
            "the run's direction was Forward, so it ends Completed even \
             though the value landed on the LOWER bound"
        );
        c.dispose();
    }

    /// Order pin for a zero-duration displacement: the new run's status must
    /// still be observed BEFORE the displaced run's cancellation, exactly
    /// like a real-duration displacement
    /// (`a_new_runs_status_listener_fires_before_the_displaced_runs_cancellation`).
    #[test]
    fn a_zero_duration_run_cancels_the_displaced_run_after_its_own_status_is_observable() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::from_millis(100), &scheduler);
        let first = c.forward().unwrap(); // a real, still-pending run

        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let order_for_status = Arc::clone(&order);
        c.add_status_listener(Arc::new(move |_status| {
            order_for_status.lock().push("new_run_status");
        }));
        let order_for_cancel = Arc::clone(&order);
        first.when_complete_or_cancel(move |_outcome| {
            order_for_cancel.lock().push("displaced_run_canceled");
        });

        // A per-run zero-duration override: this settles synchronously and
        // displaces `first`, which never ticked (still at the lower bound).
        c.animate_to(1.0, Some(Duration::ZERO)).unwrap();

        assert_eq!(
            order.lock().as_slice(),
            &["new_run_status", "displaced_run_canceled"],
            "even a synchronously-settling run must publish its own status \
             before the run it displaced observes its cancellation"
        );
        c.dispose();
    }

    /// A settle that does not move the value must not fire value listeners —
    /// Flutter: `if (value != target) { …; notifyListeners(); }`
    /// (`animation_controller.dart:675-678`).
    #[test]
    fn a_zero_distance_settle_does_not_notify_value_listeners() {
        let _serial = serial();
        let c = controller(100);
        c.set_value(1.0); // already at the upper bound

        let value_fires = Arc::new(AtomicUsize::new(0));
        let vf = Arc::clone(&value_fires);
        c.add_listener(Arc::new(move || {
            vf.fetch_add(1, Ordering::SeqCst);
        }));

        c.forward().unwrap(); // zero-distance settle: the value does not move

        assert_eq!(
            value_fires.load(Ordering::SeqCst),
            0,
            "a settle that does not move the value must not notify value listeners"
        );
        c.dispose();
    }

    /// `dispose()` mid-run leaves `status` untouched (Flutter parity) — a
    /// proxy reading `status()` on replay must see the status the run had,
    /// not a manufactured settle. The frame-loop leak that would otherwise
    /// follow is closed on `tick_at` instead: it is a no-op after dispose.
    #[test]
    fn a_disposed_controller_neither_ticks_nor_holds_the_frame_loop() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        c.forward().unwrap();
        c.tick_at(0.05); // mid-run
        let status_before = c.status();
        assert_eq!(status_before, AnimationStatus::Forward, "sanity: mid-run");

        c.dispose();
        assert_eq!(
            c.status(),
            status_before,
            "dispose() must leave status untouched"
        );

        let value_before = c.value();
        c.tick_at(0.10);
        assert_eq!(
            c.value(),
            value_before,
            "tick_at after dispose must not advance the value"
        );
        assert_eq!(
            c.status(),
            status_before,
            "tick_at after dispose must not change status either"
        );
    }

    /// `tick_at` after a mid-run `set_value` must be a no-op — `set_value`
    /// calls `stop_running()` (clearing `active_run`) but reports a
    /// directional *running* status at an interior value (Flutter parity,
    /// `settled_status_keep_direction`), so `status.is_running()` alone
    /// cannot tell "a run is installed" from "set_value just stopped one and
    /// reported a running-looking status anyway". `tick_at`'s guard must
    /// read `active_run.is_none()`, not `!status.is_running()`.
    ///
    /// Red-check: revert `tick_at`'s guard to `!inner.status.is_running()` —
    /// the final assertion sees `1.0`, not `0.2` (the stopped run's own
    /// `start_value..target_value` recomputed at `t = 1.0`).
    #[test]
    fn tick_at_after_set_value_mid_run_is_a_no_op() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        c.forward().unwrap();
        c.tick_at(0.05);
        assert!(
            (c.value() - 0.5).abs() < 1e-3,
            "sanity: halfway through the run, got {}",
            c.value()
        );

        c.set_value(0.2);
        assert_eq!(
            c.status(),
            AnimationStatus::Forward,
            "sanity: set_value at an interior value keeps the directional running status"
        );

        c.tick_at(0.10);
        assert_eq!(
            c.value(),
            0.2,
            "tick_at after a mid-run set_value must be a no-op, not recompute \
             from the run set_value already stopped"
        );
        c.dispose();
    }

    /// flutter#1913: status coalescing must not swallow an intermediate
    /// status. `forward()` from the lower bound with no tick between calls
    /// reports `Forward` (a real run: distance and duration are both
    /// nonzero); the immediately-following `reverse()` finds the value still
    /// at the lower bound (nothing ticked) and settles `Dismissed` at zero
    /// distance. The net value/status end up back where they started, but
    /// both intermediate transitions must still be delivered.
    #[test]
    fn forward_then_reverse_with_no_tick_delivers_the_intermediate_status() {
        let _serial = serial();
        let c = controller(100);

        let statuses: Arc<Mutex<Vec<AnimationStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&statuses);
        c.add_status_listener(Arc::new(move |status| s2.lock().push(status)));

        c.forward().unwrap();
        c.reverse().unwrap();

        assert_eq!(
            statuses.lock().as_slice(),
            &[AnimationStatus::Forward, AnimationStatus::Dismissed],
            "coalescing must not drop the intermediate Forward just because \
             the net status ends back at Dismissed"
        );
        c.dispose();
    }

    /// A trivially-finished [`Simulation`] test double: `is_done` is true
    /// from the first tick, so `tick_simulation`'s completion branch runs
    /// immediately without needing a real spring to settle.
    struct InstantSimulation {
        value: f32,
    }

    impl Simulation for InstantSimulation {
        fn x(&self, _time: f32) -> f32 {
            self.value
        }
        fn dx(&self, _time: f32) -> f32 {
            0.0
        }
        fn is_done(&self, _time: f32) -> bool {
            true
        }
        fn tolerance(&self) -> Tolerance {
            Tolerance::DEFAULT
        }
    }

    #[test]
    fn simulation_run_future_resolves_ok_when_the_simulation_finishes() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.animate_with(InstantSimulation { value: 0.5 }).unwrap();
        assert!(future.is_pending());

        c.tick_at(0.0);

        assert!(
            future.is_complete(),
            "tick_simulation's is_done branch must complete the run's future"
        );
        c.dispose();
    }

    #[test]
    fn finite_repeat_future_completes_when_the_count_is_exhausted() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(10));
        let future = c
            .repeat_with(None, None, false, Some(Duration::from_millis(10)), Some(2))
            .unwrap();
        assert!(future.is_pending());

        c.tick_at(0.010); // first cycle retires; one more to go
        assert!(future.is_pending(), "one of two cycles is not exhaustion");
        c.tick_at(0.020); // second cycle retires -> exhausted
        assert!(
            future.is_complete(),
            "a finite repeat must complete its future once its count is exhausted"
        );
        c.dispose();
    }

    #[test]
    fn infinite_repeat_future_only_resolves_via_stop() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(10));
        let future = c.repeat(false).unwrap();
        assert!(future.is_pending());

        // Several cycles retire; an infinite repeat has no natural end.
        c.tick_at(0.010);
        c.tick_at(0.020);
        c.tick_at(0.100);
        assert!(
            future.is_pending(),
            "an infinite repeat's future must stay pending through any \
             number of retired cycles"
        );

        c.stop().unwrap();
        assert!(
            future.is_canceled(),
            "stop() is the only thing that resolves an infinite repeat's future"
        );
        c.dispose();
    }

    #[test]
    fn stop_cancels_the_active_run() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();
        c.stop().unwrap();
        assert!(future.is_canceled(), "stop() must cancel the run in flight");
        c.dispose();
    }

    #[test]
    fn set_value_cancels_the_active_run() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();
        c.set_value(0.3);
        assert!(
            future.is_canceled(),
            "set_value() must cancel the run in flight"
        );
        c.dispose();
    }

    #[test]
    fn reset_cancels_the_active_run() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();
        c.reset().unwrap();
        assert!(
            future.is_canceled(),
            "reset() must cancel the run in flight"
        );
        c.dispose();
    }

    #[test]
    fn dispose_cancels_the_active_run() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();
        c.dispose();
        assert!(
            future.is_canceled(),
            "dispose() must cancel the run in flight"
        );
    }

    #[test]
    fn a_new_runs_status_listener_fires_before_the_displaced_runs_cancellation() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let first = c.forward().unwrap();

        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let order_for_status = Arc::clone(&order);
        c.add_status_listener(Arc::new(move |_status| {
            order_for_status.lock().push("new_run_status");
        }));
        let order_for_cancel = Arc::clone(&order);
        first.when_complete_or_cancel(move |_outcome| {
            order_for_cancel.lock().push("displaced_run_canceled");
        });

        c.reverse().unwrap();

        assert_eq!(
            order.lock().as_slice(),
            &["new_run_status", "displaced_run_canceled"],
            "the new run's status must be observed before the displaced \
             run's cancellation is delivered"
        );
        c.dispose();
    }

    #[test]
    fn zero_distance_start_returns_a_complete_future_and_cancels_the_displaced_run() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let first = c.forward().unwrap(); // 0.0 -> 1.0, a real run
        c.tick_at(0.05); // partway; still pending
        assert!(first.is_pending());

        // Already at the target -> the zero-distance settle path, no new
        // ticker run.
        let settled = c.forward_from(Some(1.0)).unwrap();

        assert!(
            settled.is_complete(),
            "a zero-distance start must return an already-complete future"
        );
        assert!(
            first.is_canceled(),
            "the zero-distance settle must still cancel whatever run it displaced"
        );
        c.dispose();
    }

    #[test]
    fn every_delivery_runs_with_the_controller_lock_free() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let inner = Arc::clone(&c.inner);
        let future = c.forward().unwrap();

        let observed = Arc::new(Mutex::new(None));
        let observed2 = Arc::clone(&observed);
        future.when_complete_or_cancel(move |_outcome| {
            *observed2.lock() = Some(inner.try_lock().is_some());
        });

        c.tick_at(0.1);

        assert_eq!(
            observed.lock().as_ref(),
            Some(&true),
            "a delivery must run with the controller's own lock free — the \
             finish chokepoint drops it before calling deliver()"
        );
        c.dispose();
    }

    #[test]
    fn a_completed_listener_that_starts_a_new_run_leaves_the_finished_run_ok() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let first = c.forward().unwrap();

        let chained = c.clone();
        let restarted = Arc::new(AtomicUsize::new(0));
        let restart_flag = Arc::clone(&restarted);
        c.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed
                && restart_flag.fetch_add(1, Ordering::SeqCst) == 0
            {
                chained.forward_from(Some(0.0)).unwrap();
            }
        }));

        c.tick_at(0.1); // completes `first`; the listener above chains a new run

        assert_eq!(
            restarted.load(Ordering::SeqCst),
            1,
            "sanity: the listener must have chained a restart"
        );
        assert!(
            first.is_complete(),
            "the finished run's own future must resolve Ok even though a \
             listener started a new run before delivery ran"
        );
        c.dispose();
    }

    #[test]
    fn a_panicking_status_listener_leaves_the_finished_run_ok() {
        let _serial = serial();
        let c = AnimationController::without_ticker(Duration::from_millis(100));
        let future = c.forward().unwrap();

        // Registered on the FUTURE, not the controller: this only runs if
        // `TickerDelivery` actually delivers. `future.is_complete()` alone
        // (the durable state `publish` writes) would stay green even with
        // `Drop for TickerDelivery` emptied out, since `finish` never
        // reaches its own `delivery.deliver()` line when `fire_status`
        // panics — only the unwind dropping the `delivery` parameter runs
        // it. This continuation is the oracle for that drop actually firing.
        let seen = Arc::new(Mutex::new(None));
        let seen2 = Arc::clone(&seen);
        future.when_complete_or_cancel(move |outcome| {
            *seen2.lock() = Some(outcome);
        });

        c.add_status_listener(Arc::new(|status| {
            assert!(
                status != AnimationStatus::Completed,
                "a status listener panics on completion"
            );
        }));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.tick_at(0.1);
        }));

        assert!(
            result.is_err(),
            "the listener's panic must propagate out of tick_at"
        );
        assert_eq!(
            *seen.lock(),
            Some(Ok(())),
            "TickerDelivery must still deliver on drop through the unwind, \
             running the continuation with the outcome published before \
             the panicking listener ran"
        );
        assert!(future.is_complete());
        c.dispose();
    }

    #[test]
    fn when_complete_or_cancel_chaining_ticks_once_per_frame_and_stop_fully_stops_it() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        // A REAL (1ms) base duration, not `Duration::ZERO`: a zero-duration
        // `reverse()` completes AT THE CALL (issue #1171) and fires this
        // `when_complete_or_cancel` continuation at REGISTRATION time,
        // before `execute_frame()` ever runs (see
        // `zero_duration_reverse_settles_dismissed_at_the_call`), which
        // would collapse the two-frame structure this test relies on
        // (per-frame tick counts, `stop()` canceling a chain still in
        // flight). A short real duration keeps the first leg genuinely
        // pending across the `sleep` + `execute_frame()` pump below, exactly
        // like the sibling status-listener version of this test
        // (`status_listener_chaining_forward_ticks_once_per_frame_and_stop_fully_stops_it`).
        // The chained leg below still takes its own explicit 10s duration
        // regardless of this controller's base duration, so it is still in
        // flight when `stop()` cancels it.
        let c = AnimationController::new(Duration::from_millis(1), &scheduler);
        c.set_value(1.0);

        let tick_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&tick_count);
        c.add_listener(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));

        let restarted = Arc::new(AtomicUsize::new(0));
        let restart_flag = Arc::clone(&restarted);
        let chained = c.clone();
        let future = c.reverse().unwrap();
        future.when_complete_or_cancel(move |outcome| {
            if outcome.is_ok() && restart_flag.fetch_add(1, Ordering::SeqCst) == 0 {
                // A LONG chained run on purpose — see the sibling
                // status-listener version of this test for why.
                chained
                    .animate_to(1.0, Some(Duration::from_secs(10)))
                    .unwrap();
            }
        });

        std::thread::sleep(Duration::from_millis(5));
        scheduler.execute_frame();

        assert_eq!(
            restarted.load(Ordering::SeqCst),
            1,
            "sanity: the first run must complete and the continuation must \
             have chained a restart"
        );
        assert_eq!(
            scheduler.transient_callback_count(),
            1,
            "the chained restart must leave exactly ONE live tick registration"
        );

        std::thread::sleep(Duration::from_millis(5));
        scheduler.execute_frame();

        assert_eq!(
            tick_count.load(Ordering::SeqCst),
            2,
            "exactly one value notification per frame across both runs"
        );
        assert_eq!(scheduler.transient_callback_count(), 1);

        c.stop().unwrap();
        assert_eq!(
            scheduler.transient_callback_count(),
            0,
            "stop() must fully cancel the single live tick chain"
        );
        assert!(!c.is_animating());

        scheduler.execute_frame();
        assert_eq!(tick_count.load(Ordering::SeqCst), 2);

        c.dispose();
    }

    /// Source guard: `TickerDelivery::deliver` must be called from exactly
    /// one place — inside `AnimationController::finish` — and no
    /// `TickerCompleter`/`TickerDelivery`-producing call
    /// (`.complete()`/`.cancel()`/`stop_running(`/`.map(TickerCompleter::..)`)
    /// or a direct `active_run = ` assignment may appear anywhere else
    /// without being bound to a name (a `let _ = ..`, a bare unbound
    /// statement, or either wrapped in `drop(..)`) — every one of those
    /// shapes delivers (or drops a completer that would have delivered)
    /// under whatever lock is live at that statement instead of routing
    /// through `finish`.
    ///
    /// Checked per STATEMENT, not per line: physical lines are stripped of
    /// `//` comments (never `://`) and doc-comment-only lines are dropped
    /// entirely, then accumulated until a `;`, `{`, or `}` is seen. Only a
    /// `;`-terminated accumulation is a real statement — a bare tail
    /// expression (no trailing `;`, e.g. `stop_running`'s own
    /// `self.active_run.take().map(TickerCompleter::cancel)` return value)
    /// is a function's return, not a discard, and is excluded by
    /// construction: it never accumulates a trailing `;` of its own before
    /// the enclosing `}` ends the accumulation instead. This is what makes a
    /// rustfmt-wrapped `let _ = inner\n    .active_run\n    .take()\n    .map(TickerCompleter::cancel);`
    /// visible as one unit regardless of where the formatter broke the
    /// lines (whitespace before a `.` is removed after joining, so a wrapped
    /// `.active_run\n.take()` still reads `active_run.take(`), which a
    /// per-line check cannot see.
    ///
    /// Known limit, by construction: a `{` or `}` ends an accumulation
    /// without checking it, so a tracked call that sits to the LEFT of a
    /// brace in the same statement — `if let Some(old) = inner.active_run.take() { .. }`,
    /// `match inner.active_run.take() { .. }` — is not seen. Those shapes are
    /// bound (the value has a name inside the block), so they are outside
    /// what this guard claims; do not cite it against them.
    #[test]
    fn ticker_completer_resolution_never_bypasses_the_finish_chokepoint() {
        let source = include_str!("controller.rs");
        // Scan production code only: this test's own body spells out the
        // exact patterns it searches for (in match strings and panic
        // messages), which would otherwise flag itself.
        let production_end = source
            .find("\n#[cfg(test)]\nmod tests {")
            .expect("controller.rs must contain its own #[cfg(test)] mod tests block");
        let production = &source[..production_end];

        // `.deliver()` is called from exactly one place: inside `finish`.
        let deliver_count = production.matches(".deliver()").count();
        assert_eq!(
            deliver_count, 1,
            "`.deliver()` must be called from exactly one place in this file \
             (AnimationController::finish); found {deliver_count} call site(s)"
        );
        let finish_start = production
            .find("fn finish(")
            .expect("AnimationController::finish must exist");
        let finish_body_start = production[finish_start..]
            .find('{')
            .map(|i| finish_start + i)
            .expect("fn finish must have a body");
        let mut depth = 0i32;
        let mut finish_body_end = None;
        for (i, ch) in production[finish_body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        finish_body_end = Some(finish_body_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let finish_body_end = finish_body_end.expect("fn finish's body braces must balance");
        let deliver_pos = production
            .find(".deliver()")
            .expect("just counted at least one occurrence above");
        assert!(
            (finish_body_start..=finish_body_end).contains(&deliver_pos),
            "the one `.deliver()` call must be inside `AnimationController::finish`'s \
             own body (byte range {finish_body_start}..={finish_body_end}), found at \
             byte {deliver_pos}"
        );

        // Tracked call shapes that must never appear unbound.
        let tracked: [&str; 6] = [
            ".complete()",
            ".cancel()",
            "stop_running(",
            ".map(TickerCompleter::",
            "active_run.replace(",
            "active_run.take(",
        ];

        let mut buffer = String::new();
        for line in production.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                continue; // prose, not code — never joined in
            }
            // Strip a trailing `//` comment, but not a `://` inside a URL.
            let bytes = line.as_bytes();
            let mut code_part = line;
            let mut search_from = 0;
            while let Some(rel) = line[search_from..].find("//") {
                let at = search_from + rel;
                if at > 0 && bytes[at - 1] == b':' {
                    search_from = at + 2;
                    continue;
                }
                code_part = &line[..at];
                break;
            }

            for ch in code_part.chars() {
                buffer.push(ch);
                if ch == ';' || ch == '{' || ch == '}' {
                    let statement: String = if ch == ';' {
                        // Re-join a wrapped method chain so `.active_run\n.take()`
                        // reads `active_run.take(` again: the tracked patterns
                        // are written without whitespace before the `.`.
                        buffer[..buffer.len() - 1]
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .replace(" .", ".")
                    } else {
                        String::new() // `{`/`}` boundary: not a value statement
                    };
                    buffer.clear();

                    if statement.is_empty() {
                        continue;
                    }
                    let is_bound = statement.starts_with("let ");

                    let discards_via_let_underscore = statement.starts_with("let _ =")
                        && tracked.iter().any(|pat| statement.contains(pat));
                    assert!(
                        !discards_via_let_underscore,
                        "discards a TickerCompleter/TickerDelivery result with \
                         `let _ =`, which delivers under whatever lock is held at \
                         this statement instead of going through \
                         `AnimationController::finish`: {statement:?}"
                    );

                    // Covers a bare `foo().map(TickerCompleter::cancel);`, a
                    // bare `stop_running();`, and either wrapped in
                    // `drop(..)` — none of them bind the result anywhere.
                    let bare_discard =
                        !is_bound && tracked.iter().any(|pat| statement.contains(pat));
                    assert!(
                        !bare_discard,
                        "calls a TickerCompleter/TickerDelivery-producing operation \
                         with no binding at all, delivering under whatever lock is \
                         held at this statement instead of going through \
                         `AnimationController::finish`: {statement:?}"
                    );

                    let direct_assignment = statement.contains("active_run = ");
                    assert!(
                        !direct_assignment,
                        "assigns `active_run` directly with `=`, which drops \
                         whatever completer was there before, inline, under \
                         whatever lock is held at this statement instead of \
                         routing it through `.replace()` + \
                         `AnimationController::finish`: {statement:?}"
                    );
                }
            }
            buffer.push(' '); // preserve the line break as whitespace
        }
    }
}
