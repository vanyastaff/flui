//! Common types and utilities for devtools

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// DevTools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevToolsConfig {
    /// Enable performance profiling
    pub profiling_enabled: bool,

    /// Enable widget inspector
    pub inspector_enabled: bool,

    /// Target frame rate (FPS)
    pub target_fps: u32,

    /// Jank threshold (ms)
    pub jank_threshold_ms: f64,

    /// Maximum number of frames to keep in history
    pub max_frame_history: usize,
}

impl Default for DevToolsConfig {
    fn default() -> Self {
        Self {
            profiling_enabled: true,
            inspector_enabled: true,
            target_fps: 60,
            jank_threshold_ms: 16.0, // 60 FPS = 16.67ms per frame
            max_frame_history: 300,  // 5 seconds at 60 FPS
        }
    }
}

/// Frame number (monotonic counter)
pub type FrameNumber = u64;

/// Timestamp in nanoseconds
pub type Timestamp = u128;

/// Duration helper
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurationNanos(pub u128);

impl DurationNanos {
    /// Create from Duration
    pub fn from_duration(dur: Duration) -> Self {
        Self(dur.as_nanos())
    }

    /// Convert to milliseconds
    pub fn as_millis(&self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    /// Convert to microseconds
    pub fn as_micros(&self) -> f64 {
        self.0 as f64 / 1_000.0
    }

    /// Convert to Duration
    pub fn to_duration(&self) -> Duration {
        Duration::from_nanos(self.0 as u64)
    }
}

impl From<Duration> for DurationNanos {
    fn from(dur: Duration) -> Self {
        Self::from_duration(dur)
    }
}

impl std::ops::Sub for DurationNanos {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::Add for DurationNanos {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}
