#![allow(clippy::all)]
#![allow(dead_code)]

//! Build phase management and element lifecycle (OLD BACKUP - NOT USED)
//!
//! The PipelineOwner coordinates widget rebuilds and manages the build phase lifecycle.
//!
//! # Key Responsibilities
//!
//! 1. **Dirty Tracking**: Maintains list of elements that need rebuild
//! 2. **Build Scheduling**: Batches multiple setState() calls for performance
//! 3. **Build Orchestration**: Coordinates rebuild order (parents before children)
//! 4. **Build Scope Management**: Prevents setState during build
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner
//!   ├─ tree: Arc<RwLock<ElementTree>>
//!   ├─ dirty_elements: Vec<(ElementId, usize)>  // (id, depth)
//!   ├─ build_count: usize
//!   ├─ in_build_scope: bool
//!   └─ batcher: Option<BuildBatcher>  // For batching rapid setState calls
//! ```
//!
//! # Build Batching
//!
//! When enabled, PipelineOwner batches multiple setState() calls within a time window:
//!
//! ```rust,ignore
//! let mut owner = PipelineOwner::new();
//! owner.enable_batching(Duration::from_millis(16)); // One frame
//!
//! // Multiple setState calls
//! owner.schedule_build_for(id1, 0);
//! owner.schedule_build_for(id2, 1);
//! owner.schedule_build_for(id1, 0); // Duplicate - batched!
//!
//! // Later...
//! if owner.should_flush_batch() {
//!     owner.flush_batch(); // Add to dirty_elements
//!     owner.build_scope(|o| o.flush_build()); // Rebuild
//! }
//! ```

use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

use crate::element::{Element, ElementId};
use super::ElementTree;

#[cfg(debug_assertions)]
use crate::debug_println;

/// PipelineOwner - manages the build phase and element lifecycle
///
/// This is the core coordinator for the widget build system.
/// It tracks dirty elements and orchestrates rebuilds.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{PipelineOwner, ComponentElement};
///
/// let mut owner = PipelineOwner::new();
///
/// // Create root element
/// let root_element = ComponentElement::new(MyApp::new());
/// let root_id = owner.set_root(Box::new(root_element));
///
/// // Mark element dirty
/// owner.schedule_build_for(element_id, depth);
///
/// // Rebuild all dirty elements
/// owner.build_scope(|o| {
///     o.flush_build();
/// });
/// ```
pub struct PipelineOwner {
    /// The element tree
    pub(crate) tree: Arc<RwLock<ElementTree>>,

    /// Build pipeline - manages widget rebuild phase
    pub(crate) build: super::BuildPipeline,

    /// Layout pipeline - manages size computation phase
    pub(crate) layout: super::LayoutPipeline,

    /// Paint pipeline - manages layer generation phase
    pub(crate) paint: super::PaintPipeline,

    /// Root element ID
    pub(crate) root_element_id: Option<ElementId>,

    /// Callback when a build is scheduled (optional)
    pub(crate) on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,

    // =========================================================================
    // Production Features (Optional)
    // =========================================================================

    /// Performance metrics (optional)
    ///
    /// When enabled, tracks FPS, frame times, cache hit rates, etc.
    /// Overhead: ~1% CPU, 480 bytes memory
    pub(crate) metrics: Option<super::PipelineMetrics>,

    /// Error recovery policy (optional)
    ///
    /// Defines how to handle pipeline errors (UseLastGoodFrame, ShowErrorWidget, etc.)
    /// Overhead: ~40 bytes memory
    pub(crate) recovery: Option<super::ErrorRecovery>,

    /// Cancellation token (optional)
    ///
    /// Enables timeout support for long-running operations
    /// Overhead: ~24 bytes memory
    pub(crate) cancellation: Option<super::CancellationToken>,

    /// Triple buffer for lock-free frame exchange (optional)
    ///
    /// When enabled, allows compositor to read frames while renderer writes
    /// Uses Arc<BoxedLayer> to enable cloning for TripleBuffer
    /// Overhead: 3× Arc size + layer size + RwLock overhead
    pub(crate) frame_buffer: Option<super::TripleBuffer<Arc<crate::BoxedLayer>>>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.root_element_id)
            .field("build", &self.build)
            .field("layout", &self.layout)
            .field("paint", &self.paint)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .field("has_recovery", &self.recovery.is_some())
            .field("has_cancellation", &self.cancellation.is_some())
            .field("has_frame_buffer", &self.frame_buffer.is_some())
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new pipeline owner
    ///
    /// **Deprecated**: Prefer using [`PipelineBuilder`](super::PipelineBuilder) for better API ergonomics.
    ///
    /// Creates a basic pipeline without production features.
    /// Use builder methods to enable optional features:
    /// - `enable_metrics()` for performance monitoring
    /// - `enable_error_recovery()` for graceful degradation
    /// - `enable_cancellation()` for timeout support
    /// - `enable_frame_buffer()` for lock-free frame exchange
    ///
    /// # Example (New API - Recommended)
    ///
    /// ```rust,ignore
    /// use flui_core::pipeline::PipelineBuilder;
    ///
    /// let owner = PipelineBuilder::new()
    ///     .with_metrics()
    ///     .with_batching(Duration::from_millis(16))
    ///     .build();
    /// ```
    ///
    /// # Example (Old API - Still Supported)
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_metrics();
    /// owner.enable_batching(Duration::from_millis(16));
    /// ```
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        Self {
            tree,
            build: super::BuildPipeline::new(),
            layout: super::LayoutPipeline::new(),
            paint: super::PaintPipeline::new(),
            root_element_id: None,
            on_build_scheduled: None,
            metrics: None,
            recovery: None,
            cancellation: None,
            frame_buffer: None,
        }
    }

    /// Get reference to the element tree
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.root_element_id
    }

    /// Set callback for when build is scheduled
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    // =========================================================================
    // Production Features (Optional)
    // =========================================================================

    /// Enable performance metrics
    ///
    /// Tracks FPS, frame times, phase timing, cache hit rates.
    ///
    /// # Overhead
    ///
    /// - CPU: ~1%
    /// - Memory: 480 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_metrics();
    ///
    /// // Build frame...
    ///
    /// // Check metrics
    /// if let Some(metrics) = owner.metrics() {
    ///     println!("FPS: {:.1}", metrics.fps());
    ///     println!("Avg frame time: {:?}", metrics.avg_frame_time());
    /// }
    /// ```
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
    ///
    /// Defines how the pipeline handles errors during build/layout/paint.
    ///
    /// # Recovery Policies
    ///
    /// - `UseLastGoodFrame` - Production default, graceful degradation
    /// - `ShowErrorWidget` - Development mode, show error overlay
    /// - `SkipFrame` - Skip failed frame, continue with next
    /// - `Panic` - Testing mode, fail fast
    ///
    /// # Overhead
    ///
    /// - Memory: ~40 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::pipeline::RecoveryPolicy;
    ///
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_error_recovery(RecoveryPolicy::UseLastGoodFrame);
    /// ```
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
    ///
    /// Allows setting timeouts for long-running operations.
    ///
    /// # Overhead
    ///
    /// - Memory: ~24 bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_cancellation();
    ///
    /// // Set timeout for frame rendering
    /// if let Some(token) = owner.cancellation_token() {
    ///     token.set_timeout(Duration::from_millis(16));
    /// }
    /// ```
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

    // =========================================================================
    // Frame Buffer
    // =========================================================================

    /// Enable triple buffer for lock-free frame exchange
    ///
    /// Creates a TripleBuffer initialized with an empty layer.
    /// This allows the compositor thread to read frames while the renderer
    /// thread writes new frames without blocking.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_engine::ContainerLayer;
    /// use std::sync::Arc;
    ///
    /// let mut owner = PipelineOwner::new();
    /// let initial = Arc::new(Box::new(ContainerLayer::new()) as crate::BoxedLayer);
    /// owner.enable_frame_buffer(initial);
    ///
    /// // Renderer thread
    /// if let Some(layer) = owner.build_frame(constraints) {
    ///     owner.publish_frame(layer);
    /// }
    ///
    /// // Compositor thread
    /// if let Some(buffer) = owner.frame_buffer() {
    ///     let layer_arc = buffer.read();
    ///     let layer = layer_arc.read();
    ///     compositor.present(&*layer);
    /// }
    /// ```
    pub fn enable_frame_buffer(&mut self, initial: Arc<crate::BoxedLayer>) {
        self.frame_buffer = Some(super::TripleBuffer::new(initial));
    }

    /// Disable frame buffer
    pub fn disable_frame_buffer(&mut self) {
        self.frame_buffer = None;
    }

    /// Get reference to frame buffer (if enabled)
    pub fn frame_buffer(&self) -> Option<&super::TripleBuffer<Arc<crate::BoxedLayer>>> {
        self.frame_buffer.as_ref()
    }

    /// Get mutable reference to frame buffer (if enabled)
    pub fn frame_buffer_mut(&mut self) -> Option<&mut super::TripleBuffer<Arc<crate::BoxedLayer>>> {
        self.frame_buffer.as_mut()
    }

    /// Publish a frame to the triple buffer (convenience method)
    ///
    /// Writes the layer to the write buffer and swaps to make it available.
    /// Does nothing if frame buffer is not enabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(layer) = owner.build_frame(constraints) {
    ///     owner.publish_frame(layer);
    /// }
    /// ```
    pub fn publish_frame(&mut self, layer: crate::BoxedLayer) {
        if let Some(buffer) = self.frame_buffer.as_mut() {
            // Get write buffer and update it
            let write_buf = buffer.write();
            let mut write_guard = write_buf.write();
            *write_guard = Arc::new(layer);
            drop(write_guard);

            // Swap to publish
            buffer.swap();
        }
    }

    // =========================================================================
    // Build Batching
    // =========================================================================

    /// Enable build batching to optimize rapid setState() calls
    ///
    /// When enabled, multiple setState() calls within `batch_duration` are
    /// batched into a single rebuild, improving performance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// owner.enable_batching(Duration::from_millis(16)); // 1 frame
    ///
    /// // Multiple setState calls
    /// owner.schedule_build(id1, 0);
    /// owner.schedule_build(id2, 1); // Batched!
    /// owner.schedule_build(id1, 0); // Duplicate - saved!
    ///
    /// // Later...
    /// if owner.should_flush_batch() {
    ///     owner.flush_batch();
    /// }
    /// ```
    pub fn enable_batching(&mut self, batch_duration: Duration) {
        self.build.enable_batching(batch_duration);
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        self.build.disable_batching();
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.build.is_batching_enabled()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.build.should_flush_batch()
    }

    /// Flush the current batch
    ///
    /// Moves all pending batched builds to dirty_elements for processing.
    pub fn flush_batch(&mut self) {
        self.build.flush_batch();
    }

    /// Get batching statistics (batches_flushed, builds_saved)
    pub fn batching_stats(&self) -> (usize, usize) {
        self.build.batching_stats()
    }

    // =========================================================================
    // Root Management
    // =========================================================================

    /// Inflate a widget and set it as the root of the tree
    ///
    /// This is the preferred way to create the root element, as it automatically
    /// determines the correct element type based on the widget variant (Stateless →
    /// ComponentElement, Stateful → StatefulElement, etc.)
    ///
    /// # Arguments
    ///
    /// - `root_widget`: The root widget of the application
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// let root_id = owner.inflate_root(MyApp::new().into_widget());
    /// ```

    // NOTE: Commented out during Widget → View migration
    // TODO(Phase 5): Reimplement using View system
    /*
    pub fn inflate_root(&mut self, root_widget: crate::Widget) -> ElementId {
        let root_element = self.inflate_widget(root_widget);
        self.set_root(root_element)
    }
    */

    /// Mount an element as the root of the tree
    ///
    /// # Arguments
    ///
    /// - `root_element`: The element to set as root (typically ComponentElement or RenderElement)
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut owner = PipelineOwner::new();
    /// let root = Element::Component(ComponentElement::new(MyApp::new()));
    /// let root_id = owner.set_root(root);
    /// ```
    pub fn set_root(&mut self, mut root_element: Element) -> ElementId {
        let mut tree_guard = self.tree.write();

        // Mount the element (no parent, slot 0)
        root_element.mount(None, Some(crate::foundation::Slot::new(0)));

        // Insert into tree
        let id = tree_guard.insert(root_element);

        // Note: init_state() method removed during Widget → View migration
        // TODO(Phase 5): Call view.build() to create child
        /*
        // Call init_state if root is a StatefulElement
        if let Some(Element::Component(stateful)) = tree_guard.get_mut(id) {
            stateful.init_state(id, std::sync::Arc::clone(&self.tree));
        }
        */

        drop(tree_guard);

        self.root_element_id = Some(id);

        // Root starts dirty
        self.schedule_build_for(id, 0);

        id
    }

    // =========================================================================
    // Build Scheduling
    // =========================================================================

    /// Schedule an element for rebuild
    ///
    /// # Parameters
    ///
    /// - `element_id`: The element to rebuild
    /// - `depth`: The depth of the element in the tree (0 = root)
    ///
    /// Elements are sorted by depth before building to ensure parents build before children.
    ///
    /// If batching is enabled, the build will be batched with other builds.
    /// Otherwise, it's added to dirty_elements immediately.
    pub fn schedule_build_for(&mut self, element_id: ElementId, depth: usize) {
        self.build.schedule(element_id, depth);

        // Trigger callback
        if let Some(ref callback) = self.on_build_scheduled {
            callback();
        }
    }

    /// Get count of dirty elements waiting to rebuild
    pub fn dirty_count(&self) -> usize {
        self.build.dirty_count()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.build.is_in_build_scope()
    }

    // =========================================================================
    // Build Execution
    // =========================================================================

    /// Execute a build scope
    ///
    /// This sets the build scope flag to prevent setState during build,
    /// then executes the callback.
    ///
    /// # Build Scope Isolation
    ///
    /// Any `markNeedsBuild()` calls during the scope will be deferred and
    /// processed after the scope completes. This prevents infinite rebuild loops.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// owner.build_scope(|owner| {
    ///     owner.flush_build();
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If the callback panics, the build scope will be properly cleaned up,
    /// but the panic will propagate.
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        // Manually manage build scope since we need to pass full PipelineOwner
        if self.build.is_in_build_scope() {
            tracing::warn!("Nested build_scope detected! This may indicate incorrect usage.");
        }

        // Set scope flag
        self.build.set_build_scope(true);

        // Execute callback
        let result = f(self);

        // Clear scope flag
        self.build.set_build_scope(false);

        result
    }

    /// Lock state changes
    ///
    /// Executes callback with state changes locked.
    /// Any setState calls during this time will be ignored/warned.
    pub fn lock_state<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        // Save previous state (though in practice it should always be false)
        // We don't have a method to get build_locked, so assume it's false
        self.build.set_build_locked(true);

        // Execute callback
        let result = f(self);

        // Restore previous state
        self.build.set_build_locked(false);

        result
    }

    /// Flush the build phase
    ///
    /// Rebuilds all dirty elements in depth order (parents before children).
    /// This ensures that parent widgets build before their children.
    pub fn flush_build(&mut self) {
        let mut tree_guard = self.tree.write();
        self.build.rebuild_dirty(&mut tree_guard);
    }

    /// Finalize the tree after build
    ///
    /// This locks further builds and performs any cleanup needed.
    pub fn finalize_tree(&mut self) {
        self.lock_state(|owner| {
            if !owner.build.has_dirty() {
                #[cfg(debug_assertions)]
                debug_println!(PRINT_BUILD_SCOPE, "finalize_tree: tree is clean");
            } else {
                tracing::warn!(
                    dirty_count = owner.build.dirty_count(),
                    "finalize_tree: dirty elements remaining after build"
                );
            }
        });
    }

    // =========================================================================
    // Hot Reload Support
    // =========================================================================

    /// Reassemble the entire element tree for hot reload
    ///
    /// Traverses all elements and calls `reassemble()` on stateful widgets,
    /// then marks all elements dirty for rebuild. This is called when source
    /// code changes during development.
    ///
    /// # How It Works
    ///
    /// 1. Walk the element tree depth-first from root
    /// 2. For each StatefulElement, call `state.reassemble()`
    /// 3. Mark all elements dirty for rebuild
    /// 4. Caller should then call `flush_build()` to rebuild the tree
    ///
    /// # Performance
    ///
    /// This is an expensive operation (O(n) tree traversal + full rebuild).
    /// Only use during development with hot reload. Tree traversal typically
    /// takes ~50µs for 1000 elements, plus rebuild time.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // When source files change during hot reload:
    /// owner.reassemble_tree();
    /// owner.build_scope(|o| o.flush_build());
    /// ```
    ///
    /// # Returns
    ///
    /// The number of StatefulElements that were reassembled
    pub fn reassemble_tree(&mut self) -> usize {
        let root_id = match self.root_element_id {
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
                // Note: reassemble() method removed during Widget → View migration
                // TODO(Phase 5): Implement View-based hot reload
                /*
                match element {
                    Element::Component(stateful) => {
                        // Call reassemble on the state
                        stateful.reassemble();
                        reassembled_count += 1;

                        #[cfg(debug_assertions)]
                        debug_println!(
                            PRINT_BUILD_SCOPE,
                            "  Reassembled state for element {:?}",
                            element_id
                        );
                    }
                    _ => {
                        // Non-stateful elements: just mark dirty
                        element.mark_dirty();
                    }
                }
                */
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
    ///
    /// Returns Vec<(ElementId, depth)> for processing order.
    /// Used by reassemble_tree to traverse the entire element tree.
    fn collect_all_elements(
        &self,
        tree: &ElementTree,  // In same module (pipeline)
        root_id: ElementId,
    ) -> Vec<(ElementId, usize)> {
        let mut result = Vec::new();
        self.collect_elements_recursive(tree, root_id, 0, &mut result);
        result
    }

    /// Recursive helper for collect_all_elements
    ///
    /// Performs depth-first traversal of the element tree.
    fn collect_elements_recursive(
        &self,
        tree: &ElementTree,  // In same module (pipeline)
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
    // Layout & Paint Phases
    // =========================================================================

    /// Request layout for a Render
    ///
    /// Adds the node to the layout dirty list if not already present.
    /// Called by Render::mark_needs_layout().
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.layout.mark_dirty(node_id);
    }

    /// Request paint for a Render
    ///
    /// Adds the node to the paint dirty list if not already present.
    /// Called by Render::mark_needs_paint().
    pub fn request_paint(&mut self, node_id: ElementId) {
        self.paint.mark_dirty(node_id);
    }

    /// Flush the layout phase
    ///
    /// Performs layout on all dirty render objects in the tree.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root element exists
    pub fn flush_layout(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<flui_types::Size>, super::PipelineError> {
        let mut tree = self.tree.write();

        // Process all dirty render objects
        let _count = self.layout.compute_layout(&mut tree, constraints)?;

        #[cfg(debug_assertions)]
        if _count > 0 {
            tracing::debug!("flush_layout: Laid out {} render objects", _count);
        }

        // Get root element's computed size
        let root_id = match self.root_element_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let root_element = match tree.get(root_id) {
            Some(elem) => elem,
            None => return Ok(None),
        };

        // Only return size if root is a RenderElement
        if let crate::element::Element::Render(render_elem) = root_element {
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.read();
            Ok(render_state.size())
        } else {
            // Root is ComponentElement or ProviderElement - no size
            Ok(None)
        }
    }

    /// Flush the paint phase
    ///
    /// Generates paint layers for all dirty render objects.
    ///
    /// # Returns
    ///
    /// The root layer for composition, or None if no root element exists.
    ///
    /// # Note
    ///
    /// Currently generates layers but returns a stub empty layer.
    /// Full layer tree composition will be implemented in a future update.
    pub fn flush_paint(&mut self) -> Result<Option<crate::BoxedLayer>, super::PipelineError> {
        let mut tree = self.tree.write();

        // Process all dirty render objects
        let _count = self.paint.generate_layers(&mut tree)?;

        #[cfg(debug_assertions)]
        if _count > 0 {
            tracing::debug!("flush_paint: Painted {} render objects", _count);
        }

        // Get root element's layer
        let root_id = match self.root_element_id {
            Some(id) => id,
            None => return Ok(None),
        };

        let root_element = match tree.get(root_id) {
            Some(elem) => elem,
            None => return Ok(None),
        };

        // Only paint if root is a RenderElement
        if let crate::element::Element::Render(render_elem) = root_element {
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.read();
            let offset = render_state.offset();
            drop(render_state); // Drop before calling paint

            let render_object = render_elem.render_object();

            // Generate root layer
            Ok(Some(render_object.paint(&tree, offset)))
        } else {
            // Root is ComponentElement or ProviderElement
            // Walk tree to find first RenderElement and paint from there
            // For now, return empty container layer
            Ok(Some(Box::new(flui_engine::ContainerLayer::new())))
        }
    }

    /// Build a complete frame
    ///
    /// This is the high-level entry point for rendering a frame.
    /// Orchestrates the three phases: build → layout → paint.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The root layer for the compositor, or None if no root element exists.
    ///
    /// # Pipeline Flow
    ///
    /// 1. **Build Phase**: Rebuilds all dirty widgets (ComponentElements)
    /// 2. **Layout Phase**: Computes sizes for all dirty RenderElements
    /// 3. **Paint Phase**: Generates paint layers for all dirty RenderElements
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::PipelineOwner;
    /// use flui_types::{Size, constraints::BoxConstraints};
    ///
    /// let mut owner = PipelineOwner::new();
    /// owner.set_root(my_element);
    ///
    /// // Build complete frame at 800x600
    /// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    /// if let Some(layer) = owner.build_frame(constraints) {
    ///     // Compositor can now render the layer
    ///     compositor.present(layer);
    /// }
    /// ```
    pub fn build_frame(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<crate::BoxedLayer>, super::PipelineError> {
        #[cfg(debug_assertions)]
        tracing::debug!("build_frame: Starting frame with constraints {:?}", constraints);

        // Phase 1: Build (rebuild dirty widgets)
        #[cfg(debug_assertions)]
        let build_count = self.build.dirty_count();

        self.flush_build();

        #[cfg(debug_assertions)]
        if build_count > 0 {
            tracing::debug!("build_frame: Build phase rebuilt {} widgets", build_count);
        }

        // Phase 2: Layout (compute sizes and positions)
        let _root_size = self.flush_layout(constraints)?;

        #[cfg(debug_assertions)]
        if let Some(size) = _root_size {
            tracing::debug!("build_frame: Layout phase computed root size {:?}", size);
        }

        // Phase 3: Paint (generate layer tree)
        let layer = self.flush_paint()?;

        #[cfg(debug_assertions)]
        if layer.is_some() {
            tracing::debug!("build_frame: Paint phase generated layer tree");
        }

        Ok(layer)
    }

    // =========================================================================
    // Widget Inflation
    // =========================================================================

    // NOTE: Commented out during Widget → View migration
    /*
    /// Inflate a widget into an element
    ///
    /// This creates the appropriate Element type based on the widget's type:
    /// - StatelessWidget → ComponentElement
    /// - StatefulWidget → StatefulElement
    /// - InheritedWidget → InheritedElement
    /// - RenderWidget → RenderElement
    /// - ParentDataWidget → ParentDataElement
    fn inflate_widget(&self, widget: crate::Widget) -> Element {
        use crate::view::BuildContext;  // Moved to view in Phase 1
        use crate::element::{ComponentElement, RenderElement};

        // Determine element type based on widget variant
        match widget {
            Widget::Stateless(_) => {
                #[cfg(debug_assertions)]
                tracing::trace!("Creating ComponentElement for Stateless widget");
                Element::Component(ComponentElement::new(widget))
            }
            Widget::Stateful(stateful_widget) => {
                #[cfg(debug_assertions)]
                tracing::trace!("Creating StatefulElement for Stateful widget");

                // Create state from the stateful widget
                let state = stateful_widget.create_state();

                // Convert Box<dyn State> to Box<dyn DynState>
                let dyn_state = crate::element::boxed_state_from_state(state);

                // Create ComponentElement with stateful widget and state
                let element = Element::Component(ComponentElement::new_stateful(
                    Widget::Stateful(stateful_widget),
                    dyn_state,
                ));
                element
            }
            Widget::Inherited(_) => {
                // TODO: Create InheritedElement when implemented
                Element::Component(ComponentElement::new(widget))
            }
            Widget::Render(_) => {
                #[cfg(debug_assertions)]
                tracing::trace!("Creating RenderElement for Render widget");

                // Create a temporary BuildContext for render object creation
                // We use ElementId 0 as placeholder since element isn't in tree yet
                let temp_context = BuildContext::new(self.tree.clone(), 0);

                // Create render object from widget
                let render_object = widget.create_render_object(&temp_context)
                    .expect("RenderWidget must have create_render_object");

                Element::Render(RenderElement::new(widget, render_object))
            }
            Widget::ParentData(_) => {
                // TODO: Create ParentDataElement when implemented
                Element::Component(ComponentElement::new(widget))
            }
        }
    }
    */
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildContext;
    use crate::element::{ComponentElement, Element};
    use crate::widget::{StatelessWidget, Widget};

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget;

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Widget {
            Box::new(TestWidget)
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
        let id = 42; // Arbitrary ElementId

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        // Scheduling same element again should not duplicate
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
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
        let id = 42;

        // Normal scheduling works
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);

        owner.lock_state(|o| {
            // Scheduling while locked should be ignored
            let id2 = 43;
            o.schedule_build_for(id2, 0);
            assert_eq!(o.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_depth_sorting() {
        let mut owner = PipelineOwner::new();

        let id1 = 1;
        let id2 = 2;
        let id3 = 3;

        // Schedule in random order
        owner.schedule_build_for(id2, 2);
        owner.schedule_build_for(id1, 1);
        owner.schedule_build_for(id3, 0);

        // flush_build sorts by depth before rebuilding
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

        let id = 42;
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
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = 42;

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
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id1 = 1;
        let id2 = 2;
        let id3 = 3;

        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);
        owner.schedule_build_for(id3, 2);

        owner.flush_batch();

        // All 3 should be dirty
        assert_eq!(owner.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(10));

        let id = 42;
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
        let mut owner = PipelineOwner::new();
        // Batching not enabled

        let id = 42;
        owner.schedule_build_for(id, 0);

        // Should add directly to dirty elements
        assert_eq!(owner.dirty_count(), 1);

        // flush_batch should be no-op
        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));

        let id = 42;

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
        let component = ComponentElement::new(TestWidget);
        let root = Element::Component(component);

        let root_id = owner.set_root(Box::new(root));

        assert_eq!(owner.root_element_id(), Some(root_id));
        // Root should be marked dirty
        assert_eq!(owner.dirty_count(), 1);
    }
}
