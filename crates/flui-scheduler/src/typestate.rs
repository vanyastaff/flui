//! Typestate pattern implementations for compile-time state safety
//!
//! This module provides zero-cost abstractions using Rust's type system
//! to enforce state machine invariants at compile time.
//!
//! ## Typestate Pattern
//!
//! The typestate pattern uses phantom type parameters to encode state
//! in the type system. State transitions are method calls that consume
//! `self` and return a new type with different state.
//!
//! ```rust
//! use flui_scheduler::typestate::{TypestateTicker, Idle, Active};
//!
//! // Create an idle ticker
//! let ticker: TypestateTicker<Idle> = TypestateTicker::new();
//!
//! // Start it - consumes Idle, returns Active
//! let ticker: TypestateTicker<Active> = ticker.start(|elapsed| {
//!     println!("Elapsed: {:.3}s", elapsed);
//! });
//!
//! // Can only tick an active ticker
//! ticker.tick();
//!
//! // Stop it - returns to Idle
//! let ticker: TypestateTicker<Idle> = ticker.stop();
//! ```

use parking_lot::Mutex;
use std::marker::PhantomData;
use std::sync::Arc;
use web_time::Instant;

// =============================================================================
// State Markers (Zero-Sized Types)
// =============================================================================

/// Marker for idle state - ticker is not running
#[derive(Debug, Clone, Copy, Default)]
pub struct Idle;

/// Marker for active state - ticker is running
#[derive(Debug, Clone, Copy, Default)]
pub struct Active;

/// Marker for muted state - ticker is paused
#[derive(Debug, Clone, Copy, Default)]
pub struct Muted;

/// Marker for stopped state - ticker is permanently stopped
#[derive(Debug, Clone, Copy, Default)]
pub struct Stopped;

// =============================================================================
// Sealed Trait for State Markers
// =============================================================================

mod sealed {
    /// Sealed trait to prevent external implementations
    pub trait TickerStateSealed {}

    impl TickerStateSealed for super::Idle {}
    impl TickerStateSealed for super::Active {}
    impl TickerStateSealed for super::Muted {}
    impl TickerStateSealed for super::Stopped {}
}

/// Trait for ticker states - sealed to prevent external implementations
///
/// # Sealed Trait
///
/// This trait is **sealed** and cannot be implemented outside of this crate.
/// The available states are:
/// - [`Idle`] - Not started
/// - [`Active`] - Running and ticking
/// - [`Muted`] - Temporarily paused
/// - [`Stopped`] - Permanently stopped
pub trait TickerState: sealed::TickerStateSealed + Send + Sync + 'static {
    /// Human-readable state name
    const NAME: &'static str;

    /// Whether the ticker can be ticked in this state
    const CAN_TICK: bool;
}

impl TickerState for Idle {
    const NAME: &'static str = "Idle";
    const CAN_TICK: bool = false;
}

impl TickerState for Active {
    const NAME: &'static str = "Active";
    const CAN_TICK: bool = true;
}

impl TickerState for Muted {
    const NAME: &'static str = "Muted";
    const CAN_TICK: bool = false;
}

impl TickerState for Stopped {
    const NAME: &'static str = "Stopped";
    const CAN_TICK: bool = false;
}

// =============================================================================
// Typestate Ticker
// =============================================================================

/// Ticker callback type
pub type TickerCallback = Box<dyn FnMut(f64) + Send>;

/// Shared ticker data (state that persists across state transitions)
struct TickerData {
    start_time: Option<Instant>,
    callback: Option<TickerCallback>,
    muted_elapsed: f64,
}

/// A ticker with compile-time state tracking
///
/// The `State` type parameter encodes the current state of the ticker.
/// State transitions are method calls that consume `self` and return
/// a new ticker in the target state.
///
/// ## State Machine
///
/// ```text
/// ┌──────┐  start()   ┌────────┐
/// │ Idle │──────────▶ │ Active │
/// └──────┘            └────────┘
///    ▲                    │ │
///    │        stop()      │ │
///    └────────────────────┘ │
///                           │ mute()
///                           ▼
///                      ┌───────┐
///                      │ Muted │
///                      └───────┘
///                           │
///                    unmute()│
///                           ▼
///                      ┌────────┐
///                      │ Active │
///                      └────────┘
/// ```
pub struct TypestateTicker<State: TickerState> {
    data: Arc<Mutex<TickerData>>,
    _state: PhantomData<State>,
}

impl TypestateTicker<Idle> {
    /// Create a new idle ticker
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(TickerData {
                start_time: None,
                callback: None,
                muted_elapsed: 0.0,
            })),
            _state: PhantomData,
        }
    }

    /// Start the ticker with a callback
    ///
    /// Transitions from `Idle` to `Active` state.
    /// The callback receives elapsed time in seconds since start.
    pub fn start<F>(self, callback: F) -> TypestateTicker<Active>
    where
        F: FnMut(f64) + Send + 'static,
    {
        {
            let mut data = self.data.lock();
            data.start_time = Some(Instant::now());
            data.callback = Some(Box::new(callback));
            data.muted_elapsed = 0.0;
        }

        TypestateTicker {
            data: self.data,
            _state: PhantomData,
        }
    }
}

impl Default for TypestateTicker<Idle> {
    fn default() -> Self {
        Self::new()
    }
}

impl TypestateTicker<Active> {
    /// Tick the ticker, invoking the callback with elapsed time
    ///
    /// Only available in `Active` state.
    pub fn tick(&self) {
        let mut data = self.data.lock();

        if let Some(start) = data.start_time {
            let elapsed = start.elapsed().as_secs_f64();

            if let Some(callback) = data.callback.as_mut() {
                callback(elapsed);
            }
        }
    }

    /// Stop the ticker and return to idle state
    ///
    /// Transitions from `Active` to `Idle` state.
    pub fn stop(self) -> TypestateTicker<Idle> {
        {
            let mut data = self.data.lock();
            data.callback = None;
            data.start_time = None;
        }

        TypestateTicker {
            data: self.data,
            _state: PhantomData,
        }
    }

    /// Mute the ticker (pause without losing callback)
    ///
    /// Transitions from `Active` to `Muted` state.
    pub fn mute(self) -> TypestateTicker<Muted> {
        {
            let mut data = self.data.lock();
            if let Some(start) = data.start_time {
                data.muted_elapsed = start.elapsed().as_secs_f64();
            }
        }

        TypestateTicker {
            data: self.data,
            _state: PhantomData,
        }
    }

    /// Get elapsed time in seconds
    pub fn elapsed(&self) -> f64 {
        self.data
            .lock()
            .start_time
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }
}

impl TypestateTicker<Muted> {
    /// Unmute the ticker and resume
    ///
    /// Transitions from `Muted` to `Active` state.
    /// Time continues from where it was paused.
    pub fn unmute(self) -> TypestateTicker<Active> {
        {
            let mut data = self.data.lock();
            let muted_elapsed = data.muted_elapsed;
            let now = Instant::now();
            let adjusted_start = now - std::time::Duration::from_secs_f64(muted_elapsed);
            data.start_time = Some(adjusted_start);
        }

        TypestateTicker {
            data: self.data,
            _state: PhantomData,
        }
    }

    /// Stop the ticker from muted state
    ///
    /// Transitions from `Muted` to `Idle` state.
    pub fn stop(self) -> TypestateTicker<Idle> {
        {
            let mut data = self.data.lock();
            data.callback = None;
            data.start_time = None;
        }

        TypestateTicker {
            data: self.data,
            _state: PhantomData,
        }
    }

    /// Get elapsed time at the moment of muting
    pub fn elapsed(&self) -> f64 {
        self.data.lock().muted_elapsed
    }
}

// Common methods available in any state
impl<State: TickerState> TypestateTicker<State> {
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        State::NAME
    }

    /// Check if the ticker can be ticked in current state
    pub fn can_tick(&self) -> bool {
        State::CAN_TICK
    }
}

impl<State: TickerState> std::fmt::Debug for TypestateTicker<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypestateTicker")
            .field("state", &State::NAME)
            .finish()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_typestate_lifecycle() {
        let ticker: TypestateTicker<Idle> = TypestateTicker::new();
        assert_eq!(ticker.state_name(), "Idle");
        assert!(!ticker.can_tick());

        let ticker: TypestateTicker<Active> = ticker.start(|_| {});
        assert_eq!(ticker.state_name(), "Active");
        assert!(ticker.can_tick());

        let ticker: TypestateTicker<Muted> = ticker.mute();
        assert_eq!(ticker.state_name(), "Muted");
        assert!(!ticker.can_tick());

        let ticker: TypestateTicker<Active> = ticker.unmute();
        assert!(ticker.can_tick());

        let ticker: TypestateTicker<Idle> = ticker.stop();
        assert_eq!(ticker.state_name(), "Idle");
    }

    #[test]
    fn test_typestate_tick() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&counter);

        let ticker = TypestateTicker::new().start(move |_| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        ticker.tick();
        ticker.tick();
        ticker.tick();

        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_typestate_elapsed() {
        let ticker = TypestateTicker::new().start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed = ticker.elapsed();
        assert!(elapsed > 0.0);
        assert!(elapsed < 1.0);
    }

    #[test]
    fn test_typestate_mute_preserves_elapsed() {
        let ticker = TypestateTicker::new().start(|_| {});

        std::thread::sleep(std::time::Duration::from_millis(10));

        let elapsed_before = ticker.elapsed();
        let ticker = ticker.mute();

        // Muted ticker should preserve elapsed time
        let elapsed_muted = ticker.elapsed();
        assert!((elapsed_before - elapsed_muted).abs() < 0.001);

        std::thread::sleep(std::time::Duration::from_millis(10));

        // Should not advance while muted
        assert!((ticker.elapsed() - elapsed_muted).abs() < 0.001);
    }
}
