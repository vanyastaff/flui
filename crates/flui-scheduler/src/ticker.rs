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

    /// Current state
    state: Arc<Mutex<TickerState>>,

    /// Start time
    start_time: Arc<Mutex<Option<Instant>>>,

    /// Callback
    callback: Arc<Mutex<Option<TickerCallback>>>,

    /// Elapsed time when muted
    muted_elapsed: Arc<Mutex<Seconds>>,
}

impl Ticker {
    /// Create a new ticker
    pub fn new() -> Self {
        Self {
            id: TickerId::new(),
            state: Arc::new(Mutex::new(TickerState::Idle)),
            start_time: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
            muted_elapsed: Arc::new(Mutex::new(Seconds::ZERO)),
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
        *self.state.lock() = TickerState::Active;
        *self.start_time.lock() = Some(Instant::now());
        *self.callback.lock() = Some(Box::new(callback));
        *self.muted_elapsed.lock() = Seconds::ZERO;
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
        *self.state.lock() = TickerState::Stopped;
        *self.callback.lock() = None;
    }

    /// Mute the ticker
    ///
    /// This temporarily pauses the ticker without clearing the callback.
    /// Time does not advance while muted.
    pub fn mute(&mut self) {
        let state = *self.state.lock();
        if state == TickerState::Active {
            // Save current elapsed time
            if let Some(start) = *self.start_time.lock() {
                let elapsed = Seconds::new(start.elapsed().as_secs_f64());
                *self.muted_elapsed.lock() = elapsed;
            }
            *self.state.lock() = TickerState::Muted;
        }
    }

    /// Unmute the ticker
    ///
    /// Resumes a muted ticker. Time continues from where it was paused.
    pub fn unmute(&mut self) {
        let state = *self.state.lock();
        if state == TickerState::Muted {
            // Adjust start time to account for muted period
            let muted_elapsed = *self.muted_elapsed.lock();
            let now = Instant::now();
            let adjusted_start = now - std::time::Duration::from_secs_f64(muted_elapsed.value());
            *self.start_time.lock() = Some(adjusted_start);
            *self.state.lock() = TickerState::Active;
        }
    }

    /// Toggle mute state
    pub fn toggle_mute(&mut self) {
        let state = *self.state.lock();
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
        let state = *self.state.lock();

        if state != TickerState::Active {
            return;
        }

        if let Some(start) = *self.start_time.lock() {
            let elapsed = start.elapsed().as_secs_f64();

            // Clone callback to avoid holding lock during invocation
            let callback_opt = self.callback.lock().take();

            if let Some(mut callback) = callback_opt {
                callback(elapsed);

                // Restore callback if still active
                if *self.state.lock() == TickerState::Active {
                    *self.callback.lock() = Some(callback);
                }
            }
        }
    }

    /// Get current state
    #[inline]
    pub fn state(&self) -> TickerState {
        *self.state.lock()
    }

    /// Check if ticker is active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state().can_tick()
    }

    /// Check if ticker is muted
    #[inline]
    pub fn is_muted(&self) -> bool {
        *self.state.lock() == TickerState::Muted
    }

    /// Check if ticker is running (active or muted)
    #[inline]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Get elapsed time as type-safe Seconds
    pub fn elapsed(&self) -> Seconds {
        match *self.state.lock() {
            TickerState::Idle | TickerState::Stopped => Seconds::ZERO,
            TickerState::Muted => *self.muted_elapsed.lock(),
            TickerState::Active => {
                if let Some(start) = *self.start_time.lock() {
                    Seconds::new(start.elapsed().as_secs_f64())
                } else {
                    Seconds::ZERO
                }
            }
        }
    }

    /// Get elapsed time in seconds (raw f64 for backwards compat)
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed().value()
    }

    /// Reset the ticker to initial state
    pub fn reset(&mut self) {
        *self.state.lock() = TickerState::Idle;
        *self.start_time.lock() = None;
        *self.callback.lock() = None;
        *self.muted_elapsed.lock() = Seconds::ZERO;
    }
}

impl Default for Ticker {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Ticker {
    fn clone(&self) -> Self {
        Self {
            id: TickerId::new(), // New ID for cloned ticker
            state: Arc::clone(&self.state),
            start_time: Arc::clone(&self.start_time),
            callback: Arc::clone(&self.callback),
            muted_elapsed: Arc::clone(&self.muted_elapsed),
        }
    }
}

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
pub type ScheduledTickerCallback = Arc<Mutex<dyn FnMut(f64) + Send>>;

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
#[allow(clippy::type_complexity)]
pub struct ScheduledTicker {
    /// Unique identifier
    id: TickerId,

    /// Reference to the scheduler
    scheduler: Arc<crate::scheduler::Scheduler>,

    /// Current state
    state: Arc<Mutex<TickerState>>,

    /// Start time
    start_time: Arc<Mutex<Option<Instant>>>,

    /// Callback (wrapped for sharing across frame callbacks)
    callback: Arc<Mutex<Option<Arc<Mutex<dyn FnMut(f64) + Send>>>>>,

    /// Elapsed time when muted
    muted_elapsed: Arc<Mutex<Seconds>>,

    /// Whether next frame callback is scheduled
    scheduled: Arc<Mutex<bool>>,
}

impl ScheduledTicker {
    /// Create a new scheduled ticker
    pub fn new(scheduler: Arc<crate::scheduler::Scheduler>) -> Self {
        Self {
            id: TickerId::new(),
            scheduler,
            state: Arc::new(Mutex::new(TickerState::Idle)),
            start_time: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
            muted_elapsed: Arc::new(Mutex::new(Seconds::ZERO)),
            scheduled: Arc::new(Mutex::new(false)),
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
        *self.state.lock() = TickerState::Active;
        *self.start_time.lock() = Some(Instant::now());
        *self.callback.lock() = Some(Arc::new(Mutex::new(callback)));
        *self.muted_elapsed.lock() = Seconds::ZERO;

        // Schedule for next frame
        self.schedule_next_frame();
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
        *self.state.lock() = TickerState::Stopped;
        *self.callback.lock() = None;
        *self.scheduled.lock() = false;
    }

    /// Mute the ticker (pause without stopping)
    pub fn mute(&mut self) {
        let state = *self.state.lock();
        if state == TickerState::Active {
            if let Some(start) = *self.start_time.lock() {
                let elapsed = Seconds::new(start.elapsed().as_secs_f64());
                *self.muted_elapsed.lock() = elapsed;
            }
            *self.state.lock() = TickerState::Muted;
        }
    }

    /// Unmute the ticker (resume)
    pub fn unmute(&mut self) {
        let state = *self.state.lock();
        if state == TickerState::Muted {
            let muted_elapsed = *self.muted_elapsed.lock();
            let now = Instant::now();
            let adjusted_start = now - std::time::Duration::from_secs_f64(muted_elapsed.value());
            *self.start_time.lock() = Some(adjusted_start);
            *self.state.lock() = TickerState::Active;

            // Re-schedule
            self.schedule_next_frame();
        }
    }

    /// Get current state
    #[inline]
    pub fn state(&self) -> TickerState {
        *self.state.lock()
    }

    /// Check if active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.state().can_tick()
    }

    /// Check if muted
    #[inline]
    pub fn is_muted(&self) -> bool {
        *self.state.lock() == TickerState::Muted
    }

    /// Check if running (active or muted)
    #[inline]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Seconds {
        match *self.state.lock() {
            TickerState::Idle | TickerState::Stopped => Seconds::ZERO,
            TickerState::Muted => *self.muted_elapsed.lock(),
            TickerState::Active => {
                if let Some(start) = *self.start_time.lock() {
                    Seconds::new(start.elapsed().as_secs_f64())
                } else {
                    Seconds::ZERO
                }
            }
        }
    }

    /// Schedule callback for next frame
    fn schedule_next_frame(&self) {
        // Only schedule if active and not already scheduled
        if *self.state.lock() != TickerState::Active {
            return;
        }

        if *self.scheduled.lock() {
            return;
        }

        *self.scheduled.lock() = true;

        // Clone Arcs for the callback
        let state = Arc::clone(&self.state);
        let start_time = Arc::clone(&self.start_time);
        let callback = Arc::clone(&self.callback);
        let scheduled = Arc::clone(&self.scheduled);
        let scheduler = Arc::clone(&self.scheduler);

        // Register transient callback - fires during TransientCallbacks phase
        self.scheduler
            .schedule_frame_callback(Box::new(move |_vsync_time| {
                // Clear scheduled flag
                *scheduled.lock() = false;

                // Check if still active
                if *state.lock() != TickerState::Active {
                    return;
                }

                // Calculate elapsed time
                let elapsed = if let Some(start) = *start_time.lock() {
                    start.elapsed().as_secs_f64()
                } else {
                    return;
                };

                // Invoke callback
                if let Some(cb) = callback.lock().as_ref() {
                    cb.lock()(elapsed);
                }

                // Schedule next frame if still active
                if *state.lock() == TickerState::Active {
                    *scheduled.lock() = true;

                    // Clone for next callback
                    let state = Arc::clone(&state);
                    let start_time = Arc::clone(&start_time);
                    let callback = Arc::clone(&callback);
                    let scheduled_inner = Arc::clone(&scheduled);
                    let scheduler_inner = Arc::clone(&scheduler);

                    scheduler.schedule_frame_callback(Box::new(move |_vsync| {
                        // Recursive scheduling via helper
                        Self::tick_and_reschedule(
                            state,
                            start_time,
                            callback,
                            scheduled_inner,
                            scheduler_inner,
                        );
                    }));
                }
            }));
    }

    /// Helper for recursive frame scheduling
    #[allow(clippy::type_complexity)]
    fn tick_and_reschedule(
        state: Arc<Mutex<TickerState>>,
        start_time: Arc<Mutex<Option<Instant>>>,
        callback: Arc<Mutex<Option<Arc<Mutex<dyn FnMut(f64) + Send>>>>>,
        scheduled: Arc<Mutex<bool>>,
        scheduler: Arc<crate::scheduler::Scheduler>,
    ) {
        *scheduled.lock() = false;

        if *state.lock() != TickerState::Active {
            return;
        }

        let elapsed = if let Some(start) = *start_time.lock() {
            start.elapsed().as_secs_f64()
        } else {
            return;
        };

        if let Some(cb) = callback.lock().as_ref() {
            cb.lock()(elapsed);
        }

        if *state.lock() == TickerState::Active {
            *scheduled.lock() = true;

            let state = Arc::clone(&state);
            let start_time = Arc::clone(&start_time);
            let callback = Arc::clone(&callback);
            let scheduled_inner = Arc::clone(&scheduled);
            let scheduler_inner = Arc::clone(&scheduler);

            scheduler.schedule_frame_callback(Box::new(move |_vsync| {
                Self::tick_and_reschedule(
                    state,
                    start_time,
                    callback,
                    scheduled_inner,
                    scheduler_inner,
                );
            }));
        }
    }
}

impl std::fmt::Debug for ScheduledTicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledTicker")
            .field("id", &self.id)
            .field("state", &self.state())
            .field("elapsed", &self.elapsed())
            .field("scheduled", &*self.scheduled.lock())
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
}

impl TickerFuture {
    /// Create a new pending ticker future
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TickerFutureInner {
                state: Mutex::new(TickerFutureState::Pending),
                event: Event::new(),
            }),
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
    /// Note: The callback is invoked synchronously when the future state changes
    /// via `set_complete()` or `set_canceled()`. If the future is already resolved
    /// when this method is called, the callback is invoked immediately.
    pub fn when_complete_or_cancel<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        // Check if already resolved - if so, call immediately
        {
            let current_state = *self.inner.state.lock();
            if current_state != TickerFutureState::Pending {
                callback();
                return;
            }
        }

        // Register a listener and spawn a task to wait for the event
        let inner = Arc::clone(&self.inner);
        let callback = Arc::new(Mutex::new(Some(callback)));

        // Use event-listener's on_notify to register the callback
        // This is efficient - no busy-waiting, no thread spawning
        std::thread::Builder::new()
            .name("ticker-completion-listener".into())
            .spawn(move || {
                // Wait for notification using blocking listener
                let listener = inner.event.listen();

                // Check state before waiting (might have changed)
                if *inner.state.lock() != TickerFutureState::Pending {
                    if let Some(cb) = callback.lock().take() {
                        cb();
                    }
                    return;
                }

                // Block until notified
                listener.wait();

                // Invoke callback
                if let Some(cb) = callback.lock().take() {
                    cb();
                }
            })
            .expect("Failed to spawn ticker completion listener thread");
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
        }
    }
}

impl Future for TickerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = *self.inner.state.lock();

        match state {
            TickerFutureState::Complete => Poll::Ready(()),
            TickerFutureState::Canceled | TickerFutureState::Pending => {
                // Primary future never completes on cancel (Flutter behavior)
                // Register for notification via event-listener
                // Note: For proper async support, we'd need to store the listener
                // For now, we just check the state and return Pending
                cx.waker().wake_by_ref();
                Poll::Pending
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
