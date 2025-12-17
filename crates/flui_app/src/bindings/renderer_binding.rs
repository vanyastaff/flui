//! RendererBinding - manages the render tree and pipeline.
//!
//! This is the Rust equivalent of Flutter's `RendererBinding` mixin.
//! It owns the `PipelineOwner` and manages the rendering phases.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `RendererBinding` mixin in `rendering/binding.dart`.
//!
//! # Responsibilities
//!
//! - Owns the root `PipelineOwner`
//! - Manages collection of `RenderView`s
//! - Executes rendering phases: layout → compositing bits → paint → semantics
//! - Creates `Scene` from `LayerTree`

use super::traits::{Binding, RendererBindingBehavior};
use crate::embedder::{FrameCoordinator, SceneCache};
use flui_layer::Scene;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// A render view managed by the RendererBinding.
///
/// This corresponds to Flutter's `RenderView` class.
/// Each render view represents a separate render tree rooted at a RenderBox.
#[derive(Debug)]
pub struct RenderView {
    /// Unique identifier for this view.
    pub id: u64,

    /// Size of this view.
    pub size: Size,

    /// Whether this view is attached to the binding.
    pub attached: bool,
}

impl RenderView {
    /// Creates a new render view with the given ID and size.
    pub fn new(id: u64, size: Size) -> Self {
        Self {
            id,
            size,
            attached: false,
        }
    }
}

/// RendererBinding - manages the render tree and pipeline.
///
/// This binding owns the `PipelineOwner` and coordinates the rendering phases.
/// It implements both [`Binding`] and [`RendererBindingBehavior`] traits.
///
/// # Frame Production
///
/// Call [`draw_frame`](Self::draw_frame) each frame to execute:
/// 1. Layout phase - compute sizes and positions
/// 2. Compositing bits phase - determine layer requirements
/// 3. Paint phase - generate display lists
/// 4. Semantics phase - build accessibility tree
///
/// # Example
///
/// ```rust,ignore
/// let mut renderer = RendererBinding::new();
/// renderer.init_instances();
///
/// // Each frame:
/// renderer.draw_frame();
/// if let Some(layer_tree) = renderer.take_layer_tree() {
///     let scene = renderer.create_scene(layer_tree, size, frame_number);
///     // Send scene to GPU
/// }
/// ```
pub struct RendererBinding {
    /// Root pipeline owner - manages the render tree
    root_pipeline_owner: PipelineOwner,

    /// Render views managed by this binding (view_id → RenderView)
    render_views: HashMap<u64, RenderView>,

    /// Scene cache for hit testing
    scene_cache: SceneCache,

    /// Frame coordinator (tracks frame statistics)
    frame_coordinator: RwLock<FrameCoordinator>,

    /// Whether the binding has been initialized
    initialized: bool,

    /// Whether the first frame has been sent
    first_frame_sent: bool,

    /// Count of deferred first frame requests
    first_frame_deferred_count: u32,
}

impl std::fmt::Debug for RendererBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RendererBinding")
            .field("initialized", &self.initialized)
            .field("first_frame_sent", &self.first_frame_sent)
            .field("render_views_count", &self.render_views.len())
            .field("has_scene", &self.scene_cache.has_scene())
            .finish()
    }
}

impl Default for RendererBinding {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Binding trait implementation
// ============================================================================

impl Binding for RendererBinding {
    fn init_instances(&mut self) {
        tracing::debug!("RendererBinding::init_instances");
        self.initialized = true;

        // In Flutter, this also attaches the pipeline owner to a manifold
        // that provides request_visual_update() and semantics_enabled()
    }

    fn init_service_extensions(&mut self) {
        // Debug service extensions for rendering
        // - debugDumpRenderTree
        // - debugDumpLayerTree
        // - debugPaint, debugRepaintRainbow, etc.
        tracing::debug!("RendererBinding::init_service_extensions");
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn perform_reassemble(&mut self) {
        // Force repaint of all render views
        for view in self.render_views.values_mut() {
            view.attached = false;
        }
        tracing::debug!("RendererBinding::perform_reassemble - marked views for repaint");
    }
}

// ============================================================================
// RendererBindingBehavior trait implementation
// ============================================================================

impl RendererBindingBehavior for RendererBinding {
    type PipelineOwner = PipelineOwner;
    type RenderView = RenderView;

    fn root_pipeline_owner(&self) -> &Self::PipelineOwner {
        &self.root_pipeline_owner
    }

    fn root_pipeline_owner_mut(&mut self) -> &mut Self::PipelineOwner {
        &mut self.root_pipeline_owner
    }

    fn render_views(&self) -> impl Iterator<Item = &Self::RenderView> {
        self.render_views.values()
    }

    fn add_render_view(&mut self, view: Self::RenderView) {
        let view_id = view.id;
        tracing::debug!("RendererBinding::add_render_view id={}", view_id);
        self.render_views.insert(view_id, view);
    }

    fn remove_render_view(&mut self, view_id: u64) {
        tracing::debug!("RendererBinding::remove_render_view id={}", view_id);
        self.render_views.remove(&view_id);
    }

    fn draw_frame(&mut self) {
        // Phase 1: Layout
        self.root_pipeline_owner.flush_layout();

        // Phase 2: Compositing bits
        self.root_pipeline_owner.flush_compositing_bits();

        // Phase 3: Paint
        self.root_pipeline_owner.flush_paint();

        // Phase 4: Composite and send to GPU (for each render view)
        if self.send_frames_to_engine() {
            // In Flutter: for each renderView, call renderView.compositeFrame()
            // This creates Scene from LayerTree and sends to engine
            self.first_frame_sent = true;
        }

        // Phase 5: Semantics
        self.root_pipeline_owner.flush_semantics();
    }

    fn handle_metrics_changed(&mut self) {
        tracing::debug!("RendererBinding::handle_metrics_changed");
        // Update configurations for all render views
        // In Flutter: view.configuration = createViewConfigurationFor(view)
    }
}

// ============================================================================
// RendererBinding specific methods
// ============================================================================

impl RendererBinding {
    /// Creates a new renderer binding.
    pub fn new() -> Self {
        Self {
            root_pipeline_owner: PipelineOwner::new(),
            render_views: HashMap::new(),
            scene_cache: SceneCache::new(),
            frame_coordinator: RwLock::new(FrameCoordinator::new()),
            initialized: false,
            first_frame_sent: false,
            first_frame_deferred_count: 0,
        }
    }

    // ========================================================================
    // First Frame Management (Flutter pattern)
    // ========================================================================

    /// Whether frames should be sent to the engine.
    ///
    /// Returns `true` if either:
    /// - The first frame has already been sent
    /// - There are no deferred first frame requests
    pub fn send_frames_to_engine(&self) -> bool {
        self.first_frame_sent || self.first_frame_deferred_count == 0
    }

    /// Defer sending the first frame.
    ///
    /// Call this to perform asynchronous initialization before the first
    /// frame is rendered. The framework will still do all work to produce
    /// frames, but they won't be sent to the engine.
    pub fn defer_first_frame(&mut self) {
        self.first_frame_deferred_count += 1;
    }

    /// Allow sending the first frame after deferral.
    pub fn allow_first_frame(&mut self) {
        if self.first_frame_deferred_count > 0 {
            self.first_frame_deferred_count -= 1;
        }
    }

    /// Reset first frame state (for testing).
    pub fn reset_first_frame_sent(&mut self) {
        self.first_frame_sent = false;
    }

    // ========================================================================
    // Scene Cache
    // ========================================================================

    /// Returns the scene cache for hit testing.
    pub fn scene_cache(&self) -> &SceneCache {
        &self.scene_cache
    }

    /// Returns the cached scene if available.
    pub fn cached_scene(&self) -> Option<Arc<Scene>> {
        self.scene_cache.get()
    }

    // ========================================================================
    // Frame Coordinator
    // ========================================================================

    /// Returns the number of frames rendered.
    pub fn frames_rendered(&self) -> u64 {
        self.frame_coordinator.read().frames_rendered()
    }

    /// Returns a reference to the frame coordinator.
    pub fn frame_coordinator(&self) -> &RwLock<FrameCoordinator> {
        &self.frame_coordinator
    }

    // ========================================================================
    // Layer Tree and Scene Creation
    // ========================================================================

    /// Takes the layer tree from the last paint phase.
    ///
    /// Call this after `draw_frame()` to get the LayerTree for scene creation.
    pub fn take_layer_tree(&mut self) -> Option<flui_layer::LayerTree> {
        self.root_pipeline_owner.take_layer_tree()
    }

    /// Creates a Scene from a LayerTree.
    ///
    /// # Arguments
    ///
    /// * `layer_tree` - The layer tree from paint phase
    /// * `size` - The size of the scene
    /// * `frame_number` - The frame number for debugging
    pub fn create_scene(
        &self,
        layer_tree: flui_layer::LayerTree,
        size: Size,
        frame_number: u64,
    ) -> Arc<Scene> {
        let root = layer_tree.root();
        tracing::debug!(
            "create_scene: {} layers, root={:?}, frame={}",
            layer_tree.len(),
            root,
            frame_number
        );
        let scene = Arc::new(Scene::new(size, layer_tree, root, frame_number));

        // Cache for hit testing
        self.scene_cache.update(Arc::clone(&scene));

        scene
    }

    // ========================================================================
    // Dirty State Queries
    // ========================================================================

    /// Returns whether there are dirty nodes needing processing.
    pub fn has_dirty_nodes(&self) -> bool {
        self.root_pipeline_owner.has_dirty_nodes()
    }

    /// Returns the count of dirty nodes.
    pub fn dirty_node_count(&self) -> usize {
        self.root_pipeline_owner.dirty_node_count()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_binding_new() {
        let binding = RendererBinding::new();
        assert!(!binding.initialized);
        assert!(!binding.first_frame_sent);
        assert!(binding.send_frames_to_engine());
    }

    #[test]
    fn test_init_instances() {
        let mut binding = RendererBinding::new();
        assert!(!binding.is_initialized());

        binding.init_instances();
        assert!(binding.is_initialized());
    }

    #[test]
    fn test_defer_first_frame() {
        let mut binding = RendererBinding::new();

        assert!(binding.send_frames_to_engine());

        binding.defer_first_frame();
        assert!(!binding.send_frames_to_engine());

        binding.allow_first_frame();
        assert!(binding.send_frames_to_engine());
    }

    #[test]
    fn test_add_remove_render_view() {
        let mut binding = RendererBinding::new();

        let view = RenderView::new(1, Size::new(800.0, 600.0));
        binding.add_render_view(view);

        assert_eq!(binding.render_views().count(), 1);

        binding.remove_render_view(1);
        assert_eq!(binding.render_views().count(), 0);
    }

    #[test]
    fn test_draw_frame_no_dirty_nodes() {
        let mut binding = RendererBinding::new();
        binding.init_instances();

        // No dirty nodes - should complete without panic
        binding.draw_frame();
    }
}
