//! Scheduler binding - wraps flui-scheduler for framework integration
//!
//! This is a thin wrapper around `flui_scheduler::Scheduler` that implements
//! the `BindingBase` trait for consistency with other bindings.

use super::BindingBase;
use flui_scheduler::Scheduler;
use std::sync::Arc;

/// Scheduler binding wrapper
///
/// # Architecture
///
/// ```text
/// SchedulerBinding (wrapper) → Arc<flui_scheduler::Scheduler>
/// ```
///
/// This provides a consistent API with other bindings while delegating all
/// scheduling logic to the production flui-scheduler crate.
///
/// # Thread-Safety
///
/// The underlying Scheduler is fully thread-safe and can be accessed from any thread.
/// Uses Arc for shared ownership with platform layer.
pub struct SchedulerBinding {
    scheduler: Arc<Scheduler>,
}

impl SchedulerBinding {
    /// Create a new SchedulerBinding with 60 FPS target
    pub fn new() -> Self {
        Self {
            scheduler: Arc::new(Scheduler::new()),
        }
    }

    /// Create a SchedulerBinding with custom target FPS
    pub fn with_target_fps(target_fps: u32) -> Self {
        Self {
            scheduler: Arc::new(Scheduler::with_target_fps(target_fps)),
        }
    }

    /// Get reference to the underlying Scheduler
    ///
    /// This provides full access to the production scheduler's features:
    /// - Frame scheduling and callbacks
    /// - Task queue with priority levels
    /// - Frame budget management
    /// - VSync coordination
    pub fn scheduler(&self) -> &Arc<Scheduler> {
        &self.scheduler
    }

    /// Get Arc clone for sharing with platform layer
    ///
    /// Returns a clone of the Arc for use with EmbedderCore.
    pub fn scheduler_arc(&self) -> Arc<Scheduler> {
        self.scheduler.clone()
    }
}

impl Default for SchedulerBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for SchedulerBinding {
    fn init(&mut self) {
        tracing::debug!(
            target_fps = self.scheduler.target_fps(),
            "SchedulerBinding initialized with flui-scheduler"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_binding_creation() {
        let binding = SchedulerBinding::new();
        // Scheduler may return slightly different FPS due to timing calculations
        assert!(binding.scheduler().target_fps() >= 59 && binding.scheduler().target_fps() <= 60);
    }

    #[test]
    fn test_custom_fps() {
        let binding = SchedulerBinding::with_target_fps(120);
        // Scheduler may return slightly different FPS due to timing calculations
        assert!(binding.scheduler().target_fps() >= 119 && binding.scheduler().target_fps() <= 120);
    }

    #[test]
    fn test_scheduler_access() {
        let binding = SchedulerBinding::new();
        let scheduler = binding.scheduler();

        // Should have production scheduler features
        assert!(!scheduler.is_frame_scheduled());
        // Scheduler may return slightly different FPS due to timing calculations
        assert!(scheduler.target_fps() >= 59 && scheduler.target_fps() <= 60);
    }

    #[test]
    fn test_scheduler_arc_sharing() {
        let binding = SchedulerBinding::new();
        let arc1 = binding.scheduler_arc();
        let arc2 = binding.scheduler_arc();

        // Should be the same instance
        assert!(Arc::ptr_eq(&arc1, &arc2));
    }
}
