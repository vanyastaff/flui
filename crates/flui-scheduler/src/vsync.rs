//! VSync scheduler - coordinate rendering with display refresh
//!
//! Ensures frames are presented at the right time to avoid tearing and
//! maintain smooth animation.
//!
//! **Note**: `wait_for_vsync()` currently uses `thread::sleep` for timing
//! simulation. Real platform VSync integration comes from `flui-platform` by
//! routing platform vsync callbacks into `Scheduler::handle_begin_frame` +
//! `handle_draw_frame` directly.
//!
//! ## Type-Safe Timing
//!
//! ```rust
//! use flui_scheduler::{
//!     duration::Microseconds,
//!     vsync::{VsyncMode, VsyncScheduler},
//! };
//!
//! let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
//! let interval = vsync.frame_interval();
//!
//! assert_eq!(interval.value(), 16666); // ~16.67ms in microseconds
//! ```

use std::{collections::VecDeque, sync::Arc, time::Duration};

use parking_lot::Mutex;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use web_time::Instant;

use crate::duration::{Microseconds, Milliseconds};

/// Configuration error for [`VsyncScheduler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidVsyncConfig {
    /// `refresh_rate == 0` rejected — no meaningful frame interval.
    #[error("refresh_rate must be greater than 0")]
    ZeroRefreshRate,
}

/// VSync callback - called when vsync signal arrives
pub type VsyncCallback = Box<dyn FnMut(Instant) + Send>;

/// VSync scheduling modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum VsyncMode {
    /// Wait for vsync (no tearing, may reduce FPS)
    #[default]
    On = 0,

    /// Don't wait for vsync (tearing possible, max FPS)
    Off = 1,

    /// Adaptive vsync (wait only when under budget)
    Adaptive = 2,

    /// Triple buffering (reduced latency, no tearing)
    TripleBuffer = 3,
}

impl VsyncMode {
    /// Check if this mode waits for vsync
    #[inline]
    pub const fn waits_for_vsync(self) -> bool {
        matches!(self, Self::On | Self::Adaptive | Self::TripleBuffer)
    }

    /// Check if this mode can tear
    #[inline]
    pub const fn can_tear(self) -> bool {
        matches!(self, Self::Off)
    }

    /// Get a description of this mode
    pub const fn description(self) -> &'static str {
        match self {
            Self::On => "Wait for vsync (no tearing)",
            Self::Off => "No vsync (max FPS, may tear)",
            Self::Adaptive => "Wait only when under budget",
            Self::TripleBuffer => "Triple buffering (low latency)",
        }
    }
}

/// VSync statistics
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VsyncStats {
    /// Total vsync signals received
    pub signal_count: u64,
    /// Missed vsync signals
    pub missed_count: u64,
    /// Average time between vsyncs
    pub avg_interval: Microseconds,
    /// Last measured interval
    pub last_interval: Microseconds,
}

impl VsyncStats {
    /// Calculate miss rate as percentage
    pub fn miss_rate(&self) -> f64 {
        if self.signal_count == 0 {
            0.0
        } else {
            (self.missed_count as f64 / self.signal_count as f64) * 100.0
        }
    }

    /// Calculate effective FPS
    pub fn effective_fps(&self) -> f64 {
        if self.avg_interval.value() == 0 {
            0.0
        } else {
            1_000_000.0 / self.avg_interval.value() as f64
        }
    }
}

/// Mutable state consolidated behind a single lock (reduces 6 Arc allocations
/// to 1)
struct VsyncInner {
    mode: VsyncMode,
    last_vsync: Option<Instant>,
    callback: Option<VsyncCallback>,
    active: bool,
    stats: VsyncStats,
    interval_history: VecDeque<Microseconds>,
}

/// VSync scheduler
///
/// Coordinates frame presentation with display refresh to avoid tearing.
/// On native platforms, this integrates with the OS vsync mechanism.
/// On web, this uses requestAnimationFrame.
///
/// # Examples
///
/// ```
/// use flui_scheduler::vsync::{VsyncMode, VsyncScheduler};
///
/// let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
/// assert_eq!(vsync.refresh_rate(), 60);
/// assert_eq!(vsync.mode(), VsyncMode::On);
///
/// // Change mode
/// vsync.set_mode(VsyncMode::Adaptive);
///
/// // Get frame interval
/// let interval = vsync.frame_interval_ms();
/// assert!((interval.value() - 16.666).abs() < 0.1);
/// ```
pub struct VsyncScheduler {
    /// Target refresh rate (Hz) — immutable after construction
    refresh_rate: u32,

    /// Frame interval in microseconds — immutable after construction
    frame_interval_us: Microseconds,

    /// All mutable state behind a single lock
    inner: Arc<Mutex<VsyncInner>>,
}

impl VsyncScheduler {
    /// Create a new vsync scheduler
    ///
    /// # Arguments
    /// * `refresh_rate` - Display refresh rate in Hz (e.g., 60, 120, 144)
    ///
    /// # Errors
    ///
    /// Returns [`InvalidVsyncConfig::ZeroRefreshRate`] if `refresh_rate == 0`
    /// (no meaningful frame interval can be computed).
    pub fn try_new(refresh_rate: u32) -> Result<Self, InvalidVsyncConfig> {
        if refresh_rate == 0 {
            return Err(InvalidVsyncConfig::ZeroRefreshRate);
        }
        let frame_interval_us = Microseconds::new(1_000_000 / refresh_rate as u64);

        Ok(Self {
            refresh_rate,
            frame_interval_us,
            inner: Arc::new(Mutex::new(VsyncInner {
                mode: VsyncMode::On,
                last_vsync: None,
                callback: None,
                active: false,
                stats: VsyncStats::default(),
                interval_history: VecDeque::with_capacity(60),
            })),
        })
    }

    /// Create with a specific VSync mode.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidVsyncConfig::ZeroRefreshRate`] if `refresh_rate == 0`.
    pub fn try_with_mode(refresh_rate: u32, mode: VsyncMode) -> Result<Self, InvalidVsyncConfig> {
        let scheduler = Self::try_new(refresh_rate)?;
        scheduler.inner.lock().mode = mode;
        Ok(scheduler)
    }

    /// Set vsync mode
    pub fn set_mode(&self, mode: VsyncMode) {
        self.inner.lock().mode = mode;
    }

    /// Get current vsync mode
    pub fn mode(&self) -> VsyncMode {
        self.inner.lock().mode
    }

    /// Get refresh rate
    pub fn refresh_rate(&self) -> u32 {
        self.refresh_rate
    }

    /// Get frame interval as type-safe Microseconds
    pub fn frame_interval(&self) -> Microseconds {
        self.frame_interval_us
    }

    /// Get frame interval as Duration
    pub fn frame_interval_duration(&self) -> Duration {
        self.frame_interval_us.to_std_duration()
    }

    /// Get frame interval as Milliseconds
    pub fn frame_interval_ms(&self) -> Milliseconds {
        self.frame_interval_us.to_ms()
    }

    /// Set vsync callback
    pub fn set_callback<F>(&self, callback: F)
    where
        F: FnMut(Instant) + Send + 'static,
    {
        self.inner.lock().callback = Some(Box::new(callback));
    }

    /// Clear vsync callback
    pub fn clear_callback(&self) {
        self.inner.lock().callback = None;
    }

    /// Start vsync loop
    pub fn start(&self) {
        self.inner.lock().active = true;
    }

    /// Stop vsync loop
    pub fn stop(&self) {
        self.inner.lock().active = false;
    }

    /// Check if vsync is active
    pub fn is_active(&self) -> bool {
        self.inner.lock().active
    }

    /// Wait for next vsync
    ///
    /// This is a blocking call that waits until the next vsync signal.
    /// In a real implementation, this would use platform-specific APIs:
    /// - Windows: DwmFlush or IDXGISwapChain::Present with vsync
    /// - macOS: CADisplayLink
    /// - Linux: glXSwapBuffers with vsync enabled
    /// - Web: requestAnimationFrame
    pub fn wait_for_vsync(&self) -> Instant {
        let now = Instant::now();

        // Read mode and last_vsync under a single lock
        let (mode, last) = {
            let inner = self.inner.lock();
            (inner.mode, inner.last_vsync)
        };

        match mode {
            VsyncMode::Off => now,
            VsyncMode::On | VsyncMode::Adaptive | VsyncMode::TripleBuffer => {
                if let Some(last) = last {
                    let elapsed = now.duration_since(last);
                    let interval = self.frame_interval_duration();

                    if elapsed < interval {
                        let wait_time = interval - elapsed;
                        std::thread::sleep(wait_time);
                        Instant::now()
                    } else {
                        // Missed vsync
                        self.inner.lock().stats.missed_count += 1;
                        now
                    }
                } else {
                    now
                }
            }
        }
    }

    /// Signal vsync (call this when vsync occurs)
    ///
    /// This updates internal state and calls the vsync callback.
    pub fn signal_vsync(&self) {
        let now = Instant::now();

        let mut inner = self.inner.lock();

        // Calculate interval from last vsync
        if let Some(last) = inner.last_vsync {
            let interval = Microseconds::new(now.duration_since(last).as_micros() as u64);

            // Update history
            inner.interval_history.push_back(interval);
            if inner.interval_history.len() > 60 {
                inner.interval_history.pop_front();
            }

            // Calculate average
            let sum: u64 = inner.interval_history.iter().map(|i| i.value()).sum();
            let avg = Microseconds::new(sum / inner.interval_history.len() as u64);

            // Update stats
            inner.stats.last_interval = interval;
            inner.stats.avg_interval = avg;
            inner.stats.signal_count += 1;
        }

        inner.last_vsync = Some(now);

        if let Some(callback) = inner.callback.as_mut() {
            callback(now);
        }
    }

    /// Get time since last vsync
    pub fn time_since_vsync(&self) -> Option<Duration> {
        self.inner
            .lock()
            .last_vsync
            .map(|last| Instant::now().duration_since(last))
    }

    /// Get time since last vsync as Milliseconds
    pub fn time_since_vsync_ms(&self) -> Option<Milliseconds> {
        self.time_since_vsync()
            .map(|d| Milliseconds::new(d.as_secs_f64() * 1000.0))
    }

    /// Predict next vsync time
    pub fn predict_next_vsync(&self) -> Option<Instant> {
        self.inner
            .lock()
            .last_vsync
            .map(|last| last + self.frame_interval_duration())
    }

    /// Get vsync statistics
    pub fn stats(&self) -> VsyncStats {
        self.inner.lock().stats
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        let mut inner = self.inner.lock();
        inner.stats = VsyncStats::default();
        inner.interval_history.clear();
    }

    /// Check if running at target refresh rate
    pub fn is_at_target_rate(&self) -> bool {
        let inner = self.inner.lock();
        if inner.stats.avg_interval.value() == 0 {
            return true;
        }

        let target = self.frame_interval_us.value();
        let actual = inner.stats.avg_interval.value();

        // Within 5% tolerance — absolute difference for u64 monotonic durations.
        let tolerance = target / 20;
        let diff = if actual > target {
            actual - target
        } else {
            target - actual
        };
        diff <= tolerance
    }
}

impl Default for VsyncScheduler {
    fn default() -> Self {
        // 60 Hz is statically valid; try_new only errors on refresh_rate == 0.
        Self::try_new(60).expect("60 Hz is a valid refresh rate")
    }
}

impl std::fmt::Debug for VsyncScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("VsyncScheduler")
            .field("refresh_rate", &self.refresh_rate)
            .field("mode", &inner.mode)
            .field("active", &inner.active)
            .field("stats", &inner.stats)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn test_vsync_modes() {
        let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
        assert_eq!(vsync.mode(), VsyncMode::On);

        vsync.set_mode(VsyncMode::Off);
        assert_eq!(vsync.mode(), VsyncMode::Off);

        vsync.set_mode(VsyncMode::Adaptive);
        assert_eq!(vsync.mode(), VsyncMode::Adaptive);
    }

    #[test]
    fn test_frame_interval() {
        let vsync_60 = VsyncScheduler::try_new(60).expect("refresh > 0");
        let interval_60 = vsync_60.frame_interval();
        assert_eq!(interval_60, Microseconds::new(16_666)); // ~16.67ms

        let vsync_120 = VsyncScheduler::try_new(120).expect("refresh > 0");
        let interval_120 = vsync_120.frame_interval();
        assert_eq!(interval_120, Microseconds::new(8_333)); // ~8.33ms
    }

    #[test]
    fn test_vsync_callback() {
        let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
        let counter = Arc::new(AtomicU32::new(0));

        let c = Arc::clone(&counter);
        vsync.set_callback(move |_instant| {
            c.fetch_add(1, Ordering::Relaxed);
        });

        vsync.signal_vsync();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        vsync.signal_vsync();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_time_tracking() {
        let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
        assert!(vsync.time_since_vsync().is_none());

        vsync.signal_vsync();
        assert!(vsync.time_since_vsync().is_some());

        let next = vsync.predict_next_vsync();
        assert!(next.is_some());
    }

    #[test]
    fn test_vsync_stats() {
        let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

        vsync.signal_vsync();
        std::thread::sleep(Duration::from_millis(16));
        vsync.signal_vsync();
        std::thread::sleep(Duration::from_millis(16));
        vsync.signal_vsync();

        let stats = vsync.stats();
        // signal_count is incremented only when there's a previous vsync to measure
        // interval
        assert!(stats.signal_count >= 2);
        assert!(stats.avg_interval.value() > 0);
    }

    #[test]
    fn test_vsync_mode_properties() {
        assert!(VsyncMode::On.waits_for_vsync());
        assert!(!VsyncMode::Off.waits_for_vsync());
        assert!(VsyncMode::Adaptive.waits_for_vsync());

        assert!(VsyncMode::Off.can_tear());
        assert!(!VsyncMode::On.can_tear());
    }

    #[test]
    fn test_frame_interval_ms() {
        let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
        let ms = vsync.frame_interval_ms();
        assert!((ms.value() - 16.666).abs() < 0.1);
    }
}
