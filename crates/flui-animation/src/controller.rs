//! `AnimationController` - The primary animation driver.

use crate::animation::{Animation, AnimationDirection, StatusCallback};
use crate::error::AnimationError;
use crate::simulation::{Simulation, SpringDescription, SpringSimulation, SpringType, Tolerance};
use crate::status::AnimationStatus;
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_scheduler::config::time_dilation;
use flui_scheduler::{Scheduler, Ticker};
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
#[allow(clippy::cast_possible_truncation)]
#[inline]
fn narrow_f32(x: f64) -> f32 {
    x as f32
}

/// Floor a non-negative cycle ratio to a whole repeat-cycle count. Used by the
/// repeat tick to retire every cycle a long frame elapsed; `as u32` saturates a
/// pathological ratio to `u32::MAX` rather than wrapping.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
/// use flui_scheduler::Scheduler;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let controller = AnimationController::new(
///     Duration::from_millis(300),
///     scheduler,
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

    /// Ticker for frame callbacks (auto-scheduling via the attached `Scheduler`).
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
}

impl AnimationController {
    /// Create a new animation controller.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - Scheduler for frame coordination
    #[must_use]
    pub fn new(duration: Duration, scheduler: Arc<Scheduler>) -> Self {
        // 0.0 < 1.0 always holds, so the default-bounds path cannot fail.
        Self::with_bounds(duration, scheduler, 0.0, 1.0)
            .expect("default bounds (0.0, 1.0) satisfy lower < upper")
    }

    /// Create an animation controller with custom bounds.
    ///
    /// # Arguments
    ///
    /// * `duration` - Duration of the forward animation
    /// * `scheduler` - Scheduler for frame coordination
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
    /// use flui_scheduler::Scheduler;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let scheduler = Arc::new(Scheduler::new());
    /// let controller = AnimationController::with_bounds(
    ///     Duration::from_millis(300),
    ///     scheduler,
    ///     0.0,
    ///     100.0,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_bounds(
        duration: Duration,
        scheduler: Arc<Scheduler>,
        lower_bound: f32,
        upper_bound: f32,
    ) -> Result<Self, AnimationError> {
        if lower_bound >= upper_bound {
            return Err(AnimationError::InvalidBounds(format!(
                "lower_bound ({lower_bound}) must be less than upper_bound ({upper_bound})"
            )));
        }

        let notifier = Arc::new(ChangeNotifier::new());
        let ticker = Ticker::new_with_scheduler(scheduler);

        let inner = AnimationControllerInner {
            value: lower_bound,
            status: AnimationStatus::Dismissed,
            duration,
            reverse_duration: None,
            lower_bound,
            upper_bound,
            ticker: Some(ticker),
            status_listeners: Vec::new(),
            direction: AnimationDirection::Forward,
            start_value: lower_bound,
            target_value: upper_bound,
            run_epoch_secs: 0.0,
            last_raw_elapsed_secs: 0.0,
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

    /// Start animation forward from current value to upper bound.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward(&self) -> Result<(), AnimationError> {
        self.forward_from(None)
    }

    /// Start animation forward from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn forward_from(&self, from: Option<f32>) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Forward;
        inner.status = AnimationStatus::Forward;
        inner.start_value = inner.value;
        inner.target_value = inner.upper_bound;
        self.restart_ticker(&mut inner);

        Self::emit_status_after_unlock(inner, AnimationStatus::Forward);
        Ok(())
    }

    /// Start animation in reverse from current value to lower bound.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse(&self) -> Result<(), AnimationError> {
        self.reverse_from(None)
    }

    /// Start animation in reverse from a specific value.
    ///
    /// If `from` is `None`, starts from the current value.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reverse_from(&self, from: Option<f32>) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        if let Some(start) = from {
            inner.value = start.clamp(inner.lower_bound, inner.upper_bound);
        }

        inner.clear_run_modes();
        inner.direction = AnimationDirection::Reverse;
        inner.status = AnimationStatus::Reverse;
        inner.start_value = inner.value;
        inner.target_value = inner.lower_bound;
        self.restart_ticker(&mut inner);

        Self::emit_status_after_unlock(inner, AnimationStatus::Reverse);
        Ok(())
    }

    /// Stop the animation at its current value.
    ///
    /// The status is updated based on the current value:
    /// - [`AnimationStatus::Completed`] at the upper bound
    /// - [`AnimationStatus::Dismissed`] at the lower bound
    /// - the previous direction status if stopped in the middle
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn stop(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        inner.clear_run_modes();
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }

        let status = inner.settled_status_keep_direction();
        inner.status = status;
        Self::emit_status_after_unlock(inner, status);
        Ok(())
    }

    /// Reset to the beginning (lower bound).
    ///
    /// Sets the value to `lower_bound` and the status to
    /// [`AnimationStatus::Dismissed`].
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn reset(&self) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        inner.clear_run_modes();
        inner.value = inner.lower_bound;
        inner.status = AnimationStatus::Dismissed;
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }

        let callbacks = Self::snapshot_status_listeners(&inner);
        drop(inner);
        self.notifier.notify_listeners();
        Self::fire_status(&callbacks, AnimationStatus::Dismissed);
        Ok(())
    }

    /// Animate to a specific value over `duration` (or the controller's forward
    /// duration when `None`).
    ///
    /// The per-run `duration` override applies to **this run only** and does not
    /// modify the controller's base duration.
    ///
    /// # Arguments
    ///
    /// * `target` - The target value (clamped to bounds)
    /// * `duration` - Optional per-run duration override
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn animate_to(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<(), AnimationError> {
        self.drive_to(target, duration)
    }

    /// Animate back to a specific value, defaulting to the reverse duration.
    ///
    /// Like [`animate_to`](Self::animate_to) but, when `duration` is `None`,
    /// defaults to the configured reverse duration (then the base duration).
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn animate_back(
        &self,
        target: f32,
        duration: Option<Duration>,
    ) -> Result<(), AnimationError> {
        let fallback = self.inner.lock().reverse_duration;
        self.drive_to(target, duration.or(fallback))
    }

    /// Shared driver for [`animate_to`](Self::animate_to)/[`animate_back`](Self::animate_back):
    /// linearly interpolate from the current value to `target`, picking
    /// direction from their order.
    fn drive_to(&self, target: f32, duration: Option<Duration>) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        let target = target.clamp(inner.lower_bound, inner.upper_bound);
        inner.clear_run_modes();
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
            inner.value = target;
            if let Some(ticker) = &mut inner.ticker
                && ticker.state().can_tick()
            {
                ticker.stop();
            }
            let status = inner.settled_status_directed();
            inner.status = status;
            let callbacks = Self::snapshot_status_listeners(&inner);
            drop(inner);
            self.notifier.notify_listeners();
            Self::fire_status(&callbacks, status);
            return Ok(());
        }

        inner.status = inner.direction.running_status();
        // Per-run override only — never clobber `inner.duration`.
        inner.run_duration = duration;
        self.restart_ticker(&mut inner);

        let status = inner.status;
        Self::emit_status_after_unlock(inner, status);
        Ok(())
    }

    /// Repeat the animation, bouncing if `reverse` is true. Repeats forever.
    ///
    /// # Errors
    ///
    /// Returns [`AnimationError::Disposed`] if the controller has been disposed.
    pub fn repeat(&self, reverse: bool) -> Result<(), AnimationError> {
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
    /// * `count` - Number of cycles; `None` repeats indefinitely
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
    ) -> Result<(), AnimationError> {
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
        self.restart_ticker(&mut inner);

        Self::emit_status_after_unlock(inner, AnimationStatus::Forward);
        Ok(())
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
    /// # use flui_scheduler::Scheduler;
    /// # use std::sync::Arc;
    /// # use std::time::Duration;
    /// # let scheduler = Arc::new(Scheduler::new());
    /// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    /// controller.fling(1.0).unwrap(); // Fling forward
    /// ```
    pub fn fling(&self, velocity: f32) -> Result<(), AnimationError> {
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
    ) -> Result<(), AnimationError> {
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
        self.restart_ticker(&mut inner);

        let status = inner.status;
        Self::emit_status_after_unlock(inner, status);
        Ok(())
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
    /// # use flui_scheduler::Scheduler;
    /// # use std::sync::Arc;
    /// # use std::time::Duration;
    /// # let scheduler = Arc::new(Scheduler::new());
    /// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    /// let spring = SpringDescription::with_damping_ratio(1.0, 300.0, 0.5);
    /// let sim = SpringSimulation::new(spring, 0.0, 1.0, 0.0);
    /// controller.animate_with(sim).unwrap();
    /// ```
    pub fn animate_with<S: Simulation + 'static>(
        &self,
        simulation: S,
    ) -> Result<(), AnimationError> {
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
    ) -> Result<(), AnimationError> {
        self.drive_simulation(Box::new(simulation), AnimationDirection::Reverse)
    }

    fn drive_simulation(
        &self,
        simulation: Box<dyn Simulation>,
        direction: AnimationDirection,
    ) -> Result<(), AnimationError> {
        let mut inner = self.inner.lock();
        Self::check_disposed(&inner)?;

        inner.clear_run_modes();
        inner.direction = direction;
        inner.status = direction.running_status();
        inner.value = simulation
            .x(0.0)
            .clamp(inner.lower_bound, inner.upper_bound);
        inner.simulation = Some(simulation);
        self.restart_ticker(&mut inner);

        let status = inner.status;
        Self::emit_status_after_unlock(inner, status);
        Ok(())
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
            let callbacks = Self::snapshot_status_listeners(&inner);
            drop(inner);
            self.notifier.notify_listeners();
            Self::fire_status(&callbacks, status);
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
        inner.value = inner.start_value + range * t;

        if t < 1.0 {
            drop(inner);
            self.notifier.notify_listeners();
            return;
        }

        // Cycle complete.
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
                let callbacks = Self::snapshot_status_listeners(&inner);
                drop(inner);
                self.notifier.notify_listeners();
                Self::fire_status(&callbacks, status);
                return;
            }

            // Advance the epoch past every retired cycle (phase-preserving — the
            // remainder within the new cycle is interpolated on the next tick).
            inner.run_epoch_secs += f64::from(cycles) * period;
            let _ = dilated; // boundary time available if a future modulo path needs it
            // `begin_next_repeat_cycle` is an idempotent reset in restart mode
            // and a pure direction flip in bounce mode, so only the parity of the
            // retired-cycle count matters — collapse N cycles to at most one
            // transition rather than looping.
            let status_changed = if inner.repeat_reverse {
                cycles % 2 == 1 && inner.begin_next_repeat_cycle()
            } else {
                inner.begin_next_repeat_cycle()
            };
            let status = inner.status;
            let callbacks = status_changed.then(|| Self::snapshot_status_listeners(&inner));
            drop(inner);
            self.notifier.notify_listeners();
            if let Some(callbacks) = callbacks {
                Self::fire_status(&callbacks, status);
            }
            return;
        }

        // Non-repeating completion.
        if let Some(ticker) = &mut inner.ticker {
            ticker.stop();
        }
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        let callbacks = Self::snapshot_status_listeners(&inner);
        drop(inner);
        self.notifier.notify_listeners();
        Self::fire_status(&callbacks, status);
    }

    /// Set the value directly without animating; recomputes status and notifies.
    pub fn set_value(&self, value: f32) {
        let mut inner = self.inner.lock();
        inner.value = value.clamp(inner.lower_bound, inner.upper_bound);
        let status = inner.settled_status_keep_direction();
        inner.status = status;
        let callbacks = Self::snapshot_status_listeners(&inner);
        drop(inner);
        self.notifier.notify_listeners();
        Self::fire_status(&callbacks, status);
    }

    /// **CRITICAL:** Dispose when done to prevent leaks.
    ///
    /// Stops the animation and clears resources. Idempotent.
    pub fn dispose(&self) {
        let mut inner = self.inner.lock();
        if inner.disposed {
            return;
        }
        if let Some(mut ticker) = inner.ticker.take() {
            ticker.stop();
        }
        inner.status_listeners.clear();
        inner.disposed = true;
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

    /// Reset run state and (re)start the ticker for a fresh run from epoch 0.
    fn restart_ticker(&self, inner: &mut AnimationControllerInner) {
        inner.run_epoch_secs = 0.0;
        inner.last_raw_elapsed_secs = 0.0;
        if let Some(ticker) = &mut inner.ticker {
            // Restart-safe: `Ticker::start` debug-asserts on the Active state, and
            // controller methods can be called repeatedly, so stop a live run first.
            if ticker.state().can_tick() {
                ticker.stop();
            }
            let controller = self.clone();
            ticker.start(move |elapsed| controller.tick_at(elapsed));
        } else {
            tracing::warn!("AnimationController has no ticker; the animation will not advance");
        }
    }

    /// Snapshot status callbacks so they can be fired after the lock is dropped.
    fn snapshot_status_listeners(
        inner: &AnimationControllerInner,
    ) -> SmallVec<[StatusCallback; 4]> {
        inner
            .status_listeners
            .iter()
            .map(|(_, cb)| Arc::clone(cb))
            .collect()
    }

    /// Fire status callbacks. MUST be called with no controller lock held.
    fn fire_status(callbacks: &[StatusCallback], status: AnimationStatus) {
        for cb in callbacks {
            cb(status);
        }
    }

    /// Drop the inner lock, then fire status listeners for `status`.
    ///
    /// Used by the run-start methods, which change status but not value.
    fn emit_status_after_unlock(
        inner: parking_lot::MutexGuard<'_, AnimationControllerInner>,
        status: AnimationStatus,
    ) {
        let callbacks = Self::snapshot_status_listeners(&inner);
        drop(inner);
        Self::fire_status(&callbacks, status);
    }
}

impl AnimationControllerInner {
    /// Clear repeat/simulation/per-run-duration modes (used when a new explicit
    /// run begins).
    fn clear_run_modes(&mut self) {
        self.is_repeating = false;
        self.run_duration = None;
        self.simulation = None;
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

    /// Dilated elapsed within the current cycle, from the last observed tick.
    fn cycle_elapsed_secs(&self) -> f64 {
        let dilated = self.last_raw_elapsed_secs / time_dilation().max(f64::MIN_POSITIVE);
        (dilated - self.run_epoch_secs).max(0.0)
    }

    /// Status at a settled value, mapping non-bound stops by direction.
    fn settled_status_directed(&self) -> AnimationStatus {
        if (self.value - self.upper_bound).abs() < BOUND_EPSILON {
            AnimationStatus::Completed
        } else if (self.value - self.lower_bound).abs() < BOUND_EPSILON {
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
        if (self.value - self.upper_bound).abs() < BOUND_EPSILON {
            AnimationStatus::Completed
        } else if (self.value - self.lower_bound).abs() < BOUND_EPSILON {
            AnimationStatus::Dismissed
        } else {
            self.direction.running_status()
        }
    }

    /// Set up the next repeat cycle; returns whether the status changed (only
    /// the bounce path flips Forward<->Reverse).
    fn begin_next_repeat_cycle(&mut self) -> bool {
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
            true
        } else {
            self.direction = AnimationDirection::Forward;
            self.status = AnimationStatus::Forward;
            self.value = self.repeat_min;
            self.start_value = self.repeat_min;
            self.target_value = self.repeat_max;
            false
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
    use flui_scheduler::Scheduler;
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
        let scheduler = Arc::new(Scheduler::new());
        AnimationController::new(Duration::from_millis(ms), scheduler)
    }

    #[test]
    fn creation_starts_dismissed_at_lower_bound() {
        let _serial = serial();
        let c = controller(100);
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), AnimationStatus::Dismissed);
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
        let scheduler = Arc::new(Scheduler::new());
        let c = AnimationController::with_bounds(Duration::from_millis(100), scheduler, 10.0, 20.0)
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
        let scheduler = Arc::new(Scheduler::new());
        let r = AnimationController::with_bounds(Duration::from_millis(100), scheduler, 20.0, 10.0);
        assert!(matches!(r, Err(AnimationError::InvalidBounds(_))));
    }

    #[test]
    fn disposed_controller_rejects_forward() {
        let _serial = serial();
        let c = controller(100);
        c.dispose();
        assert!(matches!(c.forward(), Err(AnimationError::Disposed)));
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
}
