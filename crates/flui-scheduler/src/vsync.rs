//! VSync scheduler - coordinate rendering with display refresh
//!
//! Ensures frames are presented at the right time to avoid tearing and
//! maintain smooth animation.
//!
//! ## Type-Safe Timing
//!
//! ```rust
//! use flui_scheduler::vsync::{VsyncScheduler, VsyncMode};
//! use flui_scheduler::duration::Microseconds;
//!
//! let vsync = VsyncScheduler::new(60);
//! let interval = vsync.frame_interval();
//!
//! assert_eq!(interval.value(), 16666); // ~16.67ms in microseconds
//! ```

use crate::duration::{Microseconds, Milliseconds};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

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

/// VSync scheduler
///
/// Coordinates frame presentation with display refresh to avoid tearing.
/// On native platforms, this integrates with the OS vsync mechanism.
/// On web, this uses requestAnimationFrame.
///
/// # Examples
///
/// ```
/// use flui_scheduler::vsync::{VsyncScheduler, VsyncMode};
///
/// let vsync = VsyncScheduler::new(60);
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
    /// VSync mode
    mode: Arc<Mutex<VsyncMode>>,

    /// Target refresh rate (Hz)
    refresh_rate: u32,

    /// Frame interval in microseconds
    frame_interval_us: Microseconds,

    /// Last vsync time
    last_vsync: Arc<Mutex<Option<Instant>>>,

    /// VSync callback
    callback: Arc<Mutex<Option<VsyncCallback>>>,

    /// Whether vsync is active
    active: Arc<Mutex<bool>>,

    /// Statistics
    stats: Arc<Mutex<VsyncStats>>,

    /// Interval history for averaging (last 60 frames)
    interval_history: Arc<Mutex<Vec<Microseconds>>>,
}

impl VsyncScheduler {
    /// Create a new vsync scheduler
    ///
    /// # Arguments
    /// * `refresh_rate` - Display refresh rate in Hz (e.g., 60, 120, 144)
    ///
    /// # Panics
    ///
    /// Panics if `refresh_rate` is 0.
    pub fn new(refresh_rate: u32) -> Self {
        assert!(refresh_rate > 0, "Refresh rate must be greater than 0");
        let frame_interval_us = Microseconds::new(1_000_000 / refresh_rate as i64);

        Self {
            mode: Arc::new(Mutex::new(VsyncMode::On)),
            refresh_rate,
            frame_interval_us,
            last_vsync: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(false)),
            stats: Arc::new(Mutex::new(VsyncStats::default())),
            interval_history: Arc::new(Mutex::new(Vec::with_capacity(60))),
        }
    }

    /// Create with a specific VSync mode
    pub fn with_mode(refresh_rate: u32, mode: VsyncMode) -> Self {
        let scheduler = Self::new(refresh_rate);
        *scheduler.mode.lock() = mode;
        scheduler
    }

    /// Set vsync mode
    pub fn set_mode(&self, mode: VsyncMode) {
        *self.mode.lock() = mode;
    }

    /// Get current vsync mode
    pub fn mode(&self) -> VsyncMode {
        *self.mode.lock()
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
        *self.callback.lock() = Some(Box::new(callback));
    }

    /// Clear vsync callback
    pub fn clear_callback(&self) {
        *self.callback.lock() = None;
    }

    /// Start vsync loop
    pub fn start(&self) {
        *self.active.lock() = true;
    }

    /// Stop vsync loop
    pub fn stop(&self) {
        *self.active.lock() = false;
    }

    /// Check if vsync is active
    pub fn is_active(&self) -> bool {
        *self.active.lock()
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

        match *self.mode.lock() {
            VsyncMode::Off => {
                // No waiting
                now
            }
            VsyncMode::On | VsyncMode::Adaptive | VsyncMode::TripleBuffer => {
                // Calculate time until next vsync
                if let Some(last) = *self.last_vsync.lock() {
                    let elapsed = now.duration_since(last);
                    let interval = self.frame_interval_duration();

                    if elapsed < interval {
                        let wait_time = interval - elapsed;
                        std::thread::sleep(wait_time);
                        Instant::now()
                    } else {
                        // Missed vsync
                        self.stats.lock().missed_count += 1;
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

        // Calculate interval from last vsync
        if let Some(last) = *self.last_vsync.lock() {
            let interval = Microseconds::new(now.duration_since(last).as_micros() as i64);

            // Update history
            let mut history = self.interval_history.lock();
            history.push(interval);
            if history.len() > 60 {
                history.remove(0);
            }

            // Calculate average
            let sum: i64 = history.iter().map(|i| i.value()).sum();
            let avg = Microseconds::new(sum / history.len() as i64);

            // Update stats
            let mut stats = self.stats.lock();
            stats.last_interval = interval;
            stats.avg_interval = avg;
            stats.signal_count += 1;
        }

        *self.last_vsync.lock() = Some(now);

        if let Some(callback) = self.callback.lock().as_mut() {
            callback(now);
        }
    }

    /// Get time since last vsync
    pub fn time_since_vsync(&self) -> Option<Duration> {
        self.last_vsync
            .lock()
            .map(|last| Instant::now().duration_since(last))
    }

    /// Get time since last vsync as Milliseconds
    pub fn time_since_vsync_ms(&self) -> Option<Milliseconds> {
        self.time_since_vsync()
            .map(|d| Milliseconds::new(d.as_secs_f64() * 1000.0))
    }

    /// Predict next vsync time
    pub fn predict_next_vsync(&self) -> Option<Instant> {
        self.last_vsync
            .lock()
            .map(|last| last + self.frame_interval_duration())
    }

    /// Get vsync statistics
    pub fn stats(&self) -> VsyncStats {
        *self.stats.lock()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        *self.stats.lock() = VsyncStats::default();
        self.interval_history.lock().clear();
    }

    /// Check if running at target refresh rate
    pub fn is_at_target_rate(&self) -> bool {
        let stats = self.stats.lock();
        if stats.avg_interval.value() == 0 {
            return true;
        }

        let target = self.frame_interval_us.value();
        let actual = stats.avg_interval.value();

        // Within 5% tolerance
        let tolerance = target / 20;
        (actual - target).abs() <= tolerance
    }
}

impl Default for VsyncScheduler {
    fn default() -> Self {
        Self::new(60) // Default to 60Hz
    }
}

impl std::fmt::Debug for VsyncScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsyncScheduler")
            .field("refresh_rate", &self.refresh_rate)
            .field("mode", &self.mode())
            .field("active", &self.is_active())
            .field("stats", &self.stats())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn test_vsync_modes() {
        let vsync = VsyncScheduler::new(60);
        assert_eq!(vsync.mode(), VsyncMode::On);

        vsync.set_mode(VsyncMode::Off);
        assert_eq!(vsync.mode(), VsyncMode::Off);

        vsync.set_mode(VsyncMode::Adaptive);
        assert_eq!(vsync.mode(), VsyncMode::Adaptive);
    }

    #[test]
    fn test_frame_interval() {
        let vsync_60 = VsyncScheduler::new(60);
        let interval_60 = vsync_60.frame_interval();
        assert_eq!(interval_60, Microseconds::new(16_666)); // ~16.67ms

        let vsync_120 = VsyncScheduler::new(120);
        let interval_120 = vsync_120.frame_interval();
        assert_eq!(interval_120, Microseconds::new(8_333)); // ~8.33ms
    }

    #[test]
    fn test_vsync_callback() {
        let vsync = VsyncScheduler::new(60);
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
        let vsync = VsyncScheduler::new(60);
        assert!(vsync.time_since_vsync().is_none());

        vsync.signal_vsync();
        assert!(vsync.time_since_vsync().is_some());

        let next = vsync.predict_next_vsync();
        assert!(next.is_some());
    }

    #[test]
    fn test_vsync_stats() {
        let vsync = VsyncScheduler::new(60);

        vsync.signal_vsync();
        std::thread::sleep(Duration::from_millis(16));
        vsync.signal_vsync();
        std::thread::sleep(Duration::from_millis(16));
        vsync.signal_vsync();

        let stats = vsync.stats();
        // signal_count is incremented only when there's a previous vsync to measure interval
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
        let vsync = VsyncScheduler::new(60);
        let ms = vsync.frame_interval_ms();
        assert!((ms.value() - 16.666).abs() < 0.1);
    }
}
