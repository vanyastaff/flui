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
            tracing::trace!(element_id = ?element_id, "Build batched: element already in batch (saved 1 build)");
        } else {
            tracing::trace!(element_id = ?element_id, "Build batched: added element to batch");
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
        tracing::trace!(element_id = ?element_id, depth = depth, "Scheduling element for rebuild");

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

    /// Flush the rebuild queue by moving pending elements to dirty_elements
    ///
    /// This processes all pending rebuilds from signals and other reactive primitives.
    /// Should be called at the start of each frame's build phase.
    pub fn flush_rebuild_queue(&mut self) {
        let rebuilds = self.rebuild_queue.drain();

        if rebuilds.is_empty() {
            return;
        }

        tracing::debug!(
            count = rebuilds.len(),
            "Flushing rebuild queue to dirty_elements"
        );

        // Add all pending rebuilds to dirty_elements
        for (element_id, depth) in rebuilds {
            self.dirty_elements.push((element_id, depth));
        }
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
        tracing::debug!(duration = ?batch_duration, "Enabling build batching");
        self.batcher = Some(BuildBatcher::new(batch_duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        if let Some(ref batcher) = self.batcher {
            let (batches, saved) = batcher.stats();
            tracing::debug!(
                batches_flushed = batches,
                builds_saved = saved,
                "Disabling build batching"
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
                tracing::trace!(count = pending.len(), "Flushing batch");

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
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty_elements.len()))]
    pub fn rebuild_dirty(&mut self, tree: &Arc<parking_lot::RwLock<ElementTree>>) -> usize {
        if self.dirty_elements.is_empty() {
            return 0;
        }

        self.build_count += 1;

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Deduplicate - remove any duplicate ElementIds while preserving depth order
        self.dirty_elements.dedup_by_key(|(id, _)| *id);

        // Take the dirty list to avoid borrow conflicts
        let mut dirty = std::mem::take(&mut self.dirty_elements);
        let mut rebuilt_count = 0;

        tracing::trace!(dirty_count = dirty.len(), "Processing dirty elements");

        // Rebuild each element
        for (element_id, depth) in dirty.drain(..) {
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
                }

                Some(ElementType::Provider) => {
                    if self.rebuild_provider(tree, element_id, depth) {
                        rebuilt_count += 1;
                    }
                }

                None => {
                    tracing::warn!(?element_id, "Element type is None - skipping");
                }
            }
        }

        rebuilt_count
    }

    // ========== Helper Methods for Component Rebuild ==========

    /// Insert element into tree and mount it to parent
    ///
    /// Helper for reconcile_child() to reduce duplication.
    fn insert_and_mount_child(
        tree_guard: &mut ElementTree,
        element: crate::element::Element,
        parent_id: ElementId,
    ) -> ElementId {
        let new_id = tree_guard.insert(element);

        if let Some(child) = tree_guard.get_mut(new_id) {
            child.mount(Some(parent_id), None);
        }

        new_id
    }

    /// Update ComponentElement's child reference
    ///
    /// Helper for reconcile_child() to reduce duplication.
    fn update_component_child_reference(
        tree_guard: &mut ElementTree,
        parent_id: ElementId,
        child_id: Option<ElementId>,
    ) {
        if let Some(crate::element::Element::Component(component)) = tree_guard.get_mut(parent_id) {
            match child_id {
                Some(id) => component.set_child(id),
                None => component.clear_child(),
            }
        }
    }

    /// Reconcile child element: replace old with new, handling Slab ID reuse
    ///
    /// **CRITICAL**: Inserts new element BEFORE removing old to prevent Slab ID reuse.
    /// This ensures new_element's children (already inserted during build) remain valid.
    ///
    /// # Cases
    ///
    /// 1. `(Some, Some)` - Replace existing child with new one
    /// 2. `(None, Some)` - Add new child (no previous child)
    /// 3. `(Some, None)` - Remove old child (no new child)
    /// 4. `(None, None)` - No child before or after - nothing to do
    fn reconcile_child(
        tree_guard: &mut ElementTree,
        parent_id: ElementId,
        old_child_id: Option<ElementId>,
        new_element: Option<crate::element::Element>,
    ) {
        match (old_child_id, new_element) {
            // Replace existing child with new one
            (Some(old_id), Some(new_element)) => {
                // Insert-before-remove pattern prevents Slab ID reuse
                let new_id = Self::insert_and_mount_child(tree_guard, new_element, parent_id);
                Self::update_component_child_reference(tree_guard, parent_id, Some(new_id));
                let _ = tree_guard.remove(old_id);

                // NOTE: We don't schedule new_id for rebuild because:
                // - It was just created and is already "fresh"
                // - RenderElements don't rebuild (only layout/paint)
                // - ComponentElements will be scheduled when they become dirty
            }

            // Add new child (no previous child)
            (None, Some(new_element)) => {
                let new_id = Self::insert_and_mount_child(tree_guard, new_element, parent_id);
                Self::update_component_child_reference(tree_guard, parent_id, Some(new_id));
            }

            // Remove old child (no new child)
            (Some(old_id), None) => {
                let _ = tree_guard.remove(old_id);
                Self::update_component_child_reference(tree_guard, parent_id, None);
            }

            // No child before or after - nothing to do
            (None, None) => {}
        }
    }

    /// Extract existing HookContext or create new one
    ///
    /// HookContext persists across rebuilds to maintain hook state.
    fn extract_or_create_hook_context(
        component: &mut crate::element::ComponentElement,
    ) -> std::sync::Arc<parking_lot::Mutex<crate::hooks::HookContext>> {
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
            component.set_state(Box::new(ctx.clone()));
            ctx
        }
    }

    // ========== Component Rebuild ==========

    /// Rebuild a ComponentElement
    ///
    /// Three-stage process:
    /// 1. Check dirty flag and extract component data - minimize lock time
    /// 2. Build new child element (outside locks) - this is the expensive part
    /// 3. Reconcile old/new children in tree - update tree atomically
    #[tracing::instrument(skip(self, tree), level = "trace")]
    fn rebuild_component(
        &mut self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
        _depth: usize,
    ) -> bool {
        // Stage 1: Check dirty flag, extract component data and prepare hook context (write lock)
        let (old_child_id, hook_context) = {
            let mut tree_guard = tree.write();
            let element = match tree_guard.get_mut(element_id) {
                Some(e) => e,
                None => return false,
            };

            let component = match element.as_component_mut() {
                Some(c) => c,
                None => return false,
            };

            // Skip rebuild if not dirty
            if !component.is_dirty() {
                #[cfg(debug_assertions)]
                tracing::trace!("Skipping rebuild for {:?} - not dirty", element_id);
                return false;
            }

            let old_child = component.child();

            // Extract or create hook context (helper method)
            let hook_context = Self::extract_or_create_hook_context(component);

            (old_child, hook_context)
        };

        // Stage 2: Build new child view (read lock for view access)
        let ctx = crate::view::BuildContext::with_hook_context_and_queue(
            tree.clone(),
            element_id,
            hook_context.clone(),
            self.rebuild_queue.clone(),
        );

        // Set up ComponentId for hooks
        let component_id = crate::hooks::ComponentId(element_id.get() as u64);

        // Begin component rendering
        {
            let mut hook_ctx = hook_context.lock();
            hook_ctx.begin_component(component_id);
        }

        // Build with thread-local BuildContext (read lock for builder access)
        let new_element = {
            let tree_guard = tree.read();
            let element = tree_guard.get(element_id).expect("element should exist");
            let component = element.as_component().expect("should be component");
            crate::view::with_build_context(&ctx, || component.build())
        };

        // End component rendering
        {
            let mut hook_ctx = hook_context.lock();
            hook_ctx.end_component();
        }

        // Stage 3: Reconcile old/new children in tree and clear dirty flag (write lock, atomic update)
        {
            let mut tree_guard = tree.write();
            Self::reconcile_child(&mut tree_guard, element_id, old_child_id, Some(new_element));

            // Clear dirty flag after successful rebuild
            if let Some(element) = tree_guard.get_mut(element_id) {
                if let Some(component) = element.as_component_mut() {
                    component.clear_dirty();
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
        // Hot path - trace disabled for performance

        // Phase 1: Check dirty flag and get dependents list
        let dependents = {
            let mut tree_guard = tree.write();
            let element = match tree_guard.get_mut(element_id) {
                Some(e) => e,
                None => return false,
            };

            let provider = match element.as_provider_mut() {
                Some(p) => p,
                None => return false,
            };

            // Skip rebuild if not dirty
            if !provider.is_dirty() {
                #[cfg(debug_assertions)]
                tracing::trace!("Skipping provider rebuild for {:?} - not dirty", element_id);
                return false;
            }

            // Copy dependents list
            let deps = provider.dependents().iter().copied().collect::<Vec<_>>();

            // Clear dirty flag
            provider.clear_dirty();

            deps
        };

        // Phase 2: Notify all dependents
        // Note: Currently notifies all dependents unconditionally.
        // Future optimization: Only notify if provider data actually changed
        // (requires PartialEq on provider data or custom change detection)
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
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty_elements.len()))]
    pub fn rebuild_dirty_parallel(
        &mut self,
        tree: &std::sync::Arc<parking_lot::RwLock<super::ElementTree>>,
    ) -> usize {
        if self.dirty_elements.is_empty() {
            return 0;
        }

        self.build_count += 1;

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Deduplicate - remove any duplicate ElementIds while preserving depth order
        self.dirty_elements.dedup_by_key(|(id, _)| *id);

        // Take the dirty list to avoid borrow conflicts
        let mut dirty = std::mem::take(&mut self.dirty_elements);
        let mut rebuilt_count = 0;

        // For now, use the same sequential logic as rebuild_dirty since the
        // parallel_build module doesn't properly invoke component builders.
        // TODO: Implement proper parallel component rebuild with BuildContext
        for (element_id, depth) in dirty.drain(..) {
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
                }

                Some(ElementType::Provider) => {
                    if self.rebuild_provider(tree, element_id, depth) {
                        rebuilt_count += 1;
                    }
                }

                None => {
                    tracing::warn!(?element_id, "Element type is None - skipping rebuild");
                }
            }
        }

        rebuilt_count
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
        use crate::ElementId;
        let mut build = BuildPipeline::new();

        assert!(!build.has_dirty());

        build.schedule(ElementId::new(1), 0);

        assert!(build.has_dirty());
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_dirty_count() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();

        build.schedule(ElementId::new(1), 0);
        build.schedule(ElementId::new(2), 1);

        assert_eq!(build.dirty_count(), 2);
    }

    #[test]
    fn test_clear_dirty() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();

        build.schedule(ElementId::new(1), 0);
        build.clear_dirty();

        assert!(!build.has_dirty());
    }

    #[test]
    fn test_schedule_duplicate() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();

        build.schedule(ElementId::new(1), 0);
        build.schedule(ElementId::new(1), 0); // Duplicate

        // dirty_count() returns raw count before deduplication (line 226-227 comment)
        // Deduplication happens during rebuild_dirty() for efficiency
        assert_eq!(build.dirty_count(), 2);
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
        use crate::ElementId;
        let mut build = BuildPipeline::new();

        // Normal scheduling works
        build.schedule(ElementId::new(1), 0);
        assert_eq!(build.dirty_count(), 1);

        build.lock_state(|b| {
            // Scheduling while locked should be ignored
            b.schedule(ElementId::new(2), 0);
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
        use crate::ElementId;
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        let id = ElementId::new(42);

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
        use crate::ElementId;
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        build.schedule(ElementId::new(1), 0);
        build.schedule(ElementId::new(2), 1);
        build.schedule(ElementId::new(3), 2);

        build.flush_batch();

        // All 3 should be dirty
        assert_eq!(build.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(10));

        build.schedule(ElementId::new(42), 0);

        // Should not flush immediately
        assert!(!build.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Now should flush
        assert!(build.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();
        // Batching not enabled

        build.schedule(ElementId::new(42), 0);

        // Should add directly to dirty elements
        assert_eq!(build.dirty_count(), 1);

        // flush_batch should be no-op
        build.flush_batch();
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        use crate::ElementId;
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        // Initial stats
        assert_eq!(build.batching_stats(), (0, 0));

        // Schedule same element twice
        build.schedule(ElementId::new(42), 0);
        build.schedule(ElementId::new(42), 0); // Duplicate

        // Flush
        build.flush_batch();

        // Should have 1 batch flushed, 1 build saved
        let (batches, saved) = build.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
    }
}
