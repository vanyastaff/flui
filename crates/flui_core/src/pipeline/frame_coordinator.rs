//! Frame coordination - orchestrates build→layout→paint pipeline phases
//!
//! The FrameCoordinator is responsible for:
//! - Coordinating the three pipeline phases (build, layout, paint)
//! - Managing phase execution order
//! - Collecting results from each phase
//!
//! # Design
//!
//! FrameCoordinator has a SINGLE responsibility: coordinate pipeline phases.
//! It does NOT:
//! - Manage element tree (that's ElementTree's job)
//! - Track dirty elements (that's pipeline's job)
//! - Handle errors (that's ErrorRecovery's job)
//! - Collect metrics (that's PipelineMetrics's job)
//!
//! # Example
//!
//! ```rust,ignore
//! let coordinator = FrameCoordinator {
//!     build: BuildPipeline::new(),
//!     layout: LayoutPipeline::new(),
//!     paint: PaintPipeline::new(),
//! };
//!
//! let layer = coordinator.build_frame(&mut tree, constraints)?;
//! ```

use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use super::{BuildPipeline, ElementTree, LayoutPipeline, PaintPipeline, PipelineError};
use flui_foundation::ElementId;
use flui_scheduler::FrameBudget;
use flui_types::constraints::BoxConstraints;

/// Coordinates the three pipeline phases: build → layout → paint
///
/// This is a focused component with ONE responsibility: orchestrating
/// the pipeline phases in the correct order.
///
/// # Single Responsibility
///
/// FrameCoordinator ONLY coordinates phase execution. It delegates:
/// - Dirty tracking → BuildPipeline, LayoutPipeline, PaintPipeline
/// - Element storage → ElementTree
/// - Error handling → Caller (via Result)
/// - Performance metrics → Caller (via FrameBudget)
///
/// # Example
///
/// ```rust,ignore
/// let mut coordinator = FrameCoordinator::new();
///
/// // Build complete frame
/// let layer = coordinator.build_frame(
///     &mut tree,
///     root_id,
///     constraints
/// )?;
/// ```
#[derive(Debug)]
pub struct FrameCoordinator {
    /// Build pipeline - manages widget rebuild phase
    build: BuildPipeline,

    /// Layout pipeline - manages size computation phase
    layout: LayoutPipeline,

    /// Paint pipeline - manages layer generation phase
    paint: PaintPipeline,

    /// Frame budget tracker - manages frame timing and budget (from flui-scheduler)
    budget: Arc<Mutex<FrameBudget>>,
}

impl FrameCoordinator {
    /// Create a new frame coordinator with a rebuild queue
    ///
    /// The rebuild queue is shared with the PipelineOwner for signal-triggered rebuilds.
    pub fn new_with_queue(rebuild_queue: super::RebuildQueue) -> Self {
        Self {
            build: BuildPipeline::new_with_queue(rebuild_queue),
            layout: LayoutPipeline::new(),
            paint: PaintPipeline::new(),
            budget: Arc::new(Mutex::new(FrameBudget::new(60))), // Default 60 FPS
        }
    }

    /// Create a new frame coordinator
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let coordinator = FrameCoordinator::new();
    /// ```
    pub fn new() -> Self {
        Self::new_with_queue(super::RebuildQueue::new())
    }

    /// Get reference to build pipeline
    pub fn build(&self) -> &BuildPipeline {
        &self.build
    }

    /// Get mutable reference to build pipeline
    pub fn build_mut(&mut self) -> &mut BuildPipeline {
        &mut self.build
    }

    /// Get reference to layout pipeline
    pub fn layout(&self) -> &LayoutPipeline {
        &self.layout
    }

    /// Get mutable reference to layout pipeline
    pub fn layout_mut(&mut self) -> &mut LayoutPipeline {
        &mut self.layout
    }

    /// Get reference to paint pipeline
    pub fn paint(&self) -> &PaintPipeline {
        &self.paint
    }

    /// Get mutable reference to paint pipeline
    pub fn paint_mut(&mut self) -> &mut PaintPipeline {
        &mut self.paint
    }

    /// Get reference to frame budget
    ///
    /// Returns a shared reference to the thread-safe frame budget tracker.
    pub fn budget(&self) -> &Arc<Mutex<FrameBudget>> {
        &self.budget
    }

    /// Extract root element's computed size after layout
    ///
    /// Helper method to retrieve the size from a RenderElement.
    /// Returns None if root is not a RenderElement or has no size.
    ///
    /// TODO: Re-implement when RenderState is accessible through Element
    fn extract_root_size(
        _tree_guard: &ElementTree,
        _root_id: Option<ElementId>,
    ) -> Option<flui_types::Size> {
        // Stub: RenderState not accessible through type-erased Element
        None
    }

    /// Extract root element's layer after paint
    ///
    /// Helper method to retrieve the painted layer from a RenderElement.
    /// Walks down through ComponentElements to find the first RenderElement child.
    ///
    /// TODO: Re-implement when paint_render_object returns proper Canvas type
    fn extract_root_layer(
        _tree_guard: &ElementTree,
        root_id: Option<ElementId>,
    ) -> Option<Box<flui_engine::CanvasLayer>> {
        // Stub: paint_render_object returns CanvasLayer, not Canvas
        match root_id {
            Some(_) => Some(Box::new(flui_engine::CanvasLayer::new())),
            None => None,
        }
    }

    /// Build a complete frame
    ///
    /// Orchestrates the three phases: build → layout → paint.
    ///
    /// # Parameters
    ///
    /// - `tree`: The element tree to operate on
    /// - `root_id`: Root element ID (for determining size)
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
    /// use flui_types::{Size, constraints::BoxConstraints};
    ///
    /// let mut tree = ElementTree::new();
    /// let mut coordinator = FrameCoordinator::new();
    ///
    /// // Build complete frame at 800x600
    /// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    /// if let Some(layer) = coordinator.build_frame(&mut tree, Some(root_id), constraints)? {
    ///     // Compositor can now render the layer
    ///     compositor.present(layer);
    /// }
    /// ```
    #[tracing::instrument(skip(self, tree), level = "debug")]
    pub fn build_frame(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
        constraints: BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        // Create frame span for hierarchical logging
        let frame_span = tracing::info_span!("frame", ?constraints);
        let _frame_guard = frame_span.enter();

        // Start frame and reset budget
        self.budget.lock().reset();

        // Phase 1: Build (rebuild dirty widgets)
        // IMPORTANT: Keep flushing build until tree is fully built (no more dirty elements)
        // This is critical because rebuilding one component can mark other components as dirty
        // (e.g., when signals change during rebuild, they schedule more rebuilds)
        let mut iterations = 0;
        let mut total_build_count = 0;
        loop {
            // Flush rebuild queue from signals to dirty_elements
            // This is critical: signal.set() adds to rebuild_queue, this moves to dirty_elements
            self.build.flush_rebuild_queue();

            // Flush any batched builds to dirty_elements
            // This is critical: schedule() adds to batcher, flush_batch moves to dirty_elements
            self.build.flush_batch();

            let build_count = self.build.dirty_count();

            if build_count == 0 {
                break;
            }

            let build_span = tracing::info_span!("build_iteration", iteration = iterations);
            let _build_guard = build_span.enter();

            // Use parallel build (automatically falls back to sequential if appropriate)
            self.build.rebuild_dirty_parallel(tree);
            total_build_count += build_count;

            tracing::debug!(
                count = build_count,
                iteration = iterations,
                "Build iteration complete"
            );

            iterations += 1;

            // Safety check: prevent infinite loops
            if iterations > 100 {
                tracing::warn!(
                    "Build loop exceeded 100 iterations, breaking (possible rebuild cycle)"
                );
                break;
            }
        }

        if total_build_count > 0 {
            tracing::debug!(
                total = total_build_count,
                iterations,
                "Build phase complete"
            );
        }

        // Check if we're approaching deadline after build phase
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if budget.is_deadline_near() {
                tracing::warn!(
                    "Approaching deadline after build, remaining {:.2}ms",
                    budget.remaining_ms()
                );
            }
        }

        // Phase 2: Layout (compute sizes and positions)
        let _root_size = {
            let layout_span = tracing::info_span!("layout");
            let _layout_guard = layout_span.enter();

            let mut tree_guard = tree.write();

            // Scan for RenderElements with needs_layout flag and add to dirty set
            // This ensures newly created elements (from build phase) get laid out
            let all_ids: Vec<_> = tree_guard.all_element_ids().collect();
            let mut marked_count = 0usize;
            for id in all_ids.iter().copied() {
                if let Some(element) = tree_guard.get(id) {
                    if element.is_render() {
                        if let Some(render_state) = element.render_state() {
                            if render_state.needs_layout() {
                                self.layout.mark_dirty(id);
                                marked_count += 1;
                            }
                        }
                    }
                }
            }
            tracing::debug!(
                total_elements = all_ids.len(),
                marked_for_layout = marked_count,
                "Scanned elements for needs_layout flag"
            );

            let laid_out_ids = self.layout.compute_layout(&mut tree_guard, constraints)?;

            // Mark all laid out elements for paint
            for id in &laid_out_ids {
                self.paint.mark_dirty(*id);
            }

            if !laid_out_ids.is_empty() {
                tracing::debug!(count = laid_out_ids.len(), "Layout complete");
            }

            // Get root element's computed size
            Self::extract_root_size(&tree_guard, root_id)
        };

        // Check if we're approaching deadline after layout phase
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if budget.is_deadline_near() {
                tracing::warn!(
                    "Approaching deadline after layout, remaining {:.2}ms",
                    budget.remaining_ms()
                );
            }
        }

        // Phase 3: Paint (generate layer tree)
        let layer = {
            let paint_span = tracing::info_span!("paint");
            let _paint_guard = paint_span.enter();

            let mut tree_guard = tree.write();
            let count = self.paint.generate_layers(&mut tree_guard)?;

            if count > 0 {
                tracing::debug!(count, "Paint complete");
            }

            // Get root element's layer
            Self::extract_root_layer(&tree_guard, root_id)
        };

        // Finish frame and update metrics
        self.budget.lock().finish_frame();

        // Log frame completion with timing info
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if layer.is_some() && budget.is_over_budget() {
                tracing::warn!(
                    elapsed_ms = budget.last_frame_time_ms(),
                    target_ms = budget.target_duration_ms(),
                    "Frame deadline missed"
                );
            }
        }

        Ok(layer)
    }

    /// Build a complete frame without creating frame span (for custom logging)
    ///
    /// This variant doesn't create a frame span, allowing the caller to manage spans.
    /// Useful when you want to add render phase to the same span.
    pub fn build_frame_no_span(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
        constraints: BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        // Start frame and reset budget
        self.budget.lock().reset();

        // Phase 1: Build (rebuild dirty widgets)
        // IMPORTANT: Keep flushing build until tree is fully built (no more dirty elements)
        // This is critical because rebuilding one component can mark other components as dirty
        // (e.g., when signals change during rebuild, they schedule more rebuilds)
        let mut iterations = 0;
        let mut total_build_count = 0;
        loop {
            // Flush rebuild queue from signals to dirty_elements
            // This is critical: signal.set() adds to rebuild_queue, this moves to dirty_elements
            self.build.flush_rebuild_queue();

            // Flush any batched builds to dirty_elements
            // This is critical: schedule() adds to batcher, flush_batch moves to dirty_elements
            self.build.flush_batch();

            let build_count = self.build.dirty_count();

            if build_count == 0 {
                break;
            }

            let build_span = tracing::info_span!("build_iteration", iteration = iterations);
            let _build_guard = build_span.enter();

            // Use parallel build (automatically falls back to sequential if appropriate)
            self.build.rebuild_dirty_parallel(tree);
            total_build_count += build_count;

            tracing::debug!(
                count = build_count,
                iteration = iterations,
                "Build iteration complete"
            );

            iterations += 1;

            // Safety check: prevent infinite loops
            if iterations > 100 {
                tracing::warn!(
                    "Build loop exceeded 100 iterations, breaking (possible rebuild cycle)"
                );
                break;
            }
        }

        if total_build_count > 0 {
            tracing::debug!(
                total = total_build_count,
                iterations,
                "Build phase complete"
            );
        }

        // Check if we're approaching deadline after build phase
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if budget.is_deadline_near() {
                tracing::warn!(
                    "Approaching deadline after build, remaining {:.2}ms",
                    budget.remaining_ms()
                );
            }
        }

        // Phase 2: Layout (compute sizes and positions)
        let _root_size = {
            let layout_span = tracing::info_span!("layout");
            let _layout_guard = layout_span.enter();

            let mut tree_guard = tree.write();

            // Scan for RenderElements with needs_layout flag and add to dirty set
            // This ensures newly created elements (from build phase) get laid out
            let all_ids: Vec<_> = tree_guard.all_element_ids().collect();
            let mut marked_count = 0usize;
            for id in all_ids.iter().copied() {
                if let Some(element) = tree_guard.get(id) {
                    if element.is_render() {
                        if let Some(render_state) = element.render_state() {
                            if render_state.needs_layout() {
                                self.layout.mark_dirty(id);
                                marked_count += 1;
                            }
                        }
                    }
                }
            }
            tracing::debug!(
                total_elements = all_ids.len(),
                marked_for_layout = marked_count,
                "Scanned elements for needs_layout flag"
            );

            let laid_out_ids = self.layout.compute_layout(&mut tree_guard, constraints)?;

            // Mark all laid out elements for paint
            for id in &laid_out_ids {
                self.paint.mark_dirty(*id);
            }

            if !laid_out_ids.is_empty() {
                tracing::debug!(count = laid_out_ids.len(), "Layout complete");
            }

            // Get root element's computed size
            Self::extract_root_size(&tree_guard, root_id)
        };

        // Check if we're approaching deadline after layout phase
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if budget.is_deadline_near() {
                tracing::warn!(
                    "Approaching deadline after layout, remaining {:.2}ms",
                    budget.remaining_ms()
                );
            }
        }

        // Phase 3: Paint (generate layer tree)
        let layer = {
            let paint_span = tracing::info_span!("paint");
            let _paint_guard = paint_span.enter();

            let mut tree_guard = tree.write();
            let count = self.paint.generate_layers(&mut tree_guard)?;

            if count > 0 {
                tracing::debug!(count, "Paint complete");
            }

            // Get root element's layer
            Self::extract_root_layer(&tree_guard, root_id)
        };

        // Finish frame and update metrics
        self.budget.lock().finish_frame();

        // Log frame completion with timing info
        #[cfg(debug_assertions)]
        {
            let budget = self.budget.lock();
            if layer.is_some() && budget.is_over_budget() {
                tracing::warn!(
                    elapsed_ms = budget.last_frame_time_ms(),
                    target_ms = budget.target_duration_ms(),
                    "Frame deadline missed"
                );
            }
        }

        Ok(layer)
    }

    /// Flush the build phase
    ///
    /// Rebuilds all dirty elements in depth order (with parallel execution when enabled).
    ///
    /// Uses parallel execution via rayon when:
    /// - Feature `parallel` is enabled
    /// - Number of dirty elements >= MIN_PARALLEL_ELEMENTS (50)
    /// - Multiple independent subtrees exist
    ///
    /// Falls back to sequential execution otherwise.
    #[tracing::instrument(skip(self, tree), level = "trace")]
    pub fn flush_build(&mut self, tree: &Arc<RwLock<ElementTree>>) {
        // Use parallel build (automatically falls back to sequential if appropriate)
        self.build.rebuild_dirty_parallel(tree);
    }

    /// Flush the layout phase
    ///
    /// Performs layout on all dirty render objects.
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root element exists
    #[tracing::instrument(skip(self, tree), level = "trace")]
    pub fn flush_layout(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
        constraints: BoxConstraints,
    ) -> Result<Option<flui_types::Size>, PipelineError> {
        let mut tree_guard = tree.write();

        // Scan for RenderElements with needs_layout flag and add to dirty set
        // This ensures newly created elements (from build phase) get laid out
        let all_ids: Vec<_> = tree_guard.all_element_ids().collect();
        let mut marked_count = 0usize;
        for id in all_ids.iter().copied() {
            if let Some(element) = tree_guard.get(id) {
                if element.is_render() {
                    if let Some(render_state) = element.render_state() {
                        if render_state.needs_layout() {
                            self.layout.mark_dirty(id);
                            marked_count += 1;
                        }
                    }
                }
            }
        }
        tracing::trace!(
            total_elements = all_ids.len(),
            marked_for_layout = marked_count,
            "flush_layout: scanned elements for needs_layout flag"
        );

        // Process all dirty render objects
        let laid_out_ids = self.layout.compute_layout(&mut tree_guard, constraints)?;

        // Mark all laid out elements for paint
        for id in laid_out_ids {
            self.paint.mark_dirty(id);
        }

        // Get root element's computed size
        let size = match root_id {
            Some(id) => {
                // Try direct size from RenderElement
                if let Some(size_opt) = Self::extract_root_size(&tree_guard, root_id) {
                    Some(size_opt)
                } else if let Some(element) = tree_guard.get(id) {
                    if element.as_component().is_some() {
                        // Root is ComponentElement - use its child's size
                        match element.children().first().copied() {
                            Some(child_id) => {
                                if let Some(child_element) = tree_guard.get(child_id) {
                                    if child_element.is_render() {
                                        if let Some(render_state) = child_element.render_state() {
                                            if render_state.has_size() {
                                                Some(render_state.size())
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            }
                            None => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            None => None,
        };

        Ok(size)
    }

    /// Flush the paint phase
    ///
    /// Generates paint layers for all dirty render objects.
    ///
    /// # Returns
    ///
    /// The root layer for composition, or None if no root element exists.
    #[tracing::instrument(skip(self, tree), level = "trace")]
    pub fn flush_paint(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        let mut tree_guard = tree.write();

        // Process all dirty render objects
        let _count = self.paint.generate_layers(&mut tree_guard)?;

        // Get root element's layer
        let layer = match root_id {
            Some(id) => {
                if let Some(element) = tree_guard.get(id) {
                    if element.is_render() {
                        if let Some(render_state) = element.render_state() {
                            let offset = render_state.offset();

                            // Use ElementTree's paint method instead of direct render method
                            if let Some(canvas) = tree_guard.paint_render_object(id, offset) {
                                Some(Box::new(flui_engine::CanvasLayer::from_canvas(canvas)))
                            } else {
                                Some(Box::new(flui_engine::CanvasLayer::new()))
                            }
                        } else {
                            Some(Box::new(flui_engine::CanvasLayer::new()))
                        }
                    } else if element.is_component() {
                        // Root is ComponentElement - paint its child
                        match element.children().first().copied() {
                            Some(child_id) => {
                                if let Some(child_element) = tree_guard.get(child_id) {
                                    if child_element.is_render() {
                                        if let Some(render_state) = child_element.render_state() {
                                            let offset = render_state.offset();

                                            // Use ElementTree's paint method instead of direct render method
                                            if let Some(canvas) =
                                                tree_guard.paint_render_object(child_id, offset)
                                            {
                                                Some(Box::new(
                                                    flui_engine::CanvasLayer::from_canvas(canvas),
                                                ))
                                            } else {
                                                Some(Box::new(flui_engine::CanvasLayer::new()))
                                            }
                                        } else {
                                            Some(Box::new(flui_engine::CanvasLayer::new()))
                                        }
                                    } else {
                                        Some(Box::new(flui_engine::CanvasLayer::new()))
                                    }
                                } else {
                                    Some(Box::new(flui_engine::CanvasLayer::new()))
                                }
                            }
                            None => Some(Box::new(flui_engine::CanvasLayer::new())),
                        }
                    } else {
                        // Root is ProviderElement or other type
                        Some(Box::new(flui_engine::CanvasLayer::new()))
                    }
                } else {
                    Some(Box::new(flui_engine::CanvasLayer::new()))
                }
            }
            None => None,
        };

        Ok(layer)
    }
}

impl Default for FrameCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Trait Implementations
// =============================================================================

/// Note: PipelineCoordinator trait requires mutable Tree access, but FrameCoordinator
/// uses Arc<RwLock<ElementTree>>. This implementation provides a simplified adapter.
/// For full trait compliance, consider using the direct methods on FrameCoordinator.
impl flui_pipeline::PipelineCoordinator for FrameCoordinator {
    type Tree = Arc<RwLock<ElementTree>>;
    type Constraints = BoxConstraints;
    type Size = flui_types::Size;
    type Layer = Box<flui_engine::CanvasLayer>;

    fn config(&self) -> &flui_pipeline::CoordinatorConfig {
        // Return a static default config - real config is in FrameBudget
        static DEFAULT_CONFIG: std::sync::OnceLock<flui_pipeline::CoordinatorConfig> =
            std::sync::OnceLock::new();
        DEFAULT_CONFIG.get_or_init(|| {
            let budget = self.budget.lock();
            flui_pipeline::CoordinatorConfig::new(budget.target_fps())
        })
    }

    fn set_config(&mut self, config: flui_pipeline::CoordinatorConfig) {
        let mut budget = self.budget.lock();
        budget.set_target_fps(config.target_fps);
    }

    fn frame_number(&self) -> u64 {
        self.budget.lock().frame_count()
    }

    fn has_dirty_build(&self) -> bool {
        self.build.has_dirty()
    }

    fn has_dirty_layout(&self) -> bool {
        self.layout.has_dirty()
    }

    fn has_dirty_paint(&self) -> bool {
        self.paint.has_dirty()
    }

    fn schedule_build(&mut self, id: ElementId, depth: usize) {
        self.build.schedule(id, depth);
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        self.layout.mark_dirty(id);
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        self.paint.mark_dirty(id);
    }

    fn flush_build(&mut self, tree: &mut Self::Tree) -> flui_pipeline::PipelineResult<usize> {
        // Flush queues first
        self.build.flush_rebuild_queue();
        self.build.flush_batch();

        let count = self.build.rebuild_dirty_parallel(tree);
        Ok(count)
    }

    fn flush_layout(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> flui_pipeline::PipelineResult<Option<Self::Size>> {
        // Get root_id from tree
        let root_id = {
            let tree_guard = tree.read();
            tree_guard.root_id()
        };

        FrameCoordinator::flush_layout(self, tree, root_id, constraints).map_err(Into::into)
    }

    fn flush_paint(
        &mut self,
        tree: &mut Self::Tree,
    ) -> flui_pipeline::PipelineResult<Option<Self::Layer>> {
        // Get root_id from tree
        let root_id = {
            let tree_guard = tree.read();
            tree_guard.root_id()
        };

        FrameCoordinator::flush_paint(self, tree, root_id).map_err(Into::into)
    }

    fn execute_frame(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> flui_pipeline::PipelineResult<flui_pipeline::FrameResult<Self::Layer>> {
        use std::time::Instant;

        let start = Instant::now();

        // Get root_id
        let root_id = {
            let tree_guard = tree.read();
            tree_guard.root_id()
        };

        // Execute frame using existing method
        let layer = self
            .build_frame(tree, root_id, constraints)
            .map_err(|e| -> flui_pipeline::PipelineError { e.into() })?;

        let frame_time = start.elapsed();
        let budget = self.budget.lock();

        Ok(flui_pipeline::FrameResult {
            layer,
            root_size: None, // Could extract from layout
            frame_number: budget.frame_count(),
            build_processed: 0,
            build_iterations: 0,
            layout_processed: 0,
            paint_processed: 0,
            frame_time,
            over_budget: budget.is_over_budget(),
        })
    }
}
