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
//!   [`TickerProvider::create_ticker`] on a [`UpdateScheduler`](crate::scheduler::UpdateScheduler)): the ticker
//!   self-registers a transient frame callback on every start/unmute,
//!   matching Flutter [`ticker.dart:283`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
//!   `scheduleTick(rescheduling: true)`. `stop`/`mute`/`dispose` cancel the
//!   pending callback via [`UpdateScheduler::cancel_frame_callback`](crate::scheduler::UpdateScheduler::cancel_frame_callback).
//!
//! ## Manual Ticker Example
//!
//! ```rust
//! use flui_scheduler::{UpdateScheduler, Ticker, TickerProvider};
//!
//! let scheduler = UpdateScheduler::new();
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
//! use flui_scheduler::{UpdateScheduler, Ticker};
//!
//! let scheduler = UpdateScheduler::new();
//! let mut ticker = Ticker::new_with_scheduler(&scheduler);
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
/// Implementors that own a [`UpdateScheduler`](crate::scheduler::UpdateScheduler) (e.g. `impl TickerProvider for
/// UpdateScheduler`) override [`create_ticker`](Self::create_ticker) to vend an
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

/// State of a ticker's installed callback.
///
/// Replaces a bare `Option<TickerCallback>`, which conflated two different
/// reasons for being empty: "never installed, or the run stopped" and
/// "checked out for an in-flight dispatch". A dispatch site (`Ticker::tick`,
/// `Ticker::tick_and_reschedule_static`) checks a `Ready` callback out into a
/// [`TickerLease`], leaving `CheckedOut` behind while user code runs with no
/// lock held — the lease's `Drop` resolves the checkout back to `Ready` or
/// `Vacant` once that code returns (issue #1059).
enum CallbackSlot {
    /// No callback installed.
    Vacant,
    /// Callback installed and idle — ready to be leased for the next dispatch.
    Ready(TickerCallback),
    /// A dispatch has taken the callback out; see [`TickerLease`].
    CheckedOut,
}

impl CallbackSlot {
    /// Clears an idle callback, returning it for the caller to drop AFTER
    /// releasing whatever lock guards this slot — a callback's own `Drop` is
    /// user code that may re-enter the (non-reentrant) ticker lock. Leaves a
    /// `CheckedOut` slot untouched: an in-flight dispatch's [`TickerLease`]
    /// owns that callback and is the only thing that resolves it, so
    /// clearing it here would race the lease's own restore-or-discard
    /// decision (see `TickerState::is_running` at the time the lease drops).
    fn clear_if_ready(&mut self) -> Option<TickerCallback> {
        if matches!(self, CallbackSlot::Ready(_)) {
            match std::mem::replace(self, CallbackSlot::Vacant) {
                CallbackSlot::Ready(callback) => Some(callback),
                _ => unreachable!("just matched Ready above"),
            }
        } else {
            None
        }
    }
}

/// RAII checkout of a ticker's callback for one dispatch.
///
/// Checks a [`CallbackSlot::Ready`] callback out to `CheckedOut` so the
/// dispatching call (`Ticker::tick`, `Ticker::tick_and_reschedule_static`)
/// can run it with **no lock held** — user code may reenter the ticker
/// (`stop`/`start`/`mute`/`unmute`/`dispose`/`reset`) from inside its own
/// callback (issue #1059). Dropping the lease — including on unwind, so a
/// panicking callback is covered too — resolves the checkout:
///
/// - If the slot is still exactly `CheckedOut` (nothing reentrant replaced
///   it) and the ticker is still running (`Active` or `Muted`), the
///   callback is restored to `Ready`.
/// - Otherwise — the ticker stopped/disposed/reset while checked out, or a
///   reentrant `start` already installed a fresh `Ready(new)` callback in
///   its place — the checked-out callback is dropped, OUTSIDE the inner
///   lock (its own `Drop` is user code that may call back into this same
///   ticker).
struct TickerLease {
    inner: Arc<Mutex<TickerInner>>,
    callback: Option<TickerCallback>,
}

impl TickerLease {
    /// Checks the callback out under an already-held `guard`, atomically
    /// with whatever the caller just verified under the same lock (e.g.
    /// `state == Active`).
    fn checkout(inner: &Arc<Mutex<TickerInner>>, guard: &mut TickerInner) -> Self {
        let callback = match std::mem::replace(&mut guard.slot, CallbackSlot::CheckedOut) {
            CallbackSlot::Ready(callback) => Some(callback),
            other => {
                // Nothing to dispatch: put back what was actually there
                // (`Vacant`, or an unexpected concurrent `CheckedOut` — this
                // ticker's own contract never dispatches the same run
                // twice, but a double checkout is handled rather than
                // panicking).
                guard.slot = other;
                None
            }
        };
        Self {
            inner: Arc::clone(inner),
            callback,
        }
    }

    fn callback_mut(&mut self) -> Option<&mut TickerCallback> {
        self.callback.as_mut()
    }
}

impl Drop for TickerLease {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        let discarded = {
            let mut guard = self.inner.lock();
            let should_restore =
                matches!(guard.slot, CallbackSlot::CheckedOut) && guard.state.is_running();
            if should_restore {
                guard.slot = CallbackSlot::Ready(callback);
                None
            } else {
                if matches!(guard.slot, CallbackSlot::CheckedOut) {
                    // The ticker stopped/disposed/reset while this callback
                    // was checked out — normalize back to `Vacant` rather
                    // than leaving the slot stuck reporting "checked out"
                    // forever.
                    guard.slot = CallbackSlot::Vacant;
                }
                tracing::trace!(
                    "TickerLease: discarding a superseded or stale callback outside the inner lock"
                );
                Some(callback)
            }
        };
        // The guard above has already fallen by the time this drops: the
        // callback's own `Drop` (user code) may re-enter the ticker.
        drop(discarded);
    }
}

/// Shared inner state for a Ticker (single allocation, single lock)
struct TickerInner {
    state: TickerState,
    start_time: Option<Instant>,
    slot: CallbackSlot,
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
    /// `None` for manually-driven tickers (no [`UpdateScheduler`](crate::scheduler::UpdateScheduler) attached) and
    /// auto-scheduling tickers that are not currently registered.
    ///
    /// Flutter parity: `ticker.dart:254 _animationId` (sentinel for
    /// "already-scheduled").
    scheduled_callback_id: Option<CallbackId>,
}

impl TickerInner {
    /// The single scheduling eligibility predicate, shared by every site
    /// that may register a transient frame callback: `start_inner`,
    /// `unmute` (via `schedule_tick_if_active`), and the auto-tick tail
    /// (`tick_and_reschedule_static`). A callback checked out into a
    /// [`TickerLease`] (`CallbackSlot::CheckedOut`) is not `Ready`, so this
    /// is false for the whole duration of that dispatch — which is exactly
    /// why a reentrant `mute()`-then-`unmute()` from inside the running
    /// callback cannot register a second, orphaning transient callback: its
    /// `unmute()` call reaches this predicate while the slot is still
    /// `CheckedOut`, before the lease has restored it (issue #1059).
    ///
    /// Flutter parity: [`ticker.dart:270`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart)
    /// `shouldScheduleTick = !muted && isActive && !scheduled`.
    fn should_schedule_tick(&self) -> bool {
        self.state == TickerState::Active
            && matches!(self.slot, CallbackSlot::Ready(_))
            && self.scheduled_callback_id.is_none()
    }
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
    ///   pending callback via [`UpdateScheduler::cancel_frame_callback`](crate::scheduler::UpdateScheduler::cancel_frame_callback).
    ///
    /// A [`WeakUpdateScheduler`](crate::scheduler::WeakUpdateScheduler), not a strong
    /// `UpdateScheduler` — a live ticker's re-scheduling closure sits inside the
    /// scheduler's OWN transient-callback queue (`schedule_tick_if_active`),
    /// so a strong capture here would form
    /// `UpdateScheduler → queue → closure → UpdateScheduler`, a permanent leak the
    /// instant a realm could otherwise drop its scheduler. Every
    /// schedule/cancel site upgrades-or-returns: once the backing scheduler
    /// is gone there is no queue left to register with or cancel from, so a
    /// failed upgrade is silently done, matching [`Self::disposed`]'s own
    /// short-circuit.
    ///
    /// Flutter parity: `Ticker(this._onTick, ...)` ([`ticker.dart:80`](../../../.flutter/flutter-master/packages/flutter/lib/src/scheduler/ticker.dart))
    /// implicitly carries `SchedulerBinding.instance` (singleton); FLUI
    /// stores the scheduler explicitly to keep the dependency typed.
    scheduler: Option<crate::scheduler::WeakUpdateScheduler>,

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
    /// [`UpdateScheduler`](crate::scheduler::UpdateScheduler).
    pub fn new() -> Self {
        Self {
            id: next_ticker_id(),
            inner: Arc::new(Mutex::new(TickerInner {
                state: TickerState::Idle,
                start_time: None,
                slot: CallbackSlot::Vacant,
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
    /// [`UpdateScheduler::cancel_frame_callback`](crate::scheduler::UpdateScheduler::cancel_frame_callback).
    ///
    /// Downgrades `scheduler` internally — this ticker never holds a strong
    /// reference back to it (see the `scheduler` field's own doc for why).
    pub fn new_with_scheduler(scheduler: &crate::scheduler::UpdateScheduler) -> Self {
        Self {
            id: next_ticker_id(),
            inner: Arc::new(Mutex::new(TickerInner {
                state: TickerState::Idle,
                start_time: None,
                slot: CallbackSlot::Vacant,
                muted_elapsed: Seconds::ZERO,
                active_future: None,
                scheduled_callback_id: None,
            })),
            scheduler: Some(scheduler.downgrade()),
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
    /// installed into the ticker's callback slot when [`start`](Self::start)
    /// is next invoked without a callback argument; explicit
    /// `start(callback)` overrides any preloaded value.
    pub(crate) fn set_pending_callback(&mut self, callback: TickerCallback) {
        // The displaced slot value (a real `Ready(old)` callback if one was
        // already pre-loaded) is bound out of the lock's block so its `Drop`
        // — user code — never runs while the inner lock is held.
        let displaced = {
            let mut inner = self.inner.lock();
            std::mem::replace(&mut inner.slot, CallbackSlot::Ready(callback))
        };
        drop(displaced);
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
        let (pending_id, active_future, discarded) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Stopped;
            let discarded = inner.slot.clear_if_ready();
            inner.start_time = None;
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
                discarded,
            )
        };
        // Dropped only after the lock above has released (issue #1059
        // hardening) — a callback's own `Drop` is user code.
        drop(discarded);
        if let Some(future) = active_future {
            future.set_canceled();
        }
        // Cancel pending transient callback outside the inner lock to avoid
        // lock-during-callback hazard (scheduler may also take its own locks).
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref())
            && let Some(scheduler) = scheduler.upgrade()
        {
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
        let displaced = {
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
            let displaced = if let Some(cb) = callback {
                // Explicit callback overrides any pre-loaded one from
                // `create_ticker`, OR — if this call is reentrant, made from
                // inside the ticker's own checked-out callback — installs
                // the replacement run's callback directly into the slot a
                // `TickerLease` currently holds `CheckedOut` (issue #1059:
                // an old, superseded run must never overwrite this). Either
                // way, whatever the slot held before (a real `Ready(old)`
                // callback, or `CheckedOut` with nothing to drop here) is
                // returned for the caller to drop AFTER this lock releases.
                std::mem::replace(&mut inner.slot, CallbackSlot::Ready(cb))
            } else if matches!(inner.slot, CallbackSlot::Vacant) {
                // No explicit callback, and no pre-loaded/checked-out one —
                // start is a no-op (tick has nothing to dispatch). Logged
                // for diagnostic.
                tracing::warn!(
                    ticker_id = ?self.id,
                    "Ticker::start_default called without a pre-loaded callback (no-op)"
                );
                return TickerFuture::complete();
            } else {
                // `Ready` (a pre-loaded callback exists) or `CheckedOut` (a
                // reentrant `start_default()` resuming with the callback
                // already in flight) — nothing to displace.
                CallbackSlot::Vacant
            };
            inner.state = TickerState::Active;
            inner.start_time = Some(Instant::now());
            inner.muted_elapsed = Seconds::ZERO;
            inner.active_future = Some(future.clone());
            displaced
        };
        drop(displaced);
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
        let (pending_id, active_future, discarded) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Stopped;
            let discarded = inner.slot.clear_if_ready();
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
                discarded,
            )
        };
        // Dropped only after the lock above has released (issue #1059
        // hardening) — a callback's own `Drop` is user code. If `stop()` was
        // called reentrantly (from inside the ticker's own checked-out
        // callback), the slot is `CheckedOut`, not `Ready`, so nothing is
        // discarded here — the dispatching `TickerLease` resolves the
        // checked-out callback itself once it observes this now-`Stopped`
        // (not running) state.
        drop(discarded);
        if let Some(future) = active_future {
            future.set_complete();
        }
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref())
            && let Some(scheduler) = scheduler.upgrade()
        {
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
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref())
            && let Some(scheduler) = scheduler.upgrade()
        {
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
    ///
    /// Uses the same internal `TickerLease` checkout/restore protocol as the
    /// auto-scheduling dispatch path (issue #1059): the callback runs with
    /// no lock held, so it may call `stop`/`start`/`mute`/`unmute`/
    /// `dispose`/`reset` on this same ticker, and the lease resolves the
    /// checkout — restore or discard — once the callback returns or unwinds.
    pub fn tick<T: TickerProvider>(&self, _provider: &T) {
        if !self.assert_not_disposed("tick") {
            return;
        }
        let (elapsed, mut lease) = {
            let mut inner = self.inner.lock();
            if inner.state != TickerState::Active {
                return;
            }
            let Some(start) = inner.start_time else {
                return;
            };
            let elapsed = start.elapsed().as_secs_f64();
            let lease = TickerLease::checkout(&self.inner, &mut inner);
            (elapsed, lease)
        };
        let Some(callback) = lease.callback_mut() else {
            return;
        };
        callback(elapsed);
        // `lease`'s `Drop` (end of scope) restores the callback if the
        // ticker is still running and nothing reentrant already replaced
        // it, or drops it outside the lock otherwise.
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
        let (pending_id, active_future, discarded) = {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Idle;
            inner.start_time = None;
            let discarded = inner.slot.clear_if_ready();
            inner.muted_elapsed = Seconds::ZERO;
            (
                inner.scheduled_callback_id.take(),
                inner.active_future.take(),
                discarded,
            )
        };
        // Dropped only after the lock above has released (issue #1059
        // hardening) — same reentrant-`CheckedOut` handling as `stop`/`dispose`.
        drop(discarded);
        if let Some(future) = active_future {
            future.set_canceled();
        }
        if let (Some(id), Some(scheduler)) = (pending_id, self.scheduler.as_ref())
            && let Some(scheduler) = scheduler.upgrade()
        {
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
        let Some(weak_scheduler) = self.scheduler.as_ref() else {
            return; // Manual ticker — no auto-schedule.
        };
        // Check `should_schedule_tick` and reserve the slot under the inner
        // lock so two concurrent schedulers can't both register. This is the
        // single predicate every scheduling site shares (issue #1059) — see
        // `TickerInner::should_schedule_tick`'s own doc for why checking the
        // callback slot's readiness here, not just `state`, is load-bearing.
        {
            let inner = self.inner.lock();
            if !inner.should_schedule_tick() {
                return;
            }
        }
        // A dead backing scheduler has no queue left to register with —
        // silently done, same as a disposed ticker.
        let Some(scheduler) = weak_scheduler.upgrade() else {
            return;
        };
        // The closure captures the WEAK handle, never a strong `UpdateScheduler` —
        // this is the queue entry that used to close the
        // `UpdateScheduler → queue → closure → UpdateScheduler` cycle (see the
        // `scheduler` field's doc). Capturing `Arc<Mutex<TickerInner>>` +
        // `WeakUpdateScheduler` + `Arc<AtomicBool>` (still 3 pointer-sized fields)
        // keeps the audit-recommended hot-path capture shape.
        let inner_arc = Arc::clone(&self.inner);
        let weak_next = weak_scheduler.clone();
        let disposed_arc = Arc::clone(&self.disposed);
        let cb_id = scheduler.schedule_frame_callback(Box::new(move |_vsync_time| {
            Self::tick_and_reschedule_static(inner_arc, weak_next, disposed_arc);
        }));
        // Record the ID so stop/mute/dispose can cancel — but never clobber
        // a registration a reentrant caller already installed while this
        // one was in flight (issue #1059's own root cause was exactly this
        // kind of blind overwrite, on the auto-tick tail below). Not
        // reachable from a single-threaded caller of this method alone, but
        // cheap to make structurally impossible rather than assumed absent.
        let mut inner = self.inner.lock();
        if inner.scheduled_callback_id.is_none() {
            inner.scheduled_callback_id = Some(cb_id);
        } else {
            drop(inner);
            tracing::trace!(
                ticker_cb_id = ?cb_id,
                "schedule_tick_if_active: a registration already exists; \
                 cancelling the redundant one instead of orphaning it"
            );
            scheduler.cancel_frame_callback(cb_id);
        }
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
    ///
    /// Takes `scheduler` as a [`WeakUpdateScheduler`](crate::scheduler::WeakUpdateScheduler), not a strong `UpdateScheduler`:
    /// this function itself runs FROM inside the scheduler it was
    /// registered on, so it always has a live scheduler in hand at the top —
    /// the weakness matters only for what gets captured in the
    /// *re-registration* closure below, so the cycle never re-forms one
    /// frame later.
    fn tick_and_reschedule_static(
        inner: Arc<Mutex<TickerInner>>,
        scheduler: crate::scheduler::WeakUpdateScheduler,
        disposed: Arc<AtomicBool>,
    ) {
        // Disposed ticker — short-circuit. The closure may have been queued
        // before `dispose()` cancelled it; the cancel path uses
        // `cancel_frame_callback` which marks the ID cancelled, but the
        // closure body still runs through scheduler's drain in rare races.
        if disposed.load(Ordering::Acquire) {
            return;
        }

        let (elapsed, mut lease) = {
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
            // Check the callback out into a lease and release the lock
            // before invoking it — the lease resolves the checkout (restore
            // or discard) when it drops, covering reentrant `stop`/`start`/
            // `mute`/`unmute`/`dispose`/`reset` calls AND a panicking
            // callback alike (issue #1059).
            let lease = TickerLease::checkout(&inner, &mut guard);
            (elapsed, lease)
        };

        let Some(cb) = lease.callback_mut() else {
            return;
        };
        cb(elapsed);

        // Resolve the checkout BEFORE deciding whether to reschedule: a
        // reentrant `start()` may have installed a brand-new `Ready`
        // callback in the slot already (the "restart inside tick" case),
        // and `should_schedule_tick` below must see that fresh state, not
        // the pre-callback one.
        drop(lease);

        // The user callback may have called `dispose()` — re-check before
        // touching the scheduler again.
        if disposed.load(Ordering::Acquire) {
            return;
        }
        let should_reschedule = {
            let guard = inner.lock();
            guard.should_schedule_tick()
        };
        if !should_reschedule {
            return;
        }
        // Upgrade to register the next frame's callback — mirrors Flutter
        // `scheduleTick(rescheduling: true)`. A failed upgrade means the
        // realm tore down between this tick firing and now; nothing is left
        // to reschedule against.
        let Some(strong) = scheduler.upgrade() else {
            return;
        };
        let inner_next = Arc::clone(&inner);
        let scheduler_next = scheduler.clone();
        let disposed_next = Arc::clone(&disposed);
        let cb_id = strong.schedule_frame_callback(Box::new(move |_vsync_time| {
            Self::tick_and_reschedule_static(inner_next, scheduler_next, disposed_next);
        }));
        // Record the new ID — but never clobber one a reentrant `unmute()`/
        // `start()` already installed between the `should_schedule_tick`
        // check above and this registration completing (both run with no
        // lock held). Issue #1059's root cause was exactly this blind
        // overwrite: an orphaned earlier registration that nothing ever
        // cancels. Cancel the REDUNDANT one (ours) instead, so whichever
        // registration a concurrent caller is tracking survives.
        let mut guard = inner.lock();
        if guard.scheduled_callback_id.is_none() {
            guard.scheduled_callback_id = Some(cb_id);
        } else {
            drop(guard);
            tracing::trace!(
                ticker_cb_id = ?cb_id,
                "tick_and_reschedule_static: a registration already exists; \
                 cancelling the redundant one instead of orphaning it"
            );
            strong.cancel_frame_callback(cb_id);
        }
    }
}

#[cfg(test)]
impl Ticker {
    /// Test-only probe: `Some(state)` if the inner lock was free to
    /// `try_lock` right now, `None` if it is held. Used to prove a
    /// checked-out callback's `Drop` runs with no scheduler-owned lock held
    /// (issue #1059), without risking a real hang if a regression
    /// reintroduces one — unlike calling [`Self::state`], which blocks.
    fn try_state(&self) -> Option<TickerState> {
        self.inner.try_lock().map(|guard| guard.state)
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

use event_listener::Event;
// `Listener::wait` is the blocking half of the API — absent on wasm32, where
// there is no thread to park.
#[cfg(not(target_arch = "wasm32"))]
use event_listener::Listener;

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

        // Block on the listener until notified (no thread spawning).
        // On wasm32 blocking is not possible: drop the registration and call
        // the callback immediately. The async path (or_cancel) is the
        // supported route there.
        #[cfg(not(target_arch = "wasm32"))]
        listener.wait();
        #[cfg(target_arch = "wasm32")]
        drop(listener);
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
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let mut ticker = Ticker::new_with_scheduler(&scheduler);

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
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(&scheduler);
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
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(&scheduler);
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
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = Ticker::new_with_scheduler(&scheduler);
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
        let scheduler = crate::scheduler::UpdateScheduler::new();
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
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let mut ticker = Ticker::new_with_scheduler(&scheduler);

        assert_eq!(ticker.elapsed(), Seconds::ZERO);

        ticker.start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed.value() > 0.0);
        assert!(elapsed.value() < 1.0);
    }

    /// Teardown pin: a retained ticker whose backing scheduler has
    /// already been dropped must still resolve `or_cancel()` with
    /// `Err(TickerCanceled)` once its owner disposes it — `dispose()`
    /// cancels the active future unconditionally, before it ever tries (and,
    /// with a dead scheduler, fails) to cancel the pending transient
    /// callback. A surviving handle fails closed; it never panics and never
    /// hangs a waiter.
    #[test]
    fn retained_tickers_future_resolves_canceled_after_owner_disposes_past_scheduler_drop() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let mut ticker = Ticker::new_with_scheduler(&scheduler);
        let future = ticker.start(|_| {});
        assert!(future.is_pending());

        // The realm's scheduler is gone; the ticker is still retained by its
        // owner (e.g. an `AnimationController` a widget hasn't disposed yet).
        drop(scheduler);

        ticker.dispose();

        assert!(
            future.is_canceled(),
            "dispose() must cancel the active future even though the \
             backing scheduler can no longer be upgraded"
        );

        // A single manual poll suffices: `future` is already `Canceled`, so
        // `or_cancel()` resolves on its very first poll without ever
        // registering a waker — no executor needed.
        let mut or_cancel = std::pin::pin!(future.or_cancel());
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        match or_cancel.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(result) => assert_eq!(
                result,
                Err(TickerCanceled),
                "or_cancel() must resolve Err(TickerCanceled), not Ok, once the \
                 scheduler is gone and the owner has disposed"
            ),
            std::task::Poll::Pending => {
                panic!("or_cancel() must resolve immediately: the future is already Canceled")
            }
        }
    }

    // ---- callback slot / TickerLease reentrancy (issue #1059) ----

    /// Probe #1: a tick callback that restarts the ticker (`stop()` then
    /// `start(new)`) must never let the superseded callback run again, must
    /// deliver the new callback exactly once on the following frame, and
    /// must leave exactly one live transient registration behind — not two.
    /// Before this fix: `(pending_after_restart, old_calls, new_calls)` was
    /// `(2, 3, 0)` (issue #1059's own measured evidence).
    #[test]
    fn restart_inside_auto_tick_preserves_new_callback_and_one_pending_tick() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let weak = Arc::downgrade(&ticker);
        let old_calls = Arc::new(AtomicU32::new(0));
        let new_calls = Arc::new(AtomicU32::new(0));
        let old_counter = Arc::clone(&old_calls);
        let new_counter = Arc::clone(&new_calls);

        ticker.lock().start(move |_| {
            old_counter.fetch_add(1, Ordering::SeqCst);
            let owner = weak
                .upgrade()
                .expect("the outer Arc is held by this test for its whole duration");
            let mut t = owner.lock();
            t.stop();
            let counter = Arc::clone(&new_counter);
            t.start(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            });
        });

        scheduler.execute_frame();
        let pending_after_restart = scheduler.transient_callback_count();
        scheduler.execute_frame();

        assert_eq!(
            old_calls.load(Ordering::SeqCst),
            1,
            "the superseded callback must never run again"
        );
        assert_eq!(
            new_calls.load(Ordering::SeqCst),
            1,
            "the new callback must run exactly once, on the frame after the restart"
        );
        assert_eq!(
            pending_after_restart, 1,
            "exactly one transient registration must exist after the restart frame"
        );

        ticker.lock().stop();
    }

    /// Probe #2: muting then unmuting from inside the running callback must
    /// retain the callback (delivered on the next frame) and must not
    /// duplicate the next-frame registration — `unmute()`'s own scheduling
    /// attempt runs while the slot is still checked out (not yet `Ready`),
    /// so it cannot register alongside the dispatch tail's own attempt.
    /// Before this fix the callback count never advanced past 1 (issue
    /// #1059's own measured evidence).
    #[test]
    fn mute_then_unmute_inside_tick_delivers_next_frame_once() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let weak = Arc::downgrade(&ticker);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);

        ticker.lock().start(move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                let owner = weak
                    .upgrade()
                    .expect("the outer Arc is held by this test for its whole duration");
                let mut t = owner.lock();
                t.mute();
                t.unmute();
            }
        });

        scheduler.execute_frame();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            scheduler.transient_callback_count(),
            1,
            "same-run mute/unmute inside the callback must not duplicate the \
             next-frame registration"
        );

        scheduler.execute_frame();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the callback must be retained through mute and delivered on the next frame"
        );

        ticker.lock().stop();
    }

    /// Control: `stop()` then `start(new)` BETWEEN frames (no reentrancy)
    /// already worked before this fix — a regression guard, not a probe.
    #[test]
    fn restart_between_ticks_preserves_new_callback() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let mut ticker = Ticker::new_with_scheduler(&scheduler);
        let old_calls = Arc::new(AtomicU32::new(0));
        let new_calls = Arc::new(AtomicU32::new(0));

        let counter = Arc::clone(&old_calls);
        ticker.start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        scheduler.execute_frame();
        ticker.stop();

        let counter = Arc::clone(&new_calls);
        ticker.start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        scheduler.execute_frame();

        assert_eq!(old_calls.load(Ordering::SeqCst), 1);
        assert_eq!(new_calls.load(Ordering::SeqCst), 1);
        assert_eq!(scheduler.transient_callback_count(), 1);

        ticker.stop();
    }

    /// Control: this already passed before the lease landed — `dispose`
    /// cleared the registration id and the dispatch tail returned at its
    /// own `disposed` check, so neither the restore nor the reschedule was
    /// reachable. It guards the hardening against a regression; it does not
    /// pin one of the defects this fix closes.
    #[test]
    fn dispose_inside_tick_drops_the_callback_and_does_not_reschedule() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let weak = Arc::downgrade(&ticker);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);

        ticker.lock().start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            let owner = weak
                .upgrade()
                .expect("the outer Arc is held by this test for its whole duration");
            owner.lock().dispose();
        });

        scheduler.execute_frame();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            scheduler.transient_callback_count(),
            0,
            "dispose() inside the tick must not leave a pending registration"
        );

        scheduler.execute_frame();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a disposed ticker must never tick again"
        );
        assert!(ticker.lock().is_disposed());
    }

    /// Control: this already passed before the lease landed — `reset` left
    /// the state `Idle`, so the dispatch tail's `state == Active` restore
    /// and its reschedule were both already skipped. It guards the
    /// hardening against a regression; it does not pin one of the defects
    /// this fix closes.
    #[test]
    fn reset_inside_tick_drops_the_callback_and_does_not_reschedule() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let weak = Arc::downgrade(&ticker);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);

        ticker.lock().start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            let owner = weak
                .upgrade()
                .expect("the outer Arc is held by this test for its whole duration");
            owner.lock().reset();
        });

        scheduler.execute_frame();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            scheduler.transient_callback_count(),
            0,
            "reset() inside the tick must not leave a pending registration"
        );
        assert_eq!(ticker.lock().state(), TickerState::Idle);

        scheduler.execute_frame();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a reset ticker must not tick again until restarted"
        );

        ticker.lock().stop();
    }

    /// A panicking callback must not leave the slot checked out forever —
    /// before this fix, `tick_and_reschedule_static` took the callback with
    /// a bare `Option::take()` and only restored it on the NORMAL return
    /// path, so an unwind lost it permanently.
    #[test]
    fn a_panicking_tick_callback_leaves_the_slot_restored() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let mut ticker = Ticker::new_with_scheduler(&scheduler);
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);

        ticker.start(move |_| {
            // Panics on the FIRST invocation only — the second call (after
            // the re-arm below) must actually run to completion, proving
            // the slot still holds a live, callable closure rather than
            // one that merely exists but panics unconditionally.
            assert!(
                counter.fetch_add(1, Ordering::SeqCst) != 0,
                "simulated panic inside a tick callback"
            );
        });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scheduler.execute_frame();
        }));
        assert!(
            result.is_err(),
            "the panic must propagate out of execute_frame"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            ticker.state(),
            TickerState::Active,
            "a panicking callback must not change the ticker's own state"
        );

        // The panicking dispatch is consumed by the scheduler's own
        // recovery (issue #1057) — nothing re-registers it automatically.
        // Re-arm via mute/unmute (ordinary reentry, not what this test is
        // about) and drive one more frame: if the callback were lost
        // forever (the bug this test pins), `tick_and_reschedule_static`
        // would find the slot empty and silently return without invoking
        // anything, and `calls` would never move past 1 — the OLD
        // `schedule_tick_if_active` predicate had no callback-presence
        // check, so it would still (wrongly) register a transient callback
        // that then dispatches into nothing.
        ticker.mute();
        ticker.unmute();
        scheduler.execute_frame();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the callback must still be installed after an unwind — the \
             lease restores it because the ticker was still Active when the \
             lease dropped during the panic's unwind"
        );

        ticker.stop();
    }

    /// A callback superseded by a reentrant restart is dropped with no
    /// scheduler-owned lock held. Proven with a `try_lock`-based probe
    /// rather than a blocking one (`Ticker::state()`) so a regression that
    /// reintroduces a held lock fails the assertion instead of hanging the
    /// test process.
    #[test]
    fn stale_callback_is_dropped_outside_the_lock() {
        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let weak = Arc::downgrade(&ticker);

        struct Canary {
            ticker: Arc<Mutex<Ticker>>,
            observed: Arc<Mutex<Vec<Option<TickerState>>>>,
        }
        impl Drop for Canary {
            fn drop(&mut self) {
                let state = self.ticker.lock().try_state();
                self.observed.lock().push(state);
            }
        }

        let observed: Arc<Mutex<Vec<Option<TickerState>>>> = Arc::new(Mutex::new(Vec::new()));
        let canary = Canary {
            ticker: Arc::clone(&ticker),
            observed: Arc::clone(&observed),
        };

        ticker.lock().start(move |_| {
            let _keep_alive = &canary;
            let owner = weak
                .upgrade()
                .expect("the outer Arc is held by this test for its whole duration");
            let mut t = owner.lock();
            t.stop();
            t.start(|_| {});
        });

        scheduler.execute_frame();

        assert_eq!(
            observed.lock().as_slice(),
            &[Some(TickerState::Active)],
            "the superseded callback's Drop must observe a free inner lock \
             (Some, not None) and the NEW run's Active state"
        );

        ticker.lock().stop();
    }

    /// The manual `tick(&self, ...)` path cannot support a reentrant
    /// restart the way the auto-scheduling dispatch does. Restarting from
    /// inside the callback needs `&mut Ticker` (`stop`/`start` both take
    /// it), which — since `tick` itself takes only `&self` — is only
    /// reachable through an outer wrapper like `Arc<Mutex<Ticker>>`, the
    /// SAME shape the auto-scheduling probes above use. The difference:
    /// `tick_and_reschedule_static` is a free function the scheduler
    /// invokes directly on the ticker's *inner* `Arc<Mutex<TickerInner>>`
    /// and never touches that outer wrapper, so a reentrant call through it
    /// finds the outer mutex free. `tick()` has no such indirection — a
    /// caller MUST already hold the outer `Mutex<Ticker>` (or `RefCell`) to
    /// obtain the `&Ticker` it calls `tick` on in the first place, and that
    /// guard is held for tick's entire call, callback included, because
    /// nothing inside `tick()` owns it and can release it early. A
    /// reentrant call back through that same non-reentrant lock therefore
    /// self-deadlocks (or panics, for a `RefCell`) before it ever reaches
    /// `stop()`. This is structural, not a gap to close, and not a test
    /// this suite can run: a hanging test is worse than no test. The
    /// manual path's restore contract is pinned below with a non-reentrant
    /// regression test instead.
    /// Control: the non-reentrant manual path already behaved this way; it
    /// stands in for the reentrant probe the borrow checker makes unwritable.
    #[test]
    fn stop_between_two_manual_ticks_does_not_reinvoke_callback() {
        let mut ticker = Ticker::new();
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        ticker.start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let provider = MockProvider;
        ticker.tick(&provider);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        ticker.stop();
        // `stop()` clears the (idle, `Ready`) slot; `tick()`'s own
        // `state != Active` guard then returns before checking anything out
        // again.
        ticker.tick(&provider);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a manually-stopped ticker must not tick again"
        );
    }

    /// Mixed manual and automatic dispatch on one ticker, from two threads.
    ///
    /// The slot's own state is what serialises the two paths: a checkout
    /// matches only `Ready`, so whichever dispatch arrives second finds
    /// `CheckedOut`, leaves it untouched and returns without invoking
    /// anything. Every other reentrancy test in this file drives that
    /// protocol from a single thread, where the mutex is never contended;
    /// this one contends it, so a future change that made the losing path
    /// fall through (or that restored the slot from the wrong owner) shows
    /// up as a double invocation for one logical tick rather than as
    /// reasoning about the code.
    ///
    /// Mixing the two dispatch modes is not a supported pattern — nothing
    /// in the workspace does it — so the assertion is deliberately the
    /// safety property (never more invocations than ticks issued, never a
    /// hang or a panic), not a schedule.
    #[test]
    fn manual_and_auto_dispatch_from_two_threads_never_double_invoke() {
        const ROUNDS: u32 = 200;

        let scheduler = crate::scheduler::UpdateScheduler::new();
        let ticker = Arc::new(Mutex::new(Ticker::new_with_scheduler(&scheduler)));
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);

        ticker.lock().start(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let manual_ticker = Arc::clone(&ticker);
        let manual = std::thread::spawn(move || {
            let provider = MockProvider;
            for _ in 0..ROUNDS {
                manual_ticker.lock().tick(&provider);
            }
        });

        for _ in 0..ROUNDS {
            scheduler.execute_frame();
        }
        manual
            .join()
            .expect("the manual dispatch thread does not panic");

        let observed = calls.load(Ordering::SeqCst);
        assert!(
            observed <= 2 * ROUNDS,
            "each dispatch may invoke the callback at most once: {observed} invocations for \
             {ROUNDS} manual ticks and {ROUNDS} frames"
        );
        assert!(
            observed > 0,
            "the ticker must have ticked at least once across {ROUNDS} rounds"
        );
    }
}
