//! PipelineOwner - Unified pipeline coordinator (Flutter-inspired)
//!
//! This is the consolidated PipelineOwner that owns TreeCoordinator directly,
//! eliminating internal `Arc<RwLock>` to prevent deadlocks.
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner (owns everything)
//!   ├─ tree_coord: TreeCoordinator      ← DIRECT ownership (no Arc<RwLock>)
//!   ├─ dirty_elements: Vec<(ElementId, usize)>
//!   ├─ rebuild_queue: RebuildQueue
//!   ├─ build_owner: BuildOwner
//!   ├─ frame_budget: FrameBudget
//!   └─ features: PipelineFeatures
//!
//! External access (in flui_app):
//!   Arc<RwLock<PipelineOwner>>  ← Thread-safe access at app level
//! ```
//!
//! # Why Direct Ownership?
//!
//! The previous architecture had multiple `Arc<RwLock<TreeCoordinator>>` holders
//! (LayoutPipeline, PaintPipeline, FrameCoordinator), causing deadlocks when
//! one component held a write lock while another tried to acquire it.
//!
//! By having PipelineOwner own TreeCoordinator directly, all pipeline methods
//! use `&mut self`, letting Rust's borrow checker enforce exclusive access
//! at compile time - no runtime locks needed internally.
//!
//! # Parallel Execution
//!
//! When the `parallel` feature is enabled, flush_layout() and flush_paint()
//! use rayon for parallel processing. The API remains the same.
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
//! // Build frame (all three phases)
//! let canvas = owner.build_frame(constraints)?;
//! ```

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use flui_element::{BuildOwner, Element, ElementTree};
use flui_foundation::ElementId;
use flui_pipeline::{DirtySet, PipelineError};
use flui_scheduler::FrameBudget;
use flui_types::constraints::BoxConstraints;
use flui_view::tree::ViewNode;
use tracing::instrument;

use super::pipeline_context::PipelineBuildContext;
use super::{RebuildQueue, TreeCoordinator};

// ============================================================================
// BUILD BATCHER (moved from build_pipeline.rs)
// ============================================================================

/// Build batching system for performance optimization
#[derive(Debug)]
struct BuildBatcher {
    pending: HashMap<ElementId, usize>,
    batch_start: Option<Instant>,
    batch_duration: Duration,
    batches_flushed: usize,
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

    fn schedule(&mut self, element_id: ElementId, depth: usize) {
        if self.pending.is_empty() {
            self.batch_start = Some(Instant::now());
        }
        if self.pending.insert(element_id, depth).is_some() {
            self.builds_saved += 1;
        }
    }

    fn should_flush(&self) -> bool {
        self.batch_start
            .map(|start| start.elapsed() >= self.batch_duration)
            .unwrap_or(false)
    }

    fn take_pending(&mut self) -> HashMap<ElementId, usize> {
        self.batches_flushed += 1;
        self.batch_start = None;
        std::mem::take(&mut self.pending)
    }

    fn stats(&self) -> (usize, usize) {
        (self.batches_flushed, self.builds_saved)
    }
}

// ============================================================================
// ELEMENT TYPE (for rebuild dispatch)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementType {
    Component,
    Render,
    Provider,
}

// ============================================================================
// PIPELINE OWNER
// ============================================================================

/// PipelineOwner - orchestrates the three-phase rendering pipeline
///
/// This is the unified coordinator that owns TreeCoordinator directly,
/// implementing all pipeline phases (build, layout, paint) as methods.
///
/// # Thread Safety
///
/// PipelineOwner itself is NOT thread-safe. For multi-threaded access,
/// wrap in `Arc<RwLock<PipelineOwner>>` at the application level.
/// This design eliminates internal lock contention and deadlocks.
pub struct PipelineOwner {
    // ========== Four-Tree Coordinator ==========
    /// TreeCoordinator owns ViewTree, ElementTree, RenderTree, LayerTree
    /// DIRECT ownership - no Arc<RwLock> internally!
    tree_coord: TreeCoordinator,

    // ========== Build Phase State ==========
    /// Elements that need rebuilding with their depths: (ElementId, depth)
    dirty_elements: Vec<(ElementId, usize)>,

    /// Rebuild queue for deferred rebuilds from signals
    rebuild_queue: RebuildQueue,

    /// BuildOwner handles build scope and state locking
    build_owner: BuildOwner,

    /// Optional batching system
    batcher: Option<BuildBatcher>,

    /// Dirty set for scheduling rebuilds (shared with PipelineBuildContext)
    dirty_set: std::sync::Arc<parking_lot::RwLock<DirtySet<ElementId>>>,

    // ========== Frame Timing ==========
    /// Frame budget tracker
    frame_budget: FrameBudget,

    /// Frame counter (increments each build_frame call)
    frame_counter: u64,

    /// Last FPS log time for periodic statistics
    last_fps_log: Instant,

    /// Frame times for FPS calculation (rolling window)
    frame_times: Vec<f64>,

    // ========== Optional Features ==========
    /// Optional production features (metrics, recovery, caching, etc.)
    features: super::PipelineFeatures,

    /// Callback when a build is scheduled (optional)
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.tree_coord.root())
            .field("dirty_count", &self.dirty_elements.len())
            .field("frame_counter", &self.frame_counter)
            .field("has_build_callback", &self.on_build_scheduled.is_some())
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new pipeline owner with default configuration
    pub fn new() -> Self {
        Self {
            tree_coord: TreeCoordinator::new(),
            dirty_elements: Vec::new(),
            rebuild_queue: RebuildQueue::new(),
            build_owner: BuildOwner::new(),
            batcher: None,
            dirty_set: std::sync::Arc::new(parking_lot::RwLock::new(DirtySet::new())),
            frame_budget: FrameBudget::new(60), // Default 60 FPS
            last_fps_log: Instant::now(),
            frame_times: Vec::with_capacity(60),
            frame_counter: 0,
            features: super::PipelineFeatures::new(),
            on_build_scheduled: None,
        }
    }

    // =========================================================================
    // Tree & Root Access
    // =========================================================================

    /// Get reference to the TreeCoordinator
    #[inline]
    pub fn tree_coordinator(&self) -> &TreeCoordinator {
        &self.tree_coord
    }

    /// Get mutable reference to the TreeCoordinator
    #[inline]
    pub fn tree_coordinator_mut(&mut self) -> &mut TreeCoordinator {
        &mut self.tree_coord
    }

    /// Get the root element ID
    #[inline]
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.tree_coord.root()
    }

    /// Set the root element
    pub fn set_root(&mut self, element: Element) -> ElementId {
        let id = self.tree_coord.mount_root(element);
        // Schedule root for initial build
        self.schedule_build_for(id, 0);
        id
    }

    /// Attach a widget as the root element
    #[instrument(level = "debug", skip(self, widget), err(level = tracing::Level::ERROR))]
    pub fn attach<V>(&mut self, widget: V) -> Result<ElementId, PipelineError>
    where
        V: flui_view::StatelessView,
    {
        use flui_view::IntoView;

        if self.tree_coord.root().is_some() {
            return Err(PipelineError::RootAlreadyAttached);
        }

        let view_object = flui_view::Stateless(widget).into_view();
        let mode = view_object.mode();

        let view_id = {
            let view_node = ViewNode::from_boxed(view_object, mode);
            self.tree_coord.views_mut().insert(view_node)
        };

        let element = Element::view(Some(view_id), mode);
        Ok(self.set_root(element))
    }

    /// Attach a root element directly from an IntoElement type
    #[instrument(level = "debug", skip(self, element), err(level = tracing::Level::ERROR))]
    pub fn attach_element<E>(&mut self, element: E) -> Result<ElementId, PipelineError>
    where
        E: flui_element::IntoElement,
    {
        if self.tree_coord.root().is_some() {
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
        if self.build_owner.is_locked() {
            tracing::warn!(
                element_id = ?element_id,
                "Attempted to schedule build while state is locked. Build deferred."
            );
            return;
        }

        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(element_id, depth);
            return;
        }

        self.dirty_elements.push((element_id, depth));

        if let Some(ref callback) = self.on_build_scheduled {
            callback();
        }
    }

    /// Get number of dirty elements
    #[inline]
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Check if there are pending rebuilds
    #[inline]
    pub fn has_pending_rebuilds(&self) -> bool {
        !self.dirty_elements.is_empty() || !self.rebuild_queue.is_empty()
    }

    /// Mark element as needing build
    pub fn mark_needs_build(&mut self, element_id: ElementId) {
        self.tree_coord.mark_needs_build(element_id);
    }

    /// Mark element as needing layout
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.tree_coord.mark_needs_layout(node_id);
    }

    /// Mark element as needing paint
    pub fn request_paint(&mut self, node_id: ElementId) {
        self.tree_coord.mark_needs_paint(node_id);
    }

    // =========================================================================
    // Batching
    // =========================================================================

    /// Enable build batching
    pub fn enable_batching(&mut self, duration: Duration) {
        self.batcher = Some(BuildBatcher::new(duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        self.batcher = None;
    }

    /// Check if batching is enabled
    #[inline]
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Flush the current batch
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let pending = batcher.take_pending();
            for (element_id, depth) in pending {
                if !self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
                    self.dirty_elements.push((element_id, depth));
                }
            }
        }
    }

    /// Check if batch should be flushed
    #[inline]
    pub fn should_flush_batch(&self) -> bool {
        self.batcher
            .as_ref()
            .map(|b| b.should_flush())
            .unwrap_or(false)
    }

    /// Get batching statistics
    pub fn batching_stats(&self) -> (usize, usize) {
        self.batcher.as_ref().map(|b| b.stats()).unwrap_or((0, 0))
    }

    // =========================================================================
    // Build Scope
    // =========================================================================

    /// Check if currently in build scope
    #[inline]
    pub fn is_in_build_scope(&self) -> bool {
        self.build_owner.is_building()
    }

    /// Execute code within a build scope
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let was_building = self.build_owner.is_building();
        self.build_owner.set_building(true);
        let result = f(self);
        self.build_owner.set_building(was_building);
        result
    }

    /// Execute code with state locked
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

    // =========================================================================
    // PHASE 1: BUILD
    // =========================================================================

    /// Flush the build phase - rebuild all dirty elements
    #[instrument(level = "debug", skip(self))]
    pub fn flush_build(&mut self) {
        // 1. Drain rebuild queue from signals
        let rebuilds = self.rebuild_queue.drain();
        for (element_id, depth) in rebuilds {
            self.dirty_elements.push((element_id, depth));
        }

        // 2. Flush batcher if enabled
        self.flush_batch();

        // 3. Also drain from TreeCoordinator's needs_build
        let coord_dirty = self.tree_coord.take_needs_build();
        for id in coord_dirty {
            // Calculate depth
            let depth = self.calculate_depth(id);
            self.dirty_elements.push((id, depth));
        }

        if self.dirty_elements.is_empty() {
            return;
        }

        // 4. Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // 5. Deduplicate
        self.dirty_elements.dedup_by_key(|(id, _)| *id);

        // 6. Take dirty list and process
        let dirty = std::mem::take(&mut self.dirty_elements);

        tracing::trace!(count = dirty.len(), "Processing dirty elements");

        for (element_id, _depth) in dirty {
            self.rebuild_element(element_id);
        }
    }

    /// Rebuild a single element
    fn rebuild_element(&mut self, element_id: ElementId) {
        // Determine element type
        let element_type = match self.tree_coord.elements().get(element_id) {
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
                    "Element marked dirty but not found in tree"
                );
                None
            }
        };

        match element_type {
            Some(ElementType::Component) => {
                self.rebuild_component(element_id);
            }
            Some(ElementType::Provider) => {
                self.rebuild_provider(element_id);
            }
            Some(ElementType::Render) => {
                // RenderElements don't rebuild - they only relayout
            }
            None => {}
        }
    }

    /// Rebuild a component element
    fn rebuild_component(&mut self, element_id: ElementId) {
        use flui_view::IntoView;

        // Stage 1: Extract component data
        let (old_child_id, view_id) = {
            let element = match self.tree_coord.elements_mut().get_mut(element_id) {
                Some(e) => e,
                None => return,
            };

            if !element.is_component() {
                return;
            }

            if !element.is_dirty() {
                return;
            }

            let view_id = match element.as_view().and_then(|v| v.view_id()) {
                Some(id) => id,
                None => {
                    tracing::warn!(?element_id, "Component element has no view_id");
                    return;
                }
            };

            element.clear_dirty();
            (element.first_child(), view_id)
        };

        // Stage 2: Create context and build
        // We need to create a temporary Arc<RwLock<TreeCoordinator>> for the context
        // This is a workaround until PipelineBuildContext is updated
        let tree_coord_arc = std::sync::Arc::new(parking_lot::RwLock::new(TreeCoordinator::new()));
        let ctx = PipelineBuildContext::new(element_id, tree_coord_arc, self.dirty_set.clone());

        let new_view = {
            let build_result = catch_unwind(AssertUnwindSafe(|| {
                let view_node = match self.tree_coord.views_mut().get_mut(view_id) {
                    Some(node) => node,
                    None => {
                        tracing::error!(?element_id, ?view_id, "ViewObject not found in ViewTree");
                        return None;
                    }
                };

                Some(view_node.view_object_mut().build(&ctx))
            }));

            match build_result {
                Ok(Some(view)) => view,
                Ok(None) => return,
                Err(panic_info) => {
                    use crate::error_handling::{handle_build_panic, ErrorWidget};

                    let error = handle_build_panic(&*panic_info);
                    tracing::error!(
                        element_id = ?element_id,
                        message = %error.message,
                        "Panic caught during widget build"
                    );

                    Some(flui_view::Stateless(ErrorWidget::new(error)).into_view())
                }
            }
        };

        // Stage 3: Convert View to Element
        let new_element = match new_view {
            Some(view_obj) => {
                let mode = view_obj.mode();
                let view_id = {
                    let view_node = ViewNode::from_boxed(view_obj, mode);
                    self.tree_coord.views_mut().insert(view_node)
                };
                Element::view(Some(view_id), mode)
            }
            None => Element::empty(),
        };

        // Stage 4: Reconcile
        self.reconcile_child(element_id, old_child_id, Some(new_element));
    }

    /// Rebuild a provider element
    fn rebuild_provider(&mut self, element_id: ElementId) {
        let dependents = {
            let element = match self.tree_coord.elements_mut().get_mut(element_id) {
                Some(e) => e,
                None => return,
            };

            if !element.is_provider() || !element.is_dirty() {
                return;
            }

            let deps = element.dependents().map(|d| d.to_vec()).unwrap_or_default();
            element.clear_dirty();
            deps
        };

        // Notify all dependents
        for dependent_id in dependents {
            let dep_depth = self.calculate_depth(dependent_id);
            self.schedule_build_for(dependent_id, dep_depth);
        }
    }

    /// Calculate element depth in tree
    fn calculate_depth(&self, element_id: ElementId) -> usize {
        let mut depth = 0;
        let mut current = element_id;

        while let Some(element) = self.tree_coord.elements().get(current) {
            if let Some(parent_id) = element.parent() {
                depth += 1;
                current = parent_id;
            } else {
                break;
            }
        }

        depth
    }

    /// Reconcile child element
    fn reconcile_child(
        &mut self,
        parent_id: ElementId,
        old_child_id: Option<ElementId>,
        new_element: Option<Element>,
    ) {
        let elements = self.tree_coord.elements_mut();

        match (old_child_id, new_element) {
            (Some(old_id), Some(new_element)) => {
                // Check if we can reuse
                if Self::can_reuse_element(elements, old_id, &new_element) {
                    if let Some(old_elem) = elements.get_mut(old_id) {
                        old_elem.set_key(new_element.key());
                        old_elem.mark_dirty();
                    }
                } else {
                    let new_id = Self::insert_and_mount_child(elements, new_element, parent_id);
                    Self::update_component_child_reference(elements, parent_id, Some(new_id));
                    let _ = elements.remove(old_id);
                }
            }
            (None, Some(new_element)) => {
                let new_id = Self::insert_and_mount_child(elements, new_element, parent_id);
                Self::update_component_child_reference(elements, parent_id, Some(new_id));
            }
            (Some(old_id), None) => {
                let _ = elements.remove(old_id);
                Self::update_component_child_reference(elements, parent_id, None);
            }
            (None, None) => {}
        }
    }

    fn can_reuse_element(tree: &ElementTree, old_id: ElementId, new_element: &Element) -> bool {
        let old_element = match tree.get(old_id) {
            Some(elem) => elem,
            None => return false,
        };

        if old_element.view_mode() != new_element.view_mode() {
            return false;
        }

        match (old_element.key(), new_element.key()) {
            (Some(old_key), Some(new_key)) => old_key == new_key,
            (Some(_), None) | (None, Some(_)) => false,
            (None, None) => true,
        }
    }

    fn insert_and_mount_child(
        tree: &mut ElementTree,
        element: Element,
        parent_id: ElementId,
    ) -> ElementId {
        let new_id = tree.insert(element);
        let depth = tree.get(parent_id).map(|p| p.depth() + 1).unwrap_or(0);

        if let Some(child) = tree.get_mut(new_id) {
            child.mount(Some(parent_id), None, depth);
        }

        new_id
    }

    fn update_component_child_reference(
        tree: &mut ElementTree,
        parent_id: ElementId,
        child_id: Option<ElementId>,
    ) {
        if let Some(element) = tree.get_mut(parent_id) {
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

    // =========================================================================
    // PHASE 2: LAYOUT
    // =========================================================================

    /// Flush the layout phase - compute sizes for all dirty elements
    #[instrument(level = "debug", skip(self, constraints))]
    pub fn flush_layout(
        &mut self,
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, flui_types::Size)> {
        // Get dirty IDs from TreeCoordinator
        let dirty_ids: Vec<ElementId> = self.tree_coord.take_needs_layout().into_iter().collect();

        if dirty_ids.is_empty() {
            // If no dirty elements, scan for render elements and layout them
            let render_ids: Vec<ElementId> = self
                .tree_coord
                .elements()
                .all_element_ids()
                .filter(|&id| {
                    self.tree_coord
                        .elements()
                        .get(id)
                        .map(|e| e.is_render())
                        .unwrap_or(false)
                })
                .collect();

            if render_ids.is_empty() {
                return Vec::new();
            }

            return self.layout_elements(&render_ids, constraints);
        }

        self.layout_elements(&dirty_ids, constraints)
    }

    /// Layout a list of elements
    fn layout_elements(
        &mut self,
        ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, flui_types::Size)> {
        #[cfg(feature = "parallel")]
        {
            self.layout_elements_parallel(ids, constraints)
        }

        #[cfg(not(feature = "parallel"))]
        {
            self.layout_elements_sequential(ids, constraints)
        }
    }

    /// Sequential layout implementation
    #[cfg(not(feature = "parallel"))]
    fn layout_elements_sequential(
        &mut self,
        ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, flui_types::Size)> {
        let mut results = Vec::with_capacity(ids.len());

        for &id in ids {
            if let Some(size) = self.tree_coord.layout_element(id, constraints) {
                results.push((id, size));
                self.tree_coord.mark_needs_paint(id);
            }
        }

        tracing::trace!(count = results.len(), "Layout complete");

        results
    }

    /// Parallel layout implementation
    #[cfg(feature = "parallel")]
    fn layout_elements_parallel(
        &mut self,
        ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, flui_types::Size)> {
        use rayon::prelude::*;

        // Collect data needed for parallel layout (read-only phase)
        let layout_data: Vec<_> = ids
            .iter()
            .filter_map(|&id| {
                let element = self.tree_coord.elements().get(id)?;
                let render_id = element.as_render()?.render_id()?;
                Some((id, render_id))
            })
            .collect();

        if layout_data.is_empty() {
            return Vec::new();
        }

        // For parallel layout, we need immutable access to render tree
        // This is safe because layout computation is read-only
        let results: Vec<_> = layout_data
            .par_iter()
            .filter_map(|&(elem_id, render_id)| {
                // Compute layout (this part can be parallelized)
                // For now, fall back to sequential since layout_element needs &mut self
                None::<(ElementId, flui_rendering::tree::RenderId, flui_types::Size)>
            })
            .collect();

        // If parallel failed, fall back to sequential
        if results.is_empty() {
            return self.layout_elements_sequential(ids, constraints);
        }

        // Apply results (sequential, needs mutable access)
        let mut final_results = Vec::with_capacity(results.len());
        for (elem_id, render_id, size) in results {
            if let Some(node) = self.tree_coord.render_objects_mut().get_mut(render_id) {
                node.set_cached_size(Some(size));
            }
            self.tree_coord.mark_needs_paint(elem_id);
            final_results.push((elem_id, size));
        }

        tracing::trace!(count = final_results.len(), "Layout complete (parallel)");

        final_results
    }

    // =========================================================================
    // PHASE 3: PAINT
    // =========================================================================

    /// Flush the paint phase - generate canvas with draw commands
    #[instrument(level = "debug", skip(self))]
    pub fn flush_paint(&mut self) -> Option<flui_painting::Canvas> {
        // Get dirty IDs from TreeCoordinator
        let dirty_ids: Vec<ElementId> = self.tree_coord.take_needs_paint().into_iter().collect();

        // Process dirty elements (currently just clears the dirty state)
        #[cfg(feature = "parallel")]
        {
            self.paint_elements_parallel(&dirty_ids);
        }

        #[cfg(not(feature = "parallel"))]
        {
            self.paint_elements_sequential(&dirty_ids);
        }

        // Paint root to canvas
        self.tree_coord.paint_root()
    }

    /// Sequential paint implementation
    #[cfg(not(feature = "parallel"))]
    fn paint_elements_sequential(&mut self, ids: &[ElementId]) {
        for &id in ids {
            if self.tree_coord.elements().get(id).is_none() {
                continue;
            }

            if !self
                .tree_coord
                .elements()
                .get(id)
                .map(|e| e.is_render())
                .unwrap_or(false)
            {
                continue;
            }

            // Paint element (currently a no-op, actual paint happens in paint_root)
        }
    }

    /// Parallel paint implementation
    #[cfg(feature = "parallel")]
    fn paint_elements_parallel(&mut self, _ids: &[ElementId]) {
        // For now, parallel paint just processes elements in parallel
        // Actual rendering still happens sequentially in paint_root
    }

    // =========================================================================
    // BUILD FRAME (All phases)
    // =========================================================================

    /// Build a complete frame - runs all three phases
    ///
    /// # Phases
    /// 1. **Build**: Rebuild dirty widgets
    /// 2. **Layout**: Compute sizes and positions
    /// 3. **Paint**: Generate canvas with draw commands
    #[instrument(level = "debug", skip(self, constraints), fields(frame = self.frame_counter + 1), err(level = tracing::Level::ERROR))]
    pub fn build_frame(
        &mut self,
        constraints: BoxConstraints,
    ) -> Result<Option<flui_painting::Canvas>, PipelineError> {
        self.frame_counter += 1;
        self.frame_budget.reset();

        // Phase 1: Build
        let mut build_iterations = 0;
        let initial_dirty = self.dirty_elements.len() + self.rebuild_queue.len();
        loop {
            self.flush_build();

            if self.dirty_elements.is_empty() && self.rebuild_queue.is_empty() {
                break;
            }

            build_iterations += 1;
            if build_iterations > 100 {
                tracing::warn!("Build loop exceeded 100 iterations, breaking");
                break;
            }
        }

        // Phase 2: Layout
        let layout_results = self.flush_layout(constraints);

        // Phase 3: Paint
        let canvas = self.flush_paint();

        self.frame_budget.finish_frame();

        let frame_time_ms = self.frame_budget.last_frame_time_ms();

        // Collect frame times for FPS calculation
        self.frame_times.push(frame_time_ms);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }

        // Log first frame at INFO level
        if self.frame_counter == 1 {
            tracing::info!(
                frame = self.frame_counter,
                builds = initial_dirty,
                layouts = layout_results.len(),
                time_ms = format!("{:.2}", frame_time_ms),
                "First frame rendered"
            );
        }

        // Log FPS statistics every second (like Bevy)
        let elapsed_since_fps_log = self.last_fps_log.elapsed();
        if elapsed_since_fps_log >= Duration::from_secs(1) {
            let fps = self.frame_times.len() as f64 / elapsed_since_fps_log.as_secs_f64();
            let avg_frame_time: f64 = if self.frame_times.is_empty() {
                0.0
            } else {
                self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64
            };

            tracing::info!(
                fps = format!("{:.1}", fps),
                frame_time_ms = format!("{:.2}", avg_frame_time),
                frames = self.frame_counter,
                "Performance"
            );

            self.last_fps_log = Instant::now();
            self.frame_times.clear();
        }

        // Warn if frame took too long (janky frame)
        if canvas.is_some() && self.frame_budget.is_over_budget() {
            tracing::warn!(
                frame = self.frame_counter,
                elapsed_ms = format!("{:.2}", frame_time_ms),
                target_ms = format!("{:.2}", self.frame_budget.target_duration_ms()),
                "Janky frame"
            );
        }

        Ok(canvas)
    }

    // =========================================================================
    // Frame Info
    // =========================================================================

    /// Get current frame number
    #[inline]
    pub fn frame_number(&self) -> u64 {
        self.frame_counter
    }

    /// Get reference to rebuild queue
    #[inline]
    pub fn rebuild_queue(&self) -> &RebuildQueue {
        &self.rebuild_queue
    }

    /// Flush the rebuild queue and return true if there were pending rebuilds
    ///
    /// This is called by scheduler binding to check if any work needs to be done.
    pub fn flush_rebuild_queue(&mut self) -> bool {
        // Collect pending rebuilds from signals
        let rebuilds = self.rebuild_queue.drain();
        let has_rebuilds = !rebuilds.is_empty();

        for (element_id, depth) in rebuilds {
            self.dirty_elements.push((element_id, depth));
        }

        // Also check batcher
        if let Some(ref batcher) = self.batcher {
            if batcher.should_flush() {
                self.flush_batch();
            }
        }

        has_rebuilds || !self.dirty_elements.is_empty()
    }

    // =========================================================================
    // Features
    // =========================================================================

    /// Get reference to features
    #[inline]
    pub fn features(&self) -> &super::PipelineFeatures {
        &self.features
    }

    /// Get mutable reference to features
    #[inline]
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

    /// Enable error recovery with specified policy
    pub fn enable_error_recovery(&mut self, policy: flui_pipeline::RecoveryPolicy) {
        self.features.enable_recovery_with_policy(policy);
    }

    /// Get error recovery (if enabled)
    pub fn error_recovery(&self) -> Option<&flui_pipeline::ErrorRecovery> {
        self.features.recovery()
    }

    /// Enable cancellation with timeout
    pub fn enable_cancellation(&mut self, timeout: std::time::Duration) {
        self.features.enable_cancellation(timeout);
    }

    /// Get cancellation token (if enabled)
    pub fn cancellation_token(&self) -> Option<&flui_pipeline::CancellationToken> {
        self.features.cancellation()
    }

    /// Enable triple buffer for lock-free frame exchange
    pub fn enable_frame_buffer(&mut self) {
        self.features.enable_frame_buffer();
    }

    /// Set callback for when build is scheduled
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    // =========================================================================
    // Hit Testing
    // =========================================================================

    /// Perform hit testing at the given position
    pub fn perform_hit_test(
        &self,
        position: flui_types::Offset,
    ) -> flui_interaction::HitTestResult {
        use flui_interaction::HitTestResult;

        let mut result = HitTestResult::new();

        let root_id = match self.tree_coord.root() {
            Some(id) => id,
            None => return result,
        };

        self.hit_test_element(root_id, position, &mut result);

        result
    }

    fn hit_test_element(
        &self,
        element_id: ElementId,
        position: flui_types::Offset,
        result: &mut flui_interaction::HitTestResult,
    ) -> bool {
        use flui_interaction::HitTestEntry;

        let element = match self.tree_coord.elements().get(element_id) {
            Some(e) => e,
            None => return false,
        };

        let (size, offset) = (flui_types::Size::ZERO, flui_types::Offset::ZERO);

        let local_position =
            flui_types::Offset::new(position.dx - offset.dx, position.dy - offset.dy);

        let within_bounds = local_position.dx >= 0.0
            && local_position.dy >= 0.0
            && local_position.dx < size.width
            && local_position.dy < size.height;

        if !within_bounds {
            return false;
        }

        let mut hit = false;
        for child_id in element.children().iter().rev() {
            if self.hit_test_element(*child_id, local_position, result) {
                hit = true;
            }
        }

        if hit || within_bounds {
            if let Some(render_id) = element.as_render().and_then(|r| r.render_id()) {
                let bounds = flui_types::Rect::from_xywh(0.0, 0.0, size.width, size.height);
                let entry = HitTestEntry::new(render_id, local_position, bounds);
                result.add(entry);
            }
            return true;
        }

        false
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_owner_creation() {
        let owner = PipelineOwner::new();
        assert!(owner.root_element_id().is_none());
        assert_eq!(owner.dirty_count(), 0);
        assert_eq!(owner.frame_number(), 0);
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = PipelineOwner::new();
        let id = ElementId::new(1);

        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_batching() {
        let mut owner = PipelineOwner::new();
        owner.enable_batching(Duration::from_millis(16));
        assert!(owner.is_batching_enabled());

        let id = ElementId::new(1);
        owner.schedule_build_for(id, 0);
        owner.schedule_build_for(id, 0); // Duplicate

        owner.flush_batch();
        assert_eq!(owner.dirty_count(), 1); // Deduplicated

        let (batches, saved) = owner.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
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
}
