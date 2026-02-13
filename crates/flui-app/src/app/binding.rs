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
//!   ├── gestures: GestureBinding       (hit testing, gestures)
//!   ├── scheduler: Scheduler           (frame callbacks)
//!   └── pointer_state: PointerState    (event coalescing)
//! ```

use crate::bindings::RenderingFlutterBinding;
use crate::embedder::{FrameCoordinator, PointerState};
use flui_engine::wgpu::Renderer;
use flui_foundation::HasInstance;
use flui_interaction::binding::GestureBinding;
use flui_interaction::events::{
    make_pointer_event, Event, PointerButton, PointerEventData, PointerEventKind, PointerType,
};
use flui_layer::Scene;
use flui_rendering::constraints::BoxConstraints;
use flui_scheduler::Scheduler;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{ElementBase, View, WidgetsBinding};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// Combined application binding.
///
/// AppBinding is the central coordinator for the FLUI framework.
/// It composes all the specialized bindings:
/// - [`RendererBinding`] - Manages render tree and pipeline
/// - [`WidgetsBinding`] - Manages element tree and build phase
/// - [`GestureBinding`] - Manages hit testing and gestures
/// - [`Scheduler`] - Manages frame scheduling
///
/// # Thread Safety
///
/// AppBinding is a singleton accessed via `instance()`. It uses internal
/// locking for thread-safe access to mutable state.
///
/// # Example
///
/// ```rust,ignore
/// let binding = AppBinding::instance();
/// binding.attach_root_widget(&MyApp);
/// let scene = binding.draw_frame(constraints);
/// ```
pub struct AppBinding {
    /// Renderer binding (render tree, layout/paint phases)
    renderer: RwLock<RenderingFlutterBinding>,

    /// Widgets binding (element tree, build phase)
    widgets: RwLock<WidgetsBinding>,

    /// Gesture binding (input handling, hit testing)
    gestures: GestureBinding,

    /// Frame coordinator (tracks frame statistics)
    frame_coordinator: RwLock<FrameCoordinator>,

    /// Pointer state (event coalescing)
    pointer_state: RwLock<PointerState>,

    /// Whether a redraw is needed
    needs_redraw: AtomicBool,

    /// Whether the app is initialized
    initialized: AtomicBool,

    /// Shared pipeline owner for elements (wrapped in Arc for sharing)
    /// This is the same PipelineOwner as in RendererBinding, but wrapped
    /// for sharing with elements that need `Arc<RwLock<PipelineOwner>>`.
    shared_pipeline_owner: Arc<RwLock<flui_rendering::pipeline::PipelineOwner>>,

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

        // Create and initialize RendererBinding
        let renderer = RenderingFlutterBinding::new();
        // RenderingFlutterBinding::new() already calls init_instances()

        // Create WidgetsBinding
        let mut widgets = WidgetsBinding::new();

        // Wire up frame scheduling: when widgets need rebuild, request redraw
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let needs_redraw_clone = needs_redraw.clone();
        widgets.set_on_need_frame(move || {
            needs_redraw_clone.store(true, Ordering::Relaxed);
        });

        Self {
            renderer: RwLock::new(renderer),
            widgets: RwLock::new(widgets),
            gestures: GestureBinding::new(),
            frame_coordinator: RwLock::new(FrameCoordinator::new()),
            pointer_state: RwLock::new(PointerState::new()),
            needs_redraw: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            shared_pipeline_owner,
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

    /// Get the cached scene for hit testing.
    ///
    /// Returns the most recent scene if available.
    /// TODO: Implement scene caching in RenderingFlutterBinding
    pub fn cached_scene(&self) -> Option<Arc<Scene>> {
        None // Scene caching not yet implemented in RenderingFlutterBinding
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
        let mut widgets = self.widgets.write();
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
    /// recursively rebuild the entire widget tree.
    pub fn rebuild_root(&self) {
        tracing::trace!("rebuild_root: acquiring lock");
        let mut root = self.root_element.lock();
        tracing::trace!("rebuild_root: lock acquired");
        if let Some(ref mut element) = *root {
            element.mark_needs_build();
            tracing::trace!("rebuild_root: calling perform_build");
            element.perform_build();
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

    /// Draw a frame and return Scene for GPU rendering.
    ///
    /// This executes the complete rendering pipeline:
    /// 1. Build phase (WidgetsBinding) - rebuild dirty elements
    /// 2. Layout phase - compute sizes
    /// 3. Paint phase - generate display lists
    /// 4. Create Scene from LayerTree
    ///
    /// Returns `Some(Scene)` if a new scene was produced, or cached scene otherwise.
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Option<Arc<Scene>> {
        // Phase 1: Build (WidgetsBinding)
        {
            let mut widgets = self.widgets.write();
            if widgets.has_pending_builds() {
                widgets.draw_frame();
            }
        }

        // Phase 2 & 3: Layout and Paint (using shared_pipeline_owner)
        {
            let mut pipeline = self.shared_pipeline_owner.write();
            pipeline.flush_layout();
            pipeline.flush_compositing_bits();
            pipeline.flush_paint();
            pipeline.flush_semantics();
        }

        // Phase 4: Create Scene from LayerTree
        let size = constraints.constrain(Size::ZERO);
        let frame_number = self.frame_coordinator.read().frames_rendered() + 1;

        let mut pipeline = self.shared_pipeline_owner.write();
        if let Some(layer_tree) = pipeline.take_layer_tree() {
            // Create scene from layer tree
            let root = layer_tree.root();
            let scene = Scene::new(size, layer_tree, root, frame_number);
            Some(Arc::new(scene))
        } else {
            // No new layer tree
            None
        }
    }

    /// Render a complete frame to GPU.
    ///
    /// Orchestrates: process_events → draw → render → mark_rendered
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn render_frame(&self, renderer: &mut Renderer) -> Option<Arc<Scene>> {
        // 1. Process coalesced pointer moves
        self.process_pending_events();

        // 2. Draw frame (build + layout + paint → Scene)
        let (width, height) = renderer.size();
        let constraints = BoxConstraints::tight(Size::new(px(width as f32), px(height as f32)));
        let scene = self.draw_frame(constraints);

        // 3. Render scene to GPU
        if let Some(ref scene) = scene {
            let mut coordinator = self.frame_coordinator.write();
            let _result = coordinator.render_scene(renderer, scene);
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
    // Event Handling
    // ========================================================================

    /// Process pending coalesced events.
    fn process_pending_events(&self) {
        let event = self.pointer_state.write().take_pending_move();
        if let Some(event) = event {
            self.route_event(event);
        }
    }

    /// Route event through hit testing.
    fn route_event(&self, event: Event) {
        // For pointer events, use GestureBinding's hit test system
        if let Event::Pointer(ref pointer_event) = event {
            self.gestures
                .handle_pointer_event(pointer_event, |_position| {
                    // TODO: Implement proper hit testing through scene/render tree
                    flui_interaction::routing::HitTestResult::new()
                });
        }
    }

    /// Handle cursor/touch move (coalesced).
    pub fn handle_pointer_move(&self, position: Offset, device: PointerType) {
        self.pointer_state.write().update_position(position, device);
    }

    /// Handle pointer button (mouse click / touch).
    pub fn handle_pointer_button(
        &self,
        position: Offset,
        device: PointerType,
        _button: PointerButton,
        is_down: bool,
    ) {
        let data = PointerEventData::new(position, device);

        let kind = if is_down {
            self.pointer_state.write().set_down(true);
            PointerEventKind::Down
        } else {
            self.pointer_state.write().set_down(false);
            PointerEventKind::Up
        };

        let pointer_event = make_pointer_event(kind, data);
        let event = Event::Pointer(pointer_event);
        self.route_event(event);
    }

    /// Handle keyboard event.
    pub fn handle_key_event(&self, key_event: flui_interaction::events::KeyboardEvent) {
        let event = Event::Keyboard(key_event);
        self.route_event(event);
    }

    /// Handle scroll event.
    pub fn handle_scroll_event(&self, scroll_event: flui_interaction::events::ScrollEventData) {
        let event = Event::Scroll(scroll_event);
        self.route_event(event);
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
        // Verify the renderer sub-binding is accessible (created during AppBinding::new)
        let _renderer = binding.renderer();
    }
}
