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

use parking_lot::RwLock;
use std::sync::Arc;

use super::{
    BuildPipeline, ElementTree, FrameScheduler, LayoutPipeline, PaintPipeline, PipelineError,
};
use crate::element::ElementId;
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
/// - Performance metrics → Caller (via PipelineMetrics)
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

    /// Frame scheduler - manages frame timing and budget
    scheduler: FrameScheduler,
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
            scheduler: FrameScheduler::new(),
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

    /// Get reference to frame scheduler
    pub fn scheduler(&self) -> &FrameScheduler {
        &self.scheduler
    }

    /// Get mutable reference to frame scheduler
    pub fn scheduler_mut(&mut self) -> &mut FrameScheduler {
        &mut self.scheduler
    }

    /// Extract root element's computed size after layout
    ///
    /// Helper method to retrieve the size from a RenderElement.
    /// Returns None if root is not a RenderElement or has no size.
    fn extract_root_size(
        tree_guard: &ElementTree,
        root_id: Option<ElementId>,
    ) -> Option<flui_types::Size> {
        match root_id {
            Some(id) => {
                if let Some(crate::element::Element::Render(render_elem)) = tree_guard.get(id) {
                    let render_state_lock = render_elem.render_state();
                    let render_state = render_state_lock.read();
                    render_state.size()
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Extract root element's layer after paint
    ///
    /// Helper method to retrieve the painted layer from a RenderElement.
    /// Returns empty CanvasLayer if root is a ComponentElement.
    fn extract_root_layer(
        tree_guard: &ElementTree,
        root_id: Option<ElementId>,
    ) -> Option<Box<flui_engine::CanvasLayer>> {
        match root_id {
            Some(id) => {
                if let Some(crate::element::Element::Render(render_elem)) = tree_guard.get(id) {
                    let render_state_lock = render_elem.render_state();
                    let render_state = render_state_lock.read();
                    let offset = render_state.offset();
                    drop(render_state);

                    // Convert Canvas → CanvasLayer
                    let canvas = render_elem.paint_render(tree_guard, offset);
                    Some(Box::new(flui_engine::CanvasLayer::from_canvas(canvas)))
                } else {
                    // Root is ComponentElement or ProviderElement - return empty picture
                    Some(Box::new(flui_engine::CanvasLayer::new()))
                }
            }
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
    pub fn build_frame(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
        constraints: BoxConstraints,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        // Create frame span for hierarchical logging
        let frame_span = tracing::info_span!(
            "frame",
            ?constraints
        );
        let _frame_guard = frame_span.enter();

        // Start frame and get budget
        let _frame_budget = self.scheduler.start_frame();

        // Phase 1: Build (rebuild dirty widgets)
        let build_count = self.build.dirty_count();

        if build_count > 0 {
            let build_span = tracing::debug_span!("build", dirty_count = build_count);
            let _build_guard = build_span.enter();

            // Use parallel build (automatically falls back to sequential if appropriate)
            self.build.rebuild_dirty_parallel(tree);

            tracing::debug!("Rebuilt {} widgets", build_count);
        }

        // Check if we're approaching deadline after build phase
        #[cfg(debug_assertions)]
        if self.scheduler.is_deadline_near() {
            if let Some(remaining) = self.scheduler.remaining() {
                tracing::warn!(
                    "Approaching deadline after build, remaining {:?}",
                    remaining
                );
            }
        }

        // Phase 2: Layout (compute sizes and positions)
        let _root_size = {
            let layout_span = tracing::debug_span!("layout");
            let _layout_guard = layout_span.enter();

            let mut tree_guard = tree.write();
            let laid_out_ids = self.layout.compute_layout(&mut tree_guard, constraints)?;

            if !laid_out_ids.is_empty() {
                tracing::debug!("Computed {} layouts", laid_out_ids.len());
            }

            // Mark all laid out elements for paint
            for id in laid_out_ids {
                self.paint.mark_dirty(id);
            }

            // Get root element's computed size
            Self::extract_root_size(&tree_guard, root_id)
        };

        #[cfg(debug_assertions)]
        if let Some(size) = _root_size {
            tracing::debug!(root_size = ?size, "Root layout complete");
        }

        // Check if we're approaching deadline after layout phase
        #[cfg(debug_assertions)]
        if self.scheduler.is_deadline_near() {
            if let Some(remaining) = self.scheduler.remaining() {
                tracing::warn!(
                    "Approaching deadline after layout, remaining {:?}",
                    remaining
                );
            }
        }

        // Phase 3: Paint (generate layer tree)
        let layer = {
            let paint_span = tracing::debug_span!("paint");
            let _paint_guard = paint_span.enter();

            let mut tree_guard = tree.write();
            let count = self.paint.generate_layers(&mut tree_guard)?;

            if count > 0 {
                tracing::debug!("Generated {} layers", count);
            }

            // Get root element's layer
            Self::extract_root_layer(&tree_guard, root_id)
        };

        // Finish frame and update metrics
        let frame_time = self.scheduler.finish_frame();

        // Log frame completion with timing info
        if layer.is_some() {
            if self.scheduler.is_deadline_missed() {
                tracing::warn!(
                    frame_time = ?frame_time,
                    consecutive_misses = self.scheduler.consecutive_misses(),
                    "Frame deadline missed"
                );
            } else {
                tracing::info!(frame_time = ?frame_time, "Frame complete");
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
    pub fn flush_layout(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
        constraints: BoxConstraints,
    ) -> Result<Option<flui_types::Size>, PipelineError> {
        let mut tree_guard = tree.write();

        // Process all dirty render objects
        let laid_out_ids = self.layout.compute_layout(&mut tree_guard, constraints)?;

        #[cfg(debug_assertions)]
        if !laid_out_ids.is_empty() {
            tracing::debug!(
                "flush_layout: Laid out {} render objects",
                laid_out_ids.len()
            );
        }

        // Mark all laid out elements for paint
        for id in laid_out_ids {
            self.paint.mark_dirty(id);
        }

        // Get root element's computed size
        let size = match root_id {
            Some(id) => {
                // Try direct size from RenderElement
                if let Some(size_opt) = Self::extract_root_size(&tree_guard, root_id) {
                    #[cfg(debug_assertions)]
                    tracing::debug!(
                        "flush_layout: Root (ID: {:?}) RenderState size: {:?}",
                        id,
                        size_opt
                    );
                    Some(size_opt)
                } else if let Some(crate::element::Element::Component(comp)) = tree_guard.get(id) {
                    // Root is ComponentElement - use its child's size
                    #[cfg(debug_assertions)]
                    tracing::debug!("flush_layout: Root is ComponentElement, using child for size");

                    match comp.child() {
                        Some(child_id) => match tree_guard.get(child_id) {
                            Some(crate::element::Element::Render(child_render)) => {
                                let render_state_lock = child_render.render_state();
                                let render_state = render_state_lock.read();
                                let size_opt = render_state.size();
                                #[cfg(debug_assertions)]
                                tracing::debug!(
                                    "flush_layout: ComponentElement child (ID: {:?}) size: {:?}",
                                    child_id,
                                    size_opt
                                );
                                size_opt
                            }
                            _ => {
                                #[cfg(debug_assertions)]
                                tracing::warn!(
                                    "flush_layout: ComponentElement child is not RenderElement"
                                );
                                None
                            }
                        },
                        None => {
                            #[cfg(debug_assertions)]
                            tracing::warn!("flush_layout: ComponentElement has no child");
                            None
                        }
                    }
                } else {
                    #[cfg(debug_assertions)]
                    tracing::warn!(
                        "flush_layout: Root element type not supported for size extraction"
                    );
                    None
                }
            }
            None => {
                #[cfg(debug_assertions)]
                tracing::warn!("flush_layout: No root_id provided!");
                None
            }
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
    pub fn flush_paint(
        &mut self,
        tree: &Arc<RwLock<ElementTree>>,
        root_id: Option<ElementId>,
    ) -> Result<Option<Box<flui_engine::CanvasLayer>>, PipelineError> {
        let mut tree_guard = tree.write();

        // Process all dirty render objects
        let _count = self.paint.generate_layers(&mut tree_guard)?;

        #[cfg(debug_assertions)]
        if _count > 0 {
            tracing::debug!("flush_paint: Painted {} render objects", _count);
        }

        // Get root element's layer
        let layer = match root_id {
            Some(id) => {
                let element_opt = tree_guard.get(id);

                #[cfg(debug_assertions)]
                if let Some(elem) = &element_opt {
                    match elem {
                        crate::element::Element::Component(_) => {
                            tracing::debug!(
                                "flush_paint: Root is ComponentElement - will handle below"
                            );
                        }
                        crate::element::Element::Provider(_) => {
                            tracing::warn!("flush_paint: Root is ProviderElement - returning empty ContainerLayer");
                        }
                        crate::element::Element::Render(_) => {
                            tracing::debug!(
                                "flush_paint: Root is RenderElement - will paint normally"
                            );
                        }
                    }
                } else {
                    tracing::error!("flush_paint: Root element not found in tree!");
                }

                match element_opt {
                    Some(crate::element::Element::Render(render_elem)) => {
                        let render_state_lock = render_elem.render_state();
                        let render_state = render_state_lock.read();
                        let offset = render_state.offset();
                        drop(render_state);

                        // Convert Canvas → CanvasLayer
                        let canvas = render_elem.paint_render(&tree_guard, offset);
                        Some(Box::new(flui_engine::CanvasLayer::from_canvas(canvas)))
                    }
                    Some(crate::element::Element::Component(comp)) => {
                        // Root is ComponentElement - paint its child
                        #[cfg(debug_assertions)]
                        tracing::debug!("flush_paint: Root is ComponentElement, painting child");

                        match comp.child() {
                            Some(child_id) => {
                                match tree_guard.get(child_id) {
                                    Some(crate::element::Element::Render(child_render)) => {
                                        let render_state_lock = child_render.render_state();
                                        let render_state = render_state_lock.read();
                                        let offset = render_state.offset();
                                        drop(render_state);

                                        // Convert Canvas → CanvasLayer
                                        let canvas = child_render.paint_render(&tree_guard, offset);
                                        Some(Box::new(flui_engine::CanvasLayer::from_canvas(
                                            canvas,
                                        )))
                                    }
                                    _ => {
                                        #[cfg(debug_assertions)]
                                        tracing::warn!("flush_paint: ComponentElement child is not RenderElement");
                                        Some(Box::new(flui_engine::CanvasLayer::new()))
                                    }
                                }
                            }
                            None => {
                                #[cfg(debug_assertions)]
                                tracing::warn!("flush_paint: ComponentElement has no child");
                                Some(Box::new(flui_engine::CanvasLayer::new()))
                            }
                        }
                    }
                    _ => {
                        // Root is ProviderElement or other type
                        Some(Box::new(flui_engine::CanvasLayer::new()))
                    }
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

#[cfg(test)]
#[path = "frame_coordinator_tests.rs"]
mod frame_coordinator_tests;
