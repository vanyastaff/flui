//! Animation ticker - frame-perfect animation timing
//!
//! Tickers provide a way to receive callbacks on every frame for driving animations.
//! They coordinate with the scheduler to ensure animations stay synchronized with
//! the display refresh rate.
//!
//! ## Ticker Types
//!
//! This module provides multiple ticker implementations:
//!
//! - **`Ticker`**: Manual ticking, you call `tick()` each frame
//! - **`ScheduledTicker`**: Auto-schedules with scheduler, Flutter-like behavior
//! - **`TypestateTicker`**: Compile-time state checking (see `typestate` module)
//!
//! ## Manual Ticker Example
//!
//! ```rust
//! use flui_scheduler::{Ticker, TickerProvider, Scheduler};
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
//! ## Scheduled Ticker Example (Flutter-like)
//!
//! ```rust
//! use flui_scheduler::{Scheduler, ScheduledTicker};
//! use std::sync::Arc;
//!
//! let scheduler = Arc::new(Scheduler::new());
//! let mut ticker = ScheduledTicker::new(scheduler.clone());
//!
//! // Start auto-schedules callbacks with the scheduler
//! ticker.start(|elapsed| {
//!     println!("Auto-ticked at {:.3}s", elapsed);
//! });
//!
//! // Ticker automatically registers for next frame
//! // No need to manually call tick()
//! ```

use crate::duration::Seconds;
use crate::id::{TickerIdMarker, TypedId};
use parking_lot::Mutex;
use std::sync::Arc;
use web_time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique ticker identifier
pub type TickerId = TypedId<TickerIdMarker>;

/// Ticker callback - receives elapsed time in seconds
pub type TickerCallback = Box<dyn FnMut(f64) + Send>;

/// Ticker provider trait
///
/// This trait allows different parts of the framework to provide ticker
/// functionality without tight coupling to the scheduler.
pub trait TickerProvider: Send + Sync {
    /// Schedule a tick callback for the next frame
    ///
    /// The callback will be invoked on the next frame. The `f64` parameter
    /// is reserved for frame timing information but is typically `0.0` since
    /// individual `Ticker` and `ScheduledTicker` instances track their own
    /// start times and compute elapsed time internally.
    ///
    /// This matches Flutter's `TickerProvider` behavior where the provider
    /// just schedules when ticks occur, not how elapsed time is computed.
    fn schedule_tick(&self, callback: Box<dyn FnOnce(f64) + Send>);

    /// Schedule a tick with type-safe elapsed time
    fn schedule_tick_typed(&self, callback: Box<dyn FnOnce(Seconds) + Send>) {
        self.schedule_tick(Box::new(move |elapsed| {
            callback(Seconds::new(elapsed));
        }));
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
}

/// Animation ticker with runtime state management
///
/// A Ticker provides callbacks on every frame, allowing you to drive animations
/// in sync with the display refresh rate.
///
/// For compile-time state safety, see `TypestateTicker` in the `typestate` module.
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
}

impl Ticker {
    /// Create a new ticker
    pub fn new() -> Self {
        Self {
            id: TickerId::new(),
            inner: Arc::new(Mutex::new(TickerInner {
                state: TickerState::Idle,
                start_time: None,
                callback: None,
                muted_elapsed: Seconds::ZERO,
            })),
        }
    }

    /// Get the ticker ID
    #[inline]
    pub fn id(&self) -> TickerId {
        self.id
    }

    /// Start the ticker with a callback
    ///
    /// The callback receives the elapsed time in seconds since start.
    pub fn start<F>(&mut self, callback: F)
    where
        F: FnMut(f64) + Send + 'static,
    {
        let mut inner = self.inner.lock();
        inner.state = TickerState::Active;
        inner.start_time = Some(Instant::now());
        inner.callback = Some(Box::new(callback));
        inner.muted_elapsed = Seconds::ZERO;
    }

    /// Start the ticker with a type-safe callback
    pub fn start_typed<F>(&mut self, mut callback: F)
    where
        F: FnMut(Seconds) + Send + 'static,
    {
        self.start(move |elapsed| callback(Seconds::new(elapsed)));
    }

    /// Stop the ticker
    ///
    /// This permanently stops the ticker. Call `start()` to restart.
    pub fn stop(&mut self) {
        let mut inner = self.inner.lock();
        inner.state = TickerState::Stopped;
        inner.callback = None;
    }

    /// Mute the ticker
    ///
    /// This temporarily pauses the ticker without clearing the callback.
    /// Time does not advance while muted.
    pub fn mute(&mut self) {
        let mut inner = self.inner.lock();
        if inner.state == TickerState::Active {
            if let Some(start) = inner.start_time {
                inner.muted_elapsed = Seconds::new(start.elapsed().as_secs_f64());
            }
            inner.state = TickerState::Muted;
        }
    }

    /// Unmute the ticker
    ///
    /// Resumes a muted ticker. Time continues from where it was paused.
    pub fn unmute(&mut self) {
        let mut inner = self.inner.lock();
        if inner.state == TickerState::Muted {
            let now = Instant::now();
            let adjusted_start =
                now - std::time::Duration::from_secs_f64(inner.muted_elapsed.value());
            inner.start_time = Some(adjusted_start);
            inner.state = TickerState::Active;
        }
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
                .map(|s| Seconds::new(s.elapsed().as_secs_f64()))
                .unwrap_or(Seconds::ZERO),
        }
    }

    /// Get elapsed time in seconds (raw f64 for backwards compat)
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed().value()
    }

    /// Reset the ticker to initial state
    pub fn reset(&mut self) {
        let mut inner = self.inner.lock();
        inner.state = TickerState::Idle;
        inner.start_time = None;
        inner.callback = None;
        inner.muted_elapsed = Seconds::ZERO;
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
            .finish()
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

// =============================================================================
// ScheduledTicker - Flutter-like auto-scheduling ticker
// =============================================================================

/// Callback for ScheduledTicker that receives elapsed time in seconds
pub type ScheduledTickerCallback = Box<dyn FnMut(f64) + Send>;

/// Shared inner state for a ScheduledTicker (single allocation, single lock)
struct ScheduledTickerInner {
    state: TickerState,
    start_time: Option<Instant>,
    callback: Option<ScheduledTickerCallback>,
    muted_elapsed: Seconds,
    scheduled: bool,
}

/// A Flutter-like ticker that automatically schedules with the scheduler
///
/// Unlike `Ticker` which requires manual `tick()` calls, `ScheduledTicker`
/// automatically registers transient callbacks with the scheduler on each frame.
/// This is the recommended approach for animations.
///
/// # Flutter Comparison
///
/// In Flutter, a `Ticker` is provided by a `TickerProvider` (usually a `State` mixin)
/// and automatically receives vsync callbacks. `ScheduledTicker` provides the same
/// behavior in Rust.
///
/// # Examples
///
/// ```rust
/// use flui_scheduler::{Scheduler, ScheduledTicker};
/// use std::sync::Arc;
///
/// let scheduler = Arc::new(Scheduler::new());
/// let mut ticker = ScheduledTicker::new(scheduler.clone());
///
/// ticker.start(|elapsed| {
///     println!("Animation at {:.3}s", elapsed);
/// });
///
/// // Ticker auto-schedules - just run frames
/// scheduler.execute_frame();
/// scheduler.execute_frame();
///
/// ticker.stop();
/// ```
///
/// # Why ScheduledTicker doesn't implement Clone
///
/// `ScheduledTicker` intentionally does not implement `Clone` because:
///
/// 1. **Unique Identity**: Each ticker has a unique `TickerId`. Cloning would create
///    ambiguity about which ticker is "the real one".
///
/// 2. **Shared Mutable Callback**: The callback is `FnMut`, which mutates state on
///    each invocation. Sharing it between clones would cause race conditions.
///
/// 3. **Scheduling Conflicts**: Multiple tickers sharing the same `scheduled` flag
///    would interfere with each other's frame scheduling.
///
/// 4. **Flutter Semantics**: In Flutter, `Ticker` objects are not cloneable either.
///    Each animation controller owns exactly one ticker.
///
/// If you need multiple tickers, create them individually with `ScheduledTicker::new()`.
pub struct ScheduledTicker {
    /// Unique identifier
    id: TickerId,

    /// Reference to the scheduler
    scheduler: Arc<crate::scheduler::Scheduler>,

    /// All mutable state behind a single lock
    inner: Arc<Mutex<ScheduledTickerInner>>,
}

impl ScheduledTicker {
    /// Create a new scheduled ticker
    pub fn new(scheduler: Arc<crate::scheduler::Scheduler>) -> Self {
        Self {
            id: TickerId::new(),
            scheduler,
            inner: Arc::new(Mutex::new(ScheduledTickerInner {
                state: TickerState::Idle,
                start_time: None,
                callback: None,
                muted_elapsed: Seconds::ZERO,
                scheduled: false,
            })),
        }
    }

    /// Get the ticker ID
    #[inline]
    pub fn id(&self) -> TickerId {
        self.id
    }

    /// Start the ticker with a callback
    ///
    /// The callback receives elapsed time in seconds since start.
    /// Automatically schedules for the next frame.
    pub fn start<F>(&mut self, callback: F)
    where
        F: FnMut(f64) + Send + 'static,
    {
        tracing::debug!("ScheduledTicker::start called");
        {
            let mut inner = self.inner.lock();
            inner.state = TickerState::Active;
            inner.start_time = Some(Instant::now());
            inner.callback = Some(Box::new(callback));
            inner.muted_elapsed = Seconds::ZERO;
        }

        // Schedule for next frame
        self.schedule_next_frame();
        tracing::debug!("ScheduledTicker start completed, scheduled next frame");
    }

    /// Start with a type-safe callback
    pub fn start_typed<F>(&mut self, mut callback: F)
    where
        F: FnMut(Seconds) + Send + 'static,
    {
        self.start(move |elapsed| callback(Seconds::new(elapsed)));
    }

    /// Stop the ticker
    pub fn stop(&mut self) {
        let mut inner = self.inner.lock();
        inner.state = TickerState::Stopped;
        inner.callback = None;
        inner.scheduled = false;
    }

    /// Mute the ticker (pause without stopping)
    pub fn mute(&mut self) {
        let mut inner = self.inner.lock();
        if inner.state == TickerState::Active {
            if let Some(start) = inner.start_time {
                inner.muted_elapsed = Seconds::new(start.elapsed().as_secs_f64());
            }
            inner.state = TickerState::Muted;
        }
    }

    /// Unmute the ticker (resume)
    pub fn unmute(&mut self) {
        {
            let mut inner = self.inner.lock();
            if inner.state != TickerState::Muted {
                return;
            }
            let now = Instant::now();
            let adjusted_start =
                now - std::time::Duration::from_secs_f64(inner.muted_elapsed.value());
            inner.start_time = Some(adjusted_start);
            inner.state = TickerState::Active;
        }

        // Re-schedule
        self.schedule_next_frame();
    }

    /// Get current state
    #[inline]
    pub fn state(&self) -> TickerState {
        self.inner.lock().state
    }

    /// Check if active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state().can_tick()
    }

    /// Check if muted
    #[inline]
    pub fn is_muted(&self) -> bool {
        self.inner.lock().state == TickerState::Muted
    }

    /// Check if running (active or muted)
    #[inline]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Seconds {
        let inner = self.inner.lock();
        match inner.state {
            TickerState::Idle | TickerState::Stopped => Seconds::ZERO,
            TickerState::Muted => inner.muted_elapsed,
            TickerState::Active => inner
                .start_time
                .map(|s| Seconds::new(s.elapsed().as_secs_f64()))
                .unwrap_or(Seconds::ZERO),
        }
    }

    /// Schedule callback for next frame
    fn schedule_next_frame(&self) {
        {
            let mut inner = self.inner.lock();
            if inner.state != TickerState::Active || inner.scheduled {
                return;
            }
            inner.scheduled = true;
        }

        let inner = Arc::clone(&self.inner);
        let scheduler = Arc::clone(&self.scheduler);

        self.scheduler
            .schedule_frame_callback(Box::new(move |_vsync| {
                Self::tick_and_reschedule(inner, scheduler);
            }));
    }

    /// Tick callback and reschedule for next frame if still active.
    ///
    /// This is the single code path for all scheduled ticker frame callbacks.
    /// Uses take-invoke-restore to avoid holding the lock during callback invocation
    /// and avoids the extra Arc<Mutex> wrapper that was previously on the callback.
    fn tick_and_reschedule(
        inner: Arc<Mutex<ScheduledTickerInner>>,
        scheduler: Arc<crate::scheduler::Scheduler>,
    ) {
        // Take elapsed and callback under lock, then release before invoking
        let (elapsed, mut callback) = {
            let mut guard = inner.lock();
            guard.scheduled = false;

            tracing::trace!(state = ?guard.state, "ScheduledTicker tick");
            if guard.state != TickerState::Active {
                return;
            }

            let Some(start) = guard.start_time else {
                tracing::trace!("ScheduledTicker no start_time, skipping");
                return;
            };

            // Take callback out to invoke without holding the lock
            (start.elapsed().as_secs_f64(), guard.callback.take())
        };

        if let Some(ref mut cb) = callback {
            tracing::trace!(elapsed, "ScheduledTicker invoking callback");
            cb(elapsed);
        }

        // Restore callback and re-schedule if still active
        let should_reschedule = {
            let mut guard = inner.lock();
            // Only restore if still active (stop() may have been called during callback)
            if guard.state == TickerState::Active {
                guard.callback = callback;
                tracing::trace!("ScheduledTicker scheduling next frame");
                guard.scheduled = true;
                true
            } else {
                false
            }
        };

        if should_reschedule {
            let inner = Arc::clone(&inner);
            let scheduler_inner = Arc::clone(&scheduler);

            scheduler.schedule_frame_callback(Box::new(move |_vsync| {
                Self::tick_and_reschedule(inner, scheduler_inner);
            }));
        }
    }
}

impl std::fmt::Debug for ScheduledTicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("ScheduledTicker")
            .field("id", &self.id)
            .field("state", &inner.state)
            .field("scheduled", &inner.scheduled)
            .finish()
    }
}

// ============================================================================
// TickerFuture and TickerCanceled - Flutter-compatible async ticker support
// ============================================================================

use event_listener::{Event, Listener};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

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
/// `TickerFuture` is returned by ticker start methods and completes when the ticker
/// is stopped. It provides two ways to await completion:
///
/// - Awaiting the future directly completes when the ticker stops normally
/// - Using [`or_cancel`](Self::or_cancel) returns a future that also completes
///   with an error if the ticker is canceled
///
/// # Example
///
/// ```rust
/// use flui_scheduler::ticker::{TickerFuture, TickerCanceled};
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

    /// Mark the future as complete (ticker stopped normally)
    ///
    /// Reserved for future use when ScheduledTicker integrates with TickerFuture.
    #[allow(dead_code)]
    pub(crate) fn set_complete(&self) {
        let mut state = self.inner.state.lock();
        if *state == TickerFutureState::Pending {
            *state = TickerFutureState::Complete;
            drop(state);
            // Notify all waiters
            self.inner.event.notify(usize::MAX);
        }
    }

    /// Mark the future as canceled
    ///
    /// Reserved for future use when ScheduledTicker integrates with TickerFuture.
    #[allow(dead_code)]
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
    /// canceled, the returned future will complete with a [`TickerCanceled`] error.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_scheduler::ticker::{TickerFuture, TickerCanceled};
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
    /// If the future is already resolved when this method is called, the callback
    /// is invoked immediately on the current thread. Otherwise, a lightweight
    /// listener is registered that invokes the callback when the state changes.
    ///
    /// **Note**: If the future is still pending, this method blocks the current
    /// thread until the ticker completes or is canceled. For non-blocking usage,
    /// use [`or_cancel`](Self::or_cancel) with async/await instead.
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
        listener.wait();
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
                                self.listener = None;
                                continue; // Re-check state
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
        write!(f, "TickerFuture({})", state_str)
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
                                // Event fired, clear listener and re-check state
                                self.listener = None;
                                continue;
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
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockProvider;

    impl TickerProvider for MockProvider {
        fn schedule_tick(&self, callback: Box<dyn FnOnce(f64) + Send>) {
            callback(0.0);
        }
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
        ticker.start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(5));

        ticker.reset();

        assert_eq!(ticker.state(), TickerState::Idle);
        assert_eq!(ticker.elapsed(), Seconds::ZERO);
    }

    // ScheduledTicker tests

    #[test]
    fn test_scheduled_ticker_lifecycle() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let mut ticker = ScheduledTicker::new(scheduler.clone());

        assert_eq!(ticker.state(), TickerState::Idle);
        assert!(!ticker.is_active());

        ticker.start(|_| {});
        assert_eq!(ticker.state(), TickerState::Active);
        assert!(ticker.is_active());

        ticker.stop();
        assert_eq!(ticker.state(), TickerState::Stopped);
    }

    #[test]
    fn test_scheduled_ticker_auto_scheduling() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = ScheduledTicker::new(scheduler.clone());
        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        // Execute frames - ticker should auto-tick
        scheduler.execute_frame();
        scheduler.execute_frame();
        scheduler.execute_frame();

        // Callback should have been invoked each frame
        assert_eq!(counter.load(Ordering::Relaxed), 3);

        ticker.stop();

        // After stop, no more callbacks
        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_scheduled_ticker_mute() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let counter = Arc::new(AtomicU32::new(0));

        let mut ticker = ScheduledTicker::new(scheduler.clone());
        let c = Arc::clone(&counter);
        ticker.start(move |_elapsed| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        scheduler.execute_frame();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        ticker.mute();
        scheduler.execute_frame();
        // Still 1 - muted ticker doesn't fire
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        ticker.unmute();
        scheduler.execute_frame();
        // Now 2 - unmuted
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_scheduled_ticker_elapsed() {
        let scheduler = Arc::new(crate::scheduler::Scheduler::new());
        let mut ticker = ScheduledTicker::new(scheduler);

        assert_eq!(ticker.elapsed(), Seconds::ZERO);

        ticker.start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed.value() > 0.0);
        assert!(elapsed.value() < 1.0);
    }
}
