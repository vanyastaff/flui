//! Renderer binding traits - interfaces for the rendering system.
//!
//! This module provides traits for integrating the rendering system
//! with the application layer. The concrete implementations live in `flui_app`.
//!
//! # Flutter Equivalence
//!
//! This corresponds to the abstract parts of Flutter's `rendering/binding.dart`.

use crate::hit_testing::{HitTestResult, PointerEvent};
use std::sync::Arc;

/// Interface for managing the pipeline owner tree and semantics.
///
/// This trait is implemented by the binding to provide services to pipeline
/// owners in the tree.
pub trait PipelineManifold: Send + Sync {
    /// Request that the visual display be updated.
    ///
    /// This is called by pipeline owners when they have work to do.
    fn request_visual_update(&self);

    /// Whether semantics are currently enabled.
    fn semantics_enabled(&self) -> bool;

    /// Add a listener for semantics enabled changes.
    fn add_semantics_enabled_listener(&self, listener: Arc<dyn Fn(bool) + Send + Sync>);

    /// Remove a previously added semantics enabled listener.
    fn remove_semantics_enabled_listener(&self, listener: &Arc<dyn Fn(bool) + Send + Sync>);
}

/// Interface for dispatching hit test results to targets.
pub trait HitTestDispatcher: Send + Sync {
    /// Dispatch a pointer event to all targets in the hit test result.
    fn dispatch_event(&self, event: &PointerEvent, result: &HitTestResult);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the traits are object-safe
    fn _assert_object_safe(_: &dyn PipelineManifold) {}
    fn _assert_hit_test_dispatcher_object_safe(_: &dyn HitTestDispatcher) {}

    #[test]
    fn test_traits_are_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn PipelineManifold>();
        assert_send_sync::<dyn HitTestDispatcher>();
    }
}
