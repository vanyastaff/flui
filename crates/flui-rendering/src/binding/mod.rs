//! Renderer binding trait -- the integration point between the rendering
//! system and the application layer.
//!
//! Concrete implementations live in `flui_app`.
//!
//! The pipeline-manifold and view-hit-testing surfaces are folded into this
//! single trait; a separate hit-test dispatcher trait is omitted entirely --
//! it had zero production implementations.
//!
//! # Architecture
//!
//! ```text
//! flui_app::RenderingBinding implements RendererBinding
//! ```

use std::sync::Arc;

use crate::{
    hit_testing::HitTestResult,
    pipeline::PipelineCell,
    view::{RenderView, ViewConfiguration},
};

// ============================================================================
// ViewHitTest
// ============================================================================

/// How a view answered [`RendererBinding::hit_test_in_view`].
#[derive(Debug)]
#[must_use = "a busy tree or a contained panic is lost if the answer is dropped"]
pub enum ViewHitTest {
    /// The walk ran; the result holds its path.
    ///
    /// A render object whose hit test panicked counted as a miss, and the
    /// first such payload is here for the caller to resume once the event
    /// has been delivered to the path.
    Answered {
        /// The first contained render-object panic, if any.
        panic: Option<Box<dyn std::any::Any + Send>>,
    },
    /// The tree cannot answer now — its layout is withdrawn (before the
    /// first complete frame, or after a failed one) or a frame phase has it
    /// checked out. The result is untouched: an empty path here would claim
    /// that nothing is under the position.
    TreeBusy,
}

// ============================================================================
// RendererBinding
// ============================================================================

/// The glue between the render trees and the engine.
///
/// This trait provides the rendering system integration that bindings must
/// implement. It manages multiple independent render trees, each rooted in
/// a [`RenderView`]. It also exposes the integration surface for visual-
/// update requests, semantics enablement, and view-routed hit testing --
/// historically split across `PipelineManifold` and `ViewHitTestable`
/// traits, but unified here because every concrete binding implements
/// all three together and the abstraction earned nothing.
///
/// # Responsibilities
///
/// - Managing the root [`PipelineOwner`](crate::pipeline::PipelineOwner) tree
/// - Managing [`RenderView`]s (add/remove)
/// - Creating [`ViewConfiguration`]s for views
/// - Declaring the first-frame deferral gate via
///   [`send_frames_to_engine`](Self::send_frames_to_engine)
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
/// These phase methods were lifted out of `PipelineOwner<Idle>` and
/// onto their phase-typed impls on 2026-05-20. The
/// orchestrator is [`PipelineOwner::<Idle>::run_frame`](crate::pipeline::PipelineOwner::run_frame), which
/// composes the four phase transitions and returns the owner back at
/// `Idle` plus the produced layer tree. Pumping that orchestrator and
/// gating step 6 on [`send_frames_to_engine`](Self::send_frames_to_engine)
/// is the implementer's job, not a trait default — see that method's
/// doc for why. The production incarnation is
/// `UiRuntime::render_frame_entered` (`flui-app`; there is no `AppBinding`
/// any more — that type was retired), which consults
/// `RenderingBinding::send_frames_to_engine` before presenting.
pub trait RendererBinding {
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
    /// The `view_id` selects the presentation's render tree before its
    /// render objects perform protocol-specific hit testing; `kind` is the
    /// pointer asking, whose minimum target expanded hit areas follow.
    ///
    /// The answer says whether the tree could be asked at all; see
    /// [`ViewHitTest`].
    fn hit_test_in_view(
        &self,
        result: &mut HitTestResult,
        position: flui_foundation::geometry::Offset,
        kind: flui_interaction::events::PointerKind,
        view_id: u64,
    ) -> ViewHitTest;

    // ========================================================================
    // Pipeline owner tree
    // ========================================================================

    /// Returns the root pipeline owner.
    ///
    /// This is the root of the PipelineOwner tree. Multi-window scenarios
    /// own multiple PipelineOwner instances side-by-side; the previous
    /// `PipelineOwner::adopt_child` hierarchical API was removed.
    fn root_pipeline_owner(&self) -> &PipelineCell;

    // ========================================================================
    // RenderView Management
    // ========================================================================
    //
    // The trait lends render views and never hands out a lock: storage and
    // its synchronisation stay the implementer's, and no caller can hold a
    // view's guard across a call back into the binding.

    /// Runs `visit` on the render view for `view_id`; `false` when there is
    /// none.
    ///
    /// The view is lent for the call only. While it is lent, a nested call
    /// for the same id finds no view, and a view inserted under that id
    /// during the call replaces the lent one.
    fn with_render_view(&self, view_id: u64, visit: &mut dyn FnMut(&mut RenderView)) -> bool;

    /// Returns the IDs of all render views currently managed by this
    /// binding.
    ///
    /// Iteration order is **not** guaranteed (the canonical impl uses a
    /// `HashMap`).
    fn render_view_ids(&self) -> Vec<u64>;

    /// Inserts a render view at `view_id`.
    ///
    /// If a view with `view_id` already exists, this replaces it; the
    /// prior value is dropped. The default-impl helper
    /// [`Self::add_render_view_with_config`] applies view-configuration
    /// derivation on top of this primitive.
    fn insert_render_view(&self, view_id: u64, view: RenderView);

    /// Removes a render view, returning it if it existed.
    fn remove_render_view_by_id(&self, view_id: u64) -> Option<RenderView>;

    /// Adds a render view with the binding's view-configuration derivation
    /// applied first.
    ///
    /// The binding will:
    /// - Derive a [`ViewConfiguration`] via
    ///   [`Self::create_view_configuration_for`] from the view itself,
    /// - Apply it to the view via [`RenderView::set_configuration`],
    /// - Insert the view via [`Self::insert_render_view`].
    ///
    /// Use this when adding a fresh `RenderView`; use
    /// [`Self::insert_render_view`] directly when the view's
    /// configuration is already set (e.g. carrying it from an old
    /// binding).
    fn add_render_view_with_config(&self, view_id: u64, mut view: RenderView) {
        let config = self.create_view_configuration_for(&view);
        view.set_configuration(config);
        self.insert_render_view(view_id, view);
    }

    // ========================================================================
    // View Configuration
    // ========================================================================

    /// Creates a view configuration for the given render view.
    ///
    /// This is called during
    /// [`add_render_view_with_config`](Self::add_render_view_with_config)
    /// and in response to metrics changes.
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
    // Frame Production
    // ========================================================================

    /// Whether frames should be sent to the engine.
    ///
    /// If false, the framework does all frame work but doesn't render.
    /// Used for deferring the first frame until ready.
    ///
    /// Answers `true` once the first frame has been sent, or while no
    /// first-frame deferral is outstanding.
    ///
    /// This is a **required** method (no default) deliberately: the
    /// deferral counter behind it is per-binding state (a nested
    /// defer/allow counter plus a latching "first frame already sent"
    /// flag), and a `true`-returning default previously let an
    /// implementer silently skip wiring the counter at all — the exact
    /// drift this trait method's history was flagged for. The one
    /// production implementation lives on `RenderingBinding`
    /// (`flui-runtime`'s `crates/flui-runtime/src/renderer_binding.rs`);
    /// implement this by delegating to that same counter rather than
    /// growing a second one.
    fn send_frames_to_engine(&self) -> bool;

    // ========================================================================
    // Metrics Handling
    // ========================================================================

    /// Called when system metrics change (window resize, DPI change, etc.).
    ///
    /// Updates all render view configurations and schedules a frame.
    fn handle_metrics_changed(&self) {
        let mut force_frame = false;

        for view_id in self.render_view_ids() {
            self.with_render_view(view_id, &mut |view| {
                force_frame = force_frame || view.has_configuration();
                let new_config = self.create_view_configuration_for(view);
                view.set_configuration(new_config);
            });
        }

        if force_frame {
            self.request_visual_update();
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
    let ids = binding.render_view_ids();
    if ids.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    ids.into_iter()
        .filter_map(|id| {
            let mut dump = None;
            binding.with_render_view(id, &mut |view| {
                dump = Some(format!("=== RenderView {id} ===\n{view:?}"));
            });
            dump
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Prints a textual representation of all layer trees.
pub fn debug_dump_layer_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    let ids = binding.render_view_ids();
    if ids.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    ids.into_iter()
        .filter_map(|id| {
            let mut dump = None;
            binding.with_render_view(id, &mut |view| {
                dump = Some(match view.layer() {
                    Some(layer) => format!("=== LayerTree {id} ===\n{layer:?}"),
                    None => format!("=== LayerTree {id} ===\nLayer tree unavailable"),
                });
            });
            dump
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
    let ids = binding.render_view_ids();
    if ids.is_empty() {
        return "No render tree root was added to the binding.".to_string();
    }

    const EXPLANATION: &str = "For performance reasons, the framework only generates semantics when asked to do so by the platform.\n\
         Usually, platforms only ask for semantics when assistive technologies (like screen readers) are running.\n\
         To generate semantics, try turning on an assistive technology (like VoiceOver or TalkBack) on your device.";

    let mut printed_explanation = false;

    ids.into_iter()
        .map(|id| {
            // Note: Binding-level semantics dump routing is not yet wired.
            // This would require:
            // 1. View to expose its PipelineOwner
            // 2. RendererBinding to route each view id to that PipelineOwner
            // 3. SemanticsTree to implement Debug or custom formatting
            let mut message =
                format!("=== SemanticsTree {id} ===\nSemantics dump not available via binding.");
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
///
/// When the owner has a root, this renders its typed snapshot ([`PipelineOwner::inspect`]) as
/// the plain outline: each render object's own fields, with the committed geometry, offset and
/// dirty flags the tree adds; otherwise it falls back to the owner's `Debug` representation.
pub fn debug_dump_pipeline_owner_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    use flui_foundation::inspect::{Level, Plain, SnapshotQuery, render, snapshot};

    binding.root_pipeline_owner().with(|owner| {
        match snapshot(&owner.inspect(), SnapshotQuery::default()) {
            Ok(tree) => {
                let mut text = String::new();
                render(&tree, Level::Debug, &Plain, &mut text)
                    .expect("BUG: writing to a String cannot fail");
                text
            }
            Err(_) => format!("{owner:?}"),
        }
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {

    use super::*;

    // Verify the trait is object-safe.
    fn _assert_renderer_binding_object_safe(_: &dyn RendererBinding) {}
}
