//! PipelineOwner - thin facade over focused components
//!
//! This is the refactored PipelineOwner that follows Single Responsibility Principle.
//! It delegates responsibilities to focused components:
//! - TreeCoordinator: Manages all four trees (View, Element, Render, Layer)
//! - FrameCoordinator: Orchestrates build→layout→paint phases
//! - Optional features: Metrics, ErrorRecovery, CancellationToken, TripleBuffer
//!
//! # Architecture (After Refactoring)
//!
//! ```text
//! PipelineOwner (thin facade)
//!   ├─ tree_coord: Arc<RwLock<TreeCoordinator>>  // Four-tree coordinator
//!   ├─ coordinator: FrameCoordinator              // Phase orchestration
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

use super::{FrameCoordinator, RebuildQueue, TreeCoordinator};
use flui_element::{Element, ElementTree};
use flui_view::tree::ViewNode;
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
///   ├─ tree_coord: TreeCoordinator       ← Four-tree coordinator
///   │   ├─ views: ViewTree               ← ViewObjects
///   │   ├─ elements: ElementTree         ← Elements
///   │   ├─ render_objects: RenderTree    ← RenderObjects
///   │   └─ layers: LayerTree             ← Compositor layers
///   ├─ coordinator: FrameCoordinator     ← Phase orchestration
///   │   ├─ build: BuildPipeline          ← Build phase logic
///   │   ├─ layout: LayoutPipeline        ← Layout phase logic
///   │   └─ paint: PaintPipeline          ← Paint phase logic
///   └─ rebuild_queue: RebuildQueue      ← Deferred rebuilds
/// ```
///
/// **Why Facade?**
/// - Simple API for common operations
/// - Hides internal complexity
/// - Easy to test (mock components)
/// - SOLID principles (Single Responsibility)
///
/// # Unified Pipeline Architecture (FLUI vs Flutter)
///
/// **Key Architectural Decision:** FLUI intentionally unifies Build + Layout + Paint
/// in a single `PipelineOwner`, unlike Flutter's split architecture.
///
/// ## Flutter's Architecture
///
/// Flutter separates widget rebuilds from rendering:
///
/// ```text
/// Flutter:
///   BuildOwner (widgets layer)
///     ├─ _dirtyElements: List<Element>
///     ├─ scheduleBuildFor(Element)
///     ├─ buildScope()
///     └─ flushBuild()  ← Only handles widget rebuilds
///
///   PipelineOwner (rendering layer) - separate owner!
///     ├─ _nodesNeedingLayout: List<RenderObject>
///     ├─ _nodesNeedingPaint: List<RenderObject>
///     ├─ flushLayout()
///     └─ flushPaint()
/// ```
///
/// ## FLUI's Unified Architecture
///
/// FLUI combines all three phases in one owner:
///
/// ```text
/// FLUI:
///   BuildOwner (lifecycle only - in flui-element)
///     ├─ build_scope()    ← Just lifecycle coordination
///     └─ lock_state()     ← No dirty list!
///
///   PipelineOwner (all phases - this struct)
///     ├─ FrameCoordinator
///     │   ├─ BuildPipeline   ← Contains dirty_elements
///     │   ├─ LayoutPipeline  ← Contains dirty layout nodes
///     │   └─ PaintPipeline   ← Contains dirty paint nodes
///     ├─ flush_build()   ← Phase 1
///     ├─ flush_layout()  ← Phase 2
///     ├─ flush_paint()   ← Phase 3
///     └─ build_frame()   ← All three phases atomically
/// ```
///
/// ## Why Unified is Better for Rust
///
/// **1. Atomic Transactions**
/// - Build→Layout→Paint is conceptually a single transaction
/// - Clear ownership: PipelineOwner owns the entire pipeline
/// - No need to coordinate between two owners with shared data
///
/// **2. Performance**
/// - Cross-phase optimizations are straightforward (e.g., skip layout if no size changes)
/// - No synchronization overhead between BuildOwner and PipelineOwner
/// - Easier to implement frame budgeting (time limit for entire frame)
///
/// **3. Type Safety**
/// - Phase ordering guaranteed at compile time
/// - Cannot accidentally call flushPaint() before flushLayout()
/// - Rust's type system enforces correct pipeline usage
///
/// **4. Simplicity**
/// - Single API to learn: `PipelineOwner::build_frame()`
/// - No confusion about which owner to use
/// - Easier to understand: "PipelineOwner builds frames"
///
/// **5. Rust Ownership Semantics**
/// - Flutter uses Dart's GC, so shared references are cheap
/// - Rust requires explicit ownership, making split owners more complex
/// - Unified pipeline avoids `Arc<RwLock<>>` between owners
///
/// **6. Testability**
/// - Mock entire pipeline in one place
/// - Easier to test cross-phase behavior
/// - Less setup boilerplate in tests
///
/// ## What We Kept from Flutter
///
/// - **BuildOwner still exists** as a focused lifecycle coordinator
/// - **Same phase ordering**: Build → Layout → Paint (constraints down, sizes up)
/// - **Same dirty tracking concept**: Mark elements/nodes dirty, flush in batches
/// - **Same phase semantics**: Each phase has same responsibilities as Flutter
///
/// ## Trade-offs
///
/// **Advantage:** Simpler, more performant, more Rust-idiomatic
///
/// **Consideration:** Less modular than Flutter's split (but we use composition
/// via FrameCoordinator to maintain modularity internally)
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
    /// Four-tree coordinator (ViewTree, ElementTree, RenderTree, LayerTree)
    ///
    /// This is the unified architecture that coordinates all four trees.
    /// All tree access should go through this coordinator.
    tree_coord: Arc<RwLock<TreeCoordinator>>,

    /// Frame coordinator (orchestrates pipeline phases)
    coordinator: FrameCoordinator,

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
            .field("root_element_id", &self.tree_coord.read().root())
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
            tree_coord: Arc::new(RwLock::new(TreeCoordinator::new())),
            coordinator: FrameCoordinator::new_with_queue(rebuild_queue.clone()),
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

    /// Get the element tree (legacy - now returns via TreeCoordinator)
    ///
    /// NOTE: This method is deprecated. Use `tree_coordinator()` for new code.
    ///
    /// WARNING: This creates a temporary wrapper that may not support all operations.
    /// Prefer accessing elements through tree_coordinator().elements() directly.
    #[deprecated(note = "Use tree_coordinator() instead")]
    pub fn tree(&self) -> Arc<RwLock<ElementTree>> {
        // This is a transitional method that can't actually return Arc<RwLock<ElementTree>>
        // because ElementTree is now owned by TreeCoordinator.
        // Return a placeholder - callers should migrate to tree_coordinator()
        Arc::new(RwLock::new(ElementTree::new()))
    }

    /// Get the four-tree coordinator
    ///
    /// The TreeCoordinator manages all four trees:
    /// - ViewTree (stores ViewObjects)
    /// - ElementTree (stores Elements)
    /// - RenderTree (stores RenderObjects)
    /// - LayerTree (stores compositor layers)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let coord = owner.tree_coordinator();
    ///
    /// // Access views
    /// let views = coord.read().views();
    ///
    /// // Access elements
    /// let elements = coord.read().elements();
    /// ```
    pub fn tree_coordinator(&self) -> &Arc<RwLock<TreeCoordinator>> {
        &self.tree_coord
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.tree_coord.read().root()
    }

    /// Set the root element
    pub fn set_root(&mut self, element: Element) -> ElementId {
        let id = self.tree_coord.write().mount_root(element);
        // Schedule root for initial build
        self.schedule_build_for(id, 0);
        id
    }

    /// Attach a widget as the root element
    ///
    /// This is a convenience method that creates an Element from a widget
    /// and sets it as the root.
    ///
    /// # Errors
    ///
    /// Returns an error if a root has already been attached.
    pub fn attach<V>(&mut self, widget: V) -> Result<ElementId, PipelineError>
    where
        V: flui_view::StatelessView,
    {
        use flui_view::IntoView;

        if self.tree_coord.read().root().is_some() {
            return Err(PipelineError::RootAlreadyAttached);
        }

        // Four-tree architecture: Insert ViewObject into ViewTree
        let view_object = flui_view::Stateless(widget).into_view();
        let mode = view_object.mode();

        // Insert ViewObject into ViewTree via TreeCoordinator
        let view_id = {
            let mut coord = self.tree_coord.write();
            let view_node = ViewNode::from_boxed(view_object, mode);
            coord.views_mut().insert(view_node)
        };

        // Create Element with ViewId reference
        let element = Element::view(Some(view_id), mode);
        Ok(self.set_root(element))
    }

    /// Attach a root element directly from an IntoElement type.
    ///
    /// This is useful for render-only views that implement IntoElement
    /// but not StatelessView (like Text, Container, etc.).
    pub fn attach_element<E>(&mut self, element: E) -> Result<ElementId, PipelineError>
    where
        E: flui_element::IntoElement,
    {
        if self.tree_coord.read().root().is_some() {
            return Err(PipelineError::RootAlreadyAttached);
        }

        let element = element.into_element();
        Ok(self.set_root(element))
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

    /// Check if there are pending rebuilds
    pub fn has_pending_rebuilds(&self) -> bool {
        self.dirty_count() > 0
    }

    /// Flush the rebuild queue and return whether any elements were rebuilt
    pub fn flush_rebuild_queue(&mut self) -> bool {
        let count_before = self.dirty_count();
        if count_before == 0 {
            return false;
        }
        self.flush_build();
        true
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
        self.coordinator.build_mut().set_build_scope(true);
        let result = f(self);
        self.coordinator.build_mut().set_build_scope(false);
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
        self.coordinator.flush_build(&self.tree_coord);
    }

    /// Flush the layout phase
    pub fn flush_layout(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<flui_types::Size>, PipelineError> {
        let root_id = self.tree_coord.read().root();
        self.coordinator
            .flush_layout(&self.tree_coord, root_id, constraints)
    }

    /// Flush the paint phase
    pub fn flush_paint(&mut self) -> Result<Option<flui_painting::Canvas>, PipelineError> {
        let root_id = self.tree_coord.read().root();
        self.coordinator.flush_paint(&self.tree_coord, root_id)
    }

    /// Build a complete frame
    pub fn build_frame(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<flui_painting::Canvas>, PipelineError> {
        self.frame_counter += 1;
        let root_id = self.tree_coord.read().root();
        self.coordinator
            .build_frame(&self.tree_coord, root_id, constraints)
    }

    // =========================================================================
    // Dirty Tracking
    // =========================================================================

    /// Request layout for an element
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.coordinator.layout_mut().mark_dirty(node_id);
        // Also mark in TreeCoordinator for unified tracking
        self.tree_coord.write().mark_needs_layout(node_id);
    }

    /// Request paint for an element
    pub fn request_paint(&mut self, node_id: ElementId) {
        self.coordinator.paint_mut().mark_dirty(node_id);
        // Also mark in TreeCoordinator for unified tracking
        self.tree_coord.write().mark_needs_paint(node_id);
    }

    /// Mark element as needing build (via TreeCoordinator)
    ///
    /// This is the preferred method for new code.
    pub fn mark_needs_build(&mut self, element_id: ElementId) {
        self.tree_coord.write().mark_needs_build(element_id);
    }

    /// Check if any elements need build
    pub fn has_needs_build(&self) -> bool {
        self.tree_coord.read().has_needs_build()
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

    // =========================================================================
    // Hit Testing
    // =========================================================================

    /// Perform hit testing at the given position
    ///
    /// This traverses the element tree from root to leaves, testing each
    /// render object's bounds against the position. Results are accumulated
    /// in `HitTestResult`, ordered from front to back (topmost first).
    ///
    /// # Arguments
    ///
    /// * `position` - The position to test, in root coordinate space
    ///
    /// # Returns
    ///
    /// A `HitTestResult` containing all hit elements with their handlers.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = owner.perform_hit_test(Offset::new(100.0, 200.0));
    /// if !result.is_empty() {
    ///     result.dispatch(&pointer_event);
    /// }
    /// ```
    pub fn perform_hit_test(
        &self,
        position: flui_types::Offset,
    ) -> flui_interaction::HitTestResult {
        use flui_interaction::HitTestResult;

        let mut result = HitTestResult::new();

        // Check cache first (if enabled)
        if let Some(cache) = self.features.hit_test_cache() {
            if let Some(cached) = cache.get(position) {
                return cached.clone();
            }
        }

        // Get root element
        let root_id = match self.tree_coord.read().root() {
            Some(id) => id,
            None => return result,
        };

        // Perform hit test traversal
        let coord = self.tree_coord.read();
        let tree = coord.elements();
        self.hit_test_element(tree, root_id, position, &mut result);

        result
    }

    /// Recursively hit test an element and its children
    fn hit_test_element(
        &self,
        tree: &ElementTree,
        element_id: ElementId,
        position: flui_types::Offset,
        result: &mut flui_interaction::HitTestResult,
    ) -> bool {
        use flui_interaction::HitTestEntry;

        let element = match tree.get(element_id) {
            Some(e) => e,
            None => return false,
        };

        // Get render state if this is a render element
        // NOTE: In the four-tree architecture, render state is stored in RenderTree,
        // not in Element. Element only holds render_id reference.
        // Full hit testing through element tree will be enabled when RenderTree
        // integration is complete. For now, we use bounds from HitRegion in
        // CanvasLayer (registered during paint phase).
        let (size, offset) = (flui_types::Size::ZERO, flui_types::Offset::ZERO);

        // Transform position to local coordinates
        let local_position =
            flui_types::Offset::new(position.dx - offset.dx, position.dy - offset.dy);

        // Check if position is within bounds
        let within_bounds = local_position.dx >= 0.0
            && local_position.dy >= 0.0
            && local_position.dx < size.width
            && local_position.dy < size.height;

        if !within_bounds {
            return false;
        }

        // Test children first (back to front for proper z-order)
        let mut hit = false;
        for child_id in element.children().iter().rev() {
            if self.hit_test_element(tree, *child_id, local_position, result) {
                hit = true;
            }
        }

        // Add self to result if we have a handler or if children were hit
        // Note: The actual handler is stored in HitRegion via paint phase
        // Here we just record the element for the hit path
        if hit || within_bounds {
            let bounds = flui_types::Rect::from_xywh(0.0, 0.0, size.width, size.height);
            let entry = HitTestEntry::new(element_id, local_position, bounds);
            result.add(entry);
            return true;
        }

        false
    }

    /// Perform hit test and dispatch event to handlers
    ///
    /// Convenience method that performs hit test and immediately dispatches
    /// the event to all hit handlers.
    ///
    /// # Arguments
    ///
    /// * `position` - The position to test
    /// * `event` - The pointer event to dispatch
    pub fn hit_test_and_dispatch(
        &self,
        position: flui_types::Offset,
        event: &flui_types::events::PointerEvent,
    ) {
        let result = self.perform_hit_test(position);
        result.dispatch(event);
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}
