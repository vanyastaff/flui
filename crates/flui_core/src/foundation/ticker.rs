//! Ticker system for frame callbacks.
//!
//! Provides frame-based callbacks for animations and other time-dependent operations.

use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A callback function called by a ticker each frame.
///
/// Receives the elapsed time since the ticker started.
pub type TickerCallback = Arc<dyn Fn(Duration) + Send + Sync>;

/// Provides frame callbacks for animations.
///
/// A Ticker ticks once per frame, calling a callback with the elapsed time.
/// This is the foundation for time-based animations.
///
/// # Thread Safety
///
/// Ticker is thread-safe and uses `Arc` for shared ownership.
///
/// # Examples
///
/// ```
/// use flui_core::foundation::Ticker;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let ticker = Ticker::new(Arc::new(|elapsed: Duration| {
///     println!("Elapsed: {:?}", elapsed);
/// }));
///
/// ticker.start();
/// // ... ticker.tick() called by frame scheduler ...
/// ticker.stop();
/// ```
#[derive(Clone)]
pub struct Ticker {
    inner: Arc<Mutex<TickerInner>>,
}

struct TickerInner {
    callback: TickerCallback,
    is_active: bool,
    start_time: Option<Instant>,
    muted: bool,
}

impl Ticker {
    /// Create a new ticker with the given callback.
    #[must_use]
    pub fn new(callback: TickerCallback) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TickerInner {
                callback,
                is_active: false,
                start_time: None,
                muted: false,
            })),
        }
    }

    /// Start ticking.
    ///
    /// The ticker will begin calling its callback on each frame.
    pub fn start(&self) {
        let mut inner = self.inner.lock();
        if !inner.is_active {
            inner.is_active = true;
            inner.start_time = Some(Instant::now());
        }
    }

    /// Stop ticking.
    ///
    /// The ticker will stop calling its callback.
    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        inner.is_active = false;
    }

    /// Check if the ticker is currently active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.inner.lock().is_active
    }

    /// Mute the ticker.
    ///
    /// When muted, the ticker remains active but doesn't call the callback.
    /// This is useful for animations that should pause but not reset.
    pub fn mute(&self) {
        let mut inner = self.inner.lock();
        inner.muted = true;
    }

    /// Unmute the ticker.
    pub fn unmute(&self) {
        let mut inner = self.inner.lock();
        inner.muted = false;
    }

    /// Check if the ticker is muted.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.inner.lock().muted
    }

    /// Called each frame by the frame scheduler.
    ///
    /// If the ticker is active and not muted, calls the callback with the
    /// elapsed time since start.
    pub fn tick(&self, now: Instant) {
        let inner = self.inner.lock();

        if !inner.is_active || inner.muted {
            return;
        }

        if let Some(start) = inner.start_time {
            let elapsed = now.duration_since(start);
            (inner.callback)(elapsed);
        }
    }

    /// Dispose the ticker.
    ///
    /// Stops the ticker and cleans up resources.
    pub fn dispose(&self) {
        self.stop();
    }

    /// Unschedule the ticker if it's scheduled.
    ///
    /// This is an alias for `stop()` to match Flutter's API.
    #[inline]
    pub fn unschedule(&self) {
        self.stop();
    }

    /// Schedule the ticker to start ticking.
    ///
    /// This is an alias for `start()` to match Flutter's API.
    #[inline]
    pub fn schedule_tick(&self) {
        self.start();
    }
}

impl fmt::Debug for Ticker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("Ticker")
            .field("is_active", &inner.is_active)
            .field("muted", &inner.muted)
            .field(
                "elapsed",
                &inner
                    .start_time
                    .map(|start| Instant::now().duration_since(start)),
            )
            .finish()
    }
}

/// Provides Ticker instances.
///
/// Typically implemented by State objects that need animations.
/// The TickerProvider manages the lifecycle of tickers and ensures
/// they are properly disposed when no longer needed.
///
/// # Examples
///
/// ```
/// use flui_core::foundation::{Ticker, TickerProvider, TickerCallback};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// struct MyTickerProvider;
///
/// impl TickerProvider for MyTickerProvider {
///     fn create_ticker(&self, callback: TickerCallback) -> Ticker {
///         Ticker::new(callback)
///     }
/// }
///
/// let provider = MyTickerProvider;
/// let ticker = provider.create_ticker(Arc::new(|elapsed: Duration| {
///     println!("Tick: {:?}", elapsed);
/// }));
/// ```
pub trait TickerProvider: Send + Sync {
    /// Create a new ticker with the given callback.
    fn create_ticker(&self, callback: TickerCallback) -> Ticker;
}

/// A mixin for State objects that create a single ticker.
///
/// This ensures only one ticker is created per state object.
/// Use this when your widget only needs one animation.
pub trait SingleTickerProviderMixin: TickerProvider {
    /// Verify that only one ticker has been created.
    ///
    /// This should be called in debug mode to catch programming errors.
    fn verify_single_ticker(&self) {
        #[cfg(debug_assertions)]
        {
            // In a real implementation, this would track ticker creation count
            // For now, this is a marker trait
        }
    }
}

/// A mixin for State objects that create multiple tickers.
///
/// Use this when your widget needs multiple concurrent animations.
pub trait TickerProviderMixin: TickerProvider {
    // Marker trait for multiple ticker support
}

/// A simple ticker provider implementation.
///
/// This is a basic implementation that creates tickers without
/// any additional lifecycle management.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimpleTickerProvider;

impl TickerProvider for SimpleTickerProvider {
    fn create_ticker(&self, callback: TickerCallback) -> Ticker {
        Ticker::new(callback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn test_ticker_start_stop() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let ticker = Ticker::new(Arc::new(move |_elapsed: Duration| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(!ticker.is_active());

        ticker.start();
        assert!(ticker.is_active());

        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        ticker.stop();
        assert!(!ticker.is_active());

        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 2); // Should not increment
    }

    #[test]
    fn test_ticker_mute() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let ticker = Ticker::new(Arc::new(move |_elapsed: Duration| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        ticker.start();
        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        ticker.mute();
        assert!(ticker.is_muted());

        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 1); // Should not increment when muted

        ticker.unmute();
        assert!(!ticker.is_muted());

        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_ticker_elapsed_time() {
        let elapsed_times = Arc::new(Mutex::new(Vec::new()));
        let elapsed_clone = elapsed_times.clone();

        let ticker = Ticker::new(Arc::new(move |elapsed: Duration| {
            elapsed_clone.lock().push(elapsed);
        }));

        ticker.start();
        let start = Instant::now();

        ticker.tick(start);
        thread::sleep(Duration::from_millis(10));
        ticker.tick(Instant::now());

        let times = elapsed_times.lock();
        assert_eq!(times.len(), 2);
        assert!(times[0] < times[1]); // Second elapsed time should be greater
    }

    #[test]
    fn test_ticker_debug() {
        let ticker = Ticker::new(Arc::new(|_: Duration| {}));
        let debug = format!("{:?}", ticker);
        assert!(debug.contains("Ticker"));
    }

    #[test]
    fn test_simple_ticker_provider() {
        let provider = SimpleTickerProvider;
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let ticker = provider.create_ticker(Arc::new(move |_: Duration| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));

        ticker.start();
        ticker.tick(Instant::now());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_ticker_aliases() {
        let ticker = Ticker::new(Arc::new(|_: Duration| {}));

        ticker.schedule_tick();
        assert!(ticker.is_active());

        ticker.unschedule();
        assert!(!ticker.is_active());
    }

    #[test]
    fn test_ticker_dispose() {
        let ticker = Ticker::new(Arc::new(|_: Duration| {}));
        ticker.start();
        assert!(ticker.is_active());

        ticker.dispose();
        assert!(!ticker.is_active());
    }
}
