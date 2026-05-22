//! Renderer binding trait -- the integration point between the rendering
//! system and the application layer.
//!
//! Concrete implementations live in `flui_app`.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `rendering/binding.dart` `RendererBinding`
//! mixin. Flutter's `PipelineManifold` and `HitTestable`-on-`RendererBinding`
//! mixins are folded into this single trait. The `HitTestDispatcher` mixin
//! (Flutter's `GestureBinding`-side dispatch) is omitted entirely -- it had
//! zero production implementations in FLUI.
//!
//! # Architecture
//!
//! ```text
//! flui_app::RenderingFlutterBinding implements RendererBinding
//! ```
//!
//! Mythos Step 4 (2026-05-20): the three-trait stack (`PipelineManifold`,
//! `HitTestDispatcher`, `ViewHitTestable`) was collapsed. See
//! `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 12.

use std::{collections::HashMap, sync::Arc};

use parking_lot::RwLock;

use crate::{
    hit_testing::HitTestResult,
    input::MouseTracker,
    pipeline::PipelineOwner,
    view::{RenderView, ViewConfiguration},
};

// ============================================================================
// RendererBinding
// ============================================================================

/// The glue between the render trees and the engine.
///
/// This trait provides the rendering system integration that bindings must
/// implement. It manages multiple independent render trees, each rooted in
/// a [`RenderView`]. It also exposes the integration surface for visual-
/// update requests, semantics enablement, and view-routed hit testing --
/// historically split across `PipelineManifold` and `ViewHitTestable` mixins
/// in Flutter, but unified here because every concrete binding implements
/// all three together and the abstraction earned nothing.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `RendererBinding` mixin from
/// `rendering/binding.dart`, plus the merged surface of `PipelineManifold`
/// and the `HitTestable` mixin.
///
/// # Responsibilities
///
/// - Managing the root [`PipelineOwner`] tree
/// - Managing [`RenderView`]s (add/remove)
/// - Creating [`ViewConfiguration`]s for views
/// - Coordinating frame production via [`draw_frame`](Self::draw_frame)
/// - Managing [`MouseTracker`] for hover events
/// - Responding to visual-update requests from pipeline owners
/// - Tracking semantics-enabled state and its listeners
/// - Routing hit tests to the correct view's render tree
///
/// # Frame Production
///
/// Each frame consists of these phases (in order):
///
/// 1. **Animation** - Tickers and animations update (handled by
///    SchedulerBinding)
/// 2. **Build** - Widget tree rebuilds (handled by WidgetsBinding)
/// 3. **Layout** - `PipelineOwner::<Layout>::run_layout`
/// 4. **Compositing bits** - `PipelineOwner::<Compositing>::run_compositing`
/// 5. **Paint** - `PipelineOwner::<PaintPhase>::run_paint`
/// 6. **Compositing** - Send layers to GPU
/// 7. **Semantics** - `PipelineOwner::<Semantics>::run_semantics`
///
/// Mythos Step 7 (2026-05-20) lifted these phase methods out of
/// `PipelineOwner<Idle>` and onto their phase-typed impls. The
/// orchestrator is [`PipelineOwner::<Idle>::run_frame`], which
/// composes the four phase transitions and returns the owner back at
/// `Idle` plus the produced layer tree.
pub trait RendererBinding: Send + Sync {
    // ========================================================================
    // Pipeline / Manifold (formerly PipelineManifold)
    // ========================================================================

    /// Request that the visual display be updated.
    ///
    /// Called by pipeline owners when they have work to do. The binding
    /// should schedule a frame in response.
    fn request_visual_update(&self);

    /// Whether semantics are currently enabled.
    fn semantics_enabled(&self) -> bool;

    /// Add a listener for semantics-enabled changes.
    fn add_semantics_enabled_listener(&self, listener: Arc<dyn Fn(bool) + Send + Sync>);

    /// Remove a previously added semantics-enabled listener.
    fn remove_semantics_enabled_listener(&self, listener: &Arc<dyn Fn(bool) + Send + Sync>);

    // ========================================================================
    // View-routed hit testing (formerly ViewHitTestable)
    // ========================================================================

    /// Hit test at the given position in the given view.
    ///
    /// Distinct from `flui_interaction::HitTestable`, which operates on
    /// individual render objects without a view context. This adds the
    /// `view_id` parameter to route hit tests to the correct render tree.
    fn hit_test_in_view(
        &self,
        result: &mut HitTestResult,
        position: flui_types::Offset,
        view_id: u64,
    );

    // ========================================================================
    // Pipeline owner tree
    // ========================================================================

    /// Returns the root pipeline owner.
    ///
    /// This is the root of the PipelineOwner tree. Multi-window scenarios
    /// own multiple PipelineOwner instances side-by-side; the previous
    /// `PipelineOwner::adopt_child` hierarchical API was removed in
    /// Mythos Step 9.
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
    /// This is the main entry point for frame production. Uses
    /// `mem::replace` to consume the owner out of the `RwLock`, drive
    /// it through `run_frame` (which composes all four phase transitions),
    /// and put it back. Semantics now runs inside `run_frame`, so the
    /// `send_frames_to_engine` branch only handles compositing for the
    /// engine handoff.
    ///
    /// Mythos Step 7 finalization (2026-05-20). Mythos Step 12
    /// (2026-05-20): `run_frame` returns `(PipelineOwner<Idle>,
    /// RenderResult<Option<LayerTree>>)`. On error (e.g. a render
    /// object panicked and was caught by `catch_unwind`), the frame is
    /// dropped, the error is logged via tracing, and the owner is put
    /// back ready for the next frame.
    fn draw_frame(&self) {
        let root_owner = self.root_pipeline_owner();

        // Consume the owner through the typestate transitions.
        let layer_tree = {
            let mut guard = root_owner.write();
            let owner = std::mem::take(&mut *guard);
            let (owner, result) = owner.run_frame();
            *guard = owner;
            match result {
                Ok(layer_tree) => layer_tree,
                Err(e) => {
                    tracing::error!(error = ?e, "draw_frame: pipeline failed, dropping frame");
                    None
                }
            }
        };

        // Phase 6: Composite frames (only if sending frames)
        if self.send_frames_to_engine() {
            // Composite each render view
            for (_, view) in self.render_views().read().iter() {
                let view_guard = view.read();
                let _result = view_guard.composite_frame();
                // In a real implementation, send to GPU here
            }
        }

        // Hand the produced layer tree to the compositor (the actual
        // wiring is concrete-binding territory; defaults discard it
        // because no compositor is available at the trait level).
        let _ = layer_tree;
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
        // Cycle 4 R-2: pre-cycle the body panicked via `unimplemented!()`
        // — a Constitution Principle 6 violation reachable from every
        // assistive-tech action dispatch. Post-cycle: emit a
        // `tracing::warn!` with the action context and return without
        // panicking. When `SemanticsOwner` integration lands the warn
        // is swapped for the real dispatch.
        if self.get_render_view(view_id).is_some() {
            tracing::warn!(
                view_id,
                node_id,
                action = ?action,
                "perform_semantics_action: SemanticsOwner integration pending; \
                 action is a no-op until RenderView ↔ SemanticsOwner plumbing lands"
            );
        } else {
            tracing::debug!(
                view_id,
                node_id,
                action = ?action,
                "perform_semantics_action: view not found"
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

    const EXPLANATION: &str = "For performance reasons, the framework only generates semantics when asked to do so by the platform.\n\
         Usually, platforms only ask for semantics when assistive technologies (like screen readers) are running.\n\
         To generate semantics, try turning on an assistive technology (like VoiceOver or TalkBack) on your device.";

    let mut printed_explanation = false;

    views
        .keys()
        .map(|id| {
            // Note: Semantics tree integration is not yet implemented.
            // This would require:
            // 1. View to expose its PipelineOwner
            // 2. PipelineOwner to build and expose SemanticsTree
            // 3. SemanticsTree to implement Debug or custom formatting
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

    // Verify the trait is object-safe.
    fn _assert_renderer_binding_object_safe(_: &dyn RendererBinding) {}

    #[test]
    fn test_renderer_binding_send_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn RendererBinding>();
    }
}
