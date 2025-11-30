//! Animation ticker - frame-perfect animation timing
//!
//! Tickers provide a way to receive callbacks on every frame for driving animations.
//! They coordinate with the scheduler to ensure animations stay synchronized with
//! the display refresh rate.
//!
//! ## Typestate vs Runtime Ticker
//!
//! This module provides two ticker implementations:
//!
//! - **`Ticker`**: Runtime state machine, flexible and familiar API
//! - **`TypestateTicker`**: Compile-time state checking (see `typestate` module)
//!
//! ## Example
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
//! // In your render loop
//! ticker.tick(&scheduler);
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
    /// Schedule a tick callback
    ///
    /// The callback will be invoked on the next frame with the elapsed time.
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
}
