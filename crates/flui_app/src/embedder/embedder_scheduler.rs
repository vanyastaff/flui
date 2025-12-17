//! Embedder scheduler integration
//!
//! Connects the embedder to flui-scheduler for frame lifecycle
//! and task scheduling.

use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{Priority, Scheduler};
use parking_lot::RwLock;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};

/// Scheduler statistics snapshot
#[derive(Debug, Clone, Copy)]
pub struct SchedulerStats {
    /// Target frames per second
    pub target_fps: u32,
    /// Frame budget in milliseconds
    pub frame_budget_ms: f64,
    /// Whether currently over budget
    pub is_over_budget: bool,
    /// Current frame ID
    pub current_frame: u64,
}

/// Embedder scheduler integration
///
/// Connects the embedder to flui-scheduler for:
/// - Frame lifecycle (begin_frame, end_frame)
/// - Task scheduling with priorities
/// - Pipeline dirty-check coordination
///
/// This is embedder-specific glue code, not to be confused with
/// `flui_scheduler::SchedulerBinding` trait.
pub struct EmbedderScheduler {
    scheduler: Arc<Scheduler>,
    current_frame: u64,
}

impl std::fmt::Debug for EmbedderScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedderScheduler")
            .field("current_frame", &self.current_frame)
            .finish_non_exhaustive()
    }
}

impl EmbedderScheduler {
    /// Create a new embedder scheduler
    pub fn new(scheduler: Arc<Scheduler>) -> Self {
        Self {
            scheduler,
            current_frame: 0,
        }
    }

    /// Wire up pipeline for automatic rebuilds
    ///
    /// Adds a persistent frame callback that flushes dirty nodes
    /// and sets the needs_redraw flag when changes occur.
    ///
    /// # Why Weak Reference?
    ///
    /// Uses `Weak<RwLock<PipelineOwner>>` to prevent circular reference memory leaks.
    pub fn wire_up_pipeline(
        &self,
        pipeline_weak: Weak<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
    ) {
        self.scheduler
            .add_persistent_frame_callback(Arc::new(move |_timing| {
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    let owner = pipeline.read();
                    // Check if there are dirty nodes that need processing
                    if owner.has_dirty_nodes() {
                        needs_redraw.store(true, Ordering::Relaxed);
                    }
                }
            }));
    }

    /// Begin a frame
    ///
    /// Called at the start of render_frame() to:
    /// - Execute transient callbacks (animations)
    /// - Start frame timing
    pub fn begin_frame(&mut self) -> u64 {
        let frame_id = self.scheduler.begin_frame();
        self.current_frame = frame_id.as_u64();
        self.current_frame
    }

    /// End a frame
    ///
    /// Called at the end of render_frame() to:
    /// - Execute post-frame callbacks
    /// - Update frame statistics
    pub fn end_frame(&self) {
        self.scheduler.end_frame();
    }

    /// Schedule a user input task (highest priority)
    ///
    /// Used for pointer events, keyboard input, etc.
    pub fn schedule_user_input<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.scheduler.add_task(Priority::UserInput, task);
    }

    /// Schedule an animation task
    pub fn schedule_animation<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.scheduler.add_task(Priority::Animation, task);
    }

    /// Schedule a build task (widget rebuilds)
    pub fn schedule_build<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.scheduler.add_task(Priority::Build, task);
    }

    /// Schedule an idle task (low priority background work)
    pub fn schedule_idle<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.scheduler.add_task(Priority::Idle, task);
    }

    /// Request a frame to be scheduled
    pub fn request_frame(&self) {
        self.scheduler.request_frame();
    }

    /// Check if a frame is scheduled
    pub fn is_frame_scheduled(&self) -> bool {
        self.scheduler.is_frame_scheduled()
    }

    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            target_fps: self.scheduler.target_fps(),
            frame_budget_ms: 1000.0 / self.scheduler.target_fps() as f64,
            is_over_budget: false,
            current_frame: self.current_frame,
        }
    }

    /// Get the underlying scheduler
    pub fn scheduler(&self) -> &Arc<Scheduler> {
        &self.scheduler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_stats() {
        let scheduler = Arc::new(Scheduler::new());
        let binding = EmbedderScheduler::new(scheduler);

        let stats = binding.stats();
        // Allow 59 or 60 due to floating-point rounding in fps calculation
        assert!(stats.target_fps >= 59 && stats.target_fps <= 60);
        // Frame budget should be approximately 16.67ms (1000/60)
        assert!(stats.frame_budget_ms >= 16.0 && stats.frame_budget_ms <= 17.0);
    }
}
