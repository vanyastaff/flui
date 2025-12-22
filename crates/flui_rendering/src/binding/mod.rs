//! Renderer binding traits - interfaces for the rendering system.
//!
//! This module provides traits for integrating the rendering system
//! with the application layer. The concrete implementations live in `flui_app`.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `rendering/binding.dart`:
//! - [`PipelineManifold`] - Interface for pipeline owner tree management
//! - [`RendererBinding`] - Main mixin for rendering integration
//! - [`HitTestDispatcher`] - Interface for hit test event dispatching
//!
//! # Architecture
//!
//! ```text
//! flui_app::RenderingFlutterBinding
//!     implements RendererBinding
//!         uses PipelineManifold
//!         uses HitTestDispatcher
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::hit_testing::{HitTestResult, PointerEvent};
use crate::input::MouseTracker;
use crate::pipeline::PipelineOwner;
use crate::view::{RenderView, ViewConfiguration};

// ============================================================================
// PipelineManifold
// ============================================================================

/// Interface for managing the pipeline owner tree and semantics.
///
/// This trait is implemented by the binding to provide services to pipeline
/// owners in the tree. It acts as a bridge between PipelineOwner and the
/// binding's scheduling/semantics systems.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PipelineManifold` class.
pub trait PipelineManifold: Send + Sync {
    /// Request that the visual display be updated.
    ///
    /// This is called by pipeline owners when they have work to do.
    /// The binding should schedule a frame in response.
    fn request_visual_update(&self);

    /// Whether semantics are currently enabled.
    ///
    /// When true, the framework will maintain the semantics tree.
    fn semantics_enabled(&self) -> bool;

    /// Add a listener for semantics enabled changes.
    fn add_semantics_enabled_listener(&self, listener: Arc<dyn Fn(bool) + Send + Sync>);

    /// Remove a previously added semantics enabled listener.
    fn remove_semantics_enabled_listener(&self, listener: &Arc<dyn Fn(bool) + Send + Sync>);
}

// ============================================================================
// HitTestDispatcher
// ============================================================================

/// Interface for dispatching hit test results to targets.
///
/// # Flutter Equivalence
///
/// This is part of Flutter's `GestureBinding` functionality.
pub trait HitTestDispatcher: Send + Sync {
    /// Dispatch a pointer event to all targets in the hit test result.
    fn dispatch_event(&self, event: &PointerEvent, result: &HitTestResult);
}

// ============================================================================
// HitTestable
// ============================================================================

/// Interface for objects that can be hit tested.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestable` mixin.
pub trait HitTestable: Send + Sync {
    /// Hit test at the given position in the given view.
    ///
    /// # Arguments
    ///
    /// * `result` - The hit test result to populate
    /// * `position` - The position in logical pixels
    /// * `view_id` - The ID of the view to hit test in
    fn hit_test_in_view(
        &self,
        result: &mut HitTestResult,
        position: flui_types::Offset,
        view_id: u64,
    );
}

// ============================================================================
// RendererBinding
// ============================================================================

/// The glue between the render trees and the engine.
///
/// This trait provides the rendering system integration that bindings must
/// implement. It manages multiple independent render trees, each rooted in
/// a [`RenderView`].
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `RendererBinding` mixin from `rendering/binding.dart`.
///
/// # Responsibilities
///
/// - Managing the root [`PipelineOwner`] tree
/// - Managing [`RenderView`]s (add/remove)
/// - Creating [`ViewConfiguration`]s for views
/// - Coordinating frame production via [`draw_frame`](Self::draw_frame)
/// - Managing [`MouseTracker`] for hover events
///
/// # Frame Production
///
/// Each frame consists of these phases (in order):
///
/// 1. **Animation** - Tickers and animations update (handled by SchedulerBinding)
/// 2. **Build** - Widget tree rebuilds (handled by WidgetsBinding)
/// 3. **Layout** - [`flush_layout`](PipelineOwner::flush_layout)
/// 4. **Compositing bits** - [`flush_compositing_bits`](PipelineOwner::flush_compositing_bits)
/// 5. **Paint** - [`flush_paint`](PipelineOwner::flush_paint)
/// 6. **Compositing** - Send layers to GPU
/// 7. **Semantics** - [`flush_semantics`](PipelineOwner::flush_semantics)
pub trait RendererBinding: PipelineManifold + HitTestable {
    // ========================================================================
    // Pipeline Owner Tree
    // ========================================================================

    /// Returns the root pipeline owner.
    ///
    /// This is the root of the PipelineOwner tree. Child pipeline owners
    /// are added via [`PipelineOwner::adopt_child`].
    fn root_pipeline_owner(&self) -> &RwLock<PipelineOwner>;

    /// Creates the root pipeline owner.
    ///
    /// Override this to customize the root pipeline owner configuration.
    /// By default, creates a pipeline owner that cannot have a root node.
    fn create_root_pipeline_owner(&self) -> PipelineOwner {
        PipelineOwner::new()
    }

    // ========================================================================
    // RenderView Management
    // ========================================================================

    /// Returns all render views managed by this binding.
    fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>;

    /// Adds a render view to this binding.
    ///
    /// The binding will:
    /// - Set and update the view's configuration
    /// - Call `composite_frame` when producing frames
    /// - Forward pointer events for hit testing
    ///
    /// # Arguments
    ///
    /// * `view_id` - Unique identifier for this view
    /// * `view` - The render view to add
    fn add_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
        let config = self.create_view_configuration_for(&view.read());
        view.write().set_configuration(config);
        self.render_views().write().insert(view_id, view);
    }

    /// Removes a render view from this binding.
    ///
    /// # Arguments
    ///
    /// * `view_id` - The ID of the view to remove
    ///
    /// # Returns
    ///
    /// The removed view, if it existed.
    fn remove_render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views().write().remove(&view_id)
    }

    /// Returns a render view by ID.
    fn get_render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views().read().get(&view_id).cloned()
    }

    // ========================================================================
    // View Configuration
    // ========================================================================

    /// Creates a view configuration for the given render view.
    ///
    /// This is called during [`add_render_view`](Self::add_render_view) and
    /// in response to metrics changes.
    ///
    /// Override this to customize view configuration (e.g., for testing).
    fn create_view_configuration_for(&self, render_view: &RenderView) -> ViewConfiguration {
        // Default: use the view's current configuration or create a default
        if render_view.has_configuration() {
            render_view.configuration().clone()
        } else {
            ViewConfiguration::default()
        }
    }

    // ========================================================================
    // Mouse Tracker
    // ========================================================================

    /// Returns the mouse tracker for hover notification.
    fn mouse_tracker(&self) -> &RwLock<MouseTracker>;

    // ========================================================================
    // Frame Production
    // ========================================================================

    /// Whether frames should be sent to the engine.
    ///
    /// If false, the framework does all frame work but doesn't render.
    /// Used for deferring the first frame until ready.
    fn send_frames_to_engine(&self) -> bool {
        true
    }

    /// Pump the rendering pipeline to generate a frame.
    ///
    /// This is the main entry point for frame production. It:
    /// 1. Flushes layout
    /// 2. Flushes compositing bits
    /// 3. Flushes paint
    /// 4. Composites frames (if sending to engine)
    /// 5. Flushes semantics (if sending to engine)
    fn draw_frame(&self) {
        let root_owner = self.root_pipeline_owner();

        // Phase 3: Layout
        root_owner.write().flush_layout();

        // Phase 4: Compositing bits
        root_owner.write().flush_compositing_bits();

        // Phase 5: Paint
        root_owner.write().flush_paint();

        // Phase 6 & 7: Composite and Semantics (only if sending frames)
        if self.send_frames_to_engine() {
            // Composite each render view
            for (_, view) in self.render_views().read().iter() {
                let view_guard = view.read();
                let _result = view_guard.composite_frame();
                // In a real implementation, send to GPU here
            }

            // Phase 7: Semantics
            root_owner.write().flush_semantics();
        }
    }

    // ========================================================================
    // Metrics Handling
    // ========================================================================

    /// Called when system metrics change (window resize, DPI change, etc.).
    ///
    /// Updates all render view configurations and schedules a frame.
    fn handle_metrics_changed(&self) {
        let mut force_frame = false;

        for (_, view) in self.render_views().read().iter() {
            let mut view_guard = view.write();
            // If view has configuration, it needs a frame update
            force_frame = force_frame || view_guard.has_configuration();

            let new_config = self.create_view_configuration_for(&view_guard);
            view_guard.set_configuration(new_config);
        }

        if force_frame {
            self.request_visual_update();
        }
    }

    /// Called when platform text scale factor changes.
    fn handle_text_scale_factor_changed(&self) {
        // Default: no-op. Override to handle text scale changes.
    }

    /// Called when platform brightness changes.
    fn handle_platform_brightness_changed(&self) {
        // Default: no-op. Override to handle brightness changes.
    }

    // ========================================================================
    // Semantics Actions
    // ========================================================================

    /// Performs a semantics action on a node.
    ///
    /// # Arguments
    ///
    /// * `view_id` - The view containing the semantics node
    /// * `node_id` - The semantics node ID
    /// * `action` - The action to perform
    /// * `args` - Optional action arguments
    fn perform_semantics_action(
        &self,
        view_id: u64,
        node_id: i32,
        action: flui_semantics::SemanticsAction,
        _args: Option<flui_semantics::ActionArgs>,
    ) {
        // Look up the render view and delegate to its semantics owner
        if let Some(view) = self.get_render_view(view_id) {
            let _view_guard = view.read();
            unimplemented!(
                "Semantics actions not yet implemented - requires SemanticsOwner integration. \
                 Attempted action: {:?} on node {} in view {}",
                action,
                node_id,
                view_id
            );
        }
    }
}

// ============================================================================
// Debug Functions
// ============================================================================

/// Prints a textual representation of all render trees.
///
/// Prints the tree for each [`RenderView`] managed by the binding,
/// separated by blank lines.
pub fn debug_dump_render_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    let views = binding.render_views().read();
    if views.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    views
        .iter()
        .map(|(id, view)| {
            let view_guard = view.read();
            format!("=== RenderView {} ===\n{:?}", id, view_guard)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Prints a textual representation of all layer trees.
pub fn debug_dump_layer_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    let views = binding.render_views().read();
    if views.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    views
        .iter()
        .map(|(id, view)| {
            let view_guard = view.read();
            if let Some(layer) = view_guard.layer() {
                format!("=== LayerTree {} ===\n{:?}", id, layer)
            } else {
                format!("=== LayerTree {} ===\nLayer tree unavailable", id)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Prints a textual representation of all semantics trees.
///
/// # Arguments
///
/// * `child_order` - The order to dump children
pub fn debug_dump_semantics_tree<B: RendererBinding + ?Sized>(
    binding: &B,
    _child_order: flui_semantics::DebugSemanticsDumpOrder,
) -> String {
    let views = binding.render_views().read();
    if views.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    const EXPLANATION: &str =
        "For performance reasons, the framework only generates semantics when asked to do so by the platform.\n\
         Usually, platforms only ask for semantics when assistive technologies (like screen readers) are running.\n\
         To generate semantics, try turning on an assistive technology (like VoiceOver or TalkBack) on your device.";

    let mut printed_explanation = false;

    views
        .iter()
        .map(|(id, _view)| {
            // TODO: Get semantics tree from view's pipeline owner
            let mut message = format!("=== SemanticsTree {} ===\nSemantics not generated.", id);
            if !printed_explanation {
                printed_explanation = true;
                message.push('\n');
                message.push_str(EXPLANATION);
            }
            message
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Prints a textual representation of the pipeline owner tree.
pub fn debug_dump_pipeline_owner_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    let owner = binding.root_pipeline_owner().read();
    format!("{:?}", *owner)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the traits are object-safe
    fn _assert_pipeline_manifold_object_safe(_: &dyn PipelineManifold) {}
    fn _assert_hit_test_dispatcher_object_safe(_: &dyn HitTestDispatcher) {}
    fn _assert_hittestable_object_safe(_: &dyn HitTestable) {}

    #[test]
    fn test_traits_are_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn PipelineManifold>();
        assert_send_sync::<dyn HitTestDispatcher>();
        assert_send_sync::<dyn HitTestable>();
    }
}
