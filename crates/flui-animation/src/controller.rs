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

/// Floor a non-negative cycle ratio to a whole repeat-cycle count. Used by the
/// repeat tick to retire every cycle a long frame elapsed; `as u32` saturates a
/// pathological ratio to `u32::MAX` rather than wrapping.
#[inline]
fn whole_cycles(ratio: f64) -> u32 {
    ratio.floor() as u32
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
/// layer rather than re-derived here. A per-run epoch (`run_epoch_secs`) marks
/// where the current run or repeat cycle began on the ticker timeline.
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

    /// Ticker-timeline epoch (dilated seconds) at which the current run or
    /// repeat cycle began. `cycle_elapsed = dilated_elapsed - run_epoch_secs`.
    run_epoch_secs: f64,

    /// Most recent raw (pre-dilation) elapsed seconds seen by
    /// [`AnimationController::tick_at`], so `velocity()` can report the
    /// in-progress rate without a fresh tick.
    last_raw_elapsed_secs: f64,

    /// Monotonically increasing counter, bumped once each time a fresh run is
    /// established (every [`restart_ticker`](AnimationController::restart_ticker),
    /// where `run_epoch_secs` is re-zeroed). An external frame driver reads
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

    /// Is the animation in repeat mode?
    is_repeating: bool,

    /// Should repeat bounce back and forth (reverse) rather than restart?
    repeat_reverse: bool,

    /// Lower endpoint of the repeat range (defaults to `lower_bound`).
    repeat_min: f32,

    /// Upper endpoint of the repeat range (defaults to `upper_bound`).
    repeat_max: f32,

    /// Per-cycle duration for repeat (overrides `duration` when set).
    repeat_period: Option<Duration>,

    /// Number of repeat cycles to run; `None` repeats indefinitely.
    repeat_count: Option<u32>,

    /// Completed repeat cycles so far.
    repeat_done: u32,

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
    fn with_bounds_inner(
        duration: Duration,
        ticker: Option<Ticker>,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        if lower_bound >= upper_bound {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower_bound}) must be less than upper_bound ({upper_bound})"
            )));
        }

        let notifier = Arc::new(ChangeNotifier::new());

        let inner = AnimationControllerInner {
            value: lower_bound,
            status: AnimationStatus::Dismissed,
            duration,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            ticker,
            status_listeners: Vec::new(),
            direction: AnimationDirection::Forward,
            start_value: lower_bound,
            target_value: upper_bound,
            run_epoch_secs: 0.0,
            last_raw_elapsed_secs: 0.0,
            run_generation: 0,
            run_duration: None,
            disposed: false,
            next_listener_id: 1,
            is_repeating: false,
            repeat_reverse: false,
            repeat_min: lower_bound,
            repeat_max: upper_bound,
            repeat_period: None,
            repeat_count: None,
            repeat_done: 0,
            simulation: None,
            run_curve: None,
            last_reported_status: AnimationStatus::Dismissed,
            active_run: None,
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
            notifier,
        })
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
    /// the next `forward`/`reverse` re-establishes the run epoch.
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
    /// simulation duration by the remaining fraction). Starting at the
    /// upper bound settles immediately with
    /// [`AnimationStatus::Completed`] and an already-complete
    /// [`TickerFuture`]. See [`forward`](Self::forward) for the returned
    /// future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward_from(&self, from: Option<f32>) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Forward;
        inner.start_value = inner.value;
        inner.target_value = inner.upper_bound;
        if (inner.target_value - inner.value).abs() < BOUND_EPSILON {
            return Ok(self.settle_at_target(inner));
        }

        inner.status = AnimationStatus::Forward;
        inner.run_duration = Some(inner.scaled_run_duration(inner.duration));
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
            ValueChange::Unchanged,
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
    /// Starting at the lower bound settles immediately with
    /// [`AnimationStatus::Dismissed`]. See [`forward`](Self::forward) for the
    /// returned future's contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse_from(&self, from: Option<f32>) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Reverse;
        inner.start_value = inner.value;
        inner.target_value = inner.lower_bound;
        if (inner.target_value - inner.value).abs() < BOUND_EPSILON {
            return Ok(self.settle_at_target(inner));
        }

        inner.status = AnimationStatus::Reverse;
        let base = inner.reverse_duration.unwrap_or(inner.duration);
        inner.run_duration = Some(inner.scaled_run_duration(base));
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
            ValueChange::Unchanged,
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
    /// Sets the value to `lower_bound` and the status to
    /// [`AnimationStatus::Dismissed`]. Cancels the active run's
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
        inner.value = inner.lower_bound;
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
    /// # Arguments
    ///
    /// * `target` - The target value (clamped to bounds)
    /// * `duration` - Optional per-run duration override
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
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
        self.drive_to(target, duration, false, None)
    }

    /// Animate back to a specific value, defaulting to the reverse duration.
    ///
    /// Like [`animate_to`](Self::animate_to) but, when `duration` is `None`,
    /// defaults to the configured reverse duration (then the base duration),
    /// scaled by the remaining fraction of the range.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn animate_back(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, true, None)
    }

    /// Like [`animate_to`](Self::animate_to), but eases the run through
    /// `curve` instead of running linearly. Flutter parity:
    /// `AnimationController.animateTo(target, duration: ..., curve: ...)`,
    /// which threads `curve` into `_InterpolationSimulation`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn animate_to_curved(
        &self,
        target: f32,
        duration: Option<Duration>,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, false, Some(curve))
    }

    /// Like [`animate_back`](Self::animate_back), but eases the run through
    /// `curve` instead of running linearly. Flutter parity:
    /// `AnimationController.animateBack(target, duration: ..., curve: ...)`.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn animate_back_curved(
        &self,
        target: f32,
        duration: Option<Duration>,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> Result<TickerFuture, AnimationError> {
        self.drive_to(target, duration, true, Some(curve))
    }

    /// Shared driver for [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back)
    /// and their `_curved` variants: interpolate from the current value to
    /// `target`, picking direction from their order and easing through
    /// `curve` (`None` = linear). `prefer_reverse_duration` makes the
    /// `None`-duration default the reverse duration regardless of the run's
    /// direction (the `animate_back` contract).
    fn drive_to(
        &self,
        target: f32,
        duration: Option<Duration>,
        prefer_reverse_duration: bool,
        curve: Option<Arc<dyn Curve + Send + Sync>>,
    ) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        let target = target.clamp(inner.lower_bound, inner.upper_bound);
        inner.clear_run_modes();
        inner.run_curve = curve;
        inner.start_value = inner.value;
        inner.target_value = target;
        inner.direction = if target >= inner.value {
            AnimationDirection::Forward
        } else {
            AnimationDirection::Reverse
        };

        // No-op fast path: already at the target. Starting the ticker would run
        // for the full duration, re-notifying value listeners every frame while
        // the value never changes, so settle immediately with a single
        // notification instead.
        if (target - inner.value).abs() < BOUND_EPSILON {
            return Ok(self.settle_at_target(inner));
        }

        inner.status = inner.direction.running_status();
        // Per-run override only — never clobber `inner.duration`. Without an
        // explicit duration, the direction's base duration is scaled by the
        // remaining fraction so partial runs keep the full-range velocity.
        inner.run_duration = Some(duration.unwrap_or_else(|| {
            let base = if prefer_reverse_duration {
                inner.reverse_duration.unwrap_or(inner.duration)
            } else {
                match inner.direction {
                    AnimationDirection::Forward => inner.duration,
                    AnimationDirection::Reverse => inner.reverse_duration.unwrap_or(inner.duration),
                }
            };
            inner.scaled_run_duration(base)
        }));
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

    /// Settle a run whose start value already sits at its target: snap the
    /// value, stop the ticker, cancel whatever run this displaced, and
    /// report the settled status with a single notification — no transient
    /// running status, no full-duration no-op run. Returns an
    /// already-complete [`TickerFuture`] for the (trivial, zero-distance)
    /// run this call represents.
    fn settle_at_target(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
    ) -> TickerFuture {
        inner.value = inner.target_value;
        if let Some(ticker) = &mut inner.ticker
            && ticker.state().can_tick()
        {
            ticker.stop();
        }
        let status = inner.settled_status_directed();
        inner.status = status;
        let delivery = inner.active_run.take().map(TickerCompleter::cancel);
        self.finish(status, ValueChange::Notify, delivery, inner);
        TickerFuture::complete()
    }

    /// Repeat the animation, bouncing if `reverse` is true. Repeats forever.
    ///
    /// An infinite repeat's [`TickerFuture`] resolves only by cancellation —
    /// it has no natural end. A finite [`repeat_with`](Self::repeat_with)
    /// count completes normally once exhausted. See
    /// [`forward`](Self::forward) for the returned future's general contract.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn repeat(&self, reverse: bool) -> Result<TickerFuture, AnimationError> {
        self.repeat_with(None, None, reverse, None, None)
    }

    /// Repeat the animation with full control over range, period, and count.
    ///
    /// # Arguments
    ///
    /// * `min` - Lower endpoint of the repeat range (defaults to `lower_bound`)
    /// * `max` - Upper endpoint of the repeat range (defaults to `upper_bound`)
    /// * `reverse` - Bounce back and forth instead of restarting each cycle
    /// * `period` - Per-cycle duration (defaults to the forward duration)
    /// * `count` - Number of cycles; `None` repeats indefinitely (see
    ///   [`repeat`](Self::repeat) for what that means for the returned future)
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
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

        // Clamp the repeat range into the controller's bounds and reject an
        // empty/inverted range, so a repeat run can never start `value` (or its
        // ticks) outside `[lower_bound, upper_bound]` — consistent with
        // [`with_bounds`]'s `InvalidBounds` contract.
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
        inner.is_repeating = true;
        inner.repeat_reverse = reverse;
        inner.repeat_min = lo;
        inner.repeat_max = hi;
        inner.repeat_period = period;
        inner.repeat_count = count;
        inner.repeat_done = 0;
        inner.run_duration = None;
        inner.simulation = None;

        inner.value = lo;
        inner.direction = AnimationDirection::Forward;
        inner.status = AnimationStatus::Forward;
        inner.start_value = lo;
        inner.target_value = hi;
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
            ValueChange::Unchanged,
            displaced_delivery,
            inner,
        );
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
    /// Returns [`AnimationError::InvalidSpring`] if the spring is underdamped.
    pub fn fling_with(
        &self,
        velocity: f32,
        spring: Option<SpringDescription>,
    ) -> Result<TickerFuture, AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        let spring = spring.unwrap_or_else(default_fling_spring);
        inner.direction = if velocity < 0.0 {
            AnimationDirection::Reverse
        } else {
            AnimationDirection::Forward
        };
        let target = if velocity < 0.0 {
            inner.lower_bound - FLING_TOLERANCE.distance
        } else {
            inner.upper_bound + FLING_TOLERANCE.distance
        };

        let sim =
            SpringSimulation::new(spring, inner.value, target, velocity).with_snap_to_end(true);
        if sim.spring_type() == SpringType::Underdamped {
            return Err(AnimationError::InvalidSpring(
                "Underdamped springs oscillate and cannot be used for fling. \
                 Use animate_with() for oscillating springs."
                    .to_string(),
            ));
        }

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

    /// Drive the animation according to a custom simulation.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
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

        inner.clear_run_modes();
        inner.direction = direction;
        inner.status = direction.running_status();
        inner.value = simulation
            .x(0.0)
            .clamp(inner.lower_bound, inner.upper_bound);
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
    /// that re-zeros the run epoch).
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
    /// paths (`stop`/`reset`/`set_value`/zero-distance `forward`) leave it
    /// untouched. It wraps on `u64` overflow — only its *change* is observed, so
    /// the wrap is harmless.
    #[must_use]
    pub fn run_generation(&self) -> u64 {
        self.inner.lock().run_generation
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
    /// `start_value -> target_value`, simulations sample `x(t)`, and repeats
    /// advance their cycle epoch. Value and status listeners are fired only
    /// after the inner lock is released.
    pub fn tick_at(&self, raw_elapsed_secs: f64) {
        let mut inner = self.inner.lock();
        if !inner.status.is_running() {
            return;
        }
        inner.last_raw_elapsed_secs = raw_elapsed_secs;
        let dilated = raw_elapsed_secs / time_dilation().max(f64::MIN_POSITIVE);
        let cycle = (dilated - inner.run_epoch_secs).max(0.0);

        if inner.simulation.is_some() {
            self.tick_simulation(inner, narrow_f32(cycle));
        } else {
            self.tick_time_based(inner, dilated, cycle);
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
        let new_value = sim.x(cycle).clamp(inner.lower_bound, inner.upper_bound);
        let is_done = sim.is_done(cycle);
        inner.value = new_value;

        if is_done {
            inner.simulation = None;
            if let Some(ticker) = &mut inner.ticker {
                ticker.stop();
            }
            let status = inner.settled_status_directed();
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

    /// Time-based (tween) branch of [`tick_at`](Self::tick_at).
    fn tick_time_based(
        &self,
        mut inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        dilated: f64,
        cycle: f64,
    ) {
        let duration = inner.current_duration();
        let t = if duration.is_zero() {
            1.0
        } else {
            narrow_f32((cycle / duration.as_secs_f64()).clamp(0.0, 1.0))
        };
        let range = inner.target_value - inner.start_value;
        // Flutter parity: `_InterpolationSimulation.x` special-cases the
        // endpoints to the exact begin/end value and only runs the curve
        // through the interior, so a curve that overshoots slightly at its
        // bounds (e.g. an elastic curve) never reports outside [start, target].
        let eased_t = match (&inner.run_curve, t) {
            (_, 0.0) => 0.0,
            (_, t) if t >= 1.0 => 1.0,
            (Some(curve), t) => curve.transform(t),
            (None, t) => t,
        };
        inner.value = inner.start_value + range * eased_t;

        if t < 1.0 {
            drop(inner);
            self.notifier.notify_listeners();
            return;
        }

        // Cycle complete. Provisional: this is the end of the *first* spanned
        // cycle; the repeat-exhaustion branch below overwrites it with the
        // last cycle's endpoint when several cycles retire in one frame.
        inner.value = inner.target_value;

        if inner.is_repeating {
            let period = duration.as_secs_f64();
            // Retire every whole cycle this frame spanned, not just one. A long
            // frame (dt > period, e.g. after a dropped frame) elapses several
            // cycles at once; advancing count/epoch by a single cycle would leave
            // a finite repeat active an extra frame and an infinite repeat
            // permanently out of phase. `cycle >= period` here (t reached 1.0),
            // so `spanned >= 1`. Cost is O(1): the count is arithmetic and the
            // cycle transition is applied by parity, never looped.
            let spanned = if period > 0.0 {
                whole_cycles(cycle / period).max(1)
            } else {
                // Zero-period repeat: a finite count exhausts at once; an
                // infinite one would be unbounded, so retire one cycle per tick.
                inner
                    .repeat_count
                    .map_or(1, |count| count.saturating_sub(inner.repeat_done))
                    .max(1)
            };
            let cycles = match inner.repeat_count {
                Some(count) => spanned.min(count - inner.repeat_done),
                None => spanned,
            };
            inner.repeat_done += cycles;

            let exhausted = inner
                .repeat_count
                .is_some_and(|count| inner.repeat_done >= count);
            if exhausted {
                // Land on the end of the final (count-th) retired cycle. In
                // restart mode every cycle ends at `repeat_max`; in bounce mode
                // the end alternates, so for a multi-cycle frame it depends on
                // the parity of how many cycles were retired (the value set above
                // is only the first cycle's target). This also drives the settled
                // status below, so it must be correct before that read.
                inner.value = if inner.repeat_reverse {
                    let entry_forward = inner.direction == AnimationDirection::Forward;
                    let last_forward = entry_forward == (cycles % 2 == 1);
                    if last_forward {
                        inner.repeat_max
                    } else {
                        inner.repeat_min
                    }
                } else {
                    inner.repeat_max
                };
                if let Some(ticker) = &mut inner.ticker {
                    ticker.stop();
                }
                inner.is_repeating = false;
                let status = inner.settled_status_directed();
                inner.status = status;
                let delivery = inner.active_run.take().map(TickerCompleter::complete);
                self.finish(status, ValueChange::Notify, delivery, inner);
                return;
            }

            // Advance the epoch past every retired cycle (phase-preserving — the
            // remainder within the new cycle is interpolated on the next tick).
            inner.run_epoch_secs += f64::from(cycles) * period;
            let _ = dilated; // boundary time available if a future modulo path needs it
            // `begin_next_repeat_cycle` is an idempotent reset in restart mode
            // and a pure direction flip in bounce mode, so only the parity of the
            // retired-cycle count matters — collapse N cycles to at most one
            // transition rather than looping. In bounce mode an even retired
            // count cancels out (net no flip), so the cycle-begin step is
            // skipped entirely rather than calling it twice.
            if inner.repeat_reverse {
                if cycles % 2 == 1 {
                    inner.begin_next_repeat_cycle();
                }
            } else {
                inner.begin_next_repeat_cycle();
            }
            let status = inner.status;
            self.finish(status, ValueChange::Notify, None, inner);
            return;
        }

        // Non-repeating completion. Flutter parity: `_tick` reports the
        // settled status BY DIRECTION — completed after a forward run,
        // dismissed after a reverse one — with no at-a-bound requirement
        // (`animation_controller.dart:940-944`). Keeping the running status
        // for a mid-range stop (the previous behavior) starved every status
        // listener of the run's end: on an unbounded controller (a
        // scrollable's pixel-space fling controller) a driven `animate_to`
        // NEVER lands on a bound, so its completion was silent.
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }
        let status = inner.settled_status_directed();
        inner.status = status;
        // Publish before unlocking — see the simulation branch's own comment
        // for why the order matters to a panicking listener.
        let delivery = inner.active_run.take().map(TickerCompleter::complete);
        self.finish(status, ValueChange::Notify, delivery, inner);
    }

    /// Set the value directly without animating; recomputes status and notifies.
    ///
    /// Stops any active run first (Flutter parity: `AnimationController`'s
    /// `value=` setter calls `stop()` before `_internalSetValue`) — otherwise
    /// a live ticker keeps re-registering itself with the scheduler and the
    /// next frame recomputes the value from the stale run's `start_value`/
    /// `target_value`, silently overwriting what was just set.
    ///
    /// A `NaN` input is canonicalized to the lower bound: Rust's `clamp`
    /// propagates `NaN`, which would otherwise poison every downstream
    /// curve/tween evaluation for the rest of the controller's life.
    pub fn set_value(&self, value: f32) {
        let was_nan = value.is_nan();
        let mut inner = self.inner.lock();
        let delivery = inner.stop_running();
        // Canonicalize silently under the lock; `tracing::warn!`'s
        // subscriber is arbitrary user code and `delivery` is live here —
        // the warning itself waits for `Self::warn_if_nan` below, after
        // `finish` has unlocked and delivered.
        let value = if was_nan { inner.lower_bound } else { value };
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        self.finish(status, ValueChange::Notify, delivery, inner);
        Self::warn_if_nan(was_nan);
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
    /// `run_epoch_secs`/`last_raw_elapsed_secs`/`run_generation`/`ticker` —
    /// disjoint fields, so reordering the two calls is free.
    #[must_use]
    fn restart_ticker(&self, inner: &mut AnimationControllerInner) -> bool {
        inner.run_epoch_secs = 0.0;
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

    /// Emits the `set_value(NaN)` warning — call only after `finish` has
    /// unlocked and delivered, for the same reason as
    /// [`warn_if_no_ticker`](Self::warn_if_no_ticker).
    fn warn_if_nan(was_nan: bool) {
        if was_nan {
            tracing::warn!(
                "set_value(NaN) canonicalized to lower bound; drive the controller with finite values"
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
        self.is_repeating = false;
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
        if self.is_repeating
            && let Some(period) = self.repeat_period
        {
            return period;
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

    /// Dilated elapsed within the current cycle, from the last observed tick.
    fn cycle_elapsed_secs(&self) -> f64 {
        let dilated = self.last_raw_elapsed_secs / time_dilation().max(f64::MIN_POSITIVE);
        (dilated - self.run_epoch_secs).max(0.0)
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
    fn settled_status_keep_direction(&self) -> AnimationStatus {
        if self.is_at_upper_bound() {
            AnimationStatus::Completed
        } else if self.is_at_lower_bound() {
            AnimationStatus::Dismissed
        } else {
            self.direction.running_status()
        }
    }

    /// Set up the next repeat cycle: flips direction in bounce mode, restarts
    /// at `repeat_min` otherwise. Status change detection is the caller's
    /// job, via [`take_status_change`](Self::take_status_change).
    fn begin_next_repeat_cycle(&mut self) {
        if self.repeat_reverse {
            let was_forward = self.direction == AnimationDirection::Forward;
            self.direction = if was_forward {
                AnimationDirection::Reverse
            } else {
                AnimationDirection::Forward
            };
            self.status = self.direction.running_status();
            if was_forward {
                self.start_value = self.repeat_max;
                self.target_value = self.repeat_min;
            } else {
                self.start_value = self.repeat_min;
                self.target_value = self.repeat_max;
            }
        } else {
            self.direction = AnimationDirection::Forward;
            self.status = AnimationStatus::Forward;
            self.value = self.repeat_min;
            self.start_value = self.repeat_min;
            self.target_value = self.repeat_max;
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

    /// `without_ticker_bounds` is the fling/ballistic-simulation shape:
    /// wide-open bounds, no ticker.
    #[test]
    fn without_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones() {
        let _serial = serial();
        let rejected =
            AnimationController::without_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
        assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

        let c = AnimationController::without_ticker_bounds(
            Duration::from_millis(1),
            f32::NEG_INFINITY,
            f32::INFINITY,
        )
        .expect("NEG_INFINITY < INFINITY satisfies the bounds invariant");
        assert_eq!(c.value(), f32::NEG_INFINITY);
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
    /// mirrors `without_ticker_bounds`'s coverage of bounds validation and
    /// the wide-open fling/ballistic-simulation range.
    #[test]
    fn with_detached_ticker_bounds_rejects_invalid_bounds_and_accepts_wide_open_ones() {
        let _serial = serial();
        let rejected =
            AnimationController::with_detached_ticker_bounds(Duration::from_millis(1), 20.0, 10.0);
        assert!(matches!(rejected, Err(AnimationError::InvalidBounds(_))));

        let c = AnimationController::with_detached_ticker_bounds(
            Duration::from_millis(1),
            f32::NEG_INFINITY,
            f32::INFINITY,
        )
        .expect("NEG_INFINITY < INFINITY satisfies the bounds invariant");
        assert_eq!(c.value(), f32::NEG_INFINITY);
        c.dispose();
    }

    #[test]
    fn disposed_controller_rejects_forward() {
        let _serial = serial();
        let c = controller(100);
        c.dispose();
        assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
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

    #[test]
    fn repeat_with_clamps_range_into_bounds() {
        let _serial = serial();
        let c = controller(100);
        // Out-of-bounds min/max are clamped into [0, 1]; the run starts at the
        // clamped min and never leaves the controller bounds.
        c.repeat_with(
            Some(-5.0),
            Some(5.0),
            false,
            Some(Duration::from_millis(10)),
            None,
        )
        .unwrap();
        assert_eq!(c.value(), 0.0, "clamped min = lower_bound");
        c.tick_at(0.005); // mid-cycle
        assert!(
            c.value() >= 0.0 && c.value() <= 1.0,
            "stays within bounds: {}",
            c.value()
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
    fn zero_duration_forward_completes_on_the_first_scheduler_driven_frame() {
        let _serial = serial();
        let scheduler = UpdateScheduler::new();
        let c = AnimationController::new(Duration::ZERO, &scheduler);
        let future = c.forward().unwrap();
        assert!(future.is_pending());

        scheduler.execute_frame();

        assert!(
            future.is_complete(),
            "tick_time_based's is_zero => t = 1.0 branch must complete a \
             zero-duration run on its very first tick"
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
        // `Duration::ZERO` completes the reverse leg on the FIRST
        // `execute_frame()` regardless of real elapsed time
        // (`tick_time_based`'s `duration.is_zero() => t = 1.0`, proven by
        // `zero_duration_forward_completes_on_the_first_scheduler_driven_frame`)
        // — deterministic, unlike waiting on a real millisecond duration,
        // and needs no `thread::sleep`. The chained leg below still takes
        // its own explicit 10s duration regardless of this controller's
        // base duration, so it is still in flight when `stop()` cancels it.
        let c = AnimationController::new(Duration::ZERO, &scheduler);
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
