//! Build pipeline for widget rebuild phase.
//!
//! The build pipeline is responsible for:
//! - Rebuilding dirty widgets
//! - Updating element tree
//! - Reconciling widget changes
//! - Batching rapid setState() calls for performance
//!
//! # Design
//!
//! The build phase processes views top-down:
//! 1. Identify dirty elements (marked for rebuild)
//! 2. Call `View::build()` for each dirty element
//! 3. Reconcile old and new view trees
//! 4. Update element tree accordingly
//!
//! # Build Batching
//!
//! When enabled, BuildPipeline batches multiple setState() calls within a time window:
//!
//! ```rust,ignore
//! let mut build = BuildPipeline::new();
//! build.enable_batching(Duration::from_millis(16)); // One frame
//!
//! // Multiple setState calls
//! build.schedule(id1, 0);
//! build.schedule(id2, 1);
//! build.schedule(id1, 0); // Duplicate - batched!
//!
//! // Later...
//! if build.should_flush_batch() {
//!     build.flush_batch();
//! }
//! build.rebuild_dirty(&tree);
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! let mut build = BuildPipeline::new();
//! build.schedule(root_id, 0);
//! build.rebuild_dirty(&tree);
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::element::ElementId;
use crate::element::ElementTree;

#[cfg(debug_assertions)]
use crate::debug_println;

/// Element type classification for rebuild dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementType {
    Component,
    Render,
    Provider,
}

/// Build batching system for performance optimization
///
/// Batches multiple setState() calls into a single rebuild to avoid redundant work.
/// This is especially useful for:
/// - Animations with many rapid setState() calls
/// - User input triggering multiple widgets
/// - Computed values that update multiple times per frame
#[derive(Debug)]
struct BuildBatcher {
    /// Elements pending in current batch (with depths)
    pending: HashMap<ElementId, usize>,
    /// When the current batch started
    batch_start: Option<Instant>,
    /// How long to wait before flushing batch
    batch_duration: Duration,
    /// Total number of batches flushed
    batches_flushed: usize,
    /// Total number of builds saved by batching
    builds_saved: usize,
}

impl BuildBatcher {
    fn new(batch_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            batch_start: None,
            batch_duration,
            batches_flushed: 0,
            builds_saved: 0,
        }
    }

    /// Add element to batch
    fn schedule(&mut self, element_id: ElementId, depth: usize) {
        // Start batch timer if first element
        if self.pending.is_empty() {
            self.batch_start = Some(Instant::now());
        }

        // Track if this is a duplicate (saved build)
        if self.pending.insert(element_id, depth).is_some() {
            self.builds_saved += 1;
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: element {:?} already in batch (saved 1 build)",
                element_id
            );
        } else {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: added element {:?} to batch",
                element_id
            );
        }
    }

    /// Check if batch is ready to flush
    fn should_flush(&self) -> bool {
        if let Some(start) = self.batch_start {
            start.elapsed() >= self.batch_duration
        } else {
            false
        }
    }

    /// Take all pending builds
    fn take_pending(&mut self) -> HashMap<ElementId, usize> {
        self.batches_flushed += 1;
        self.batch_start = None;
        std::mem::take(&mut self.pending)
    }

    /// Get statistics (batches_flushed, builds_saved)
    fn stats(&self) -> (usize, usize) {
        (self.batches_flushed, self.builds_saved)
    }
}

/// Build pipeline manages widget rebuild phase.
///
/// Tracks which elements need rebuilding (with their depths) and processes them
/// in depth-first order (parents before children).
///
/// # Depth Tracking
///
/// Each element is tracked with its depth in the tree (0 = root).
/// This ensures parent widgets build before children, which is critical
/// for correct widget tree construction.
#[derive(Debug)]
pub struct BuildPipeline {
    /// Elements that need rebuilding with their depths: (ElementId, depth)
    dirty_elements: Vec<(ElementId, usize)>,
    /// Build count for tracking
    build_count: usize,
    /// Whether currently in a build scope
    in_build_scope: bool,
    /// Whether state changes are locked
    build_locked: bool,
    /// Optional batching system
    batcher: Option<BuildBatcher>,
    /// Rebuild queue for deferred rebuilds from signals
    rebuild_queue: super::RebuildQueue,
}

impl BuildPipeline {
    /// Creates a new build pipeline with a rebuild queue.
    pub fn new_with_queue(rebuild_queue: super::RebuildQueue) -> Self {
        Self {
            dirty_elements: Vec::new(),
            build_count: 0,
            in_build_scope: false,
            build_locked: false,
            batcher: None,
            rebuild_queue,
        }
    }

    /// Creates a new build pipeline.
    pub fn new() -> Self {
        Self::new_with_queue(super::RebuildQueue::new())
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
    pub fn schedule(&mut self, element_id: ElementId, depth: usize) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "schedule element={:?}, depth={}, build_locked={}",
            element_id,
            depth,
            self.build_locked
        );

        if self.build_locked {
            tracing::warn!(
                element_id = ?element_id,
                "Attempted to schedule build while state is locked. Build deferred."
            );
            return;
        }

        // If batching enabled, use batcher
        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(element_id, depth);
            return;
        }

        // Add directly to dirty elements
        // Note: Duplicates are allowed and will be deduplicated during rebuild_dirty()
        // This is more efficient than O(n) check on every schedule()
        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_SCHEDULE_BUILD,
            "Scheduling element {:?} for rebuild (depth {})",
            element_id,
            depth
        );

        self.dirty_elements.push((element_id, depth));
    }

    /// Checks if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Returns the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.in_build_scope
    }

    // =========================================================================
    // Build Batching
    // =========================================================================

    /// Enable build batching with given duration
    ///
    /// When batching is enabled, multiple setState() calls within the batch_duration
    /// will be combined into a single rebuild. This dramatically improves performance
    /// for animations and other rapid state changes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// build.enable_batching(Duration::from_millis(16)); // 1 frame
    ///
    /// // Multiple setState calls
    /// build.schedule(id1, 0);
    /// build.schedule(id2, 1); // Batched!
    /// build.schedule(id1, 0); // Duplicate - saved!
    ///
    /// // Later...
    /// if build.should_flush_batch() {
    ///     build.flush_batch();
    /// }
    /// ```
    pub fn enable_batching(&mut self, batch_duration: Duration) {
        #[cfg(debug_assertions)]
        println!("Enabling build batching with duration {:?}", batch_duration);
        self.batcher = Some(BuildBatcher::new(batch_duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        if let Some(ref batcher) = self.batcher {
            let (batches, saved) = batcher.stats();
            #[cfg(debug_assertions)]
            println!(
                "Disabling build batching (flushed {} batches, saved {} builds)",
                batches, saved
            );
        }
        self.batcher = None;
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.batcher
            .as_ref()
            .map(|b| b.should_flush())
            .unwrap_or(false)
    }

    /// Flush the current batch
    ///
    /// Moves all pending batched builds to dirty_elements for processing.
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let pending = batcher.take_pending();
            if !pending.is_empty() {
                #[cfg(debug_assertions)]
                debug_println!(
                    PRINT_SCHEDULE_BUILD,
                    "Flushing batch: {} elements",
                    pending.len()
                );

                for (element_id, depth) in pending {
                    // Add to dirty elements (bypass batching)
                    if !self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
                        self.dirty_elements.push((element_id, depth));
                    }
                }
            }
        }
    }

    /// Get batching statistics (batches_flushed, builds_saved)
    pub fn batching_stats(&self) -> (usize, usize) {
        self.batcher.as_ref().map(|b| b.stats()).unwrap_or((0, 0))
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
    /// build.build_scope(|build| {
    ///     build.rebuild_dirty(&mut tree);
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
        if self.in_build_scope {
            tracing::warn!("Nested build_scope detected! This may indicate incorrect usage.");
        }

        self.in_build_scope = true;

        // Execute callback
        let result = f(self);

        // Clear flag
        self.in_build_scope = false;

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
        let was_locked = self.build_locked;
        self.build_locked = true;
        let result = f(self);
        self.build_locked = was_locked;
        result
    }

    /// Rebuilds all dirty elements.
    ///
    /// Processes elements top-down (sorted by depth), rebuilding widgets and
    /// reconciling the element tree.
    ///
    /// Returns the number of elements rebuilt.
    pub fn rebuild_dirty(&mut self, tree: &Arc<parking_lot::RwLock<ElementTree>>) -> usize {
        if self.dirty_elements.is_empty() {
            return 0;
        }

        self.build_count += 1;
        let build_num = self.build_count;
        let dirty_count = self.dirty_elements.len();

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "rebuild_dirty #{}: rebuilding {} dirty elements",
            build_num,
            dirty_count
        );

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Deduplicate - remove any duplicate ElementIds while preserving depth order
        self.dirty_elements.dedup_by_key(|(id, _)| *id);

        // Take the dirty list to avoid borrow conflicts
        let mut dirty = std::mem::take(&mut self.dirty_elements);
        let mut rebuilt_count = 0;

        // Rebuild each element
        for (element_id, depth) in dirty.drain(..) {
            #[cfg(debug_assertions)]
            tracing::trace!(
                "rebuild_dirty: Processing element {:?} at depth {}",
                element_id,
                depth
            );

            // Determine element type (read-only scope)
            let element_type = {
                let tree_guard = tree.read();
                match tree_guard.get(element_id) {
                    Some(elem) => {
                        if elem.as_component().is_some() {
                            Some(ElementType::Component)
                        } else if elem.as_provider().is_some() {
                            Some(ElementType::Provider)
                        } else if elem.as_render().is_some() {
                            Some(ElementType::Render)
                        } else {
                            None
                        }
                    }
                    None => {
                        tracing::error!(
                            element_id = ?element_id,
                            "Element marked dirty but not found in tree during rebuild"
                        );
                        None
                    }
                }
            };

            // Dispatch rebuild based on element type
            match element_type {
                Some(ElementType::Component) => {
                    if self.rebuild_component(tree, element_id, depth) {
                        rebuilt_count += 1;
                    }
                }

                Some(ElementType::Render) => {
                    // RenderElements don't rebuild - they only relayout
                    #[cfg(debug_assertions)]
                    tracing::trace!(
                        "Render element {:?} skipped (rebuilds via layout)",
                        element_id
                    );
                }

                Some(ElementType::Provider) => {
                    if self.rebuild_provider(tree, element_id, depth) {
                        rebuilt_count += 1;
                    }
                }

                None => {
                    // Element not found or unrecognized type
                }
            }

            #[cfg(debug_assertions)]
            tracing::trace!("Processed element {:?}", element_id);
        }

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "rebuild_dirty #{}: complete ({} elements rebuilt)",
            build_num,
            rebuilt_count
        );

        rebuilt_count
    }

    /// Rebuild a ComponentElement
    ///
    /// Creates BuildContext, calls view.build(), and reconciles child elements.
    fn rebuild_component(
        &mut self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
        depth: usize,
    ) -> bool {
        #[cfg(debug_assertions)]
        tracing::debug!("Rebuilding component element {:?}", element_id);

        // Phase 1: Extract component data (minimize lock time)
        let (view, old_child_id, hook_context) = {
            let mut tree_guard = tree.write();
            let element = match tree_guard.get_mut(element_id) {
                Some(e) => e,
                None => return false,
            };

            let component = match element.as_component_mut() {
                Some(c) => c,
                None => return false,
            };

            // Clone view for rebuild
            let view = component.view().clone_box();
            let old_child = component.child();

            // Extract or create hook context from component state
            let hook_context: std::sync::Arc<parking_lot::Mutex<crate::hooks::HookContext>> =
                if let Some(ctx) = component
                    .state_mut()
                    .downcast_mut::<std::sync::Arc<parking_lot::Mutex<crate::hooks::HookContext>>>()
                {
                    // Reuse existing HookContext (preserves hook state across rebuilds!)
                    ctx.clone()
                } else {
                    // First build - create new HookContext and store it
                    let ctx =
                        std::sync::Arc::new(parking_lot::Mutex::new(crate::hooks::HookContext::new()));
                    // Replace the state Box with our new HookContext
                    component.set_state(Box::new(ctx.clone()));
                    ctx
                };

            (view, old_child, hook_context)
        };

        // Phase 2: Build new child view (outside locks)
        let ctx = crate::view::BuildContext::with_hook_context_and_queue(
            tree.clone(),
            element_id,
            hook_context.clone(),
            self.rebuild_queue.clone(),
        );

        // Set up ComponentId for hooks
        // Convert ElementId to ComponentId (hooks use u64, ElementId is usize)
        let component_id = crate::hooks::ComponentId(element_id.get() as u64);

        // Begin component rendering (locks temporarily)
        {
            let mut hook_ctx = hook_context.lock();
            hook_ctx.begin_component(component_id);
        }

        // Set up thread-local BuildContext and build
        // Note: BuildContext will lock/unlock hook_context as needed during hooks
        let new_element = crate::view::with_build_context(&ctx, || view.build_any());

        // End component rendering (locks temporarily)
        {
            let mut hook_ctx = hook_context.lock();
            hook_ctx.end_component();
        }

        // Phase 3: Reconcile child (write lock)

        {
            let mut tree_guard = tree.write();

            // Reconcile based on old/new child state
            match (old_child_id, Some(new_element)) {
                (Some(old_id), Some(new_element)) => {
                    // Check if we can update in-place or need to replace
                    let can_update = {
                        let old_elem = tree_guard.get(old_id);
                        match old_elem {
                            Some(old) => {
                                // Check if types match
                                std::mem::discriminant(old) == std::mem::discriminant(&new_element)
                            }
                            None => false,
                        }
                    };

                    if can_update {
                        // Update existing child element in-place
                        if let Some(old) = tree_guard.get_mut(old_id) {
                            *old = new_element;
                            // Schedule child for rebuild
                            self.schedule(old_id, depth + 1);
                        }
                    } else {
                        // Type changed - need to replace
                        // Remove old child
                        let _ = tree_guard.remove(old_id);

                        // Insert new child
                        let new_id = tree_guard.insert(new_element);

                        // Mount new child
                        if let Some(child) = tree_guard.get_mut(new_id) {
                            child.mount(Some(element_id), None);
                        }

                        // Update parent component's child reference
                        if let Some(crate::element::Element::Component(component)) =
                            tree_guard.get_mut(element_id)
                        {
                            component.set_child(new_id);
                        }

                        self.schedule(new_id, depth + 1);
                    }
                }

                (None, Some(new_element)) => {
                    // No old child - insert new one
                    let new_id = tree_guard.insert(new_element);

                    // Mount new child
                    if let Some(child) = tree_guard.get_mut(new_id) {
                        child.mount(Some(element_id), None);
                    }

                    // Update parent component's child reference
                    if let Some(crate::element::Element::Component(component)) =
                        tree_guard.get_mut(element_id)
                    {
                        component.set_child(new_id);
                    }

                    self.schedule(new_id, depth + 1);
                }

                (Some(old_id), None) => {
                    // Had child before, none now - remove old child
                    let _ = tree_guard.remove(old_id);

                    // Update parent component
                    if let Some(crate::element::Element::Component(component)) =
                        tree_guard.get_mut(element_id)
                    {
                        component.clear_child();
                    }
                }

                (None, None) => {
                    // No child before, none now - nothing to do
                }
            }
        }

        true
    }

    /// Rebuild a ProviderElement and notify dependents
    ///
    /// Checks if provider data changed and schedules dependent elements for rebuild.
    fn rebuild_provider(
        &mut self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
        _depth: usize,
    ) -> bool {
        #[cfg(debug_assertions)]
        tracing::debug!("Rebuilding provider element {:?}", element_id);

        // Phase 1: Get dependents list (read-only)
        let dependents = {
            let tree_guard = tree.read();
            let element = match tree_guard.get(element_id) {
                Some(e) => e,
                None => return false,
            };

            let provider = match element.as_provider() {
                Some(p) => p,
                None => return false,
            };

            // Copy dependents list
            provider.dependents().iter().copied().collect::<Vec<_>>()
        };

        // Phase 2: Notify all dependents
        // For now, assume provider data changed (TODO: proper change detection)
        if !dependents.is_empty() {
            #[cfg(debug_assertions)]
            tracing::debug!(
                "Provider {:?} notifying {} dependents",
                element_id,
                dependents.len()
            );

            for dependent_id in dependents {
                // Calculate depth for dependent
                let dep_depth = self.calculate_depth(tree, dependent_id);
                self.schedule(dependent_id, dep_depth);
            }
        }

        true
    }

    /// Calculate element depth in tree
    fn calculate_depth(
        &self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
    ) -> usize {
        let tree_guard = tree.read();
        let mut depth = 0;
        let mut current = element_id;

        while let Some(element) = tree_guard.get(current) {
            if let Some(parent_id) = element.parent() {
                depth += 1;
                current = parent_id;
            } else {
                break;
            }
        }

        depth
    }

    /// Rebuilds all dirty elements using parallel execution (when feature enabled)
    ///
    /// This is an alternative to `rebuild_dirty()` that works with `Arc<RwLock<ElementTree>>`
    /// for thread-safe parallel execution.
    ///
    /// # Strategy
    ///
    /// - When `parallel` feature is enabled and element count > threshold: parallel execution
    /// - Otherwise: sequential execution
    ///
    /// Returns the number of elements rebuilt.
    pub fn rebuild_dirty_parallel(
        &mut self,
        tree: &std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> usize {
        if self.dirty_elements.is_empty() {
            return 0;
        }

        self.build_count += 1;
        let build_num = self.build_count;
        let dirty_count = self.dirty_elements.len();

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "rebuild_dirty_parallel #{}: rebuilding {} dirty elements",
            build_num,
            dirty_count
        );

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Deduplicate - remove any duplicate ElementIds while preserving depth order
        self.dirty_elements.dedup_by_key(|(id, _)| *id);

        // Take the dirty list to avoid borrow conflicts
        let dirty = std::mem::take(&mut self.dirty_elements);

        // Call parallel build implementation
        let count = super::parallel_build::rebuild_dirty_parallel(tree, dirty);

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "rebuild_dirty_parallel #{}: complete ({} elements processed)",
            build_num,
            count
        );

        count
    }

    /// Clears all dirty elements without rebuilding.
    pub fn clear_dirty(&mut self) {
        self.dirty_elements.clear();
    }

    // =========================================================================
    // Internal API (for PipelineOwner guards)
    // =========================================================================

    /// Set in_build_scope flag (internal use only)
    pub(super) fn set_build_scope(&mut self, value: bool) {
        self.in_build_scope = value;
    }

    /// Set build_locked flag (internal use only)
    pub(super) fn set_build_locked(&mut self, value: bool) {
        self.build_locked = value;
    }
}

impl Default for BuildPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule() {
        let mut build = BuildPipeline::new();

        assert!(!build.has_dirty());

        build.schedule(1, 0);

        assert!(build.has_dirty());
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_dirty_count() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.schedule(2, 1);

        assert_eq!(build.dirty_count(), 2);
    }

    #[test]
    fn test_clear_dirty() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.clear_dirty();

        assert!(!build.has_dirty());
    }

    #[test]
    fn test_schedule_duplicate() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.schedule(1, 0); // Duplicate

        // Should only have 1 dirty element
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut build = BuildPipeline::new();

        assert!(!build.is_in_build_scope());

        build.build_scope(|b| {
            assert!(b.is_in_build_scope());
        });

        assert!(!build.is_in_build_scope());
    }

    #[test]
    fn test_lock_state() {
        let mut build = BuildPipeline::new();

        // Normal scheduling works
        build.schedule(1, 0);
        assert_eq!(build.dirty_count(), 1);

        build.lock_state(|b| {
            // Scheduling while locked should be ignored
            b.schedule(2, 0);
            assert_eq!(b.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_batching_disabled_by_default() {
        let build = BuildPipeline::new();
        assert!(!build.is_batching_enabled());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut build = BuildPipeline::new();

        build.enable_batching(Duration::from_millis(16));
        assert!(build.is_batching_enabled());

        build.disable_batching();
        assert!(!build.is_batching_enabled());
    }

    #[test]
    fn test_batching_deduplicates() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        let id = 42;

        // Schedule same element 3 times
        build.schedule(id, 0);
        build.schedule(id, 0);
        build.schedule(id, 0);

        // Flush batch
        build.flush_batch();

        // Should only have 1 dirty element
        assert_eq!(build.dirty_count(), 1);

        // Stats should show 2 builds saved
        let (batches, saved) = build.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_multiple_elements() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        build.schedule(1, 0);
        build.schedule(2, 1);
        build.schedule(3, 2);

        build.flush_batch();

        // All 3 should be dirty
        assert_eq!(build.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(10));

        build.schedule(42, 0);

        // Should not flush immediately
        assert!(!build.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Now should flush
        assert!(build.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        let mut build = BuildPipeline::new();
        // Batching not enabled

        build.schedule(42, 0);

        // Should add directly to dirty elements
        assert_eq!(build.dirty_count(), 1);

        // flush_batch should be no-op
        build.flush_batch();
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        // Initial stats
        assert_eq!(build.batching_stats(), (0, 0));

        // Schedule same element twice
        build.schedule(42, 0);
        build.schedule(42, 0); // Duplicate

        // Flush
        build.flush_batch();

        // Should have 1 batch flushed, 1 build saved
        let (batches, saved) = build.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
    }
}
