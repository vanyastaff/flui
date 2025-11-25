//! PipelineOwner - thin facade over focused components
//!
//! This is the refactored PipelineOwner that follows Single Responsibility Principle.
//! It delegates responsibilities to focused components:
//! - FrameCoordinator: Orchestrates build→layout→paint phases
//! - RootManager: Manages root element
//! - ElementTree: Stores elements
//! - Optional features: Metrics, ErrorRecovery, CancellationToken, TripleBuffer
//!
//! # Architecture (After Refactoring)
//!
//! ```text
//! PipelineOwner (thin facade)
//!   ├─ tree: Arc<RwLock<ElementTree>>      // Element storage
//!   ├─ coordinator: FrameCoordinator        // Phase orchestration
//!   ├─ root_mgr: RootManager               // Root management
//!   └─ Optional features:
//!       ├─ metrics: PipelineMetrics
//!       ├─ recovery: ErrorRecovery
//!       ├─ cancellation: CancellationToken
//!       └─ frame_buffer: TripleBuffer
//! ```
//!
//! # Benefits
//!
//! - **Single Responsibility**: Each component has ONE clear purpose
//! - **Testability**: Easy to test components in isolation
//! - **Maintainability**: Changes localized to specific components
//! - **Extensibility**: New features don't bloat PipelineOwner
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::PipelineOwner;
//!
//! let mut owner = PipelineOwner::new();
//!
//! // Set root element
//! let root_id = owner.set_root(my_element);
//!
//! // Build frame
//! let layer = owner.build_frame(constraints)?;
//! ```

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

use super::{FrameCoordinator, RebuildQueue, RootManager};
use flui_element::{Element, ElementTree};
use flui_foundation::ElementId;
use flui_pipeline::PipelineError;

/// PipelineOwner - orchestrates the three-phase rendering pipeline
///
/// PipelineOwner is the main entry point for FLUI's rendering system. It coordinates
/// the three phases of frame rendering: **Build**, **Layout**, and **Paint**.
///
/// # Three-Phase Pipeline
///
/// Every frame goes through three sequential phases:
///
/// ```text
/// ┌─────────┐     ┌────────┐     ┌───────┐     ┌────────────┐
/// │  Build  │ ──> │ Layout │ ──> │ Paint │ ──> │ GPU Render │
/// └─────────┘     └────────┘     └───────┘     └────────────┘
///     ↓               ↓              ↓
/// Rebuild dirty   Compute sizes  Generate layers
/// components      and positions  for GPU
/// ```
///
/// **1. Build Phase** (`flush_build`)
/// - Rebuilds dirty components (Views marked as needing rebuild)
/// - Calls `View::build()` to create/update Elements
/// - Updates the Element tree with new configurations
/// - **Output**: Updated Element tree
///
/// **2. Layout Phase** (`flush_layout`)
/// - Computes sizes and positions for all dirty RenderObjects
/// - Calls `Render::layout()` with BoxConstraints
/// - Propagates constraints down, sizes up (like Flutter)
/// - **Output**: Size information stored in RenderState
///
/// **3. Paint Phase** (`flush_paint`)
/// - Generates layer tree for GPU rendering
/// - Calls `Render::paint()` with computed offsets
/// - Creates PictureLayers, TransformLayers, etc.
/// - **Output**: BoxedLayer tree ready for compositor
///
/// # Architecture (Facade Pattern)
///
/// PipelineOwner is a facade that delegates to focused components:
///
/// ```text
/// PipelineOwner (Facade)
///   ├─ tree: Arc<RwLock<ElementTree>>   ← Element storage
///   ├─ coordinator: FrameCoordinator     ← Phase orchestration
///   │   ├─ build: BuildPipeline          ← Build phase logic
///   │   ├─ layout: LayoutPipeline        ← Layout phase logic
///   │   └─ paint: PaintPipeline          ← Paint phase logic
///   ├─ root_mgr: RootManager            ← Root element tracking
///   └─ rebuild_queue: RebuildQueue      ← Deferred rebuilds
/// ```
///
/// **Why Facade?**
/// - Simple API for common operations
/// - Hides internal complexity
/// - Easy to test (mock components)
/// - SOLID principles (Single Responsibility)
///
/// # Critical Pattern: request_layout() Must Set Both Flags
///
/// When requesting layout, you must set **both**:
/// 1. Dirty set flag: `coordinator.layout_mut().mark_dirty(node_id)`
/// 2. RenderState flag: `render_state.mark_needs_layout()`
///
/// **Failing to set both will cause layout to skip elements!**
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::{PipelineOwner, Element, RenderElement};
///
/// // Create pipeline
/// let mut owner = PipelineOwner::new();
///
/// // Set root element
/// let root_element = Element::from_render_element(RenderElement::new(my_render));
/// let root_id = owner.set_root(root_element);
///
/// // Mark element dirty (triggers rebuild on next frame)
/// owner.schedule_build_for(element_id, depth);
///
/// // Build complete frame (all three phases)
/// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
/// let layer = owner.build_frame(constraints)?;
///
/// // Or run phases individually:
/// owner.flush_build()?;                          // Phase 1: Build
/// owner.flush_layout(root_id, constraints)?;     // Phase 2: Layout
/// let layer = owner.flush_paint(root_id)?;       // Phase 3: Paint
/// ```
///
/// # Thread Safety
///
/// - ElementTree: Protected by `Arc<RwLock<>>` for multi-threaded access
/// - Build phase: Can run in parallel with `parallel` feature
/// - Layout/Paint: Single-threaded (uses thread-local stacks)
/// - Rebuild queue: Lock-free with atomic operations
pub struct PipelineOwner {
    /// The element tree (shared storage)
    tree: Arc<RwLock<ElementTree>>,

    /// Frame coordinator (orchestrates pipeline phases)
    coordinator: FrameCoordinator,

    /// Root manager (tracks root element)
    root_mgr: RootManager,

    /// Rebuild queue for deferred component rebuilds
    /// Used by signals and other reactive primitives
    rebuild_queue: RebuildQueue,

    /// Callback when a build is scheduled (optional)
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,

    /// Frame counter (increments each build_frame call)
    /// Used for debugging, profiling, and scene identification
    frame_counter: u64,

    // =========================================================================
    // Production Features (Optional)
    // =========================================================================
    /// Optional production features (metrics, recovery, caching, etc.)
    ///
    /// All optional features are grouped into PipelineFeatures for better
    /// Single Responsibility Principle compliance. Features can be enabled/disabled
    /// independently via `features()` and `features_mut()` accessors.
    ///
    /// See [`PipelineFeatures`] for available features.
    features: super::PipelineFeatures,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.root_mgr.root_id())
            .field("coordinator", &self.coordinator)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .field("features", &self.features)
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new pipeline owner with default configuration
    ///
    /// Creates a basic pipeline without production features (no metrics, batching, etc.).
    /// For advanced configuration, consider using [`PipelineBuilder`] instead.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Basic usage
    /// let owner = PipelineOwner::new();
    ///
    /// // For advanced features, use PipelineBuilder:
    /// let owner = PipelineBuilder::new()
    ///     .with_metrics()
    ///     .with_batching(Duration::from_millis(16))
    ///     .build();
    /// ```
    ///
    /// [`PipelineBuilder`]: super::PipelineBuilder
    pub fn new() -> Self {
        let rebuild_queue = RebuildQueue::new();
        Self {
            tree: Arc::new(RwLock::new(ElementTree::new())),
            coordinator: FrameCoordinator::new_with_queue(rebuild_queue.clone()),
            root_mgr: RootManager::new(),
            rebuild_queue,
            on_build_scheduled: None,
            frame_counter: 0,
            features: super::PipelineFeatures::new(),
        }
    }

    /// Get reference to the rebuild queue
    pub fn rebuild_queue(&self) -> &RebuildQueue {
        &self.rebuild_queue
    }

    /// Get current frame number
    ///
    /// This counter increments each time build_frame() is called.
    /// Useful for debugging, profiling, and scene identification.
    ///
    /// # Returns
    ///
    /// The current frame counter value
    pub fn frame_number(&self) -> u64 {
        self.frame_counter
    }

    // =========================================================================
    // Tree & Root Access (Delegation to RootManager)
    // =========================================================================

    /// Get the element tree
    pub fn tree(&self) -> Arc<RwLock<ElementTree>> {
        self.tree.clone()
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.root_mgr.root_id()
    }

    /// Set the root element
    pub fn set_root(&mut self, element: Element) -> ElementId {
        let id = self.root_mgr.set_root(&self.tree, element);
        // Schedule root for initial build
        self.schedule_build_for(id, 0);
        id
    }

    // =========================================================================
    // Build Scheduling
    // =========================================================================

    /// Schedule an element for rebuild
    pub fn schedule_build_for(&mut self, element_id: ElementId, depth: usize) {
        self.coordinator.build_mut().schedule(element_id, depth);

        // Call optional callback
        if let Some(ref callback) = self.on_build_scheduled {
            callback();
        }
    }

    /// Get number of dirty elements
    pub fn dirty_count(&self) -> usize {
        self.coordinator.build().dirty_count()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.coordinator.build().is_in_build_scope()
    }

    /// Execute code within a build scope
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.coordinator.build_mut().set_in_build_scope(true);
        let result = f(self);
        self.coordinator.build_mut().set_in_build_scope(false);
        result
    }

    /// Execute code with state locked (no new builds allowed)
    pub fn lock_state<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.coordinator.build_mut().set_build_locked(true);
        let result = f(self);
        self.coordinator.build_mut().set_build_locked(false);
        result
    }

    // =========================================================================
    // Batching
    // =========================================================================

    /// Enable build batching
    pub fn enable_batching(&mut self, duration: Duration) {
        self.coordinator.build_mut().enable_batching(duration);
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        self.coordinator.build_mut().disable_batching();
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.coordinator.build().is_batching_enabled()
    }

    /// Flush the current batch
    pub fn flush_batch(&mut self) {
        self.coordinator.build_mut().flush_batch();
    }

    /// Check if batch should be flushed
    pub fn should_flush_batch(&self) -> bool {
        self.coordinator.build().should_flush_batch()
    }

    /// Get batching statistics
    pub fn batching_stats(&self) -> (usize, usize) {
        self.coordinator.build().batching_stats()
    }

    // =========================================================================
    // Pipeline Phases
    // =========================================================================

    /// Flush the build phase
    pub fn flush_build(&mut self) {
        self.coordinator.flush_build(&self.tree);
    }

    /// Flush the layout phase
    pub fn flush_layout(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<flui_types::Size>, PipelineError> {
        let root_id = self.root_mgr.root_id();
        self.coordinator
            .flush_layout(&self.tree, root_id, constraints)
    }

    /// Flush the paint phase
    pub fn flush_paint(&mut self) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        let root_id = self.root_mgr.root_id();
        self.coordinator.flush_paint(&self.tree, root_id)
    }

    /// Build a complete frame
    pub fn build_frame(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        self.frame_counter += 1;
        let root_id = self.root_mgr.root_id();
        self.coordinator
            .build_frame(&self.tree, root_id, constraints)
    }

    // =========================================================================
    // Dirty Tracking
    // =========================================================================

    /// Request layout for an element
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.coordinator.layout_mut().mark_dirty(node_id);

        // Also mark RenderState flag
        if let Some(element) = self.tree.write().get_mut(node_id) {
            if let Some(render_state) = element.render_state_mut() {
                render_state.mark_needs_layout();
            }
        }
    }

    /// Request paint for an element
    pub fn request_paint(&mut self, node_id: ElementId) {
        self.coordinator.paint_mut().mark_dirty(node_id);

        // Also mark RenderState flag
        if let Some(element) = self.tree.write().get_mut(node_id) {
            if let Some(render_state) = element.render_state_mut() {
                render_state.mark_needs_paint();
            }
        }
    }

    // =========================================================================
    // Callback
    // =========================================================================

    /// Set callback for when build is scheduled
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    // =========================================================================
    // Features
    // =========================================================================

    /// Get reference to features
    pub fn features(&self) -> &super::PipelineFeatures {
        &self.features
    }

    /// Get mutable reference to features
    pub fn features_mut(&mut self) -> &mut super::PipelineFeatures {
        &mut self.features
    }

    /// Enable metrics
    pub fn enable_metrics(&mut self) {
        self.features.enable_metrics();
    }

    /// Get metrics
    pub fn metrics(&self) -> Option<&flui_pipeline::PipelineMetrics> {
        self.features.metrics()
    }

    /// Enable error recovery
    pub fn enable_error_recovery(&mut self, policy: flui_pipeline::RecoveryPolicy) {
        self.features.enable_recovery_with_policy(policy);
    }

    /// Get error recovery
    pub fn error_recovery(&self) -> Option<&flui_pipeline::ErrorRecovery> {
        self.features.recovery()
    }

    /// Enable cancellation
    pub fn enable_cancellation(&mut self, timeout: Duration) {
        self.features.enable_cancellation(timeout);
    }

    /// Get cancellation token
    pub fn cancellation_token(&self) -> Option<&flui_pipeline::CancellationToken> {
        self.features.cancellation()
    }

    /// Enable frame buffer
    pub fn enable_frame_buffer(&mut self) {
        self.features.enable_frame_buffer();
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;
    use std::any::Any;

    // Mock ViewObject for testing
    struct MockViewObject;

    impl flui_view::ViewObject for MockViewObject {
        fn view_mode(&self) -> flui_view::ViewMode {
            flui_view::ViewMode::Stateless
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_build_owner_creation() {
        let owner = PipelineOwner::new();
        assert!(owner.root_element_id().is_none());
        assert_eq!(owner.dirty_count(), 0);
        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = PipelineOwner::new();
        let id = ElementId::new(42);

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 2);
    }

    #[test]
    fn test_build_scope() {
        let mut owner = PipelineOwner::new();

        assert!(!owner.is_in_build_scope());

        owner.build_scope(|o| {
            assert!(o.is_in_build_scope());
        });

        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_lock_state() {
        let mut owner = PipelineOwner::new();
        let id = ElementId::new(42);

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.lock_state(|o| {
            let id2 = ElementId::new(43);
            o.schedule_build_for(id2, 0);
            assert_eq!(o.dirty_count(), 1);
        });
    }

    #[test]
    fn test_depth_sorting() {
        let mut owner = PipelineOwner::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        owner.schedule_build_for(id2, 2);
        owner.schedule_build_for(id1, 1);
        owner.schedule_build_for(id3, 0);

        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_on_build_scheduled_callback() {
        use std::sync::{Arc, Mutex};

        let mut owner = PipelineOwner::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        owner.set_on_build_scheduled(move || {
            *called_clone.lock().unwrap() = true;
        });

        let id = ElementId::new(42);
        owner.schedule_build_for(id, 0);

        assert!(*called.lock().unwrap());
    }

    #[test]
    fn test_batching_disabled_by_default() {
        let owner = PipelineOwner::new();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut owner = PipelineOwner::new();

        owner.enable_batching(Duration::from_millis(16));
        assert!(owner.is_batching_enabled());

        owner.disable_batching();
        assert!(!owner.is_batching_enabled());
    }

    #[test]
    fn test_batching_deduplicates() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new(42);

        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);

        owner.flush_batch();

        assert_eq!(owner.dirty_count(), 1);

        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_multiple_elements() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);
        owner.schedule_build_for(id3, 2);

        owner.flush_batch();

        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(10));

        let id = ElementId::new(42);
        owner.schedule_build_for(id, 0);

        assert!(!owner.should_flush_batch());

        std::thread::sleep(Duration::from_millis(15));

        assert!(owner.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        let mut owner = PipelineOwner::new();

        let id = ElementId::new(42);
        owner.schedule_build_for(id, 0);

        assert_eq!(owner.dirty_count(), 1);

        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new(42);

        assert_eq!(owner.batching_stats(), (0, 0));

        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);

        owner.flush_batch();

        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
    }

    #[test]
    fn test_build_scope_returns_result() {
        let mut owner = PipelineOwner::new();

        let result = owner.build_scope(|_| 42);

        assert_eq!(result, 42);
    }

    #[test]
    fn test_set_root() {
        let mut owner = PipelineOwner::new();

        let root = Element::new(Box::new(MockViewObject));

        let root_id = owner.set_root(root);

        assert_eq!(owner.root_element_id(), Some(root_id));
        assert_eq!(owner.dirty_count(), 1);
    }
}
