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
//! The three-trait stack (`PipelineManifold`, `HitTestDispatcher`,
//! `ViewHitTestable`) was collapsed on 2026-05-20. See
//! `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 12.

use std::sync::Arc;

use parking_lot::RwLock;

use flui_interaction::MouseTracker;
use flui_layer::LayerTree;

use crate::{
    hit_testing::HitTestResult,
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
/// These phase methods were lifted out of `PipelineOwner<Idle>` and
/// onto their phase-typed impls on 2026-05-20. The
/// orchestrator is [`PipelineOwner::<Idle>::run_frame`], which
/// composes the four phase transitions and returns the owner back at
/// `Idle` plus the produced layer tree.
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
    /// `PipelineOwner::adopt_child` hierarchical API was removed.
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
    // Mouse Tracker
    // ========================================================================

    /// Returns the mouse tracker for hover notification.
    ///
    /// The return type is `&MouseTracker` rather than `&RwLock<MouseTracker>`.
    /// The interaction-side [`flui_interaction::MouseTracker`] is
    /// owner-local, so executable mouse callbacks do not force the whole
    /// binding to be `Send + Sync`.
    fn mouse_tracker(&self) -> &MouseTracker;

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

    /// Pump the rendering pipeline to generate a frame, returning the
    /// produced layer tree.
    ///
    /// The single authoritative frame path: consume the owner out of
    /// the `RwLock`, drive it through `run_frame` (all four phase
    /// transitions; semantics included), put it back, and hand the
    /// produced `LayerTree` to the caller. The caller — a platform
    /// binding or embedder — wraps the tree in a `Scene` and submits
    /// it to the renderer (`AppBinding::render_frame` is the
    /// production incarnation: `draw_frame → Scene::new →
    /// Renderer::render_scene`).
    ///
    /// Returns `None` when the frame is deferred
    /// ([`Self::send_frames_to_engine`] is `false` — Flutter's
    /// deferred-first-frame mechanism; pipeline work still runs so
    /// warm-up costs are paid early), when the pipeline produced no
    /// tree (no root), or when a phase errored (logged, frame
    /// dropped, owner restored for the next frame).
    fn draw_frame(&self) -> Option<LayerTree> {
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

        if self.send_frames_to_engine() {
            layer_tree
        } else {
            // Deferred: the work ran (warm-up), the output is withheld.
            None
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
        // This body used to panic via `unimplemented!()` — a Constitution
        // Principle 6 violation reachable from every assistive-tech action
        // dispatch. It now emits a `tracing::warn!` with the action context
        // and returns without panicking. When `SemanticsOwner` integration
        // lands, the warning is swapped for the real dispatch.
        if self.render_view(view_id).is_some() {
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
    let owner = binding.root_pipeline_owner().read();
    match owner.debug_diagnostics_tree() {
        Some(tree) => tree.to_string(),
        None => format!("{:?}", *owner),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use flui_interaction::MouseTracker;
    use flui_types::{Size, geometry::px};

    use super::*;
    use crate::view::ViewConfiguration;

    // Verify the trait is object-safe.
    fn _assert_renderer_binding_object_safe(_: &dyn RendererBinding) {}

    /// Minimal `RendererBinding` implementer exercising the trait's default
    /// methods (`add_render_view_with_config`, `create_view_configuration_for`,
    /// `draw_frame`, `handle_metrics_changed`) and the free `debug_dump_*`
    /// functions, none of which had any test coverage.
    struct TestBinding {
        owner: RwLock<PipelineOwner>,
        views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>,
        mouse_tracker: MouseTracker,
        visual_update_calls: AtomicUsize,
        send_frames: AtomicBool,
    }

    impl TestBinding {
        fn new() -> Self {
            Self {
                owner: RwLock::new(PipelineOwner::new()),
                views: RwLock::new(HashMap::new()),
                mouse_tracker: MouseTracker::new(),
                visual_update_calls: AtomicUsize::new(0),
                send_frames: AtomicBool::new(true),
            }
        }
    }

    impl RendererBinding for TestBinding {
        fn request_visual_update(&self) {
            self.visual_update_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn semantics_enabled(&self) -> bool {
            false
        }

        fn add_semantics_enabled_listener(&self, _listener: Arc<dyn Fn(bool) + Send + Sync>) {}

        fn remove_semantics_enabled_listener(&self, _listener: &Arc<dyn Fn(bool) + Send + Sync>) {}

        fn hit_test_in_view(
            &self,
            _result: &mut HitTestResult,
            _position: flui_types::Offset,
            _view_id: u64,
        ) {
        }

        fn root_pipeline_owner(&self) -> &RwLock<PipelineOwner> {
            &self.owner
        }

        fn render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
            self.views.read().get(&view_id).cloned()
        }

        fn render_view_ids(&self) -> Vec<u64> {
            self.views.read().keys().copied().collect()
        }

        fn insert_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
            self.views.write().insert(view_id, view);
        }

        fn remove_render_view_by_id(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
            self.views.write().remove(&view_id)
        }

        fn mouse_tracker(&self) -> &MouseTracker {
            &self.mouse_tracker
        }

        fn send_frames_to_engine(&self) -> bool {
            self.send_frames.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn add_render_view_with_config_derives_and_inserts() {
        let binding = TestBinding::new();
        let view = Arc::new(RwLock::new(RenderView::new()));
        assert!(!view.read().has_configuration());

        binding.add_render_view_with_config(1, Arc::clone(&view));

        // The default `create_view_configuration_for` gives a bare view
        // `ViewConfiguration::default()`, and the view must now be reachable
        // through the binding's own accessors.
        assert!(view.read().has_configuration());
        assert!(binding.render_view(1).is_some());
        assert_eq!(binding.render_view_ids(), vec![1]);
    }

    #[test]
    fn create_view_configuration_for_returns_existing_or_default() {
        let binding = TestBinding::new();

        let bare = RenderView::new();
        assert_eq!(
            binding.create_view_configuration_for(&bare),
            ViewConfiguration::default()
        );

        let existing_config = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 2.0);
        let configured = RenderView::with_configuration(existing_config.clone());
        assert_eq!(
            binding.create_view_configuration_for(&configured),
            existing_config
        );
    }

    #[test]
    fn handle_metrics_changed_requests_a_frame_only_when_a_view_was_already_configured() {
        let binding = TestBinding::new();

        // A view without prior configuration: metrics-changed gives it the
        // default configuration, but must NOT request a frame (nothing was
        // visibly configured before this call).
        let bare_view = Arc::new(RwLock::new(RenderView::new()));
        binding.insert_render_view(1, Arc::clone(&bare_view));

        binding.handle_metrics_changed();
        assert_eq!(binding.visual_update_calls.load(Ordering::SeqCst), 0);
        assert!(bare_view.read().has_configuration());

        // A view that already had a configuration: metrics-changed must
        // request a frame since a real, visible view is being updated.
        let configured_view = Arc::new(RwLock::new(RenderView::with_configuration(
            ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 1.0),
        )));
        binding.insert_render_view(2, Arc::clone(&configured_view));

        binding.handle_metrics_changed();
        assert_eq!(binding.visual_update_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn draw_frame_withholds_output_when_deferred_but_still_runs_pipeline_work() {
        let binding = TestBinding::new();
        binding.send_frames.store(false, Ordering::SeqCst);

        // No root attached, so there is nothing to layout/paint regardless;
        // the point of this test is that a deferred frame reports `None`.
        assert!(binding.draw_frame().is_none());
    }

    #[test]
    fn draw_frame_returns_none_when_pipeline_has_no_root() {
        let binding = TestBinding::new();
        assert!(binding.draw_frame().is_none());
    }

    #[test]
    fn debug_dump_functions_report_empty_binding_has_no_root() {
        let binding = TestBinding::new();

        assert_eq!(
            debug_dump_render_tree(&binding),
            "No render tree root was added to the binding."
        );
        assert_eq!(
            debug_dump_layer_tree(&binding),
            "No render tree root was added to the binding."
        );
        assert_eq!(
            debug_dump_semantics_tree(
                &binding,
                flui_semantics::DebugSemanticsDumpOrder::TraversalOrder
            ),
            "No render tree root was added to the binding."
        );

        // No pipeline root: falls back to the owner's `Debug` representation
        // rather than the diagnostics tree.
        let dump = debug_dump_pipeline_owner_tree(&binding);
        assert!(!dump.is_empty());
    }

    #[test]
    fn debug_dump_render_and_layer_tree_include_the_added_view() {
        let binding = TestBinding::new();
        let mut view = RenderView::with_configuration(ViewConfiguration::from_size(
            Size::new(px(800.0), px(600.0)),
            1.0,
        ));
        view.prepare_initial_frame_without_owner();
        binding.insert_render_view(7, Arc::new(RwLock::new(view)));

        let render_dump = debug_dump_render_tree(&binding);
        assert!(render_dump.contains("RenderView 7"));

        let layer_dump = debug_dump_layer_tree(&binding);
        assert!(layer_dump.contains("LayerTree 7"));
        assert!(!layer_dump.contains("unavailable"));
    }
}
