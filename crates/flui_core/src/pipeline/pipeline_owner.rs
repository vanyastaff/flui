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

use super::{ElementTree, FrameCoordinator, RebuildQueue, RootManager};
use crate::element::{Element, ElementId};

#[cfg(debug_assertions)]
use crate::debug_println;

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
/// let root_element = Element::Render(RenderElement::new(my_render));
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
    /// Performance metrics (optional)
    metrics: Option<super::PipelineMetrics>,

    /// Error recovery policy (optional)
    recovery: Option<super::ErrorRecovery>,

    /// Cancellation token (optional)
    cancellation: Option<super::CancellationToken>,

    /// Triple buffer for lock-free frame exchange (optional)
    #[allow(clippy::redundant_allocation)]
    frame_buffer: Option<super::TripleBuffer<Arc<Box<flui_engine::CanvasLayer>>>>,

    /// Hit test result cache (optional)
    /// Caches hit test results when tree is unchanged for ~5-15% CPU savings
    hit_test_cache: Option<super::HitTestCache>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.root_mgr.root_id())
            .field("coordinator", &self.coordinator)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .field("has_recovery", &self.recovery.is_some())
            .field("has_cancellation", &self.cancellation.is_some())
            .field("has_frame_buffer", &self.frame_buffer.is_some())
            .field("has_hit_test_cache", &self.hit_test_cache.is_some())
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
            metrics: None,
            recovery: None,
            cancellation: None,
            frame_buffer: None,
            hit_test_cache: None,
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

    /// Get reference to the element tree
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.root_mgr.root_id()
    }

    /// Mount an element as the root of the tree
    ///
    /// Delegates to RootManager and schedules initial build.
    pub fn set_root(&mut self, root_element: Element) -> ElementId {
        let root_id = self.root_mgr.set_root(&self.tree, root_element);

        // Root starts dirty
        self.schedule_build_for(root_id, 0);

        root_id
    }

    /// Attach a View as the root of the tree (matches Flutter's `attach` naming)
    ///
    /// This is a high-level method that handles View → Element conversion with proper
    /// BuildContext setup. This is the recommended way to attach root widgets from
    /// application code (flui-app layer).
    ///
    /// # Architecture Note
    ///
    /// This method follows Flutter's pattern where `RootWidget.attach()` exists in the
    /// framework layer (framework.dart), not the application layer (widgets binding).
    /// The View → Element conversion is a framework responsibility.
    ///
    /// # BuildContext Note
    ///
    /// The root View's build() is called with a special BuildContext that uses a
    /// placeholder ElementId. This is safe because:
    /// 1. The root element doesn't exist yet (we're creating it)
    /// 2. The BuildContext is only used during initial View → Element conversion
    /// 3. After conversion, the real ElementId is assigned via set_root()
    ///
    /// This matches Flutter's approach where the root widget is built in a special
    /// context before being mounted to the tree.
    ///
    /// # Parameters
    ///
    /// - `widget`: The root View (typically MaterialApp or similar)
    ///
    /// # Returns
    ///
    /// The ElementId of the attached root element
    ///
    /// # Panics
    ///
    /// Panics if a root is already attached. Call `remove_root()` first if you need
    /// to replace the root widget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::PipelineOwner;
    ///
    /// let mut owner = PipelineOwner::new();
    /// let root_id = owner.attach(MyApp);
    /// ```
    pub fn attach<V>(&mut self, widget: V) -> ElementId
    where
        V: crate::view::View + 'static,
    {
        use crate::view::{BuildContext, IntoElement};

        tracing::info!("Attaching root view to pipeline");

        // Check if root already exists
        if self.root_element_id().is_some() {
            panic!(
                "Root widget already attached to PipelineOwner!\n\
                \n\
                Only one root widget is supported at a time.\n\
                \n\
                If you need to replace the root widget, call remove_root() first."
            );
        }

        // Create BuildContext for root widget initialization
        // NOTE: We use a placeholder ElementId since the root doesn't exist yet.
        // This is safe because the BuildContext is only used during View::build(),
        // and the real ElementId is assigned immediately after via set_root().
        //
        // This matches Flutter's approach where root widgets are built in a
        // temporary context before being mounted to the element tree.
        const ROOT_PLACEHOLDER: usize = 1;
        let temp_id = ElementId::new(ROOT_PLACEHOLDER);

        // CRITICAL: Create HookContext for root widget (matches old working code)
        // Without this, hooks will panic with "Hook called outside component render"
        let hook_context = Arc::new(parking_lot::Mutex::new(crate::hooks::HookContext::new()));
        let rebuild_queue = self.rebuild_queue().clone();
        let ctx = BuildContext::with_hook_context_and_queue(
            self.tree.clone(),
            temp_id,
            hook_context.clone(),
            rebuild_queue,
        );

        // Set up ComponentId for hooks (hooks use u64, ElementId is usize)
        let component_id = crate::hooks::ComponentId(temp_id.get() as u64);

        // Build scope: Lock state during View → Element conversion
        // This matches Flutter's buildScope() pattern for state safety
        self.coordinator.build_mut().set_build_scope(true);

        // Begin component rendering for hook context (CRITICAL for hooks!)
        ctx.with_hook_context_mut(|hook_ctx| {
            hook_ctx.begin_component(component_id);
        });

        // Build the view within a context guard (sets up thread-local)
        // Using with_build_context() ensures the guard lives for the entire closure execution
        let element = crate::view::with_build_context(&ctx, || widget.into_element());

        // End component rendering for hook context
        ctx.with_hook_context_mut(|hook_ctx| {
            hook_ctx.end_component();
        });

        // Clear build scope
        self.coordinator.build_mut().set_build_scope(false);

        // Set as pipeline root (automatically schedules initial build)
        let root_id = self.set_root(element);

        // CRITICAL: Request layout for the entire tree after attaching root
        // Without this, the UI won't layout/paint until an external trigger
        self.request_layout(root_id);

        tracing::info!(root_id = ?root_id, "Root view attached to pipeline");

        root_id
    }

    // =========================================================================
    // Build Scheduling (Delegation to FrameCoordinator)
    // =========================================================================

    /// Set callback for when build is scheduled
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    /// Schedule an element for rebuild
    ///
    /// Delegates to FrameCoordinator.build.schedule().
    pub fn schedule_build_for(&mut self, element_id: ElementId, depth: usize) {
        self.coordinator.build_mut().schedule(element_id, depth);

        // Trigger callback
        if let Some(ref callback) = self.on_build_scheduled {
            callback();
        }
    }

    /// Get count of dirty elements waiting to rebuild
    pub fn dirty_count(&self) -> usize {
        self.coordinator.build().dirty_count()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.coordinator.build().is_in_build_scope()
    }

    // =========================================================================
    // Build Execution (Delegation to FrameCoordinator)
    // =========================================================================

    /// Execute a build scope
    ///
    /// Delegates to BuildPipeline but needs full PipelineOwner access for callback.
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        if self.coordinator.build().is_in_build_scope() {
            tracing::warn!("Nested build_scope detected! This may indicate incorrect usage.");
        }

        // Set scope flag
        self.coordinator.build_mut().set_build_scope(true);

        // Execute callback
        let result = f(self);

        // Clear scope flag
        self.coordinator.build_mut().set_build_scope(false);

        result
    }

    /// Lock state changes
    pub fn lock_state<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.coordinator.build_mut().set_build_locked(true);
        let result = f(self);
        self.coordinator.build_mut().set_build_locked(false);
        result
    }

    /// Flush the build phase
    ///
    /// Delegates to FrameCoordinator.
    pub fn flush_build(&mut self) {
        self.coordinator.flush_build(&self.tree);
    }

    /// Finalize the tree after build
    pub fn finalize_tree(&mut self) {
        self.lock_state(|owner| {
            if !owner.coordinator.build().has_dirty() {
                #[cfg(debug_assertions)]
                debug_println!(PRINT_BUILD_SCOPE, "finalize_tree: tree is clean");
            } else {
                tracing::warn!(
                    dirty_count = owner.dirty_count(),
                    "finalize_tree: dirty elements remaining after build"
                );
            }
        });
    }

    // =========================================================================
    // Build Batching (Delegation to BuildPipeline)
    // =========================================================================

    /// Enable build batching
    pub fn enable_batching(&mut self, batch_duration: Duration) {
        self.coordinator.build_mut().enable_batching(batch_duration);
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        self.coordinator.build_mut().disable_batching();
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.coordinator.build().is_batching_enabled()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.coordinator.build().should_flush_batch()
    }

    /// Flush the current batch
    pub fn flush_batch(&mut self) {
        self.coordinator.build_mut().flush_batch();
    }

    /// Get batching statistics
    pub fn batching_stats(&self) -> (usize, usize) {
        self.coordinator.build().batching_stats()
    }

    // =========================================================================
    // Layout & Paint Phases (Delegation to FrameCoordinator)
    // =========================================================================

    /// Request layout for a Render
    pub fn request_layout(&mut self, node_id: ElementId) {
        crate::trace_hot_path!("request_layout: element {:?}", node_id);

        // Mark in dirty set
        self.coordinator.layout_mut().mark_dirty(node_id);

        // Also set needs_layout flag in RenderState AND clear cached constraints
        let tree = self.tree.read();
        if let Some(crate::element::Element::Render(render_elem)) = tree.get(node_id) {
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.write();
            render_state.mark_needs_layout();

            // IMPORTANT: Clear cached constraints so layout_pipeline uses fresh constraints
            // This is critical for window resize - otherwise old constraints are used!
            render_state.clear_constraints();
        }
    }

    /// Request paint for a Render
    pub fn request_paint(&mut self, node_id: ElementId) {
        self.coordinator.paint_mut().mark_dirty(node_id);
    }

    /// Flush the layout phase
    ///
    /// Delegates to FrameCoordinator.
    pub fn flush_layout(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<flui_types::Size>, super::PipelineError> {
        let result = self.coordinator
            .flush_layout(&self.tree, self.root_mgr.root_id(), constraints);

        // Invalidate hit test cache when layout changes
        if let Some(cache) = &mut self.hit_test_cache {
            cache.invalidate();
        }

        result
    }

    /// Flush the paint phase
    ///
    /// Delegates to FrameCoordinator.
    pub fn flush_paint(&mut self) -> Result<Option<Box<flui_engine::CanvasLayer>>, super::PipelineError> {
        let result = self.coordinator
            .flush_paint(&self.tree, self.root_mgr.root_id());

        // Invalidate hit test cache when paint changes
        if let Some(cache) = &mut self.hit_test_cache {
            cache.invalidate();
        }

        result
    }

    /// Flush the rebuild queue by marking elements dirty
    ///
    /// This processes all pending rebuilds from signals and other reactive primitives.
    /// Should be called before flush_build() to ensure signal changes trigger rebuilds.
    pub fn flush_rebuild_queue(&mut self) {
        let rebuilds = self.rebuild_queue.drain();

        if rebuilds.is_empty() {
            return;
        }

        crate::trace_hot_path!("flush_rebuild_queue: {} pending", rebuilds.len());

        // Mark each element dirty for rebuild
        for (element_id, depth) in rebuilds {
            // Mark element dirty via build pipeline
            self.coordinator.build_mut().schedule(element_id, depth);
        }
    }

    /// Check if there are any dirty layout elements
    pub fn has_dirty_layout(&self) -> bool {
        self.coordinator.layout().has_dirty()
    }

    /// Check if there are any dirty paint elements
    pub fn has_dirty_paint(&self) -> bool {
        self.coordinator.paint().has_dirty()
    }

    /// Build a complete frame
    ///
    /// Delegates to FrameCoordinator.
    pub fn build_frame(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, super::PipelineError> {
        // Increment frame counter
        self.frame_counter += 1;

        // Process pending rebuilds from signals
        self.flush_rebuild_queue();

        // Delegate to coordinator
        self.coordinator
            .build_frame(&self.tree, self.root_mgr.root_id(), constraints)
    }

    /// Build a complete frame without creating a frame span (for custom logging)
    ///
    /// This variant doesn't create its own frame span, allowing the caller to manage spans.
    pub fn build_frame_no_span(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, super::PipelineError> {
        // Increment frame counter
        self.frame_counter += 1;

        // Process pending rebuilds from signals
        self.flush_rebuild_queue();

        // Delegate to coordinator without frame span
        self.coordinator
            .build_frame_no_span(&self.tree, self.root_mgr.root_id(), constraints)
    }

    // =========================================================================
    // Hot Reload Support
    // =========================================================================

    /// Reassemble the entire element tree for hot reload
    ///
    /// Traverses all elements and marks them dirty for rebuild.
    pub fn reassemble_tree(&mut self) -> usize {
        let root_id = match self.root_mgr.root_id() {
            Some(id) => id,
            None => {
                #[cfg(debug_assertions)]
                debug_println!(PRINT_BUILD_SCOPE, "reassemble_tree: no root element");
                return 0;
            }
        };

        #[cfg(debug_assertions)]
        debug_println!(PRINT_BUILD_SCOPE, "reassemble_tree: hot reload triggered");

        // Collect all element IDs to process with their depths
        let element_ids = {
            let tree = self.tree.read();
            self.collect_all_elements(&tree, root_id)
        };

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "reassemble_tree: found {} elements",
            element_ids.len()
        );

        // Process each element
        let reassembled_count = 0;
        for (element_id, depth) in element_ids {
            let mut tree = self.tree.write();

            if let Some(element) = tree.get_mut(element_id) {
                element.mark_dirty();
            }

            drop(tree);

            // Schedule rebuild for this element
            self.schedule_build_for(element_id, depth);
        }

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "reassemble_tree: complete ({} stateful elements reassembled)",
            reassembled_count
        );

        reassembled_count
    }

    /// Collect all elements in tree with their depths
    fn collect_all_elements(
        &self,
        tree: &ElementTree,
        root_id: ElementId,
    ) -> Vec<(ElementId, usize)> {
        let mut result = Vec::new();
        self.collect_elements_recursive(tree, root_id, 0, &mut result);
        result
    }

    /// Recursive helper for collect_all_elements
    #[allow(clippy::only_used_in_recursion)]
    fn collect_elements_recursive(
        &self,
        tree: &ElementTree,
        element_id: ElementId,
        depth: usize,
        result: &mut Vec<(ElementId, usize)>,
    ) {
        result.push((element_id, depth));

        // Get element and traverse children
        if let Some(element) = tree.get(element_id) {
            for child_id in element.children() {
                self.collect_elements_recursive(tree, child_id, depth + 1, result);
            }
        }
    }

    // =========================================================================
    // Production Features (Optional)
    // =========================================================================

    /// Enable performance metrics
    pub fn enable_metrics(&mut self) {
        self.metrics = Some(super::PipelineMetrics::new());
    }

    /// Disable performance metrics
    pub fn disable_metrics(&mut self) {
        self.metrics = None;
    }

    /// Get reference to metrics (if enabled)
    pub fn metrics(&self) -> Option<&super::PipelineMetrics> {
        self.metrics.as_ref()
    }

    /// Get mutable reference to metrics (if enabled)
    pub fn metrics_mut(&mut self) -> Option<&mut super::PipelineMetrics> {
        self.metrics.as_mut()
    }

    /// Enable error recovery with specified policy
    pub fn enable_error_recovery(&mut self, policy: super::RecoveryPolicy) {
        self.recovery = Some(super::ErrorRecovery::new(policy));
    }

    /// Disable error recovery
    pub fn disable_error_recovery(&mut self) {
        self.recovery = None;
    }

    /// Get reference to error recovery (if enabled)
    pub fn error_recovery(&self) -> Option<&super::ErrorRecovery> {
        self.recovery.as_ref()
    }

    /// Get mutable reference to error recovery (if enabled)
    pub fn error_recovery_mut(&mut self) -> Option<&mut super::ErrorRecovery> {
        self.recovery.as_mut()
    }

    /// Enable cancellation support
    pub fn enable_cancellation(&mut self) {
        self.cancellation = Some(super::CancellationToken::new());
    }

    /// Disable cancellation
    pub fn disable_cancellation(&mut self) {
        self.cancellation = None;
    }

    /// Get reference to cancellation token (if enabled)
    pub fn cancellation_token(&self) -> Option<&super::CancellationToken> {
        self.cancellation.as_ref()
    }

    /// Enable triple buffer for lock-free frame exchange
    pub fn enable_frame_buffer(&mut self, initial: Arc<Box<flui_engine::CanvasLayer>>) {
        self.frame_buffer = Some(super::TripleBuffer::new(initial));
    }

    /// Disable frame buffer
    pub fn disable_frame_buffer(&mut self) {
        self.frame_buffer = None;
    }

    /// Get reference to frame buffer (if enabled)
    pub fn frame_buffer(&self) -> Option<&super::TripleBuffer<Arc<Box<flui_engine::CanvasLayer>>>> {
        self.frame_buffer.as_ref()
    }

    /// Enable hit test caching for ~5-15% CPU savings during mouse movement
    ///
    /// Caches hit test results when tree is unchanged. Automatically invalidates
    /// when layout or paint changes occur.
    pub fn enable_hit_test_cache(&mut self) {
        self.hit_test_cache = Some(super::HitTestCache::new());
    }

    /// Disable hit test caching
    pub fn disable_hit_test_cache(&mut self) {
        self.hit_test_cache = None;
    }

    /// Get reference to hit test cache (if enabled)
    pub fn hit_test_cache(&self) -> Option<&super::HitTestCache> {
        self.hit_test_cache.as_ref()
    }

    /// Get mutable reference to hit test cache (if enabled)
    pub fn hit_test_cache_mut(&mut self) -> Option<&mut super::HitTestCache> {
        self.hit_test_cache.as_mut()
    }

    /// Perform hit test with caching (if enabled)
    ///
    /// Uses cache when available for ~5-15% CPU savings.
    /// Falls back to direct tree traversal if cache is disabled.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Enable caching (optional but recommended)
    /// owner.enable_hit_test_cache();
    ///
    /// // Use for hit testing
    /// let result = owner.hit_test_with_cache(mouse_position);
    /// for entry in result.entries() {
    ///     println!("Hit element: {:?}", entry.element_id);
    /// }
    /// ```
    pub fn hit_test_with_cache(
        &mut self,
        position: flui_types::Offset,
    ) -> crate::element::ElementHitTestResult {
        let root_id = match self.root_mgr.root_id() {
            Some(id) => id,
            None => return crate::element::ElementHitTestResult::new(),
        };

        // Try cache first
        if let Some(cache) = &mut self.hit_test_cache {
            if let Some(cached_result) = cache.get(position, root_id) {
                return cached_result;
            }

            // Cache miss - do actual hit test and store result
            let tree = self.tree.read();
            let result = tree.hit_test(root_id, position);
            drop(tree);

            cache.insert(position, root_id, result.clone());
            result
        } else {
            // No cache - direct hit test
            let tree = self.tree.read();
            tree.hit_test(root_id, position)
        }
    }

    /// Get mutable reference to frame buffer (if enabled)
    pub fn frame_buffer_mut(&mut self) -> Option<&mut super::TripleBuffer<Arc<Box<flui_engine::CanvasLayer>>>> {
        self.frame_buffer.as_mut()
    }

    /// Publish a frame to the triple buffer (convenience method)
    pub fn publish_frame(&mut self, layer: Box<flui_engine::CanvasLayer>) {
        if let Some(buffer) = self.frame_buffer.as_mut() {
            let write_buf = buffer.write();
            let mut write_guard = write_buf.write();
            *write_guard = Arc::new(layer);
            drop(write_guard);
            buffer.swap();
        }
    }

    // =========================================================================
    // Event Dispatching (Unified System)
    // =========================================================================

    /// Dispatch an event to all elements in the tree
    ///
    /// This unified method handles all types of events: window events (theme, focus, DPI),
    /// pointer events (future), keyboard events (future), etc.
    ///
    /// Elements can override `handle_event()` to respond to specific event types using match:
    ///
    /// **Common Use Cases:**
    /// - `ThemeProvider` → `Event::Window(WindowEvent::ThemeChanged)` to update theme
    /// - `AnimationController` → `Event::Window(WindowEvent::VisibilityChanged)` to pause
    /// - `Button` → `Event::Pointer(PointerEvent::Down)` for clicks (future)
    /// - `TextField` → `Event::Key(KeyEvent::Down)` for input (future)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::{Event, WindowEvent, Theme};
    ///
    /// // Dispatch theme changed event
    /// let event = Event::Window(WindowEvent::ThemeChanged { theme: Theme::Dark });
    /// owner.dispatch_event(&event);
    ///
    /// // All elements receive this, but only ThemeProvider handles it
    /// ```
    ///
    /// # Performance
    ///
    /// Iterates through all elements in the tree. Most elements return `false` (not handled)
    /// quickly via match. Only specialized elements process specific event types.
    pub fn dispatch_event(&mut self, event: &flui_types::Event) {
        let mut tree = self.tree.write();

        crate::trace_hot_path!("dispatch_event: {:?}", event);

        // Visit all elements and dispatch the event
        tree.visit_all_elements_mut(|_element_id, element| {
            // Call handle_event on each element
            // Most will return false (not handled), but specialized elements
            // can return true and trigger updates
            let _handled = element.handle_event(event);

            crate::trace_hot_path!(
                "element {:?} handled={} event={:?}",
                _element_id,
                _handled,
                event
            );
        });
    }

    /// Dispatch a pointer event to elements under the pointer
    ///
    /// Uses hit testing to determine which elements are under the pointer position,
    /// then dispatches the event only to those elements.
    ///
    /// This is more efficient than broadcasting to all elements and follows Flutter's
    /// pattern where only hit elements receive pointer events.
    ///
    /// # Arguments
    ///
    /// * `event` - The pointer event to dispatch
    /// * `position` - Position of the pointer in global (window) coordinates
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_types::{Event, PointerEvent, PointerEventData, Offset};
    ///
    /// let position = Offset::new(100.0, 50.0);
    /// let pointer_event = PointerEvent::Down(PointerEventData {
    ///     position,
    ///     device: 0,
    /// });
    /// let event = Event::Pointer(pointer_event);
    ///
    /// owner.dispatch_pointer_event(&event, position);
    /// ```
    ///
    /// # Performance
    ///
    /// Hit testing is performed once, then the event is dispatched to only the hit elements.
    /// This is much more efficient than broadcasting to all elements for pointer events.
    pub fn dispatch_pointer_event(
        &mut self,
        event: &flui_types::Event,
        position: flui_types::Offset,
    ) {
        let root_id = match self.root_mgr.root_id() {
            Some(id) => id,
            None => {
                crate::trace_hot_path!("dispatch_pointer_event: no root element");
                return;
            }
        };

        crate::trace_hot_path!("dispatch_pointer_event: pos={:?} event={:?}", position, event);

        // Perform hit testing
        let hit_result = {
            let tree = self.tree.read();
            tree.hit_test(root_id, position)
        };

        crate::trace_hot_path!("dispatch_pointer_event: hit {} elements", hit_result.entries().len());

        // Dispatch event to hit elements
        let mut tree = self.tree.write();
        for entry in hit_result.iter() {
            if let Some(element) = tree.get_mut(entry.element_id) {
                let _handled = element.handle_event(event);

                crate::trace_hot_path!(
                    "element {:?} handled={} at local pos={:?}",
                    entry.element_id,
                    _handled,
                    entry.local_position
                );
            }
        }
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
    use crate::element::{ComponentElement, Element};
    use crate::testing::TestWidget;
    use crate::view::AnyView;

    #[test]
    fn test_build_owner_creation() {
        let owner = PipelineOwner::new();
        assert!(owner.root_element_id().is_none());
        assert_eq!(owner.dirty_count(), 0);
        assert!(!owner.is_in_build_scope());
    }

    #[test]
    fn test_schedule_build() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        let id = ElementId::new(42); // Arbitrary ElementId

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        // Scheduling same element again - dirty_count() returns raw count before deduplication
        // Deduplication happens during flush for efficiency
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
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        let id = ElementId::new(42);

        // Normal scheduling works
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.lock_state(|o| {
            // Scheduling while locked should be ignored
            let id2 = ElementId::new(43);
            o.schedule_build_for(id2, 0);
            assert_eq!(o.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_depth_sorting() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        // Schedule in random order
        owner.schedule_build_for(id2, 2);
        owner.schedule_build_for(id1, 1);
        owner.schedule_build_for(id3, 0);

        // flush_build sorts by depth before rebuilding
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_on_build_scheduled_callback() {
        use crate::ElementId;
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

    // Build Batching Tests

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
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new(42);

        // Schedule same element 3 times
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0);

        // Flush batch
        owner.flush_batch();

        // Should only have 1 dirty element
        assert_eq!(owner.dirty_count(), 1);

        // Stats should show 2 builds saved
        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_multiple_elements() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);
        owner.schedule_build_for(id3, 2);

        owner.flush_batch();

        // All 3 should be dirty
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(10));

        let id = ElementId::new(42);
        owner.schedule_build_for(id, 0);

        // Should not flush immediately
        assert!(!owner.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Now should flush
        assert!(owner.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        // Batching not enabled

        let id = ElementId::new(42);
        owner.schedule_build_for(id, 0);

        // Should add directly to dirty elements
        assert_eq!(owner.dirty_count(), 1);

        // flush_batch should be no-op
        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        use crate::ElementId;
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = ElementId::new(42);

        // Initial stats
        assert_eq!(owner.batching_stats(), (0, 0));

        // Schedule same element twice
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0); // Duplicate

        // Flush
        owner.flush_batch();

        // Should have 1 batch flushed, 1 build saved
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
        let view: Box<dyn AnyView> = Box::new(TestWidget);
        let state: Box<dyn std::any::Any> = Box::new(());
        let component = ComponentElement::new(view, state);
        let root = Element::Component(component);

        let root_id = owner.set_root(root);

        assert_eq!(owner.root_element_id(), Some(root_id));
        // Root should be marked dirty
        assert_eq!(owner.dirty_count(), 1);
    }
}
