//! Animation ticker - frame-perfect animation timing
//!
//! Tickers provide a way to receive callbacks on every frame for driving
//! animations. They coordinate with the scheduler to ensure animations stay
//! synchronized with the display refresh rate.
//!
//! ## Single Canonical Ticker
//!
//! Auto-scheduling is absorbed into a single canonical [`Ticker`]
//! type. It supports two driving modes selected at construction:
//!
//! - **Manual tick** ([`Ticker::new`]): caller drives ticks via
//!   [`Ticker::tick`] each frame. Used by tests, custom render loops, and
//!   embedders that own their own frame scheduler.
//! - **Auto-schedule** ([`Ticker::new_with_scheduler`] / vended via
//!   [`TickerProvider::create_ticker`] on a [`Scheduler`](crate::scheduler::Scheduler)): the ticker
//!   self-registers a transient frame callback on every start/unmute,
//!   matching Flutter [`ticker.dart:283`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
//!   `scheduleTick(rescheduling: true)`. `stop`/`mute`/`dispose` cancel the
//!   pending callback via [`Scheduler::cancel_frame_callback`](crate::scheduler::Scheduler::cancel_frame_callback).
//!
//! ## Manual Ticker Example
//!
//! ```rust
//! use flui_scheduler::{Scheduler, Ticker, TickerProvider};
//!
//! let scheduler = Scheduler::new();
//! let mut ticker = Ticker::new();
//!
//! ticker.start(|elapsed| {
//!     println!("Frame at {:.3}s", elapsed);
//! });
//!
//! // In your render loop - manual tick
//! ticker.tick(&scheduler);
//! ```
//!
//! ## Auto-scheduling Ticker Example (Flutter-like)
//!
//! ```rust
//! use std::sync::Arc;
//!
//! use flui_scheduler::{Scheduler, Ticker};
//!
//! let scheduler = Arc::new(Scheduler::new());
//! let mut ticker = Ticker::new_with_scheduler(Arc::clone(&scheduler));
//!
//! // Start auto-registers a transient frame callback that fires every frame
//! ticker.start(|elapsed| {
//!     println!("Auto-ticked at {:.3}s", elapsed);
//! });
//!
//! // Each frame, the ticker fires its callback and re-schedules itself —
//! // no need to manually call tick().
//! ```

use std::sync::Arc;

use parking_lot::Mutex;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use web_time::Instant;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::duration::Seconds;
use crate::id::CallbackId;

/// Unique ticker identifier from `flui_foundation`.
pub use flui_foundation::TickerId;

/// Generate the next unique ticker ID using a global atomic counter.
fn next_ticker_id() -> TickerId {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    TickerId::zip(value)
}

/// Ticker callback - receives elapsed time in seconds
pub type TickerCallback = Box<dyn FnMut(f64) + Send>;

/// Ticker provider trait — Flutter-faithful factory shape.
///
/// Flutter parity: [`ticker.dart:248`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
/// `Ticker createTicker(TickerCallback)`. The provider vends an owned
/// [`Ticker`] preloaded with the caller-supplied callback; the caller drives
/// state transitions via `start`/`stop`/`mute`/`unmute`/`dispose`.
///
/// This trait allows different parts of the framework to provide ticker
/// functionality without tight coupling to the scheduler.
///
/// The default impl produces a manually-driven [`Ticker`] (no auto-schedule).
/// Implementors that own a [`Scheduler`](crate::scheduler::Scheduler) (e.g. `impl TickerProvider for
/// Scheduler`) override [`create_ticker`](Self::create_ticker) to vend an
/// auto-scheduling ticker via [`Ticker::new_with_scheduler`].
pub trait TickerProvider: Send + Sync {
    /// Create a fresh ticker preloaded with the given callback.
    ///
    /// Returns a ticker in [`TickerState::Idle`]. The caller must call
    /// [`Ticker::start_default`] (or [`Ticker::start`] with an explicit
    /// override) to begin ticking.
    ///
    /// Flutter parity: `ticker.dart:248 Ticker createTicker(TickerCallback)`.
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker {
        let mut ticker = Ticker::new();
        ticker.set_pending_callback(on_tick);
        ticker
    }
}

/// State of a ticker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum TickerState {
    /// Not started yet
    #[default]
    Idle = 0,

    /// Currently ticking
    Active = 1,

    /// Temporarily paused
    Muted = 2,

    /// Permanently stopped
    Stopped = 3,
}

impl TickerState {
    /// Check if the ticker can be ticked
    #[inline]
    pub const fn can_tick(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if the ticker is running (Active or Muted)
    #[inline]
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Active | Self::Muted)
    }

    /// Check if the ticker can be started
    #[inline]
    pub const fn can_start(self) -> bool {
        matches!(self, Self::Idle | Self::Stopped)
    }
}

/// Shared inner state for a Ticker (single allocation, single lock)
struct TickerInner {
    state: TickerState,
    start_time: Option<Instant>,
    callback: Option<TickerCallback>,
    muted_elapsed: Seconds,
    /// Future for the currently active ticker run.
    ///
    /// Created by `start` / `start_default`, completed by `stop`, and
    /// canceled by `dispose` / `reset`. Stored here so lifecycle methods can
    /// resolve the exact run that produced the handle returned to callers.
    active_future: Option<TickerFuture>,
    /// Pending transient frame callback ID — set when an auto-scheduling
    /// ticker has registered itself with the scheduler for the next frame,
    /// cleared on `stop`/`mute`/`dispose` or when the callback fires.
    ///
    /// `None` for manually-driven tickers (no [`Scheduler`](crate::scheduler::Scheduler) attached) and
    /// auto-scheduling tickers that are not currently registered.
    ///
    /// Flutter parity: `ticker.dart:254 _animationId` (sentinel for
    /// "already-scheduled").
    scheduled_callback_id: Option<CallbackId>,
}

/// Animation ticker with runtime state management
///
/// A Ticker provides callbacks on every frame, allowing you to drive animations
/// in sync with the display refresh rate.
///
/// Lifecycle state is explicit at runtime: `start` returns a
/// [`TickerFuture`], `stop` completes it, and `dispose` / `Drop` cancel it.
///
/// # Examples
///
/// ```
/// use flui_scheduler::ticker::{Ticker, TickerState};
///
/// let mut ticker = Ticker::new();
/// assert_eq!(ticker.state(), TickerState::Idle);
///
/// ticker.start(|elapsed| {
///     println!("Elapsed: {:.3}s", elapsed);
/// });
/// assert_eq!(ticker.state(), TickerState::Active);
///
/// // Mute temporarily
/// ticker.mute();
/// assert_eq!(ticker.state(), TickerState::Muted);
///
/// // Resume
/// ticker.unmute();
/// assert_eq!(ticker.state(), TickerState::Active);
/// ```
pub struct Ticker {
    /// Unique identifier
    id: TickerId,

    /// All mutable state behind a single lock
    inner: Arc<Mutex<TickerInner>>,

    /// Optional scheduler attached at construction for auto-rescheduling.
    ///
    /// - `None`: manually-driven ticker — caller invokes [`Ticker::tick`]
    ///   per frame.
    /// - `Some`: auto-scheduling ticker — [`start`](Self::start) /
    ///   [`unmute`](Self::unmute) register a transient frame callback that
    ///   fires the user callback and re-schedules itself; [`stop`](Self::stop)
    ///   / [`mute`](Self::mute) / [`dispose`](Self::dispose) cancel the
    ///   pending callback via [`Scheduler::cancel_frame_callback`](crate::scheduler::Scheduler::cancel_frame_callback).
    ///
    /// Flutter parity: `Ticker(this._onTick, ...)` ([`ticker.dart:80`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart))
    /// implicitly carries `SchedulerBinding.instance` (singleton); FLUI
    /// stores the scheduler explicitly to keep the dependency typed and
    /// avoid the singleton-acquisition cost on the hot path.
    scheduler: Option<Arc<crate::scheduler::Scheduler>>,

    /// Disposed-state flag (lock-free).
    ///
    /// Set once on `dispose()`. After that, all public methods are no-ops in
    /// release mode and panic via `debug_assert!` in debug. Matches the PR #84
    /// `ChangeNotifier::dispose` pattern at
    /// [`flui-foundation/src/notifier.rs`](../../crates/flui-foundation/src/notifier.rs)
    /// and Flutter [`ticker.dart:362-379`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart).
    disposed: Arc<AtomicBool>,
}

impl Ticker {
    /// Create a new manually-driven ticker.
    ///
    /// The caller must invoke [`Ticker::tick`] each frame to fire the
    /// callback. For an auto-scheduling ticker, use
    /// [`Ticker::new_with_scheduler`] or call
    /// [`TickerProvider::create_ticker`] on a
    /// [`Scheduler`](crate::scheduler::Scheduler).
    pub fn new() -> Self {
        Self {
            id: next_ticker_id(),
            inner: Arc::new(Mutex::new(TickerInner {
                state: TickerState::Idle,
                start_time: None,
                callback: None,
                muted_elapsed: Seconds::ZERO,
                active_future: None,
                scheduled_callback_id: None,
            })),
            scheduler: None,
            disposed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a new auto-scheduling ticker attached to `scheduler`.
    ///
    /// After [`start`](Self::start) or [`unmute`](Self::unmute) is called, the
    /// ticker self-registers a transient frame callback that fires the user
    /// callback and re-schedules itself on each frame, matching Flutter
    /// [`ticker.dart:283`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `scheduleTick(rescheduling: true)`.
    ///
    /// [`stop`](Self::stop) / [`mute`](Self::mute) / [`dispose`](Self::dispose)
    /// cancel the pending callback via
    /// [`Scheduler::cancel_frame_callback`](crate::scheduler::Scheduler::cancel_frame_callback).
    pub fn new_with_scheduler(scheduler: Arc<crate::scheduler::Scheduler>) -> Self {
        Self {
            id: next_ticker_id(),
            inner: Arc::new(Mutex::new(TickerInner {
                state: TickerState::Idle,
                start_time: None,
                callback: None,
                muted_elapsed: Seconds::ZERO,
                active_future: None,
                scheduled_callback_id: None,
            })),
            scheduler: Some(scheduler),
            disposed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the ticker ID
    #[inline]
    pub fn id(&self) -> TickerId {
        self.id
    }

    /// Returns true if `dispose()` has been called.
    #[inline]
    pub fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::Acquire)
    }

    /// Pre-load a callback to be used when `start()` is called without
    /// passing one.
    ///
    /// Used by [`TickerProvider::create_ticker`] (Flutter factory shape) to
    /// vend a ticker preloaded with its tick callback. The callback is
    /// installed into [`TickerInner::callback`] when [`start`](Self::start) is
    /// next invoked without a callback argument; explicit `start(callback)`
    /// overrides any preloaded value.
    pub(crate) fn set_pending_callback(&mut self, callback: TickerCallback) {
        self.inner.lock().callback = Some(callback);
    }

    /// Dispose of the ticker — idempotent.
    ///
    /// Clears the callback, sets state to Stopped, cancels the active
    /// [`TickerFuture`], cancels any pending transient frame callback
    /// (auto-scheduling tickers), and marks disposed. Subsequent calls to
    /// `start`/`stop`/`mute`/`unmute`/`reset`/`tick` panic in debug builds via
    /// [`debug_assert!`] and emit a `tracing::warn!` + no-op in release.
    ///
    /// Mirrors Flutter [`ticker.dart:362-379`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `@mustCallSuper dispose()` semantics and PR #84's `ChangeNotifier::dispose`
    /// adoption template.
    pub fn dispose(&mut self) {
        if self.disposed.swap(true, Ordering::Release) {
            return; // already disposed — idempotent
        }
        let (pending_id, active_future) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Stopped;
            inner.callback = None;
            inner.start_time = None;
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
            )
        };
        if let Some(future) = active_future {
            future.set_canceled();
        }
        // Cancel pending transient callback outside the inner lock to avoid
        // lock-during-callback hazard (scheduler may also take its own locks).
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref()) {
            scheduler.cancel_frame_callback(id);
        }
    }

    /// Debug-assert that this ticker hasn't been disposed. Release builds
    /// emit a tracing warning instead of panicking.
    #[inline]
    fn assert_not_disposed(&self, op: &'static str) -> bool {
        if self.disposed.load(Ordering::Acquire) {
            debug_assert!(false, "Ticker::{op} called after dispose");
            tracing::warn!(op, ticker_id = ?self.id, "Ticker used after dispose");
            return false;
        }
        true
    }

    /// Start the ticker with a callback.
    ///
    /// The callback receives the elapsed time in seconds since start.
    /// Returns a [`TickerFuture`] that completes when [`stop`](Self::stop) is
    /// called and is canceled by [`dispose`](Self::dispose) / [`reset`](Self::reset).
    /// Overrides any callback pre-loaded via [`TickerProvider::create_ticker`].
    ///
    /// # Panics
    ///
    /// Debug-asserts that the ticker has not been disposed and that it is
    /// not already in [`TickerState::Active`] (matches Flutter
    /// [`ticker.dart:188`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `throw FlutterError('A ticker was started twice.')`).
    pub fn start<F>(&mut self, callback: F) -> TickerFuture
    where
        F: FnMut(f64) + Send + 'static,
    {
        self.start_inner(Some(Box::new(callback)))
    }

    /// Start the ticker using the callback installed via
    /// [`TickerProvider::create_ticker`].
    ///
    /// Returns a [`TickerFuture`] for the active run. If no callback is
    /// pre-loaded, this is a no-op and returns an already-complete future.
    ///
    /// Returns immediately as a no-op if no callback is pre-loaded. This
    /// fixes an earlier bug where `create_ticker` stored the callback but
    /// `start` required a fresh one to be passed in, leaving the
    /// pre-loaded callback unreachable.
    pub fn start_default(&mut self) -> TickerFuture {
        self.start_inner(None)
    }

    fn start_inner(&mut self, callback: Option<TickerCallback>) -> TickerFuture {
        if !self.assert_not_disposed("start") {
            return TickerFuture::canceled();
        }
        let future = TickerFuture::new();
        {
            let mut inner = self.inner.lock();
            debug_assert!(
                inner.state != TickerState::Active,
                "A ticker was started twice (id={:?})",
                self.id
            );
            if inner.state == TickerState::Active {
                tracing::error!(
                    ticker_id = ?self.id,
                    "Ticker::start called while already Active"
                );
                return inner
                    .active_future
                    .clone()
                    .unwrap_or_else(TickerFuture::canceled);
            }
            if let Some(cb) = callback {
                // Explicit callback overrides any pre-loaded one from create_ticker.
                inner.callback = Some(cb);
            } else if inner.callback.is_none() {
                // No explicit callback, no pre-loaded callback — start is a no-op
                // (tick has nothing to dispatch). Logged for diagnostic.
                tracing::warn!(
                    ticker_id = ?self.id,
                    "Ticker::start_default called without a pre-loaded callback (no-op)"
                );
                return TickerFuture::complete();
            }
            inner.state = TickerState::Active;
            inner.start_time = Some(Instant::now());
            inner.muted_elapsed = Seconds::ZERO;
            inner.active_future = Some(future.clone());
        }
        // Auto-scheduling tickers register a transient frame callback now.
        // Flutter parity: `ticker.dart:200-202 if (shouldScheduleTick)
        // scheduleTick()`.
        self.schedule_tick_if_active();
        future
    }

    /// Start the ticker with a type-safe callback
    pub fn start_typed<F>(&mut self, mut callback: F) -> TickerFuture
    where
        F: FnMut(Seconds) + Send + 'static,
    {
        self.start(move |elapsed| callback(Seconds::new(elapsed)))
    }

    /// Stop the ticker.
    ///
    /// This permanently stops the ticker and completes the active
    /// [`TickerFuture`] normally. Cancels any pending transient frame callback
    /// (auto-scheduling tickers). Call [`start`](Self::start) to restart.
    pub fn stop(&mut self) {
        if !self.assert_not_disposed("stop") {
            return;
        }
        let (pending_id, active_future) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Stopped;
            inner.callback = None;
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
            )
        };
        if let Some(future) = active_future {
            future.set_complete();
        }
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref()) {
            scheduler.cancel_frame_callback(id);
        }
    }

    /// Mute the ticker.
    ///
    /// This temporarily pauses the ticker without clearing the callback.
    /// Time does not advance while muted. Cancels any pending transient
    /// frame callback (auto-scheduling tickers) — matches Flutter
    /// [`ticker.dart:124-128`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// where `muted = true` calls `unscheduleTick()`.
    pub fn mute(&mut self) {
        if !self.assert_not_disposed("mute") {
            return;
        }
        let pending_id = {
            let mut inner = self.inner.lock();
            if inner.state == TickerState::Active {
                if let Some(start) = inner.start_time {
                    inner.muted_elapsed = Seconds::new(start.elapsed().as_secs_f64());
                }
                inner.state = TickerState::Muted;
                inner.scheduled_callback_id.take()
            } else {
                None
            }
        };
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref()) {
            scheduler.cancel_frame_callback(id);
        }
    }

    /// Unmute the ticker.
    ///
    /// Resumes a muted ticker. Time continues from where it was paused.
    /// Re-registers the auto-scheduling transient callback if attached to
    /// a scheduler — matches Flutter
    /// [`ticker.dart:126-128`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// where setting `muted = false` calls `scheduleTick()` when
    /// `shouldScheduleTick`.
    pub fn unmute(&mut self) {
        if !self.assert_not_disposed("unmute") {
            return;
        }
        {
            let mut inner = self.inner.lock();
            if inner.state == TickerState::Muted {
                let now = Instant::now();
                let adjusted_start = now
                    .checked_sub(std::time::Duration::from_secs_f64(
                        inner.muted_elapsed.value(),
                    ))
                    .expect(
                        "BUG: muted_elapsed was measured as (mute instant - start_time), so \
                         subtracting it from a later `now` cannot precede the ticker's start \
                         instant, which is a valid Instant",
                    );
                inner.start_time = Some(adjusted_start);
                inner.state = TickerState::Active;
            }
        }
        self.schedule_tick_if_active();
    }

    /// Toggle mute state
    pub fn toggle_mute(&mut self) {
        let state = self.inner.lock().state;
        match state {
            TickerState::Active => self.mute(),
            TickerState::Muted => self.unmute(),
            _ => {}
        }
    }

    /// Tick the ticker
    ///
    /// This should be called once per frame. It invokes the callback if the
    /// ticker is active.
    pub fn tick<T: TickerProvider>(&self, _provider: &T) {
        if !self.assert_not_disposed("tick") {
            return;
        }
        let mut inner = self.inner.lock();

        if inner.state != TickerState::Active {
            return;
        }

        let Some(start) = inner.start_time else {
            return;
        };
        let elapsed = start.elapsed().as_secs_f64();

        // Take callback to avoid borrowing inner during invocation
        let Some(mut callback) = inner.callback.take() else {
            return;
        };

        // Release lock during callback invocation
        drop(inner);
        callback(elapsed);

        // Restore callback if still active
        let mut inner = self.inner.lock();
        if inner.state == TickerState::Active {
            inner.callback = Some(callback);
        }
    }

    /// Get current state
    #[inline]
    pub fn state(&self) -> TickerState {
        self.inner.lock().state
    }

    /// Check if ticker is active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state().can_tick()
    }

    /// Check if ticker is muted
    #[inline]
    pub fn is_muted(&self) -> bool {
        self.inner.lock().state == TickerState::Muted
    }

    /// Check if ticker is running (active or muted)
    #[inline]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Get elapsed time as type-safe Seconds
    pub fn elapsed(&self) -> Seconds {
        let inner = self.inner.lock();
        match inner.state {
            TickerState::Idle | TickerState::Stopped => Seconds::ZERO,
            TickerState::Muted => inner.muted_elapsed,
            TickerState::Active => inner
                .start_time
                .map_or(Seconds::ZERO, |s| Seconds::new(s.elapsed().as_secs_f64())),
        }
    }

    /// Get elapsed time in seconds (raw f64 for backwards compat)
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed().value()
    }

    /// Reset the ticker to initial state.
    ///
    /// Cancels the active [`TickerFuture`], cancels any pending transient
    /// frame callback (auto-scheduling tickers), and clears all state. The
    /// ticker can be re-armed via [`start`](Self::start) afterwards.
    pub fn reset(&mut self) {
        if !self.assert_not_disposed("reset") {
            return;
        }
        let (pending_id, active_future) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Idle;
            inner.start_time = None;
            inner.callback = None;
            inner.muted_elapsed = Seconds::ZERO;
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
            )
        };
        if let Some(future) = active_future {
            future.set_canceled();
        }
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref()) {
            scheduler.cancel_frame_callback(id);
        }
    }

    /// Register a transient frame callback if this ticker is auto-scheduling,
    /// active, and not already scheduled. No-op for manual tickers, inactive
    /// tickers, or tickers that already have a pending callback.
    ///
    /// Flutter parity: [`ticker.dart:270`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `shouldScheduleTick = !muted && isActive && !scheduled`.
    fn schedule_tick_if_active(&self) {
        let Some(scheduler) = self.scheduler.as_ref() else {
            return; // Manual ticker — no auto-schedule.
        };
        // Check `shouldScheduleTick` and reserve the slot under the inner
        // lock so two concurrent schedulers can't both register.
        {
            let inner = self.inner.lock();
            if inner.state != TickerState::Active || inner.scheduled_callback_id.is_some() {
                return;
            }
        }
        // Capture only Arc clones in the closure — total capture size is
        // 3 × 8 bytes (Arc<Mutex<TickerInner>> + Arc<Scheduler> + Arc<AtomicBool>),
        // matching the audit-recommended hot-path shape. The Box<dyn FnOnce>
        // wrapping is unavoidable with the current `OneShotFrameCallback`
        // signature; full elimination of per-frame Box requires an AtomicU8
        // state machine plus a persistent-callback model and is deferred.
        let inner_arc = Arc::clone(&self.inner);
        let scheduler_arc = Arc::clone(scheduler);
        let disposed_arc = Arc::clone(&self.disposed);
        let cb_id = scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
            Self::tick_and_reschedule_static(inner_arc, scheduler_arc, disposed_arc);
        }));
        // Record the ID so stop/mute/dispose can cancel.
        self.inner.lock().scheduled_callback_id = Some(cb_id);
    }

    /// Tick + auto-reschedule entry point invoked by the scheduler's
    /// transient-callback drain.
    ///
    /// Flutter parity: [`ticker.dart:272-285`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `_tick(timeStamp)` — clear `_animationId`, fire `_onTick`, then
    /// `scheduleTick(rescheduling: true)` if still `shouldScheduleTick`.
    ///
    /// This is a free associated function rather than a method so it can
    /// be invoked from inside the captured closure without retaining a
    /// `&self` borrow across the callback registration boundary.
    fn tick_and_reschedule_static(
        inner: Arc<Mutex<TickerInner>>,
        scheduler: Arc<crate::scheduler::Scheduler>,
        disposed: Arc<AtomicBool>,
    ) {
        // Disposed ticker — short-circuit. The closure may have been queued
        // before `dispose()` cancelled it; the cancel path uses
        // `cancel_frame_callback` which marks the ID cancelled, but the
        // closure body still runs through scheduler's drain in rare races.
        if disposed.load(Ordering::Acquire) {
            return;
        }

        let (elapsed, mut callback) = {
            let mut guard = inner.lock();
            // Clear scheduled_callback_id — this callback just fired.
            guard.scheduled_callback_id = None;

            if guard.state != TickerState::Active {
                return;
            }
            let Some(start) = guard.start_time else {
                return;
            };
            let elapsed = start.elapsed().as_secs_f64();
            // Take callback to release the lock before invoking. Restore
            // afterwards if still active.
            (elapsed, guard.callback.take())
        };

        let Some(ref mut cb) = callback else {
            return;
        };
        cb(elapsed);

        // Restore callback + reschedule if still active and not disposed.
        // The user callback may have called `stop`/`dispose` — re-read
        // state under the lock to honor that.
        if disposed.load(Ordering::Acquire) {
            return;
        }
        let should_reschedule = {
            let mut guard = inner.lock();
            if guard.state == TickerState::Active {
                guard.callback = callback;
                true
            } else {
                false
            }
        };
        if !should_reschedule {
            return;
        }
        // Register the next frame's callback. Mirrors Flutter
        // `scheduleTick(rescheduling: true)`.
        let inner_next = Arc::clone(&inner);
        let scheduler_next = Arc::clone(&scheduler);
        let disposed_next = Arc::clone(&disposed);
        let cb_id = scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
            Self::tick_and_reschedule_static(inner_next, scheduler_next, disposed_next);
        }));
        // Record the new ID — race-safe because we just cleared the slot at
        // the top of this function and the stop/mute path takes the lock
        // before clearing.
        inner.lock().scheduled_callback_id = Some(cb_id);
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        if !self.disposed.load(Ordering::Acquire) {
            self.dispose();
        }
    }
}

impl Default for Ticker {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: Ticker intentionally does NOT implement Clone.
// The previous Clone impl shared `Arc<Mutex<TickerInner>>` with a new TickerId,
// meaning two tickers with different IDs controlled the same callback/state.
// Stopping one would silently stop the other — a correctness footgun.
// If you need multiple tickers, create them individually with `Ticker::new()`.

impl std::fmt::Debug for Ticker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ticker")
            .field("id", &self.id)
            .field("state", &self.state())
            .field("elapsed", &self.elapsed())
            .finish_non_exhaustive()
    }
}

/// Multiple tickers managed together
///
/// # Examples
///
/// ```
/// use flui_scheduler::ticker::TickerGroup;
///
/// let mut group = TickerGroup::new();
///
/// // Create tickers with callbacks
/// group.create(|elapsed| println!("Ticker 1: {:.3}s", elapsed));
/// group.create(|elapsed| println!("Ticker 2: {:.3}s", elapsed));
///
/// assert_eq!(group.len(), 2);
/// assert_eq!(group.active_count(), 2);
///
/// // Control all tickers at once
/// group.mute_all();
/// group.unmute_all();
/// group.stop_all();
/// ```
#[derive(Debug)]
pub struct TickerGroup {
    tickers: Vec<Ticker>,
}

impl TickerGroup {
    /// Create a new empty ticker group
    pub fn new() -> Self {
        Self {
            tickers: Vec::new(),
        }
    }

    /// Create with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tickers: Vec::with_capacity(capacity),
        }
    }

    /// Add a ticker to the group
    pub fn add(&mut self, ticker: Ticker) {
        self.tickers.push(ticker);
    }

    /// Create and add a new ticker with callback
    pub fn create<F>(&mut self, callback: F) -> TickerId
    where
        F: FnMut(f64) + Send + 'static,
    {
        let mut ticker = Ticker::new();
        let id = ticker.id();
        ticker.start(callback);
        self.tickers.push(ticker);
        id
    }

    /// Tick all active tickers
    pub fn tick_all<T: TickerProvider>(&self, provider: &T) {
        for ticker in &self.tickers {
            ticker.tick(provider);
        }
    }

    /// Stop all tickers
    pub fn stop_all(&mut self) {
        for ticker in &mut self.tickers {
            ticker.stop();
        }
    }

    /// Mute all tickers
    pub fn mute_all(&mut self) {
        for ticker in &mut self.tickers {
            ticker.mute();
        }
    }

    /// Unmute all tickers
    pub fn unmute_all(&mut self) {
        for ticker in &mut self.tickers {
            ticker.unmute();
        }
    }

    /// Remove stopped tickers
    pub fn cleanup(&mut self) {
        self.tickers.retain(|t| t.state() != TickerState::Stopped);
    }

    /// Get number of tickers
    pub fn len(&self) -> usize {
        self.tickers.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.tickers.is_empty()
    }

    /// Get count of active tickers
    pub fn active_count(&self) -> usize {
        self.tickers.iter().filter(|t| t.is_active()).count()
    }

    /// Iterate over all tickers
    pub fn iter(&self) -> impl Iterator<Item = &Ticker> {
        self.tickers.iter()
    }

    /// Iterate over all tickers mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Ticker> {
        self.tickers.iter_mut()
    }
}

impl Default for TickerGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for TickerGroup {
    type Item = Ticker;
    type IntoIter = std::vec::IntoIter<Ticker>;

    fn into_iter(self) -> Self::IntoIter {
        self.tickers.into_iter()
    }
}

impl<'a> IntoIterator for &'a TickerGroup {
    type Item = &'a Ticker;
    type IntoIter = std::slice::Iter<'a, Ticker>;

    fn into_iter(self) -> Self::IntoIter {
        self.tickers.iter()
    }
}

impl<'a> IntoIterator for &'a mut TickerGroup {
    type Item = &'a mut Ticker;
    type IntoIter = std::slice::IterMut<'a, Ticker>;

    fn into_iter(self) -> Self::IntoIter {
        self.tickers.iter_mut()
    }
}

impl std::iter::FromIterator<Ticker> for TickerGroup {
    fn from_iter<I: IntoIterator<Item = Ticker>>(iter: I) -> Self {
        Self {
            tickers: iter.into_iter().collect(),
        }
    }
}

impl Extend<Ticker> for TickerGroup {
    fn extend<I: IntoIterator<Item = Ticker>>(&mut self, iter: I) {
        self.tickers.extend(iter);
    }
}

// ============================================================================
// TickerFuture and TickerCanceled - Flutter-compatible async ticker support
// ============================================================================

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use event_listener::{Event, Listener};

/// Completion state of a ticker future
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickerFutureState {
    /// Ticker is still running
    Pending,
    /// Ticker completed normally
    Complete,
    /// Ticker was canceled
    Canceled,
}

/// Shared state for TickerFuture
struct TickerFutureInner {
    /// Current state
    state: Mutex<TickerFutureState>,
    /// Event for notifying waiters when state changes
    event: Event,
}

/// A future representing an ongoing ticker sequence.
///
/// `TickerFuture` is returned by ticker start methods and completes when the
/// ticker is stopped. It provides two ways to await completion:
///
/// - Awaiting the future directly completes when the ticker stops normally
/// - Using [`or_cancel`](Self::or_cancel) returns a future that also completes
///   with an error if the ticker is canceled
///
/// # Example
///
/// ```rust
/// use flui_scheduler::ticker::{TickerCanceled, TickerFuture};
///
/// // Create a pre-completed future
/// let future = TickerFuture::complete();
/// assert!(future.is_complete());
///
/// // Create a new pending future
/// let future = TickerFuture::new();
/// assert!(!future.is_complete());
/// ```
pub struct TickerFuture {
    /// Shared inner state
    inner: Arc<TickerFutureInner>,
    /// Event listener for async notification (avoids busy-loop)
    listener: Option<event_listener::EventListener>,
}

impl TickerFuture {
    /// Create a new pending ticker future
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TickerFutureInner {
                state: Mutex::new(TickerFutureState::Pending),
                event: Event::new(),
            }),
            listener: None,
        }
    }

    /// Create an already-completed ticker future.
    ///
    /// This is useful for implementing objects that normally defer to a ticker
    /// but sometimes can skip the ticker because the animation is of zero
    /// duration, but which still need to represent the completed animation.
    pub fn complete() -> Self {
        Self {
            inner: Arc::new(TickerFutureInner {
                state: Mutex::new(TickerFutureState::Complete),
                event: Event::new(),
            }),
            listener: None,
        }
    }

    /// Create an already-canceled ticker future.
    ///
    /// Used when a ticker operation is rejected after dispose. Awaiting the
    /// base future keeps Flutter semantics (it does not resolve on cancel),
    /// while [`or_cancel`](Self::or_cancel) resolves with
    /// [`TickerCanceled`].
    pub fn canceled() -> Self {
        Self {
            inner: Arc::new(TickerFutureInner {
                state: Mutex::new(TickerFutureState::Canceled),
                event: Event::new(),
            }),
            listener: None,
        }
    }

    /// Mark the future as complete (ticker stopped normally).
    pub(crate) fn set_complete(&self) {
        let mut state = self.inner.state.lock();
        if *state == TickerFutureState::Pending {
            *state = TickerFutureState::Complete;
            drop(state);
            // Notify all waiters
            self.inner.event.notify(usize::MAX);
        }
    }

    /// Mark the future as canceled.
    pub(crate) fn set_canceled(&self) {
        let mut state = self.inner.state.lock();
        if *state == TickerFutureState::Pending {
            *state = TickerFutureState::Canceled;
            drop(state);
            // Notify all waiters
            self.inner.event.notify(usize::MAX);
        }
    }

    /// Check if the ticker completed normally
    pub fn is_complete(&self) -> bool {
        *self.inner.state.lock() == TickerFutureState::Complete
    }

    /// Check if the ticker was canceled
    pub fn is_canceled(&self) -> bool {
        *self.inner.state.lock() == TickerFutureState::Canceled
    }

    /// Check if the ticker is still pending
    pub fn is_pending(&self) -> bool {
        *self.inner.state.lock() == TickerFutureState::Pending
    }

    /// Returns a future that completes when this future resolves OR throws
    /// when the ticker is canceled.
    ///
    /// If this property is never accessed, then canceling the ticker does not
    /// throw any exceptions. Once this property is accessed, if the ticker is
    /// canceled, the returned future will complete with a [`TickerCanceled`]
    /// error.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::ticker::{TickerCanceled, TickerFuture};
    ///
    /// async fn example() {
    ///     let future = TickerFuture::new();
    ///
    ///     // This will error if the ticker is canceled
    ///     match future.or_cancel().await {
    ///         Ok(()) => println!("Ticker completed normally"),
    ///         Err(TickerCanceled) => println!("Ticker was canceled"),
    ///     }
    /// }
    /// ```
    pub fn or_cancel(&self) -> TickerFutureOrCancel {
        TickerFutureOrCancel {
            inner: Arc::clone(&self.inner),
            listener: None,
        }
    }

    /// Calls the callback either when this future resolves or when the ticker
    /// is canceled.
    ///
    /// This is useful for cleanup operations that should run regardless of
    /// how the ticker ends.
    ///
    /// If the future is already resolved when this method is called, the
    /// callback is invoked immediately on the current thread. Otherwise, a
    /// lightweight listener is registered that invokes the callback when
    /// the state changes.
    ///
    /// **Note**: If the future is still pending, this method blocks the current
    /// thread until the ticker completes or is canceled. For non-blocking
    /// usage, use [`or_cancel`](Self::or_cancel) with async/await instead.
    pub fn when_complete_or_cancel<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        // Fast path: already resolved — call immediately
        if *self.inner.state.lock() != TickerFutureState::Pending {
            callback();
            return;
        }

        // Register listener BEFORE re-checking state (avoid race)
        let listener = self.inner.event.listen();

        // Re-check after registering (state may have changed)
        if *self.inner.state.lock() != TickerFutureState::Pending {
            callback();
            return;
        }

        // Block on the listener until notified (no thread spawning)
        // Note: On wasm32, blocking is not possible — call the callback
        // immediately. The async path (or_cancel) should be used instead.
        #[cfg(not(target_arch = "wasm32"))]
        {
            listener.wait();
        }
        callback();
    }
}

impl Default for TickerFuture {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for TickerFuture {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            listener: None, // Fresh listener per clone
        }
    }
}

impl Future for TickerFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let state = *self.inner.state.lock();

            match state {
                TickerFutureState::Complete => return Poll::Ready(()),
                TickerFutureState::Canceled | TickerFutureState::Pending => {
                    // Primary future only completes on Complete (Flutter behavior:
                    // cancel doesn't resolve the base future, only or_cancel() does)
                    if self.listener.is_none() {
                        self.listener = Some(self.inner.event.listen());
                    }
                    if let Some(ref mut listener) = self.listener {
                        match Pin::new(listener).poll(cx) {
                            Poll::Ready(()) => {
                                // Event fired: clear the listener and fall
                                // through to re-check state on the next
                                // loop iteration.
                                self.listener = None;
                            }
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for TickerFuture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = *self.inner.state.lock();
        let state_str = match state {
            TickerFutureState::Pending => "active",
            TickerFutureState::Complete => "complete",
            TickerFutureState::Canceled => "canceled",
        };
        write!(f, "TickerFuture({state_str})")
    }
}

/// A derivative future from [`TickerFuture::or_cancel`] that completes with
/// an error if the ticker is canceled.
pub struct TickerFutureOrCancel {
    inner: Arc<TickerFutureInner>,
    listener: Option<event_listener::EventListener>,
}

impl Future for TickerFutureOrCancel {
    type Output = Result<(), TickerCanceled>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let state = *self.inner.state.lock();

            match state {
                TickerFutureState::Complete => return Poll::Ready(Ok(())),
                TickerFutureState::Canceled => return Poll::Ready(Err(TickerCanceled)),
                TickerFutureState::Pending => {
                    // Set up listener if not already listening
                    if self.listener.is_none() {
                        self.listener = Some(self.inner.event.listen());
                    }

                    // Poll the listener
                    if let Some(ref mut listener) = self.listener {
                        // Use pin projection for the listener
                        let pinned = Pin::new(listener);
                        match pinned.poll(cx) {
                            Poll::Ready(()) => {
                                // Event fired: clear the listener and fall
                                // through to re-check state on the next
                                // loop iteration.
                                self.listener = None;
                            }
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                }
            }
        }
    }
}

impl std::fmt::Debug for TickerFutureOrCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TickerFutureOrCancel")
    }
}

/// Exception thrown by ticker objects when the ticker is canceled.
///
/// This error is returned by [`TickerFuture::or_cancel`] when the ticker
/// is stopped with cancellation.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::ticker::TickerCanceled;
///
/// let error = TickerCanceled;
/// assert_eq!(error.to_string(), "The ticker was canceled");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickerCanceled;

impl std::fmt::Display for TickerCanceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The ticker was canceled")
    }
}

impl std::error::Error for TickerCanceled {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    struct MockProvider;

    // Uses the default `create_ticker` impl from `TickerProvider`. Sufficient
    // for the manual-tick `Ticker::tick(&provider)` callsites below — the
    // provider is just a marker type since `tick` doesn't actually call into
    // it (the unused parameter exists for future hooks).
    impl TickerProvider for MockProvider {}

    #[test]
    fn test_ticker_dispose_is_idempotent() {
        let mut ticker = Ticker::new();
        ticker.start(|_| {});
        assert!(!ticker.is_disposed());
        ticker.dispose();
        assert!(ticker.is_disposed());
        ticker.dispose(); // idempotent — no panic, no state change
        assert!(ticker.is_disposed());
    }

    #[test]
    fn test_ticker_start_future_completes_on_stop() {
        let mut ticker = Ticker::new();
        let future = ticker.start(|_| {});
        assert!(future.is_pending());

        ticker.stop();

        assert!(future.is_complete());
        assert!(!future.is_canceled());
        assert!(!future.is_pending());
    }

    #[test]
    fn test_ticker_dispose_cancels_future() {
        let mut ticker = Ticker::new();
        let future = ticker.start(|_| {});
        assert!(future.is_pending());

        ticker.dispose();

        assert!(future.is_canceled());
        assert!(!future.is_complete());
        assert!(!future.is_pending());
    }

    #[test]
    fn test_ticker_drop_disposes() {
        let mut ticker = Ticker::new();
        let future = ticker.start(|_| {});
        // Take Arc clone of disposed flag to observe after drop
        let disposed_flag = ticker.disposed.clone();
        drop(ticker);
        assert!(disposed_flag.load(Ordering::Acquire));
        assert!(
            future.is_canceled(),
            "Drop must dispose the ticker and cancel the active future"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_ticker_use_after_dispose_panics_in_debug() {
        let mut ticker = Ticker::new();
        ticker.dispose();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ticker.start(|_| {});
        }));
        assert!(
            result.is_err(),
            "start() after dispose should panic in debug"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_ticker_started_twice_panics_in_debug() {
        let mut ticker = Ticker::new();
        ticker.start(|_| {});
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ticker.start(|_| {});
        }));
        assert!(
            result.is_err(),
            "start() while Active should panic in debug"
        );
    }

    #[test]
    fn test_ticker_lifecycle() {
        let mut ticker = Ticker::new();
        assert_eq!(ticker.state(), TickerState::Idle);
        assert!(!ticker.is_active());

        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);

        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        assert_eq!(ticker.state(), TickerState::Active);
        assert!(ticker.is_active());

        ticker.stop();
        assert_eq!(ticker.state(), TickerState::Stopped);
        assert!(!ticker.is_active());
    }

    #[test]
    fn test_ticker_mute() {
        let mut ticker = Ticker::new();

        ticker.start(|_| {});
        assert!(ticker.is_active());

        ticker.mute();
        assert!(ticker.is_muted());
        assert!(!ticker.is_active());
        assert!(ticker.is_running());

        ticker.unmute();
        assert!(ticker.is_active());
        assert!(!ticker.is_muted());
    }

    #[test]
    fn test_ticker_elapsed() {
        let mut ticker = Ticker::new();
        assert_eq!(ticker.elapsed(), Seconds::ZERO);

        ticker.start(|_| {});

        // Give some time to elapse
        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed.value() > 0.0);
        assert!(elapsed.value() < 1.0); // Should be less than 1 second
    }

    #[test]
    fn test_ticker_callback_invocation() {
        let mut ticker = Ticker::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        let provider = MockProvider;

        ticker.tick(&provider);
        ticker.tick(&provider);
        ticker.tick(&provider);

        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_ticker_id() {
        let ticker1 = Ticker::new();
        let ticker2 = Ticker::new();

        assert_ne!(ticker1.id(), ticker2.id());
    }

    #[test]
    fn test_ticker_group() {
        let mut group = TickerGroup::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c1 = Arc::clone(&counter);
        group.create(move |_| {
            c1.fetch_add(1, Ordering::Relaxed);
        });

        let c2 = Arc::clone(&counter);
        group.create(move |_| {
            c2.fetch_add(10, Ordering::Relaxed);
        });

        assert_eq!(group.len(), 2);
        assert_eq!(group.active_count(), 2);

        let provider = MockProvider;
        group.tick_all(&provider);

        assert_eq!(counter.load(Ordering::Relaxed), 11);
    }

    #[test]
    fn test_ticker_reset() {
        let mut ticker = Ticker::new();
        let future = ticker.start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(5));

        ticker.reset();

        assert_eq!(ticker.state(), TickerState::Idle);
        assert_eq!(ticker.elapsed(), Seconds::ZERO);
        assert!(future.is_canceled());
    }

    // Auto-scheduling Ticker tests

    #[test]
    fn test_auto_scheduling_ticker_lifecycle() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let mut ticker = Ticker::new_with_scheduler(scheduler.clone());

        assert_eq!(ticker.state(), TickerState::Idle);
        assert!(!ticker.is_active());

        ticker.start(|_| {});
        assert_eq!(ticker.state(), TickerState::Active);
        assert!(ticker.is_active());

        ticker.stop();
        assert_eq!(ticker.state(), TickerState::Stopped);
    }

    #[test]
    fn test_auto_scheduling_ticker_fires_each_frame() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(scheduler.clone());
        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        // Execute frames — ticker should auto-tick and re-register each frame.
        scheduler.execute_frame();
        scheduler.execute_frame();
        scheduler.execute_frame();

        assert_eq!(counter.load(Ordering::Relaxed), 3);

        ticker.stop();

        // After stop, no more callbacks fire.
        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_auto_scheduling_ticker_mute_unmute() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(scheduler.clone());
        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        ticker.mute();
        scheduler.execute_frame();
        // Still 1 — muted ticker cancels its pending callback.
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        ticker.unmute();
        scheduler.execute_frame();
        // Now 2 — unmute re-registers the auto-schedule.
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_auto_scheduling_ticker_dispose_cancels_pending() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(scheduler.clone());
        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        // Dispose before any frame fires — pending transient callback is cancelled.
        ticker.dispose();
        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_create_ticker_via_provider_auto_schedules() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        // Provider factory path: create_ticker preloads callback; start_default
        // arms the ticker.
        let c = Arc::clone(&counter);
        let on_tick: TickerCallback = Box::new(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });
        let mut ticker = scheduler.create_ticker(on_tick);
        let future = ticker.start_default();
        assert!(future.is_pending());

        scheduler.execute_frame();
        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 2);

        ticker.stop();
        assert!(future.is_complete());
    }

    #[test]
    fn test_start_default_without_callback_returns_complete_future() {
        let mut ticker = Ticker::new();

        let future = ticker.start_default();

        assert!(future.is_complete());
        assert_eq!(ticker.state(), TickerState::Idle);
    }

    #[test]
    fn test_auto_scheduling_ticker_elapsed() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let mut ticker = Ticker::new_with_scheduler(scheduler);

        assert_eq!(ticker.elapsed(), Seconds::ZERO);

        ticker.start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed.value() > 0.0);
        assert!(elapsed.value() < 1.0);
    }
}
