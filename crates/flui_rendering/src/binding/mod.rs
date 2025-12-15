//! Renderer binding - glue between the render tree and the platform.
//!
//! This module provides traits and types for integrating the rendering system
//! with the underlying platform (windowing, input, etc.).
//!
//! # Architecture
//!
//! The binding system follows Flutter's mixin-based architecture, but adapted
//! for Rust's trait system:
//!
//! - [`RendererBinding`]: Main trait for connecting render trees to the platform
//! - [`PipelineManifold`]: Interface for pipeline owner tree management
//! - [`HitTestDispatcher`]: Interface for dispatching hit test results
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `rendering/binding.dart`.

use crate::{
    hit_testing::{HitTestResult, PointerEvent},
    input::MouseTracker,
    pipeline::PipelineOwner,
    view::RenderView,
};
use flui_types::Offset;
use std::sync::Arc;

/// The glue between the render trees and the platform.
///
/// The `RendererBinding` manages multiple independent render trees. Each render
/// tree is rooted in a [`RenderView`] that must be added to the binding via
/// [`add_render_view`](Self::add_render_view) to be considered during frame
/// production, hit testing, etc.
///
/// Furthermore, the render tree must be managed by a [`PipelineOwner`] that is
/// part of the pipeline owner tree rooted at [`root_pipeline_owner`](Self::root_pipeline_owner).
pub trait RendererBinding: Send + Sync {
    /// The mouse tracker for managing hover events.
    fn mouse_tracker(&self) -> &MouseTracker;

    /// The root of the pipeline owner tree.
    ///
    /// By default, the root pipeline owner is not set up to manage a render tree.
    /// Child pipeline owners are added to it via [`PipelineOwner::adopt_child`].
    fn root_pipeline_owner(&self) -> &PipelineOwner;

    /// Returns an iterator over all render views managed by this binding.
    fn render_views(&self) -> Box<dyn Iterator<Item = &RenderView> + '_>;

    /// Adds a render view to this binding.
    ///
    /// The binding will interact with the render view in the following ways:
    /// - Setting and updating its configuration
    /// - Calling `composite_frame` when it's time to produce a new frame
    /// - Forwarding relevant pointer events for hit testing
    fn add_render_view(&mut self, view: RenderView);

    /// Removes a render view previously added with [`add_render_view`](Self::add_render_view).
    fn remove_render_view(&mut self, view_id: u64);

    /// Pump the rendering pipeline to generate a frame.
    ///
    /// This method is called when it's time to lay out and paint a frame.
    /// Each frame consists of the following phases:
    ///
    /// 1. **Layout phase**: All dirty render objects are laid out
    /// 2. **Compositing bits phase**: Compositing bits are updated
    /// 3. **Paint phase**: All dirty render objects are repainted
    /// 4. **Compositing phase**: Layer tree is turned into a scene
    /// 5. **Semantics phase**: Semantics tree is updated
    fn draw_frame(&mut self);

    /// Called when the system metrics change (window resize, DPI change, etc.).
    fn handle_metrics_changed(&mut self);

    /// Called when the platform text scale factor changes.
    fn handle_text_scale_factor_changed(&mut self) {}

    /// Called when the platform brightness changes.
    fn handle_platform_brightness_changed(&mut self) {}

    /// Dispatch a pointer event to the appropriate render view.
    fn dispatch_event(&mut self, event: PointerEvent, hit_test_result: Option<&HitTestResult>);

    /// Perform a hit test at the given position in the specified view.
    fn hit_test_in_view(&self, result: &mut HitTestResult, position: Offset, view_id: u64);

    /// Schedule a frame to be drawn.
    fn schedule_frame(&mut self);

    /// Whether frames produced by [`draw_frame`](Self::draw_frame) are sent to the engine.
    fn send_frames_to_engine(&self) -> bool {
        true
    }
}

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

/// Debug utilities for the rendering system.
pub mod debug {
    use super::*;

    /// Prints a textual representation of all render trees.
    pub fn dump_render_trees<B: RendererBinding + ?Sized>(binding: &B) -> String {
        let views: Vec<_> = binding.render_views().collect();
        if views.is_empty() {
            return "No render tree root was added to the binding.".to_string();
        }

        views
            .iter()
            .map(|view| format!("{:?}", view))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Prints a textual representation of all layer trees.
    pub fn dump_layer_trees<B: RendererBinding + ?Sized>(binding: &B) -> String {
        let views: Vec<_> = binding.render_views().collect();
        if views.is_empty() {
            return "No render tree root was added to the binding.".to_string();
        }

        views
            .iter()
            .map(|view| {
                if let Some(layer) = view.layer() {
                    format!("{:?}", layer)
                } else {
                    format!("Layer tree unavailable for {:?}", view)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
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
