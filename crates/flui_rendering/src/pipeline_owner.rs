//! RenderPipelineOwner - Manages the rendering pipeline for RenderObjects.
//!
//! This module implements Flutter's PipelineOwner pattern, managing dirty tracking
//! and flush operations for the render tree.
//!
//! # Flutter Analogy
//!
//! This is equivalent to Flutter's `PipelineOwner` class in `rendering/object.dart`.
//! It manages:
//! - Dirty tracking for layout/paint/compositing
//! - Flush operations that process dirty nodes
//! - Root render object management
//!
//! # Architecture
//!
//! ```text
//! RenderPipelineOwner
//!   ├── render_tree: RenderTree       (storage for RenderObjects)
//!   ├── needs_layout: HashSet<RenderId>
//!   ├── needs_paint: HashSet<RenderId>
//!   ├── needs_compositing_bits_update: HashSet<RenderId>
//!   └── root: Option<RenderId>
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::RenderPipelineOwner;
//!
//! let mut pipeline = RenderPipelineOwner::new();
//!
//! // Insert render object
//! let id = pipeline.insert(my_render_node);
//! pipeline.set_root(Some(id));
//!
//! // Mark dirty
//! pipeline.mark_needs_layout(id);
//!
//! // Flush phases
//! pipeline.flush_layout();
//! let display_list = pipeline.flush_paint();
//! ```

use std::collections::HashSet;

use flui_foundation::RenderId;

use crate::render_tree::{RenderNode, RenderTree};

// ============================================================================
// RENDER PIPELINE OWNER
// ============================================================================

/// Manages the rendering pipeline for RenderObjects.
///
/// Like Flutter's `PipelineOwner`, this struct:
/// - Owns the RenderTree
/// - Tracks which nodes need layout/paint/compositing
/// - Provides flush methods to process dirty nodes
///
/// # Dirty Tracking
///
/// Dirty tracking uses `RenderId` (not `ElementId`) because:
/// - RenderObjects are self-contained for layout/paint
/// - Decouples rendering from element tree
/// - Matches Flutter's architecture
#[derive(Debug)]
pub struct RenderPipelineOwner {
    /// The render tree storing all RenderObjects
    render_tree: RenderTree,

    /// Render objects that need layout
    needs_layout: HashSet<RenderId>,

    /// Render objects that need paint
    needs_paint: HashSet<RenderId>,

    /// Render objects that need compositing bits update
    needs_compositing_bits_update: HashSet<RenderId>,

    /// Root render object
    root: Option<RenderId>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl RenderPipelineOwner {
    /// Creates a new RenderPipelineOwner with an empty render tree.
    pub fn new() -> Self {
        Self {
            render_tree: RenderTree::new(),
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing_bits_update: HashSet::new(),
            root: None,
        }
    }

    /// Creates a RenderPipelineOwner with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            render_tree: RenderTree::with_capacity(capacity),
            needs_layout: HashSet::with_capacity(capacity),
            needs_paint: HashSet::with_capacity(capacity),
            needs_compositing_bits_update: HashSet::with_capacity(capacity),
            root: None,
        }
    }
}

impl Default for RenderPipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RENDER TREE ACCESS
// ============================================================================

impl RenderPipelineOwner {
    /// Returns a reference to the render tree.
    #[inline]
    pub fn render_tree(&self) -> &RenderTree {
        &self.render_tree
    }

    /// Returns a mutable reference to the render tree.
    #[inline]
    pub fn render_tree_mut(&mut self) -> &mut RenderTree {
        &mut self.render_tree
    }

    /// Inserts a mounted render node into the tree.
    ///
    /// **Note**: Node must be in `Mounted` state. Use `node.mount()` first.
    #[inline]
    pub fn insert(&mut self, node: RenderNode<flui_tree::Mounted>) -> RenderId {
        self.render_tree.insert(node)
    }

    /// Gets a render node by ID.
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode<flui_tree::Mounted>> {
        self.render_tree.get(id)
    }

    /// Gets a mutable render node by ID.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode<flui_tree::Mounted>> {
        self.render_tree.get_mut(id)
    }

    /// Removes a render node from the tree.
    ///
    /// Returns the mounted node (still in `Mounted` state).
    /// Call `.unmount()` on the result to transition to `Unmounted`.
    #[inline]
    pub fn remove(&mut self, id: RenderId) -> Option<RenderNode<flui_tree::Mounted>> {
        // Also remove from dirty sets
        self.needs_layout.remove(&id);
        self.needs_paint.remove(&id);
        self.needs_compositing_bits_update.remove(&id);
        self.render_tree.remove(id)
    }

    /// Adds a child to a parent render node.
    #[inline]
    pub fn add_child(&mut self, parent: RenderId, child: RenderId) {
        self.render_tree.add_child(parent, child);
    }

    /// Removes a child from a parent render node.
    #[inline]
    pub fn remove_child(&mut self, parent: RenderId, child: RenderId) {
        self.render_tree.remove_child(parent, child);
    }
}

// ============================================================================
// ROOT MANAGEMENT
// ============================================================================

impl RenderPipelineOwner {
    /// Gets the root render object ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Sets the root render object ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }
}

// ============================================================================
// DIRTY TRACKING (Flutter PipelineOwner pattern)
// ============================================================================

impl RenderPipelineOwner {
    /// Marks a render object as needing layout.
    ///
    /// This is called when:
    /// - Constraints change
    /// - A render object's intrinsic dimensions change
    /// - Children are added/removed
    ///
    /// Layout changes automatically mark the object for paint as well.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        self.needs_layout.insert(id);
        // Layout changes require repaint (Flutter pattern)
        self.needs_paint.insert(id);
    }

    /// Marks a render object as needing paint.
    ///
    /// This is called when visual properties change (color, opacity, etc.)
    /// but layout remains the same.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        self.needs_paint.insert(id);
    }

    /// Marks a render object as needing compositing bits update.
    ///
    /// This is called when:
    /// - `isRepaintBoundary` changes
    /// - `needsCompositing` changes
    pub fn mark_needs_compositing_bits_update(&mut self, id: RenderId) {
        self.needs_compositing_bits_update.insert(id);
    }

    /// Returns the set of render objects that need layout.
    #[inline]
    pub fn needs_layout(&self) -> &HashSet<RenderId> {
        &self.needs_layout
    }

    /// Returns the set of render objects that need paint.
    #[inline]
    pub fn needs_paint(&self) -> &HashSet<RenderId> {
        &self.needs_paint
    }

    /// Returns the set of render objects that need compositing bits update.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> &HashSet<RenderId> {
        &self.needs_compositing_bits_update
    }

    /// Checks if there are any dirty render objects.
    pub fn has_dirty_nodes(&self) -> bool {
        !self.needs_layout.is_empty()
            || !self.needs_paint.is_empty()
            || !self.needs_compositing_bits_update.is_empty()
    }

    /// Checks if any render object needs layout.
    #[inline]
    pub fn has_needs_layout(&self) -> bool {
        !self.needs_layout.is_empty()
    }

    /// Checks if any render object needs paint.
    #[inline]
    pub fn has_needs_paint(&self) -> bool {
        !self.needs_paint.is_empty()
    }

    /// Clears all dirty tracking sets.
    pub fn clear_dirty(&mut self) {
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing_bits_update.clear();
    }
}

// ============================================================================
// FLUSH OPERATIONS (Flutter PipelineOwner pattern)
// ============================================================================

impl RenderPipelineOwner {
    /// Flushes the layout phase.
    ///
    /// Processes all render objects marked as needing layout, in depth order
    /// (parents before children). This matches Flutter's `flushLayout()`.
    ///
    /// # Algorithm
    ///
    /// 1. Collect dirty nodes with their depths
    /// 2. Sort by depth (shallowest first = parents before children)
    /// 3. For each dirty node, call layout if still dirty
    /// 4. Clear the needs_layout set
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushLayout() {
    ///   while (_nodesNeedingLayout.isNotEmpty) {
    ///     final dirtyNodes = _nodesNeedingLayout;
    ///     _nodesNeedingLayout = [];
    ///     // Sort shallowest first (parents before children)
    ///     dirtyNodes.sort((a, b) => a.depth - b.depth);
    ///     for (final node in dirtyNodes) {
    ///       if (node._needsLayout && node.owner == this) {
    ///         node._layoutWithoutResize();
    ///       }
    ///     }
    ///   }
    /// }
    /// ```
    pub fn flush_layout(&mut self) {
        if self.needs_layout.is_empty() {
            return;
        }

        // Collect dirty nodes with their depths
        let mut dirty_nodes: Vec<(RenderId, usize)> = self
            .needs_layout
            .iter()
            .filter_map(|&id| {
                self.render_tree
                    .get(id)
                    .map(|node| (id, node.depth().get()))
            })
            .collect();

        // Clear the dirty set
        self.needs_layout.clear();

        // Sort by depth: SHALLOWEST FIRST (parents before children)
        // This is critical for Flutter protocol compliance
        dirty_nodes.sort_by_key(|(_, depth)| *depth);

        for (id, _depth) in dirty_nodes {
            if let Some(_node) = self.render_tree.get_mut(id) {
                // TODO: Call performLayout() with proper constraints
                // This requires integration with the constraint system
                tracing::trace!(?id, depth = _depth, "flush_layout: processing node (shallowest first)");
            }
        }
    }

    /// Flushes the compositing bits update phase.
    ///
    /// This updates the `needsCompositing` flag on render objects.
    /// Must be called before `flush_paint()`.
    pub fn flush_compositing_bits(&mut self) {
        if self.needs_compositing_bits_update.is_empty() {
            return;
        }

        let dirty_nodes: Vec<RenderId> = self.needs_compositing_bits_update.drain().collect();

        for id in dirty_nodes {
            if let Some(node) = self.render_tree.get_mut(id) {
                // Update compositing bits
                // TODO: Implement proper compositing bits calculation
                tracing::trace!(?id, "flush_compositing_bits: processing node");
                let _ = node; // Suppress unused warning for now
            }
        }
    }

    /// Flushes the paint phase.
    ///
    /// Processes all render objects marked as needing paint, in reverse depth order
    /// (children before parents). This matches Flutter's `flushPaint()`.
    ///
    /// # Algorithm
    ///
    /// 1. Collect dirty nodes with their depths
    /// 2. Sort by depth (deepest first = children before parents)
    /// 3. For each dirty node, call paint if still dirty
    /// 4. Clear the needs_paint set
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void flushPaint() {
    ///   final dirtyNodes = _nodesNeedingPaint;
    ///   _nodesNeedingPaint = [];
    ///   // Sort DEEPEST first (children before parents)
    ///   for (final node in dirtyNodes..sort((a, b) => b.depth - a.depth)) {
    ///     if ((node._needsPaint || node._needsCompositedLayerUpdate)
    ///         && node.owner == this) {
    ///       if (node._layerHandle.layer!.attached) {
    ///         PaintingContext.repaintCompositedChild(node);
    ///       }
    ///     }
    ///   }
    /// }
    /// ```
    ///
    /// # Returns
    ///
    /// Currently returns unit. In the future, this will return a DisplayList
    /// or similar structure for compositor integration.
    pub fn flush_paint(&mut self) {
        if self.needs_paint.is_empty() {
            return;
        }

        // Collect dirty nodes with their depths
        let mut dirty_nodes: Vec<(RenderId, usize)> = self
            .needs_paint
            .iter()
            .filter_map(|&id| {
                self.render_tree
                    .get(id)
                    .map(|node| (id, node.depth().get()))
            })
            .collect();

        // Clear the dirty set
        self.needs_paint.clear();

        // Sort by depth: DEEPEST FIRST (children before parents)
        // This is critical for correct painting order
        dirty_nodes.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));

        for (id, _depth) in dirty_nodes {
            if let Some(_node) = self.render_tree.get_mut(id) {
                // TODO: Call paint() with proper PaintContext
                tracing::trace!(?id, depth = _depth, "flush_paint: processing node (deepest first)");
            }
        }
    }

    /// Performs a complete flush cycle: layout → compositing bits → paint.
    ///
    /// This is the main entry point for processing a frame.
    pub fn flush_pipeline(&mut self) {
        self.flush_layout();
        self.flush_compositing_bits();
        self.flush_paint();
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderObject;

    #[derive(Debug)]
    struct TestRenderObject;

    impl RenderObject for TestRenderObject {}

    #[test]
    fn test_pipeline_owner_creation() {
        let pipeline = RenderPipelineOwner::new();
        assert!(pipeline.root().is_none());
        assert!(!pipeline.has_dirty_nodes());
    }

    #[test]
    fn test_insert_and_mark_dirty() {
        use flui_tree::MountableExt;

        let mut pipeline = RenderPipelineOwner::new();

        // Must mount before inserting
        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);
        pipeline.set_root(Some(id));

        assert_eq!(pipeline.root(), Some(id));
        assert!(!pipeline.has_dirty_nodes());

        // Mark needs layout
        pipeline.mark_needs_layout(id);
        assert!(pipeline.has_needs_layout());
        assert!(pipeline.has_needs_paint()); // layout implies paint

        // Flush
        pipeline.flush_layout();
        assert!(!pipeline.has_needs_layout());
    }

    #[test]
    fn test_mark_needs_paint() {
        use flui_tree::MountableExt;

        let mut pipeline = RenderPipelineOwner::new();

        // Must mount before inserting
        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_paint(id);
        assert!(pipeline.has_needs_paint());
        assert!(!pipeline.has_needs_layout());

        pipeline.flush_paint();
        assert!(!pipeline.has_needs_paint());
    }

    #[test]
    fn test_remove_clears_dirty() {
        use flui_tree::MountableExt;

        let mut pipeline = RenderPipelineOwner::new();

        // Must mount before inserting
        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_layout(id);
        pipeline.mark_needs_paint(id);
        assert!(pipeline.has_dirty_nodes());

        pipeline.remove(id);
        assert!(!pipeline.needs_layout().contains(&id));
        assert!(!pipeline.needs_paint().contains(&id));
    }

    #[test]
    fn test_flush_pipeline() {
        use flui_tree::MountableExt;

        let mut pipeline = RenderPipelineOwner::new();

        // Must mount before inserting
        let node = RenderNode::new(TestRenderObject).mount_root();
        let id = pipeline.insert(node);

        pipeline.mark_needs_layout(id);
        pipeline.mark_needs_compositing_bits_update(id);

        assert!(pipeline.has_dirty_nodes());

        pipeline.flush_pipeline();

        assert!(!pipeline.has_dirty_nodes());
    }
}
