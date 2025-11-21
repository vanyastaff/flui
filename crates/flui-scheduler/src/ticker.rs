//! Animation ticker - frame-perfect animation timing
//!
//! Tickers provide a way to receive callbacks on every frame for driving animations.
//! They coordinate with the scheduler to ensure animations stay synchronized with
//! the display refresh rate.

use instant::Instant;
use parking_lot::Mutex;
use std::sync::Arc;

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
}

/// State of a ticker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickerState {
    /// Not started yet
    Idle,

    /// Currently ticking
    Active,

    /// Temporarily paused
    Muted,

    /// Permanently stopped
    Stopped,
}

/// Animation ticker
///
/// A Ticker provides callbacks on every frame, allowing you to drive animations
/// in sync with the display refresh rate.
///
/// ## Example
///
/// ```rust
/// use flui_scheduler::{Ticker, TickerProvider, Scheduler};
///
/// let scheduler = Scheduler::new();
/// let mut ticker = Ticker::new();
///
/// ticker.start(|elapsed| {
///     println!("Frame at {:.3}s", elapsed);
/// });
///
/// // In your render loop
/// ticker.tick(&scheduler);
/// ```
pub struct Ticker {
    /// Current state
    state: Arc<Mutex<TickerState>>,

    /// Start time
    start_time: Arc<Mutex<Option<Instant>>>,

    /// Callback
    callback: Arc<Mutex<Option<TickerCallback>>>,

    /// Elapsed time when muted
    muted_elapsed: Arc<Mutex<f64>>,
}

impl Ticker {
    /// Create a new ticker
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TickerState::Idle)),
            start_time: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
            muted_elapsed: Arc::new(Mutex::new(0.0)),
        }
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
        *self.muted_elapsed.lock() = 0.0;
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
                let elapsed = start.elapsed().as_secs_f64();
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
            let adjusted_start = now - std::time::Duration::from_secs_f64(muted_elapsed);
            *self.start_time.lock() = Some(adjusted_start);
            *self.state.lock() = TickerState::Active;
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
    pub fn state(&self) -> TickerState {
        *self.state.lock()
    }

    /// Check if ticker is active
    pub fn is_active(&self) -> bool {
        *self.state.lock() == TickerState::Active
    }

    /// Check if ticker is muted
    pub fn is_muted(&self) -> bool {
        *self.state.lock() == TickerState::Muted
    }

    /// Get elapsed time
    ///
    /// Returns elapsed time in seconds, or 0.0 if not started.
    pub fn elapsed(&self) -> f64 {
        match *self.state.lock() {
            TickerState::Idle | TickerState::Stopped => 0.0,
            TickerState::Muted => *self.muted_elapsed.lock(),
            TickerState::Active => {
                if let Some(start) = *self.start_time.lock() {
                    start.elapsed().as_secs_f64()
                } else {
                    0.0
                }
            }
        }
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
            state: Arc::clone(&self.state),
            start_time: Arc::clone(&self.start_time),
            callback: Arc::clone(&self.callback),
            muted_elapsed: Arc::clone(&self.muted_elapsed),
        }
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

        ticker.unmute();
        assert!(ticker.is_active());
        assert!(!ticker.is_muted());
    }

    #[test]
    fn test_ticker_elapsed() {
        let mut ticker = Ticker::new();
        assert_eq!(ticker.elapsed(), 0.0);

        ticker.start(|_| {});

        // Give some time to elapse
        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed > 0.0);
        assert!(elapsed < 1.0); // Should be less than 1 second
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
}
