//! Frame management - lifecycle, timing, and callbacks
//!
//! A frame represents one render cycle from VSync to present:
//! ```text
//! VSync → BeginFrame → Build → Layout → Paint → EndFrame → Present
//! ```

use instant::Instant;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Unique frame identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameId(u64);

impl FrameId {
    /// Create a new unique frame ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw frame number
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

impl Default for FrameId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Frame#{}", self.0)
    }
}

/// Frame phase - which part of the render pipeline is executing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePhase {
    /// Waiting for frame to start
    Idle,
    /// Building widget tree (View → Element)
    Build,
    /// Computing layout (constraints → sizes)
    Layout,
    /// Painting to layers (Element → DisplayList)
    Paint,
    /// Compositing layers to screen
    Composite,
}

impl fmt::Display for FramePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Build => write!(f, "Build"),
            Self::Layout => write!(f, "Layout"),
            Self::Paint => write!(f, "Paint"),
            Self::Composite => write!(f, "Composite"),
        }
    }
}

/// Frame timing information
#[derive(Debug, Clone, Copy)]
pub struct FrameTiming {
    /// Frame identifier
    pub id: FrameId,
    
    /// When the frame started (vsync time)
    pub start_time: Instant,
    
    /// Target frame duration (e.g., 16.67ms for 60fps)
    pub target_duration_ms: f64,
    
    /// Current phase
    pub phase: FramePhase,
}

impl FrameTiming {
    /// Create a new frame timing
    pub fn new(target_fps: u32) -> Self {
        Self {
            id: FrameId::new(),
            start_time: Instant::now(),
            target_duration_ms: 1000.0 / target_fps as f64,
            phase: FramePhase::Idle,
        }
    }

    /// Get elapsed time since frame start in milliseconds
    pub fn elapsed_ms(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64() * 1000.0
    }

    /// Get elapsed time since frame start in seconds
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Check if frame is over budget
    pub fn is_over_budget(&self) -> bool {
        self.elapsed_ms() > self.target_duration_ms
    }

    /// Get remaining budget in milliseconds
    pub fn remaining_budget_ms(&self) -> f64 {
        (self.target_duration_ms - self.elapsed_ms()).max(0.0)
    }

    /// Calculate how much over/under budget (negative = over budget)
    pub fn budget_delta_ms(&self) -> f64 {
        self.target_duration_ms - self.elapsed_ms()
    }
}

/// Frame callback - executed at frame boundaries
pub type FrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>;

/// Persistent frame callback (can be called multiple times)
///
/// Uses Arc for cheap cloning - persistent callbacks are cloned before execution
/// to avoid holding locks during callback invocation.
pub type PersistentFrameCallback = Arc<dyn Fn(&FrameTiming) + Send + Sync>;

/// Post-frame callback - executed after frame completes
pub type PostFrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_id_unique() {
        let id1 = FrameId::new();
        let id2 = FrameId::new();
        assert_ne!(id1, id2);
        assert!(id2.as_u64() > id1.as_u64());
    }

    #[test]
    fn test_frame_timing_budget() {
        let timing = FrameTiming::new(60); // 16.67ms target
        assert!(!timing.is_over_budget());
        assert!(timing.remaining_budget_ms() > 0.0);
        assert_eq!(timing.target_duration_ms, 1000.0 / 60.0);
    }

    #[test]
    fn test_frame_phase_display() {
        assert_eq!(format!("{}", FramePhase::Build), "Build");
        assert_eq!(format!("{}", FramePhase::Layout), "Layout");
        assert_eq!(format!("{}", FramePhase::Paint), "Paint");
    }
}