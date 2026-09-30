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

use parking_lot::RwLock;

use crate::{
    hit_testing::HitTestResult,
    pipeline::PipelineCell,
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
/// `UiRealm::render_frame_entered` (`flui-app`; there is no `AppBinding`
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
    /// Distinct from `flui_interaction::HitTestable`, which operates on
    /// individual render objects without a view context. This adds the
    /// `view_id` parameter to route hit tests to the correct render tree.
    fn hit_test_in_view(
        &self,
        result: &mut HitTestResult,
        position: flui_foundation::geometry::Offset,
        view_id: u64,
    );

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
    // This section used to expose `render_views()` returning a
    // `&RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` — a triple-lock
    // topology baked into the trait surface. Every consumer had to reason
    // about the outer `HashMap` lock, the inner `Arc<RwLock<RenderView>>`
    // lock, and the implicit map-entry refcount. An audit flagged it as a
    // newtype-getter violation at trait level: a getter returning a raw
    // lock forces every caller to re-derive its own locking discipline
    // instead of the trait stating what operation is needed.
    //
    // The trait surface now exposes four primitives instead:
    //   - `render_view(id)`         — single lookup, refcount bump
    //   - `render_view_ids()`       — owned `Vec<u64>` snapshot
    //   - `insert_render_view`      — single-write
    //   - `remove_render_view_by_id` — single-write + return
    //
    // The implementer retains full freedom over container choice
    // (`HashMap`, `DashMap`, `IndexMap`...) and lock primitive
    // (`RwLock`, `Mutex`, lock-free). The trait says what the lock
    // does, not how it is held — *Gjengset, Rust for Rustaceans* ch.3.

    /// Returns the render view for `view_id`, if present.
    ///
    /// The returned `Arc<RwLock<RenderView>>` is a reference-count bump;
    /// the caller acquires the inner lock for actual access. The
    /// implementer's outer container lock is held only for the duration
    /// of the lookup.
    fn render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;

    /// Returns the IDs of all render views currently managed by this
    /// binding.
    ///
    /// Iteration order is **not** guaranteed (the canonical impl uses a
    /// `HashMap`). The returned `Vec` is owned; the implementer's outer
    /// container lock is held only for the duration of collection.
    fn render_view_ids(&self) -> Vec<u64>;

    /// Inserts a render view at `view_id`.
    ///
    /// If a view with `view_id` already exists, this replaces it; the
    /// prior value is dropped. Implementers wanting custom replace
    /// semantics override this directly. The default-impl helper
    /// [`Self::add_render_view_with_config`] applies view-configuration
    /// derivation on top of this primitive.
    fn insert_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>);

    /// Removes a render view, returning it if it existed.
    fn remove_render_view_by_id(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>>;

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
    ///
    /// # Arguments
    ///
    /// * `view_id` - Unique identifier for this view
    /// * `view` - The render view to add
    fn add_render_view_with_config(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
        let config = self.create_view_configuration_for(&view.read());
        view.write().set_configuration(config);
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

        // Ids-then-lookup iteration: the outer-container lock is released
        // between snapshot collection and per-view writes. Previously this
        // method held the read-lock on the container for the duration of
        // every view's write-lock, which is the exact nested-lock topology
        // the trait reshape above was meant to avoid.
        for view_id in self.render_view_ids() {
            if let Some(view) = self.render_view(view_id) {
                let mut view_guard = view.write();
                force_frame = force_frame || view_guard.has_configuration();
                let new_config = self.create_view_configuration_for(&view_guard);
                view_guard.set_configuration(new_config);
            }
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
            binding.render_view(id).map(|view| {
                let view_guard = view.read();
                format!("=== RenderView {id} ===\n{view_guard:?}")
            })
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
            binding.render_view(id).map(|view| {
                let view_guard = view.read();
                if let Some(layer) = view_guard.layer() {
                    format!("=== LayerTree {id} ===\n{layer:?}")
                } else {
                    format!("=== LayerTree {id} ===\nLayer tree unavailable")
                }
            })
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
/// When the owner has a root, this renders the `Diagnosticable`-backed
/// diagnostics tree (each render object self-describes its properties, with
/// committed geometry/offset layered on); otherwise it falls back to the
/// owner's `Debug` representation.
pub fn debug_dump_pipeline_owner_tree<B: RendererBinding + ?Sized>(binding: &B) -> String {
    binding
        .root_pipeline_owner()
        .with(|owner| match owner.debug_diagnostics_tree() {
            Some(tree) => tree.to_string(),
            None => format!("{owner:?}"),
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
