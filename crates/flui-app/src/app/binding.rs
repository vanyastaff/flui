//! AppBinding - Combined application binding.
//!
//! This is the central coordinator that combines all bindings like Flutter's
//! `WidgetsFlutterBinding`.
//!
//! # Flutter Equivalence
//!
//! ```dart
//! // Flutter's combined binding
//! class WidgetsFlutterBinding extends BindingBase
//!     with GestureBinding, SchedulerBinding, ServicesBinding,
//!          SemanticsBinding, PaintingBinding, RendererBinding,
//!          WidgetsBinding { }
//! ```
//!
//! In Rust, we compose the bindings as owned fields instead of mixins.
//!
//! # Architecture
//!
//! ```text
//! AppBinding (singleton)
//!   ├── renderer: RendererBinding      (render tree, pipeline)
//!   ├── widgets: WidgetsBinding        (element tree, build)
//!   ├── gestures: GestureBinding       (hit testing, pointer coalescing)
//!   └── scheduler: Scheduler           (frame callbacks)
//! ```

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use flui_engine::{EngineError, wgpu::Renderer};
use flui_foundation::HasInstance;
use flui_interaction::{binding::GestureBinding, routing::FocusManager};
use flui_layer::Scene;
use flui_platform::traits::{PlatformInput, PlatformWindow};
use flui_rendering::constraints::BoxConstraints;
use flui_scheduler::Scheduler;
use flui_types::{Size, geometry::px};
use flui_view::{View, WidgetsBinding};
use parking_lot::{Mutex, RwLock};

use crate::{
    app::lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle},
    bindings::RenderingFlutterBinding,
};

/// Combined application binding.
///
/// AppBinding is the central coordinator for the FLUI framework.
/// It composes all the specialized bindings:
/// - [`RendererBinding`](crate::bindings::RendererBinding) - Manages render tree and pipeline
/// - [`WidgetsBinding`] - Manages element tree and build phase
/// - [`GestureBinding`] - Manages hit testing, pointer coalescing, and gestures
/// - [`Scheduler`] - Manages frame scheduling
///
/// # Input Handling
///
/// Platform events enter through [`handle_input()`](Self::handle_input):
/// - Pointer events → `GestureBinding::handle_pointer_event()` (with
///   coalescing)
/// - Keyboard events → `FocusManager::dispatch_key_event()`
///
/// # Thread Safety
///
/// AppBinding is a singleton accessed via `instance()`. It uses internal
/// locking for thread-safe access to mutable state.
pub struct AppBinding {
    /// Renderer binding (render tree, layout/paint phases)
    renderer: RwLock<RenderingFlutterBinding>,

    /// Widgets binding (element tree, build phase)
    widgets: RwLock<WidgetsBinding>,

    /// Gesture binding (input handling, hit testing, pointer coalescing)
    gestures: GestureBinding,

    /// Whether a redraw is needed
    needs_redraw: AtomicBool,

    /// Whether the app is initialized
    initialized: AtomicBool,

    /// Total frames rendered successfully
    frames_rendered: AtomicU64,

    /// Frames dropped due to surface errors
    frames_dropped: AtomicU64,

    /// Shared pipeline owner for elements (wrapped in Arc for sharing)
    /// This is the same PipelineOwner as in RendererBinding, but wrapped
    /// for sharing with elements that need `Arc<RwLock<PipelineOwner>>`.
    shared_pipeline_owner: Arc<RwLock<flui_rendering::pipeline::PipelineOwner>>,

    /// Application lifecycle state tracker.
    lifecycle: Mutex<DefaultLifecycle>,

    /// Active platform window (set during run_desktop).
    active_window: Mutex<Option<Box<dyn PlatformWindow>>>,
}

impl AppBinding {
    /// Create a new AppBinding.
    fn new() -> Self {
        // Ensure the global Scheduler singleton is initialized
        let _ = Scheduler::instance();

        // Create shared pipeline owner first (elements need Arc access)
        let shared_pipeline_owner =
            Arc::new(RwLock::new(flui_rendering::pipeline::PipelineOwner::new()));

        // Idle-wake wiring: a dirty mark (mark_needs_layout /
        // add_node_needing_paint) fires this callback so a quiescent
        // event loop produces the frame — without it, work scheduled
        // while the app is idle (an async image decode, a timer-driven
        // setState) would sit in the dirty queues until some unrelated
        // input forced a redraw.
        //
        // Lock order is safe: the callback fires while the CALLER holds
        // the pipeline-owner lock, and `wake_frame` acquires only the
        // `active_window` leaf Mutex — never the owner, never `widgets`.
        // `AppBinding::instance()` is resolved lazily at fire time, not
        // captured, so this closure cannot re-enter `new()` during
        // singleton construction (dirty marks only happen after init).
        shared_pipeline_owner
            .write()
            .set_on_need_visual_update(|| AppBinding::instance().wake_frame());

        // Animation-wake wiring: scheduling a frame callback (a ticker
        // tick) fires this hook on the scheduler's false→true
        // `frame_scheduled` transition (Flutter parity:
        // `SchedulerBinding.scheduleFrame` → platform `scheduleFrame`).
        // Without it an AnimationController only advances on frames some
        // OTHER source produces — after the first idle frame the ticker
        // starves and the animation freezes. Same lock-safety argument as
        // the visual-update hook above: `wake_frame` touches only the
        // `active_window` leaf Mutex.
        Scheduler::instance().set_on_frame_scheduled(Some(std::sync::Arc::new(|| {
            AppBinding::instance().wake_frame();
        })));

        // Create RendererBinding sharing the SAME PipelineOwner
        let renderer =
            RenderingFlutterBinding::new_with_pipeline(Arc::clone(&shared_pipeline_owner));

        // Create WidgetsBinding and hand it the SAME PipelineOwner the
        // renderer shares. `attach_root_widget*` bootstraps the root render
        // tree through `mount_root_with_pipeline_owner`; without the owner in
        // scope the root element mounts with no PipelineOwner and never
        // creates its RenderView — the window renders nothing.
        let widgets = WidgetsBinding::new();
        widgets.set_pipeline_owner(Arc::clone(&shared_pipeline_owner));

        Self {
            renderer: RwLock::new(renderer),
            widgets: RwLock::new(widgets),
            gestures: GestureBinding::new(),
            needs_redraw: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            frames_rendered: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            shared_pipeline_owner,
            lifecycle: Mutex::new(DefaultLifecycle::new()),
            active_window: Mutex::new(None),
        }
    }

    /// Get the singleton instance.
    ///
    /// Creates the instance on first call.
    pub fn instance() -> &'static Self {
        static INSTANCE: OnceLock<AppBinding> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            tracing::info!("Initializing AppBinding");
            AppBinding::new()
        })
    }

    /// Check if the binding is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Renderer Binding Access
    // ========================================================================

    /// Get read access to RendererBinding.
    pub fn renderer(&self) -> parking_lot::RwLockReadGuard<'_, RenderingFlutterBinding> {
        // PORT-CHECK-OK-SP6: AppBinding renderer accessor; pre-existing SP-6
        self.renderer.read()
    }

    /// Get write access to RendererBinding.
    pub fn renderer_mut(&self) -> parking_lot::RwLockWriteGuard<'_, RenderingFlutterBinding> {
        // PORT-CHECK-OK-SP6: AppBinding renderer_mut accessor; pre-existing SP-6
        self.renderer.write()
    }

    // ========================================================================
    // Widgets Binding Access
    // ========================================================================

    // PORT-TARGET: flui-app runner root-bootstrap consolidation, pending Cycle 6 element-ownership unification (V-7 deferral)
    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// # Errors
    ///
    /// Forwards every [`AttachError`](flui_view::AttachError) the
    /// underlying [`WidgetsBinding::attach_root_widget`] returns —
    /// notably [`AttachError::AlreadyAttached`](flui_view::AttachError::AlreadyAttached)
    /// when a root widget is already mounted. Callers MUST handle the
    /// `Result` (PR #119 review — copilot); the previous log-and-
    /// swallow shape hid `AlreadyAttached` (and any future variant
    /// added under the enum's `#[non_exhaustive]` cover) from the
    /// caller.
    pub fn attach_root_widget<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + Send + Sync + 'static,
    {
        let widgets = self.widgets.write();
        widgets.attach_root_widget(view)?;
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!("Root widget attached");
        Ok(())
    }

    /// Attach a root widget sizing the root view to an explicit logical
    /// `width` × `height` — the platform window's surface size.
    ///
    /// Identical to [`attach_root_widget`](Self::attach_root_widget) except the
    /// root [`RenderView`](flui_rendering::view::RenderView) is born at the real
    /// window size instead of the 800×600 fallback. This is the runner's
    /// bootstrap entry point.
    ///
    /// # Errors
    ///
    /// Forwards every [`AttachError`](flui_view::AttachError) from
    /// [`WidgetsBinding::attach_root_widget_with_size`].
    pub fn attach_root_widget_with_size<V>(
        &self,
        view: &V,
        width: f32,
        height: f32,
    ) -> Result<(), flui_view::AttachError>
    where
        V: View + Clone + Send + Sync + 'static,
    {
        let widgets = self.widgets.write();
        widgets.attach_root_widget_with_size(view, width, height)?;
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!(width, height, "Root widget attached (sized)");
        Ok(())
    }

    /// Get read access to WidgetsBinding.
    pub fn widgets(&self) -> parking_lot::RwLockReadGuard<'_, WidgetsBinding> {
        // PORT-CHECK-OK-SP6: AppBinding widgets accessor; pre-existing SP-6
        self.widgets.read()
    }

    /// Get write access to WidgetsBinding.
    pub fn widgets_mut(&self) -> parking_lot::RwLockWriteGuard<'_, WidgetsBinding> {
        // PORT-CHECK-OK-SP6: AppBinding widgets_mut accessor; pre-existing SP-6
        self.widgets.write()
    }

    // ========================================================================
    // Render Pipeline Access (for elements)
    // ========================================================================

    /// Get the Arc to RenderPipelineOwner for sharing with elements.
    ///
    /// This is used by RootRenderElement to insert RenderObjects into the tree.
    /// Elements need `Arc<RwLock<PipelineOwner>>` for concurrent access.
    pub fn render_pipeline_arc(&self) -> Arc<RwLock<flui_rendering::pipeline::PipelineOwner>> {
        // PORT-CHECK-OK-SP6: AppBinding render_pipeline_arc accessor; pre-existing SP-6
        Arc::clone(&self.shared_pipeline_owner)
    }

    /// Get read access to RenderPipelineOwner.
    pub fn render_pipeline(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, flui_rendering::pipeline::PipelineOwner> {
        self.shared_pipeline_owner.read()
    }

    /// Get write access to RenderPipelineOwner.
    pub fn render_pipeline_mut(
        &self,
    ) -> parking_lot::RwLockWriteGuard<'_, flui_rendering::pipeline::PipelineOwner> {
        self.shared_pipeline_owner.write()
    }

    // ========================================================================
    // Gesture Binding Access
    // ========================================================================

    /// Get the gesture binding.
    pub fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    // ========================================================================
    // Scheduler Access
    // ========================================================================

    /// Get the scheduler singleton.
    pub fn scheduler(&self) -> &'static Scheduler {
        Scheduler::instance()
    }

    // ========================================================================
    // Lifecycle Management
    // ========================================================================

    /// Get the current lifecycle state.
    pub fn lifecycle_state(&self) -> LifecycleState {
        self.lifecycle.lock().state()
    }

    /// Transition the lifecycle via an event.
    ///
    /// Delegates to [`DefaultLifecycle::handle_event`] and logs the transition.
    pub fn transition_lifecycle(&self, event: LifecycleEvent) {
        self.lifecycle.lock().handle_event(event);
        tracing::debug!(?event, state = ?self.lifecycle_state(), "Lifecycle transition");
    }

    /// Check if the lifecycle state allows rendering.
    pub fn should_render(&self) -> bool {
        self.lifecycle.lock().should_render()
    }

    // ========================================================================
    // Window Access
    // ========================================================================

    /// Store the active platform window.
    ///
    /// Called by the runner after all callbacks have been registered.
    pub fn set_window(&self, window: Box<dyn PlatformWindow>) {
        *self.active_window.lock() = Some(window);
        tracing::debug!("Active window stored in AppBinding");
    }

    /// Access the active window.
    ///
    /// Calls the provided function with a reference to the window.
    /// Returns `None` if no window is set.
    pub fn with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R> {
        self.active_window.lock().as_ref().map(|w| f(w.as_ref()))
    }

    // ========================================================================
    // Frame Management
    // ========================================================================

    /// Request a redraw.
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Wake the platform event loop so the next frame is rendered.
    ///
    /// Sets the `needs_redraw` atomic flag **and** calls
    /// `PlatformWindow::request_redraw()` on the active window so a
    /// quiescent winit / platform event loop wakes up and fires the
    /// `on_request_frame` callback.
    ///
    /// # Deadlock-safety
    ///
    /// This method acquires only `self.active_window` (a leaf `Mutex`)
    /// and never touches `self.widgets` or `self.inner`. It is safe to
    /// call from any context, including from inside a `build_scope`
    /// callback that is executing while `AppBinding::widgets` is held —
    /// the two locks are disjoint.
    pub fn wake_frame(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
        if let Some(()) = self.with_window(|w| w.request_redraw()) {
            tracing::trace!("wake_frame: platform window request_redraw sent");
        }
    }

    /// Check if a redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered.
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Get total frames rendered successfully.
    pub fn frames_rendered(&self) -> u64 {
        self.frames_rendered.load(Ordering::Relaxed)
    }

    /// Get frames dropped due to surface errors.
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped.load(Ordering::Relaxed)
    }

    /// Draw a frame and return Scene for GPU rendering.
    ///
    /// This executes the complete rendering pipeline:
    /// 1. Build phase (WidgetsBinding) - rebuild dirty elements
    /// 2. Layout phase - compute sizes
    /// 3. Paint phase - generate display lists
    /// 4. Create Scene from LayerTree
    ///
    /// Returns `Some(Scene)` if a new scene was produced, or cached scene
    /// otherwise.
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Option<Arc<Scene>> {
        // Phase 1: Build (WidgetsBinding)
        {
            let w = self.widgets.write();
            if w.has_pending_builds() {
                w.draw_frame();
            }
        }

        // Phase 2 & 3: Layout, Compositing, Paint, Semantics through the
        // typestate-driven orchestrator. Mythos Step 7 finalization
        // (2026-05-20): the four `flush_*` calls are gone; `run_frame`
        // is the single entry point and the layer tree comes back as
        // its second return value.
        //
        // Mythos Step 12 (2026-05-20): `run_frame` now returns
        // `(PipelineOwner<Idle>, RenderResult<Option<LayerTree>>)`. The
        // owner always comes back at Idle, so we always restore it. If
        // the frame errored (e.g. a render object panicked and was
        // caught by `catch_unwind`), we log via tracing and drop the
        // frame -- the owner is still usable for the next call.
        let layer_tree = {
            let mut guard = self.shared_pipeline_owner.write();
            // The window's constraints ARE the root constraints — without
            // this, frame 1 has neither cached state nor root_constraints
            // and run_layout drops the root dirty entry (blank window).
            // set_root_constraints marks the root dirty only on CHANGE,
            // so the per-frame call is idempotent and resize-correct.
            guard.set_root_constraints(Some(constraints));
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

        // Production↔headless convergence point: service lazy-sliver child
        // requests accumulated by `run_frame`'s layout pass. This is the
        // production equivalent of `HeadlessBinding::pump_frame` step 6.
        // Lock order: `widgets` (brief write for the split-borrow) → `pipeline`
        // (brief write inside `service_child_requests` to drain the two pending
        // Vecs). The `run_frame` pipeline write-guard above is fully released
        // before this call — no nested write. Future gap-#2 work (production
        // Vsync / implicit-animation tick) will be added HERE immediately after
        // this call, keeping the two frame paths converged.
        {
            let w = self.widgets.write();
            w.service_child_requests(&self.shared_pipeline_owner);
        }

        // Phase 4: Create Scene from LayerTree
        let size = constraints.constrain(Size::ZERO);
        let frame_number = self.frames_rendered.load(Ordering::Relaxed) + 1;

        if let Some(layer_tree) = layer_tree {
            // Create scene from layer tree. `Scene` is `Send` (auto-derived
            // from `LayerTree` + `LinkRegistry` + `Vec<CompositionCallback>`
            // whose payload is `FnOnce() + Send + 'static`) but is *not*
            // `Sync` because the `FnOnce + Send` callback payload itself is
            // not `Sync`. Making `Scene: Sync` requires either dropping the
            // composition-callback list or relaxing it to `Fn + Send + Sync`
            // — tracked under the engine composition redesign. Until then,
            // the binding thread is the sole reader of this `Arc<Scene>`,
            // so the lint is suppressed with an honest justification.
            let root = layer_tree.root();
            let scene = Scene::new(size, layer_tree, root, frame_number);
            #[expect(
                clippy::arc_with_non_send_sync,
                reason = "Scene: Send but !Sync due to CompositionCallback (FnOnce + Send + 'static, no Sync). Sole reader is the binding thread; relaxing the callback bound is tracked under the engine composition redesign."
            )]
            let arc = Arc::new(scene);
            Some(arc)
        } else {
            // No new layer tree
            None
        }
    }

    /// Render a complete frame to GPU.
    ///
    /// Orchestrates: flush_coalesced_moves → draw → render → mark_rendered
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn render_frame(&self, renderer: &mut Renderer) -> Option<Arc<Scene>> {
        // 1. Flush coalesced pointer moves (GestureBinding handles coalescing)
        self.gestures.flush_pending_moves();

        // 1b. Advance recognizer deadlines so a held-still pointer past its
        //     timeout (e.g. long press) fires without a further input event.
        self.gestures.tick_deadlines();

        // 2. Draw frame (build + layout + paint → Scene). The surface
        // reports PHYSICAL pixels; the framework lays out in LOGICAL
        // pixels — the paint root's DPR transform bridges back. A
        // physical-sized layout at DPR 2 would paint everything double
        // size (the "red box covers a quarter of the window" bug).
        let (width, height) = renderer.size();
        let dpr = self.shared_pipeline_owner.read().device_pixel_ratio();
        let constraints =
            BoxConstraints::tight(Size::new(px(width as f32 / dpr), px(height as f32 / dpr)));
        let scene = self.draw_frame(constraints);

        // 3. Render scene to GPU
        if let Some(ref scene) = scene
            && scene.has_content()
        {
            // The pipeline painted a FRESH scene this frame, so the
            // on-screen content is stale. The engine's damage tracker is
            // only marked by resize/surface-create paths; without this
            // mark, `render_scene` early-returns on "no damage" and every
            // animation frame is silently dropped — the screen then only
            // updates on resize. Until fine-grained damage from the layer
            // diff lands, a new scene is a full repaint.
            renderer.mark_full_repaint();
            match renderer.render_scene(scene) {
                Ok(()) => {
                    self.frames_rendered.fetch_add(1, Ordering::Relaxed);
                    tracing::trace!(
                        frame = scene.frame_number(),
                        total = self.frames_rendered.load(Ordering::Relaxed),
                        "Frame rendered successfully"
                    );
                }
                Err(EngineError::SurfaceLost) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Surface lost, will retry next frame");
                }
                Err(EngineError::DeviceLost) => {
                    // GPU device lost (TDR / driver crash / GPU switch). Recovery
                    // requires rebuilding the entire GPU context asynchronously; it
                    // is handled by the platform runner after `render_frame` returns.
                    // `render_frame` itself has no async context and no raw window
                    // handle, so it must not attempt recovery here.
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        "GPU device lost — recovery will be attempted by the platform runner"
                    );
                }
                Err(EngineError::SurfaceValidation) => {
                    // Surface misconfig (wgpu Validation). Drop this frame and
                    // log; reconfiguration is NOT automatic — it requires an
                    // external trigger (window resize / surface recreate).
                    // `render_scene` only reconfigures in the Outdated/Lost arm,
                    // so without such a trigger this would drop + error-log
                    // every frame. We do not retry blindly: re-reconfiguring the
                    // same bad config would re-validate and loop forever.
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::error!(
                        "Surface validation error — surface misconfig; external reconfigure required"
                    );
                }
                Err(e) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::error!(error = ?e, "Render error (non-recoverable this frame)");
                }
            }
        }

        // 4. Mark rendered
        self.mark_rendered();

        scene
    }

    /// Check if there is pending work.
    pub fn has_pending_work(&self) -> bool {
        self.widgets.read().has_pending_builds()
            || self.shared_pipeline_owner.read().has_dirty_nodes()
    }

    // ========================================================================
    // Input Handling
    // ========================================================================

    /// Handle a platform input event.
    ///
    /// This is the single entry point for all input from the platform layer.
    /// Routes pointer events to `GestureBinding` and keyboard events to
    /// `FocusManager`.
    ///
    /// Pointer events are coalesced by `GestureBinding` — high-frequency move
    /// events are stored and flushed once per frame via
    /// `flush_pending_moves()` in `render_frame()`.
    pub fn handle_input(&self, input: PlatformInput) {
        match input {
            PlatformInput::Pointer(pointer_event) => {
                self.gestures
                    .handle_pointer_event(&pointer_event, |position| {
                        // A single canonical `HitTestResult` flows through both
                        // rendering traversal and gesture dispatch: the
                        // rendering crate re-exports
                        // `flui_interaction::routing::HitTestResult`, so the
                        // same instance crosses both layers without conversion
                        // (no per-hit bridge that could silently drop targets).
                        use flui_rendering::binding::RendererBinding;
                        let renderer = self.renderer.read();
                        let mut result = flui_interaction::routing::HitTestResult::new();
                        let offset = flui_types::Offset::new(position.dx, position.dy);
                        renderer.hit_test_in_view(&mut result, offset, 0);
                        if !result.is_empty() {
                            tracing::debug!(hits = result.len(), "Hit test found targets");
                        }
                        result
                    });
                self.request_redraw();
            }
            PlatformInput::Keyboard(keyboard_event) => {
                FocusManager::global().dispatch_key_event(&keyboard_event);
                self.request_redraw();
            }
        }
    }
}

impl std::fmt::Debug for AppBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBinding")
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .field("needs_redraw", &self.needs_redraw.load(Ordering::Relaxed))
            .field("renderer", &*self.renderer.read())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singleton() {
        let binding1 = AppBinding::instance();
        let binding2 = AppBinding::instance();
        assert!(std::ptr::eq(binding1, binding2));
    }

    /// Idle-wake wiring smoke test: a dirty mark on the shared
    /// pipeline owner must reach `AppBinding::wake_frame` through the
    /// visual-update notifier set in `AppBinding::new`, flipping
    /// `needs_redraw` so the platform loop produces a frame.
    #[test]
    fn dirty_mark_fires_wake_via_notifier() {
        let binding = AppBinding::instance();

        let id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        binding.mark_rendered();

        binding.shared_pipeline_owner.write().mark_needs_layout(id);
        assert!(
            binding.needs_redraw(),
            "an owner dirty mark must wake the binding via the \
             visual-update notifier wired in AppBinding::new",
        );
    }

    #[test]
    fn test_needs_redraw() {
        let binding = AppBinding::instance();

        binding.mark_rendered();
        assert!(!binding.needs_redraw());

        binding.request_redraw();
        assert!(binding.needs_redraw());

        binding.mark_rendered();
        assert!(!binding.needs_redraw());
    }

    #[test]
    fn test_renderer_initialized() {
        let binding = AppBinding::instance();
        // Verify the renderer sub-binding is accessible (created during
        // AppBinding::new)
        let _renderer = binding.renderer();
    }

    /// Minimal leaf view/element so a headless `attach_root_widget` has
    /// something to mount without pulling in a widget crate.
    #[derive(Clone)]
    struct LeafView;

    impl View for LeafView {
        fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
            Box::new(LeafElement {
                lifecycle: flui_view::element::Lifecycle::Initial,
            })
        }
    }

    struct LeafElement {
        lifecycle: flui_view::element::Lifecycle,
    }

    impl flui_view::ElementBase for LeafElement {
        fn view_type_id(&self) -> std::any::TypeId {
            std::any::TypeId::of::<LeafView>()
        }
        fn depth(&self) -> usize {
            0
        }
        fn lifecycle(&self) -> flui_view::element::Lifecycle {
            self.lifecycle
        }
        fn mount(
            &mut self,
            _parent: Option<flui_foundation::ElementId>,
            _slot: usize,
            _owner: &mut flui_view::ElementOwner<'_>,
        ) {
            self.lifecycle = flui_view::element::Lifecycle::Active;
        }
        fn unmount(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {
            self.lifecycle = flui_view::element::Lifecycle::Defunct;
        }
        fn activate(&mut self) {
            self.lifecycle = flui_view::element::Lifecycle::Active;
        }
        fn deactivate(&mut self) {
            self.lifecycle = flui_view::element::Lifecycle::Inactive;
        }
        fn update(&mut self, _new: &dyn View, _owner: &mut flui_view::ElementOwner<'_>) {}
        fn mark_needs_build(&mut self) {}
        fn build_into_views(
            &mut self,
            _owner: &mut flui_view::ElementOwner<'_>,
        ) -> Vec<Box<dyn View>> {
            Vec::new()
        }
    }

    /// E2/E3 regression: `AppBinding` hands its shared `PipelineOwner` to the
    /// `WidgetsBinding` it owns, so `attach_root_widget` actually bootstraps
    /// the root render tree. Without that wiring the root mounts with no
    /// PipelineOwner, no `RenderView` is created, and the window renders
    /// nothing — the shared owner's root id stays `None`.
    #[test]
    fn attach_root_widget_bootstraps_shared_render_tree() {
        let app = AppBinding::new();
        app.attach_root_widget(&LeafView).expect("attach succeeds");
        assert!(
            app.shared_pipeline_owner.read().root_id().is_some(),
            "AppBinding must pass its PipelineOwner to the widgets binding so the \
             root render tree bootstraps; without it the window renders nothing",
        );
    }

    // ========================================================================
    // E0a — wake_frame
    // ========================================================================

    /// `wake_frame` must set `needs_redraw` even when no window is stored
    /// (the window lock is a leaf that is independently optional).
    #[test]
    fn wake_frame_sets_needs_redraw_without_window() {
        // Use a fresh binding rather than the singleton so this test does not
        // race with other tests' redraw state.
        let binding = AppBinding::new();
        binding.mark_rendered();
        assert!(!binding.needs_redraw(), "precondition: no redraw pending");

        // No window installed — wake_frame must still set the atomic.
        binding.wake_frame();

        assert!(
            binding.needs_redraw(),
            "wake_frame must set needs_redraw even without an active window"
        );
    }

    /// `wake_frame` must call `PlatformWindow::request_redraw` when a window
    /// is installed, and must NOT acquire `widgets` or `inner`.
    ///
    /// A minimal inline mock records how many times `request_redraw` was
    /// called without touching any binding lock.
    #[test]
    fn wake_frame_calls_platform_request_redraw() {
        use std::sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        };

        use flui_platform::traits::PlatformWindow;
        use flui_types::geometry::{DevicePixels, Pixels, Size, device_px, px};

        struct CountingWindow {
            redraw_count: Arc<AtomicU32>,
        }

        impl PlatformWindow for CountingWindow {
            fn physical_size(&self) -> Size<DevicePixels> {
                Size::new(device_px(800), device_px(600))
            }
            fn logical_size(&self) -> Size<Pixels> {
                Size::new(px(800.0), px(600.0))
            }
            fn scale_factor(&self) -> f64 {
                1.0
            }
            fn request_redraw(&self) {
                self.redraw_count.fetch_add(1, Ordering::Relaxed);
            }
            fn is_focused(&self) -> bool {
                false
            }
            fn is_visible(&self) -> bool {
                true
            }
            // Trait default impls cover the remaining callback-registration
            // methods; only the required methods above need bodies.
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let redraw_count = Arc::new(AtomicU32::new(0));
        let window = CountingWindow {
            redraw_count: Arc::clone(&redraw_count),
        };

        let binding = AppBinding::new();
        binding.mark_rendered();
        binding.set_window(Box::new(window));

        binding.wake_frame();

        assert!(binding.needs_redraw(), "wake_frame must set needs_redraw");
        assert_eq!(
            redraw_count.load(Ordering::Relaxed),
            1,
            "wake_frame must call PlatformWindow::request_redraw exactly once"
        );
    }

    /// `wake_frame` must be callable while `widgets` read-lock is held on
    /// the same thread — proving the implementation does not acquire
    /// `widgets` or `inner`.
    ///
    /// parking_lot's RwLock is non-reentrant: a read-under-existing-read on
    /// the same thread upgrades correctly but a write attempt deadlocks.
    /// Holding the read guard here would expose any hidden write attempt.
    #[test]
    fn wake_frame_does_not_acquire_widgets_lock() {
        let binding = AppBinding::new();
        binding.mark_rendered();

        // Hold widgets read-lock across the call.
        let _guard = binding.widgets.read();
        // Must return without deadlocking.
        binding.wake_frame();

        assert!(
            binding.needs_redraw(),
            "wake_frame must set needs_redraw even while widgets is read-locked"
        );
    }

    #[test]
    fn input_dispatches_through_the_exposed_gesture_binding() {
        use std::sync::atomic::{AtomicBool, Ordering};

        use flui_interaction::PointerId;
        use flui_interaction::events::{PointerEvent, PointerType, make_down_event_for_id};
        use flui_interaction::routing::HitTestResult;
        use flui_types::geometry::{Offset, Pixels};

        // A handler registered on the gesture binding the public accessor
        // exposes must observe an event dispatched through that same binding —
        // proving registration and dispatch share ONE authoritative gesture
        // binding / arena, with no separate global instance to diverge.
        let app = AppBinding::instance();

        // A test-local pointer id keeps this isolated from other tests that
        // share the `AppBinding` singleton (its arena / router / hit-test maps),
        // and a per-pointer route is scoped to that id (unlike a global handler).
        let pointer = PointerId::new(9001).expect("nonzero pointer id");

        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        let handler: Arc<dyn Fn(&PointerEvent) + Send + Sync> =
            Arc::new(move |_| f.store(true, Ordering::Relaxed));
        app.gestures().pointer_router().add_route(pointer, handler);

        // Dispatch straight through the accessor-exposed binding via the
        // explicit-result path, which bypasses hit testing (no renderer lock,
        // no simultaneous-pointer cap that other tests could exhaust).
        let event = make_down_event_for_id(
            pointer,
            Offset::new(Pixels(10.0), Pixels(10.0)),
            PointerType::Touch,
        );
        app.gestures()
            .handle_pointer_event_with_result(&event, &HitTestResult::new());

        assert!(
            fired.load(Ordering::Relaxed),
            "the binding AppBinding::gestures() exposes must dispatch the event it is handed"
        );

        // Shared process singleton — clean up this pointer's route + arena entry.
        app.gestures().pointer_router().remove_all_routes(pointer);
        app.gestures().sweep_arena(pointer);
    }

    // ========================================================================
    // U4.4 — service_child_requests wiring tests
    // ========================================================================

    /// Wiring test: `AppBinding::draw_frame` must invoke
    /// `WidgetsBinding::service_child_requests`, which drains the pipeline's
    /// `pending_child_requests` buffer. We verify the drain happened by:
    ///   1. Seeding one request via `push_pending_child_request_for_test`
    ///      (`#[cfg(test)]` helper on `PipelineOwner`).
    ///   2. Running one `draw_frame`.
    ///   3. Asserting the buffer is now empty — `take_pending_child_requests`
    ///      was called, proving the wiring is present.
    ///
    /// Without the `service_child_requests` call at ~line 460 of `draw_frame`,
    /// `take_pending_child_requests` is never called and the buffer remains
    /// non-empty after the frame. The test is RED without step-2 and GREEN with
    /// it; no root attach is needed.
    #[test]
    fn draw_frame_invokes_service_child_requests() {
        // A fresh binding so we avoid the singleton root-attach collision.
        let binding = AppBinding::new();

        // Insert a dummy render object to obtain a valid RenderId (the pending
        // buffer stores `(RenderId, index)` pairs — any valid id works).
        let sliver_id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);

        // Seed one pending child-build request. The seeding helper is gated
        // behind flui-rendering's `testing` feature (enabled for this crate's
        // dev builds) — the test-only mirror of `SubtreeArena::request_child_build`.
        binding
            .shared_pipeline_owner
            .write()
            .push_pending_child_request_for_test(sliver_id, 0);

        // Verify the seed is present (precondition).
        {
            let mut guard = binding.shared_pipeline_owner.write();
            let drained = guard.take_pending_child_requests();
            assert_eq!(drained.len(), 1, "seed must be present before draw_frame");
            // Re-push so draw_frame sees it.
            guard.push_pending_child_request_for_test(sliver_id, 0);
        }

        // Run one draw_frame.  No root render object is attached (fresh binding)
        // so no scene is produced, but the service path must still be traversed.
        let _ = binding.draw_frame(flui_rendering::constraints::BoxConstraints::tight(
            flui_types::Size::new(
                flui_types::geometry::px(800.0),
                flui_types::geometry::px(600.0),
            ),
        ));

        // After draw_frame the pending buffer must be empty — drained by
        // `service_child_requests`.  Without the wiring the buffer is never
        // drained and this take returns a non-empty vec.
        let remaining = binding
            .shared_pipeline_owner
            .write()
            .take_pending_child_requests();
        assert!(
            remaining.is_empty(),
            "draw_frame must drain pending_child_requests via service_child_requests; \
             {} request(s) remained undrained — wiring is absent",
            remaining.len(),
        );
    }

    /// Wake-gate contract: after a frame marks a render node dirty (simulating
    /// what `service_child_requests` does to a sliver after building new
    /// children), `has_pending_work()` must return `true` so the runner gate
    /// (`runner.rs:225`: `needs_redraw() || has_pending_work()`) schedules the
    /// settling frame.
    ///
    /// Also asserts the quiescence direction: once no nodes are dirty,
    /// `has_pending_work()` is `false` and the app can go idle.
    ///
    /// # Wake-gate invariant
    ///
    /// The settling frame survives because `layout` marks the sliver dirty
    /// (`has_dirty_nodes`), NOT because the pending-request buffer is non-empty
    /// (`has_pending_work` does not consult `pending_child_requests` or
    /// `pending_retain_bands`). A future change that emits a child request
    /// WITHOUT calling `mark_needs_layout` would strand the settling frame —
    /// this test documents and guards that invariant.
    #[test]
    fn wake_gate_schedules_settling_frame_after_dirty_mark() {
        let binding = AppBinding::new();

        // `mark_rendered` puts `needs_redraw` in a known state so the
        // has_pending_work assertion is insulated from any prior redraw state
        // set by other singleton tests sharing the same process.
        binding.mark_rendered();
        assert!(!binding.needs_redraw(), "precondition: needs_redraw clear");

        // Insert a node and mark it dirty — this is what service_child_requests
        // does to a sliver after building new children.
        let node_id = binding.shared_pipeline_owner.write().insert(Box::new(
            flui_objects::RenderColoredBox::red(10.0, 10.0),
        )
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        // Confirm quiescence baseline: nothing dirty yet.
        assert!(
            !binding.has_pending_work(),
            "baseline: no pending work after clearing dirty nodes",
        );

        // Mark the node dirty (as service_child_requests does after building children).
        binding
            .shared_pipeline_owner
            .write()
            .mark_needs_layout(node_id);

        // The runner gate reads has_pending_work(); it must be true now.
        assert!(
            binding.has_pending_work(),
            "a dirty layout node must make has_pending_work() true so the runner \
             schedules the settling frame; this is the invariant that lazy-list \
             settling depends on (NOT the pending_child_requests buffer)",
        );

        // Once all dirty nodes are cleared (settled frame ran layout), the app
        // must go idle — no infinite redraw.
        binding
            .shared_pipeline_owner
            .write()
            .clear_all_dirty_nodes();
        assert!(
            !binding.has_pending_work(),
            "after clearing dirty nodes has_pending_work() must be false so a \
             settled lazy-list app does not loop forever",
        );
    }
}
