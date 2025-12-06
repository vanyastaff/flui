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
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use flui_element::{BuildOwner, Element, ElementTree};
use flui_foundation::ElementId;
use flui_pipeline::context::PipelineBuildContext;
use flui_pipeline::DirtySet;

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
/// Build pipeline manages widget rebuild phase.
///
/// Tracks which elements need rebuilding (with their depths) and processes them
/// in depth-first order (parents before children).
///
/// # Architecture (SOLID)
///
/// - `BuildOwner` (from flui-element): Manages build scope and state locking
/// - `BuildPipeline` (this): Manages dirty tracking and rebuild execution
///
/// This separation follows Single Responsibility Principle.
#[derive(Debug)]
pub struct BuildPipeline {
    /// Elements that need rebuilding with their depths: (ElementId, depth)
    dirty_elements: Vec<(ElementId, usize)>,
    /// Build count for tracking (local counter, BuildOwner has its own)
    build_count: usize,
    /// BuildOwner handles build scope and state locking
    build_owner: BuildOwner,
    /// Optional batching system
    batcher: Option<BuildBatcher>,
    /// Rebuild queue for deferred rebuilds from signals
    rebuild_queue: super::RebuildQueue,
    /// Dirty set for scheduling rebuilds (shared with PipelineBuildContext)
    dirty_set: Arc<parking_lot::RwLock<DirtySet>>,
}

impl BuildPipeline {
    /// Creates a new build pipeline with a rebuild queue.
    pub fn new_with_queue(rebuild_queue: super::RebuildQueue) -> Self {
        Self {
            dirty_elements: Vec::new(),
            build_count: 0,
            build_owner: BuildOwner::new(),
            batcher: None,
            rebuild_queue,
            dirty_set: Arc::new(parking_lot::RwLock::new(DirtySet::new())),
        }
    }

    /// Creates a new build pipeline.
    pub fn new() -> Self {
        Self::new_with_queue(super::RebuildQueue::new())
    }

    /// Get reference to the BuildOwner
    pub fn build_owner(&self) -> &BuildOwner {
        &self.build_owner
    }

    /// Get mutable reference to the BuildOwner
    pub fn build_owner_mut(&mut self) -> &mut BuildOwner {
        &mut self.build_owner
    }

    /// Get the dirty set for use with PipelineBuildContext
    pub fn dirty_set(&self) -> Arc<parking_lot::RwLock<DirtySet>> {
        self.dirty_set.clone()
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
            self.build_owner.is_locked()
        );

        if self.build_owner.is_locked() {
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
        self.build_owner.is_building()
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
        // Check for nested builds
        let was_building = self.build_owner.is_building();
        if was_building {
            tracing::warn!("Nested build_scope detected! This may indicate incorrect usage.");
        }

        // Set building flag
        self.build_owner.set_building(true);

        // Execute callback
        let result = f(self);

        // Restore previous state
        self.build_owner.set_building(was_building);

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
        let was_locked = self.build_owner.is_locked();
        self.build_owner.set_locked(true);

        let result = f(self);

        self.build_owner.set_locked(was_locked);
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
                        if elem.is_component() {
                            Some(ElementType::Component)
                        } else if elem.is_provider() {
                            Some(ElementType::Provider)
                        } else if elem.is_render() {
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
        element: Element,
        parent_id: ElementId,
    ) -> ElementId {
        let new_id = tree_guard.insert(element);

        // Calculate depth: parent.depth() + 1
        let depth = tree_guard
            .get(parent_id)
            .map(|p| p.depth() + 1)
            .unwrap_or(0);

        if let Some(child) = tree_guard.get_mut(new_id) {
            child.mount(Some(parent_id), None, depth);
        }

        // Process pending children (Element-owned lifecycle pattern)
        // If this element has pending children, we need to:
        // 1. Extract pending child Elements from Element
        // 2. Insert each child into tree -> get ElementId
        // 3. Add ElementIds to this element's children list
        //
        // This works for ALL element types (RenderBox, RenderSliver, Containers, etc.)
        if let Some(element) = tree_guard.get_mut(new_id) {
            if let Some(pending_children) = element.take_pending_children() {
                // Insert each child Element into tree
                let child_depth = depth + 1;
                for child_element in pending_children {
                    // Downcast from Box<dyn Any + Send + Sync> to Element
                    if let Ok(element_box) = child_element.downcast::<Element>() {
                        let child_id = tree_guard.insert(*element_box);

                        // Mount child
                        if let Some(child) = tree_guard.get_mut(child_id) {
                            child.mount(Some(new_id), None, child_depth);
                        }

                        // Add child ID to parent's children list
                        if let Some(parent) = tree_guard.get_mut(new_id) {
                            parent.add_child(child_id);
                        }
                    }
                }
            }
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
        if let Some(element) = tree_guard.get_mut(parent_id) {
            if element.is_component() {
                match child_id {
                    Some(id) => {
                        element.clear_children();
                        element.add_child(id);
                    }
                    None => element.clear_children(),
                }
            }
        }
    }

    /// Check if an existing element can be reused for a new element
    ///
    /// Elements can be reused if:
    /// 1. ViewMode matches (Stateless <-> Stateless, RenderBox <-> RenderBox, etc.)
    /// 2. If both have keys, keys must match
    /// 3. If only one has a key, cannot reuse (different identity)
    ///
    /// # Returns
    ///
    /// - `true` if old element should be updated with new data
    /// - `false` if old element should be removed and new one inserted
    fn can_reuse(tree_guard: &ElementTree, old_id: ElementId, new_element: &Element) -> bool {
        let old_element = match tree_guard.get(old_id) {
            Some(elem) => elem,
            None => return false, // Old element doesn't exist
        };

        // Check 1: ViewMode must match
        if old_element.view_mode() != new_element.view_mode() {
            #[cfg(debug_assertions)]
            tracing::trace!(
                old_mode = ?old_element.view_mode(),
                new_mode = ?new_element.view_mode(),
                "Cannot reuse: ViewMode mismatch"
            );
            return false;
        }

        // Check 2: Key matching
        match (old_element.key(), new_element.key()) {
            // Both have keys - must match
            (Some(old_key), Some(new_key)) => {
                if old_key != new_key {
                    #[cfg(debug_assertions)]
                    tracing::trace!(?old_key, ?new_key, "Cannot reuse: Key mismatch");
                    return false;
                }
            }
            // Only one has a key - different identity
            (Some(_), None) | (None, Some(_)) => {
                #[cfg(debug_assertions)]
                tracing::trace!("Cannot reuse: Key presence mismatch");
                return false;
            }
            // Neither has a key - OK (rely on position)
            (None, None) => {}
        }

        // All checks passed - can reuse
        true
    }

    /// Update an existing element with new view data (in-place reuse)
    ///
    /// This is called when `can_reuse()` returns true.
    ///
    /// # Steps
    ///
    /// 1. Call did_update() lifecycle hook on new view with old view as argument
    /// 2. Replace old element's view_object with new one
    /// 3. Mark element dirty for rebuild
    ///
    /// # Why replace view_object?
    ///
    /// Even though ViewMode matches, the view data (props) may have changed.
    /// Example: `Text { text: "old" }` -> `Text { text: "new" }`
    ///
    /// We replace the view_object entirely because we can't compare old vs new props
    /// (no PartialEq requirement).
    fn update_element(
        tree_guard: &mut ElementTree,
        element_id: ElementId,
        mut new_element: Element,
        ctx: &PipelineBuildContext,
    ) {
        let old_element = match tree_guard.get_mut(element_id) {
            Some(elem) => elem,
            None => {
                tracing::error!(?element_id, "Cannot update: element not found");
                return;
            }
        };

        #[cfg(debug_assertions)]
        tracing::trace!(?element_id, "Updating element in-place (reuse)");

        // Lifecycle: Call did_update() on new view_object with old view as argument
        if let (Some(new_vo), Some(old_vo)) =
            (new_element.view_object_mut(), old_element.view_object())
        {
            new_vo.did_update(old_vo.as_any(), ctx);
        }

        // Take new view_object and swap it in
        if let Some(new_view_object) = new_element.take_view_object() {
            old_element.set_view_object_boxed(new_view_object);
        }

        // Update key if changed
        old_element.set_key(new_element.key());

        // Mark dirty to trigger rebuild with new view data
        old_element.mark_dirty();
    }

    /// Reconcile child element with element reuse support
    ///
    /// **Element Reuse**: If ViewMode and Key match, reuses existing element.
    /// **Replacement**: If type/key mismatch, inserts new BEFORE removing old (prevents Slab ID reuse).
    ///
    /// # Cases
    ///
    /// 1. `(Some, Some)` - Check can_reuse:
    ///    - If yes: Update old element in-place, reuse ElementId
    ///    - If no: Insert new, remove old (Slab ID reuse prevention)
    /// 2. `(None, Some)` - Insert new child
    /// 3. `(Some, None)` - Remove old child
    /// 4. `(None, None)` - No-op
    fn reconcile_child(
        tree_guard: &mut ElementTree,
        parent_id: ElementId,
        old_child_id: Option<ElementId>,
        new_element: Option<Element>,
        tree_arc: &Arc<RwLock<ElementTree>>,
        dirty_set: &Arc<RwLock<DirtySet>>,
    ) {
        match (old_child_id, new_element) {
            // Both old and new exist - check if we can reuse
            (Some(old_id), Some(new_element)) => {
                if Self::can_reuse(tree_guard, old_id, &new_element) {
                    // REUSE PATH: Update existing element
                    #[cfg(debug_assertions)]
                    tracing::trace!(
                        ?old_id,
                        ?parent_id,
                        "Reconcile: Reusing element (same type/key)"
                    );

                    // Create context for did_update() lifecycle hook
                    let ctx =
                        PipelineBuildContext::new(old_id, tree_arc.clone(), dirty_set.clone());

                    Self::update_element(tree_guard, old_id, new_element, &ctx);
                    // Element ID stays the same, no parent reference update needed
                } else {
                    // REPLACE PATH: Insert new, remove old
                    #[cfg(debug_assertions)]
                    tracing::trace!(
                        ?old_id,
                        ?parent_id,
                        "Reconcile: Replacing element (type/key mismatch)"
                    );

                    // Lifecycle: Call dispose() on old element before removing
                    if let Some(old_elem) = tree_guard.get_mut(old_id) {
                        if let Some(vo) = old_elem.view_object_mut() {
                            let ctx = PipelineBuildContext::new(
                                old_id,
                                tree_arc.clone(),
                                dirty_set.clone(),
                            );
                            vo.dispose(&ctx);
                        }
                    }

                    // Insert-before-remove pattern prevents Slab ID reuse
                    let new_id = Self::insert_and_mount_child(tree_guard, new_element, parent_id);

                    // Lifecycle: Call init() on new element
                    if let Some(new_elem) = tree_guard.get_mut(new_id) {
                        if let Some(vo) = new_elem.view_object_mut() {
                            let ctx = PipelineBuildContext::new(
                                new_id,
                                tree_arc.clone(),
                                dirty_set.clone(),
                            );
                            vo.init(&ctx);
                        }
                    }

                    Self::update_component_child_reference(tree_guard, parent_id, Some(new_id));
                    let _ = tree_guard.remove(old_id);
                }
            }

            // Add new child (no previous child)
            (None, Some(new_element)) => {
                let new_id = Self::insert_and_mount_child(tree_guard, new_element, parent_id);

                // Lifecycle: Call init() on new element
                if let Some(new_elem) = tree_guard.get_mut(new_id) {
                    if let Some(vo) = new_elem.view_object_mut() {
                        let ctx =
                            PipelineBuildContext::new(new_id, tree_arc.clone(), dirty_set.clone());
                        vo.init(&ctx);
                    }
                }

                Self::update_component_child_reference(tree_guard, parent_id, Some(new_id));
            }

            // Remove old child (no new child)
            (Some(old_id), None) => {
                // Lifecycle: Call dispose() on old element before removing
                if let Some(old_elem) = tree_guard.get_mut(old_id) {
                    if let Some(vo) = old_elem.view_object_mut() {
                        let ctx =
                            PipelineBuildContext::new(old_id, tree_arc.clone(), dirty_set.clone());
                        vo.dispose(&ctx);
                    }
                }

                let _ = tree_guard.remove(old_id);
                Self::update_component_child_reference(tree_guard, parent_id, None);
            }

            // No child before or after - nothing to do
            (None, None) => {}
        }
    }

    // ========== Component Rebuild ==========

    /// Rebuild a ComponentElement
    ///
    /// Two-stage process:
    /// 1. Check dirty flag and extract component data - minimize lock time
    /// 2. Build new child element and reconcile tree atomically
    #[tracing::instrument(skip(self, tree), level = "trace")]
    fn rebuild_component(
        &mut self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
        _depth: usize,
    ) -> bool {
        // Stage 1: Check dirty flag and extract component data (write lock)
        let old_child_id = {
            let mut tree_guard = tree.write();
            let element = match tree_guard.get_mut(element_id) {
                Some(e) => e,
                None => return false,
            };

            // Check if it's a component
            if !element.is_component() {
                return false;
            }

            // Skip rebuild if not dirty
            if !element.is_dirty() {
                #[cfg(debug_assertions)]
                tracing::trace!("Skipping rebuild for {:?} - not dirty", element_id);
                return false;
            }

            element.first_child()
        };

        // Stage 2: Create context and build new child element
        let ctx = PipelineBuildContext::new(element_id, tree.clone(), self.dirty_set.clone());

        let new_element = {
            let mut tree_guard = tree.write();
            let element = match tree_guard.get_mut(element_id) {
                Some(e) => e,
                None => return false,
            };

            // Clear dirty flag before build (in case build() marks dirty)
            element.clear_dirty();

            // Check if view object exists
            if element.view_object().is_none() {
                // No view object
                return true;
            }

            // Release the lock before calling build
            drop(tree_guard);

            // Re-acquire lock and build with panic catching
            let build_result = catch_unwind(AssertUnwindSafe(|| {
                let mut tree_guard = tree.write();
                let element = match tree_guard.get_mut(element_id) {
                    Some(e) => e,
                    None => return None,
                };

                // Call view_object.build() to get new child
                Some(element.view_object_mut().unwrap().build(&ctx))
            }));

            match build_result {
                Ok(Some(element)) => element,
                Ok(None) => return false,
                Err(panic_info) => {
                    // Panic occurred during build - handle it
                    use crate::error_handling::{handle_build_panic, ErrorWidget};
                    use flui_element::IntoElement;
                    use flui_view::StatelessView;

                    let error = handle_build_panic(&*panic_info);

                    tracing::error!(
                        element_id = ?element_id,
                        message = %error.message,
                        "Panic caught during widget build"
                    );

                    // Try to find ErrorBoundary and set error
                    if let Some(boundary_id) = self.find_error_boundary(tree, element_id) {
                        self.set_boundary_error(tree, boundary_id, error.clone());
                    }

                    // Return ErrorWidget as child
                    use flui_view::IntoView;
                    Some(flui_view::Stateless(ErrorWidget::new(error)).into_view())
                }
            }
        };

        // Stage 3: Reconcile old child with new child
        {
            use flui_element::IntoElement;

            // Convert Option<Box<dyn ViewObject>> to Option<Element>
            let new_child_element = new_element.map(|view_obj| view_obj.into_element());

            let mut tree_guard = tree.write();
            Self::reconcile_child(
                &mut tree_guard,
                element_id,
                old_child_id,
                new_child_element,
                tree,
                &self.dirty_set,
            );
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

            // Check if it's a provider
            if !element.is_provider() {
                return false;
            }

            // Skip rebuild if not dirty
            if !element.is_dirty() {
                #[cfg(debug_assertions)]
                tracing::trace!("Skipping provider rebuild for {:?} - not dirty", element_id);
                return false;
            }

            // Copy dependents list
            let deps = element.dependents().map(|d| d.to_vec()).unwrap_or_default();

            // Clear dirty flag
            element.clear_dirty();

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

    /// Find the nearest ErrorBoundary ancestor
    ///
    /// Walks up the element tree looking for an element that is a StatefulView
    /// with ErrorBoundary type.
    ///
    /// Returns the ElementId of the ErrorBoundary, or None if not found.
    fn find_error_boundary(
        &self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        element_id: ElementId,
    ) -> Option<ElementId> {
        use crate::error_handling::ErrorBoundary;
        use flui_view::StatefulViewWrapper;

        let tree_guard = tree.read();
        let mut current = element_id;

        loop {
            // Check if current element is an ErrorBoundary
            if let Some(element) = tree_guard.get(current) {
                // Try to downcast view_object to check if it's ErrorBoundary
                if let Some(view_obj) = element.view_object() {
                    // Check if the view object is a StatefulViewWrapper<ErrorBoundary>
                    if view_obj
                        .as_any()
                        .downcast_ref::<StatefulViewWrapper<ErrorBoundary>>()
                        .is_some()
                    {
                        return Some(current);
                    }
                }

                // Move to parent
                if let Some(parent_id) = element.parent() {
                    current = parent_id;
                } else {
                    // Reached root, no ErrorBoundary found
                    return None;
                }
            } else {
                return None;
            }
        }
    }

    /// Set error in ErrorBoundary state and mark for rebuild
    ///
    /// This accesses the ErrorBoundaryState and sets the error,
    /// then marks the boundary element dirty for rebuild.
    fn set_boundary_error(
        &mut self,
        tree: &Arc<parking_lot::RwLock<ElementTree>>,
        boundary_id: ElementId,
        error: crate::error_handling::ErrorInfo,
    ) {
        use crate::error_handling::ErrorBoundary;
        use flui_view::StatefulViewWrapper;

        let mut tree_guard = tree.write();

        if let Some(element) = tree_guard.get_mut(boundary_id) {
            if let Some(view_obj) = element.view_object_mut() {
                // Downcast to StatefulViewWrapper<ErrorBoundary>
                if let Some(wrapper) = view_obj
                    .as_any_mut()
                    .downcast_mut::<StatefulViewWrapper<ErrorBoundary>>()
                {
                    // Get state and set error
                    if let Some(state) = wrapper.state_mut() {
                        state.set_error(error);

                        // Mark element dirty for rebuild
                        element.mark_dirty();

                        // Calculate depth for scheduling
                        drop(tree_guard);
                        let depth = self.calculate_depth(tree, boundary_id);
                        self.schedule(boundary_id, depth);

                        tracing::debug!(
                            boundary_id = ?boundary_id,
                            "Error set in ErrorBoundary, scheduled for rebuild"
                        );
                    }
                }
            }
        }
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
                        if elem.is_component() {
                            Some(ElementType::Component)
                        } else if elem.is_provider() {
                            Some(ElementType::Provider)
                        } else if elem.is_render() {
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
    ///
    /// Note: This uses atomic operations on BuildOwner
    pub(super) fn set_build_scope(&mut self, value: bool) {
        self.build_owner.set_building(value);
    }

    /// Set build_locked flag (internal use only)
    ///
    /// Note: This uses atomic operations on BuildOwner
    pub(super) fn set_build_locked(&mut self, value: bool) {
        self.build_owner.set_locked(value);
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
        use flui_foundation::ElementId;
        let mut build = BuildPipeline::new();

        assert!(!build.has_dirty());

        build.schedule(ElementId::new(1), 0);

        assert!(build.has_dirty());
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_dirty_count() {
        use flui_foundation::ElementId;
        let mut build = BuildPipeline::new();

        build.schedule(ElementId::new(1), 0);
        build.schedule(ElementId::new(2), 1);

        assert_eq!(build.dirty_count(), 2);
    }

    #[test]
    fn test_clear_dirty() {
        use flui_foundation::ElementId;
        let mut build = BuildPipeline::new();

        build.schedule(ElementId::new(1), 0);
        build.clear_dirty();

        assert!(!build.has_dirty());
    }

    #[test]
    fn test_schedule_duplicate() {
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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
        use flui_foundation::ElementId;
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

// =============================================================================
// Trait Implementations
// =============================================================================

impl flui_pipeline::BuildPhase for BuildPipeline {
    type Tree = Arc<parking_lot::RwLock<ElementTree>>;

    fn schedule(&mut self, element_id: ElementId, depth: usize) {
        // Delegate to existing method
        BuildPipeline::schedule(self, element_id, depth);
    }

    fn has_dirty(&self) -> bool {
        BuildPipeline::has_dirty(self)
    }

    fn dirty_count(&self) -> usize {
        BuildPipeline::dirty_count(self)
    }

    fn clear_dirty(&mut self) {
        BuildPipeline::clear_dirty(self);
    }

    fn rebuild_dirty(&mut self, tree: &Self::Tree) -> usize {
        BuildPipeline::rebuild_dirty_parallel(self, tree)
    }

    fn flush_queues(&mut self) {
        // Flush rebuild queue from signals
        self.flush_rebuild_queue();
        // Flush batch if batching enabled
        self.flush_batch();
    }
}

impl flui_pipeline::BatchedExecution for BuildPipeline {
    fn enable_batching(&mut self, duration: Duration) {
        BuildPipeline::enable_batching(self, duration);
    }

    fn disable_batching(&mut self) {
        BuildPipeline::disable_batching(self);
    }

    fn is_batching_enabled(&self) -> bool {
        BuildPipeline::is_batching_enabled(self)
    }

    fn flush_batch(&mut self) {
        BuildPipeline::flush_batch(self);
    }

    fn should_flush_batch(&self) -> bool {
        BuildPipeline::should_flush_batch(self)
    }
}
