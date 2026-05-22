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
use flui_view::{ElementBase, View, WidgetsBinding};
use parking_lot::{Mutex, RwLock};

use crate::{
    app::lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle},
    bindings::RenderingFlutterBinding,
};

/// Combined application binding.
///
/// AppBinding is the central coordinator for the FLUI framework.
/// It composes all the specialized bindings:
/// - [`RendererBinding`] - Manages render tree and pipeline
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

    /// Root element stored for rebuild support.
    /// This is set by `set_root_element()` and rebuilt by `rebuild_root()`.
    root_element: Mutex<Option<Box<dyn ElementBase>>>,
}

impl AppBinding {
    /// Create a new AppBinding.
    fn new() -> Self {
        // Ensure the global Scheduler singleton is initialized
        let _ = Scheduler::instance();

        // Create shared pipeline owner first (elements need Arc access)
        let shared_pipeline_owner =
            Arc::new(RwLock::new(flui_rendering::pipeline::PipelineOwner::new()));

        // Create RendererBinding sharing the SAME PipelineOwner
        let renderer =
            RenderingFlutterBinding::new_with_pipeline(Arc::clone(&shared_pipeline_owner));

        // Create WidgetsBinding
        let widgets = WidgetsBinding::new();

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
            root_element: Mutex::new(None),
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
        self.renderer.read()
    }

    /// Get write access to RendererBinding.
    pub fn renderer_mut(&self) -> parking_lot::RwLockWriteGuard<'_, RenderingFlutterBinding> {
        self.renderer.write()
    }

    // ========================================================================
    // Widgets Binding Access
    // ========================================================================

    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached.
    pub fn attach_root_widget<V: View>(&self, view: &V) {
        let widgets = self.widgets.write();
        widgets.attach_root_widget(view);
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!("Root widget attached");
    }

    /// Get read access to WidgetsBinding.
    pub fn widgets(&self) -> parking_lot::RwLockReadGuard<'_, WidgetsBinding> {
        self.widgets.read()
    }

    /// Get write access to WidgetsBinding.
    pub fn widgets_mut(&self) -> parking_lot::RwLockWriteGuard<'_, WidgetsBinding> {
        self.widgets.write()
    }

    // ========================================================================
    // Root Element Management
    // ========================================================================

    /// Store a root element for rebuild support.
    ///
    /// This should be called by the app runner after creating the root element.
    /// The stored element will be rebuilt when `rebuild_root()` is called.
    pub fn set_root_element(&self, element: Box<dyn ElementBase>) {
        let mut root = self.root_element.lock();
        *root = Some(element);
        tracing::debug!("Root element stored in AppBinding");
    }

    /// Take the root element out of storage.
    ///
    /// Returns the stored root element, leaving None in its place.
    pub fn take_root_element(&self) -> Option<Box<dyn ElementBase>> {
        self.root_element.lock().take()
    }

    /// Rebuild the stored root element.
    ///
    /// This triggers `perform_build()` on the root element which will
    /// recursively rebuild the entire widget tree. Acquires an
    /// [`ElementOwner`](flui_view::ElementOwner) split-borrow handle
    /// from `WidgetsBinding`'s `BuildOwner` for the duration of the
    /// build so descendants can register `GlobalKey`s / schedule
    /// rebuilds (plan §U8).
    pub fn rebuild_root(&self) {
        tracing::trace!("rebuild_root: acquiring lock");
        let mut root = self.root_element.lock();
        tracing::trace!("rebuild_root: lock acquired");
        if let Some(ref mut element) = *root {
            element.mark_needs_build();
            tracing::trace!("rebuild_root: calling perform_build");
            let widgets = self.widgets();
            widgets.with_build_owner_mut(|build_owner| {
                element.perform_build(&mut build_owner.element_owner_mut());
            });
            tracing::debug!("Root element rebuilt");
        } else {
            tracing::warn!("rebuild_root called but no root element stored");
        }
        tracing::trace!("rebuild_root: complete");
    }

    // ========================================================================
    // Render Pipeline Access (for elements)
    // ========================================================================

    /// Get the Arc to RenderPipelineOwner for sharing with elements.
    ///
    /// This is used by RootRenderElement to insert RenderObjects into the tree.
    /// Elements need `Arc<RwLock<PipelineOwner>>` for concurrent access.
    pub fn render_pipeline_arc(&self) -> Arc<RwLock<flui_rendering::pipeline::PipelineOwner>> {
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

        // 2. Draw frame (build + layout + paint → Scene)
        let (width, height) = renderer.size();
        let constraints = BoxConstraints::tight(Size::new(px(width as f32), px(height as f32)));
        let scene = self.draw_frame(constraints);

        // 3. Render scene to GPU
        if let Some(ref scene) = scene
            && scene.has_content()
        {
            match renderer.render_scene(scene) {
                Ok(()) => {
                    self.frames_rendered.fetch_add(1, Ordering::Relaxed);
                    tracing::trace!(
                        frame = scene.frame_number(),
                        total = self.frames_rendered.load(Ordering::Relaxed),
                        "Frame rendered successfully"
                    );
                }
                Err(EngineError::SurfaceLost) | Err(EngineError::SurfaceOutdated) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!("Surface lost/outdated, will retry next frame");
                }
                Err(e) => {
                    self.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::error!("Render error: {:?}", e);
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
                        // Perform rendering-layer hit test through the RenderView
                        use flui_rendering::binding::RendererBinding;
                        let renderer = self.renderer.read();
                        let mut render_result = flui_rendering::hit_testing::HitTestResult::new();
                        let offset = flui_types::Offset::new(position.dx, position.dy);
                        renderer.hit_test_in_view(&mut render_result, offset, 0);

                        // Bridge to interaction-layer result
                        // TODO: Convert rendering HitTestEntry targets to interaction targets
                        // once render objects implement gesture handling
                        let result = flui_interaction::routing::HitTestResult::new();
                        if !render_result.is_empty() {
                            tracing::debug!(hits = render_result.len(), "Hit test found targets");
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
}
