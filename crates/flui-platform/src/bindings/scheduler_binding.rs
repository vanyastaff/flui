//! Scheduler binding
//!
//! Connects the platform layer to flui-scheduler for frame lifecycle
//! and task scheduling. Follows Flutter's SchedulerBinding pattern.

use flui_core::pipeline::PipelineOwner;
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

/// Binding between platform and scheduler
///
/// Provides frame lifecycle and task scheduling integration:
/// - Frame lifecycle (begin_frame, end_frame)
/// - Task scheduling with priorities
/// - Pipeline rebuild coordination
///
/// # Flutter Analogy
///
/// Similar to Flutter's `SchedulerBinding` which manages:
/// - `scheduleFrameCallback` → `schedule_animation`
/// - `addPostFrameCallback` → handled via `end_frame`
/// - Frame timing and statistics
///
/// # Example
///
/// ```rust,ignore
/// let binding = SchedulerBinding::new(scheduler);
///
/// // Wire up pipeline for automatic rebuilds
/// binding.wire_up_pipeline(pipeline_weak, needs_redraw);
///
/// // In render loop
/// binding.begin_frame();
/// // ... render ...
/// binding.end_frame();
/// ```
pub struct SchedulerBinding {
    scheduler: Arc<Scheduler>,
    current_frame: u64,
}

impl SchedulerBinding {
    /// Create a new scheduler binding
    pub fn new(scheduler: Arc<Scheduler>) -> Self {
        Self {
            scheduler,
            current_frame: 0,
        }
    }

    /// Wire up pipeline for automatic rebuilds
    ///
    /// Adds a persistent frame callback that flushes the rebuild queue
    /// and sets the needs_redraw flag when changes occur.
    ///
    /// # Why Weak Reference?
    ///
    /// Uses `Weak<RwLock<PipelineOwner>>` to prevent circular reference memory leaks.
    /// Without Weak, we'd have:
    ///
    /// ```text
    /// EmbedderCore → Scheduler → Callback(Arc<PipelineOwner>) → PipelineOwner
    ///        ↑                                                           │
    ///        └───────────────────────────────────────────────────────────┘
    ///                            (circular reference = memory leak)
    /// ```
    ///
    /// With Weak, the cycle is broken:
    ///
    /// ```text
    /// EmbedderCore → Scheduler → Callback(Weak<PipelineOwner>) ⇢ PipelineOwner
    ///        ↑                                                           │
    ///        └───────────────────────────────────────────────────────────┘
    ///                            (weak link breaks cycle = no leak)
    /// ```
    ///
    /// When `EmbedderCore` is dropped, `PipelineOwner` can be freed even though
    /// the `Scheduler` still holds callbacks. The `upgrade()` returns `None` for
    /// graceful cleanup.
    pub fn wire_up_pipeline(
        &self,
        pipeline_weak: Weak<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
    ) {
        tracing::debug!("Wiring up scheduler callbacks to pipeline");

        self.scheduler
            .add_persistent_frame_callback(Arc::new(move |_timing| {
                // Attempt to upgrade Weak to Arc - succeeds if PipelineOwner still exists
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    let mut owner = pipeline.write();
                    // Only mark for redraw if there were actual changes
                    if owner.flush_rebuild_queue() {
                        needs_redraw.store(true, Ordering::Relaxed);
                    }
                } else {
                    // PipelineOwner was dropped - this is expected during shutdown
                    tracing::warn!("Pipeline dropped during frame callback");
                }
            }));

        tracing::debug!("Scheduler wired up with Weak reference");
    }

    /// Begin a frame
    ///
    /// Called at the start of render_frame() to:
    /// - Execute transient callbacks (animations)
    /// - Start frame timing
    pub fn begin_frame(&mut self) -> u64 {
        let frame_id = self.scheduler.begin_frame();
        self.current_frame = frame_id.as_u64();
        tracing::trace!(frame = self.current_frame, "Frame started");
        self.current_frame
    }

    /// End a frame
    ///
    /// Called at the end of render_frame() to:
    /// - Execute post-frame callbacks
    /// - Update frame statistics
    pub fn end_frame(&self) {
        self.scheduler.end_frame();
        tracing::trace!(frame = self.current_frame, "Frame ended");
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
            is_over_budget: false, // Would need access to budget tracker
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
        let binding = SchedulerBinding::new(scheduler);

        let stats = binding.stats();
        // Allow 59 or 60 due to floating-point rounding in fps calculation
        assert!(stats.target_fps >= 59 && stats.target_fps <= 60);
        // Frame budget should be approximately 16.67ms (1000/60)
        assert!(stats.frame_budget_ms >= 16.0 && stats.frame_budget_ms <= 17.0);
    }
}
