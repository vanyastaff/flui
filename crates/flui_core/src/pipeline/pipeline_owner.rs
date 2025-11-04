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

use crate::element::{Element, ElementId};
use super::{ElementTree, FrameCoordinator, RootManager};

#[cfg(debug_assertions)]
use crate::debug_println;

/// PipelineOwner - thin facade over focused components
///
/// This is a facade that composes focused components to provide a clean API.
/// It follows the Facade Pattern from Gang of Four design patterns.
///
/// # Responsibilities (Delegated)
///
/// - **Frame Coordination** → FrameCoordinator
/// - **Root Management** → RootManager
/// - **Element Storage** → ElementTree
/// - **Build Scheduling** → FrameCoordinator.build
/// - **Layout Scheduling** → FrameCoordinator.layout
/// - **Paint Scheduling** → FrameCoordinator.paint
///
/// # Example
///
/// ```rust,ignore
/// let mut owner = PipelineOwner::new();
///
/// // Set root
/// let root_id = owner.set_root(my_element);
///
/// // Mark element dirty
/// owner.schedule_build_for(element_id, depth);
///
/// // Build frame
/// let layer = owner.build_frame(constraints)?;
/// ```
pub struct PipelineOwner {
    /// The element tree (shared storage)
    tree: Arc<RwLock<ElementTree>>,

    /// Frame coordinator (orchestrates pipeline phases)
    coordinator: FrameCoordinator,

    /// Root manager (tracks root element)
    root_mgr: RootManager,

    /// Callback when a build is scheduled (optional)
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,

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
    frame_buffer: Option<super::TripleBuffer<Arc<crate::BoxedLayer>>>,
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
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new pipeline owner
    ///
    /// Creates a basic pipeline without production features.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let owner = PipelineOwner::new();
    /// ```
    #[deprecated(
        since = "0.1.0",
        note = "Prefer using PipelineBuilder for better API ergonomics and production features. \
                Example: PipelineBuilder::new().build()"
    )]
    pub fn new() -> Self {
        Self {
            tree: Arc::new(RwLock::new(ElementTree::new())),
            coordinator: FrameCoordinator::new(),
            root_mgr: RootManager::new(),
            on_build_scheduled: None,
            metrics: None,
            recovery: None,
            cancellation: None,
            frame_buffer: None,
        }
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
        self.coordinator.layout_mut().mark_dirty(node_id);
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
        self.coordinator.flush_layout(&self.tree, self.root_mgr.root_id(), constraints)
    }

    /// Flush the paint phase
    ///
    /// Delegates to FrameCoordinator.
    pub fn flush_paint(&mut self) -> Result<Option<crate::BoxedLayer>, super::PipelineError> {
        self.coordinator.flush_paint(&self.tree, self.root_mgr.root_id())
    }

    /// Build a complete frame
    ///
    /// Delegates to FrameCoordinator.
    pub fn build_frame(
        &mut self,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Result<Option<crate::BoxedLayer>, super::PipelineError> {
        #[cfg(debug_assertions)]
        tracing::debug!("build_frame: Starting frame with constraints {:?}", constraints);

        // Delegate to coordinator
        self.coordinator.build_frame(&self.tree, self.root_mgr.root_id(), constraints)
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
    pub fn publish_frame(&mut self, layer: crate::BoxedLayer) {
        if let Some(buffer) = self.frame_buffer.as_mut() {
            let write_buf = buffer.write();
            let mut write_guard = write_buf.write();
            *write_guard = Arc::new(layer);
            drop(write_guard);
            buffer.swap();
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

        let root_id = owner.set_root(root);

        assert_eq!(owner.root_element_id(), Some(root_id));
        // Root should be marked dirty
        assert_eq!(owner.dirty_count(), 1);
    }
}
