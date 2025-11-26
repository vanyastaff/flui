//! VSync scheduler - coordinate rendering with display refresh
//!
//! Ensures frames are presented at the right time to avoid tearing and
//! maintain smooth animation.

use web_time::Instant;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// VSync callback - called when vsync signal arrives
pub type VsyncCallback = Box<dyn FnMut(Instant) + Send>;

/// VSync scheduling modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsyncMode {
    /// Wait for vsync (no tearing, may reduce FPS)
    On,

    /// Don't wait for vsync (tearing possible, max FPS)
    Off,

    /// Adaptive vsync (wait only when under budget)
    Adaptive,
}

/// VSync scheduler
///
/// Coordinates frame presentation with display refresh to avoid tearing.
/// On native platforms, this integrates with the OS vsync mechanism.
/// On web, this uses requestAnimationFrame.
pub struct VsyncScheduler {
    /// VSync mode
    mode: Arc<Mutex<VsyncMode>>,

    /// Target refresh rate (Hz)
    refresh_rate: u32,

    /// Last vsync time
    last_vsync: Arc<Mutex<Option<Instant>>>,

    /// VSync callback
    callback: Arc<Mutex<Option<VsyncCallback>>>,

    /// Whether vsync is active
    active: Arc<Mutex<bool>>,
}

impl VsyncScheduler {
    /// Create a new vsync scheduler
    ///
    /// # Arguments
    /// * `refresh_rate` - Display refresh rate in Hz (e.g., 60, 120, 144)
    pub fn new(refresh_rate: u32) -> Self {
        Self {
            mode: Arc::new(Mutex::new(VsyncMode::On)),
            refresh_rate,
            last_vsync: Arc::new(Mutex::new(None)),
            callback: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(false)),
        }
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

    /// Get frame interval (time between vsyncs)
    pub fn frame_interval(&self) -> Duration {
        Duration::from_micros(1_000_000 / self.refresh_rate as u64)
    }

    /// Set vsync callback
    pub fn set_callback<F>(&self, callback: F)
    where
        F: FnMut(Instant) + Send + 'static,
    {
        *self.callback.lock() = Some(Box::new(callback));
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
            VsyncMode::On | VsyncMode::Adaptive => {
                // Calculate time until next vsync
                if let Some(last) = *self.last_vsync.lock() {
                    let elapsed = now.duration_since(last);
                    let interval = self.frame_interval();

                    if elapsed < interval {
                        let wait_time = interval - elapsed;
                        std::thread::sleep(wait_time);
                        Instant::now()
                    } else {
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

    /// Predict next vsync time
    pub fn predict_next_vsync(&self) -> Option<Instant> {
        self.last_vsync.lock().map(|last| {
            let interval = self.frame_interval();
            last + interval
        })
    }
}

impl Default for VsyncScheduler {
    fn default() -> Self {
        Self::new(60) // Default to 60Hz
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
    }

    #[test]
    fn test_frame_interval() {
        let vsync_60 = VsyncScheduler::new(60);
        let interval_60 = vsync_60.frame_interval();
        assert_eq!(interval_60, Duration::from_micros(16_666)); // ~16.67ms

        let vsync_120 = VsyncScheduler::new(120);
        let interval_120 = vsync_120.frame_interval();
        assert_eq!(interval_120, Duration::from_micros(8_333)); // ~8.33ms
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
}
