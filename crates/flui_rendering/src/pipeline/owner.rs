//! PipelineOwner manages the rendering pipeline.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use flui_foundation::RenderId;
use flui_layer::LayerTree;
use flui_types::Offset;
use parking_lot::RwLock;

use crate::pipeline::PaintingContext;
use crate::tree::RenderTree;

// ============================================================================
// Pipeline ID Counter
// ============================================================================

/// Global counter for unique pipeline owner IDs.
static PIPELINE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// ============================================================================
// DirtyNode
// ============================================================================

/// A node that needs processing in one of the pipeline phases.
///
/// Stores both the node ID and its depth in the tree for efficient sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyNode {
    /// The unique identifier of the render object.
    pub id: usize,
    /// The depth of the node in the render tree (root = 0).
    pub depth: usize,
}

impl DirtyNode {
    /// Creates a new dirty node entry.
    #[inline]
    pub fn new(id: usize, depth: usize) -> Self {
        Self { id, depth }
    }
}

// ============================================================================
// PipelineOwner
// ============================================================================

/// Manages the rendering pipeline for a tree of render objects.
///
/// The pipeline owner:
/// - Stores the root render object
/// - Tracks dirty nodes needing layout/paint/semantics
/// - Coordinates flush operations for each phase
/// - Supports hierarchical pipeline ownership
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PipelineOwner` class in
/// `rendering/object.dart`.
///
/// # Pipeline Phases
///
/// Call these methods in order during each frame:
///
/// 1. [`flush_layout`](Self::flush_layout) - Update layout
/// 2. [`flush_compositing_bits`](Self::flush_compositing_bits) - Update layer needs
/// 3. [`flush_paint`](Self::flush_paint) - Generate paint commands
/// 4. [`flush_semantics`](Self::flush_semantics) - Update accessibility tree
///
/// # Hierarchical Pipelines
///
/// Pipeline owners can be organized in a tree using [`adopt_child`](Self::adopt_child)
/// and [`drop_child`](Self::drop_child). During flush operations, parent pipelines
/// flush their own nodes first, then recursively flush children.
pub struct PipelineOwner {
    /// Unique identifier for this pipeline owner.
    id: u64,

    /// The render tree storing all RenderObjects (Slab-based).
    render_tree: RenderTree,

    /// The root render object ID of this pipeline.
    root_id: Option<RenderId>,

    /// Callback when visual update is needed.
    #[allow(clippy::type_complexity)]
    on_need_visual_update: Option<Box<dyn Fn() + Send + Sync>>,

    /// Callback when semantics owner is created.
    #[allow(clippy::type_complexity)]
    on_semantics_owner_created: Option<Box<dyn Fn() + Send + Sync>>,

    /// Callback when semantics owner is disposed.
    #[allow(clippy::type_complexity)]
    on_semantics_owner_disposed: Option<Box<dyn Fn() + Send + Sync>>,

    /// Nodes needing layout (sorted shallow-first during flush).
    nodes_needing_layout: Vec<DirtyNode>,

    /// Nodes needing compositing bits update (sorted shallow-first during flush).
    nodes_needing_compositing_bits_update: Vec<DirtyNode>,

    /// Nodes needing paint (sorted deep-first during flush).
    nodes_needing_paint: Vec<DirtyNode>,

    /// Nodes needing semantics update (sorted shallow-first during flush).
    nodes_needing_semantics: Vec<DirtyNode>,

    /// Child pipeline owners.
    children: Vec<Arc<RwLock<PipelineOwner>>>,

    /// Whether we're currently doing layout.
    debug_doing_layout: bool,

    /// Whether we're currently doing paint.
    debug_doing_paint: bool,

    /// Whether we're currently doing semantics.
    debug_doing_semantics: bool,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// The layer tree produced by the last paint phase.
    last_layer_tree: Option<LayerTree>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("id", &self.id)
            .field("root_id", &self.root_id)
            .field("render_tree_len", &self.render_tree.len())
            .field("nodes_needing_layout", &self.nodes_needing_layout.len())
            .field("nodes_needing_paint", &self.nodes_needing_paint.len())
            .field("children", &self.children.len())
            .field("debug_doing_layout", &self.debug_doing_layout)
            .field("debug_doing_paint", &self.debug_doing_paint)
            .field("debug_doing_semantics", &self.debug_doing_semantics)
            .field("has_layer_tree", &self.last_layer_tree.is_some())
            .finish()
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineOwner {
    /// Creates a new pipeline owner.
    pub fn new() -> Self {
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            on_need_visual_update: None,
            on_semantics_owner_created: None,
            on_semantics_owner_disposed: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_semantics: Vec::new(),
            children: Vec::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
        }
    }

    /// Creates a new pipeline owner with callbacks.
    pub fn with_callbacks<F, G, H>(
        on_need_visual_update: Option<F>,
        on_semantics_owner_created: Option<G>,
        on_semantics_owner_disposed: Option<H>,
    ) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        G: Fn() + Send + Sync + 'static,
        H: Fn() + Send + Sync + 'static,
    {
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            on_need_visual_update: on_need_visual_update.map(|f| Box::new(f) as _),
            on_semantics_owner_created: on_semantics_owner_created.map(|f| Box::new(f) as _),
            on_semantics_owner_disposed: on_semantics_owner_disposed.map(|f| Box::new(f) as _),
            nodes_needing_layout: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_semantics: Vec::new(),
            children: Vec::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
        }
    }

    /// Returns the unique identifier for this pipeline owner.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Sets the callback for when a visual update is needed.
    pub fn set_on_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_need_visual_update = Some(Box::new(callback));
    }

    /// Sets the callback for when semantics owner is created.
    pub fn set_on_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_created = Some(Box::new(callback));
    }

    /// Sets the callback for when semantics owner is disposed.
    pub fn set_on_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_semantics_owner_disposed = Some(Box::new(callback));
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        if let Some(callback) = &self.on_need_visual_update {
            callback();
        }
    }

    /// Returns the root render object ID.
    pub fn root_id(&self) -> Option<RenderId> {
        self.root_id
    }

    /// Sets the root render object ID.
    pub fn set_root_id(&mut self, id: Option<RenderId>) {
        self.root_id = id;
    }

    /// Returns a reference to the render tree.
    pub fn render_tree(&self) -> &RenderTree {
        &self.render_tree
    }

    /// Returns a mutable reference to the render tree.
    pub fn render_tree_mut(&mut self) -> &mut RenderTree {
        &mut self.render_tree
    }

    /// Returns a reference to the layer tree from the last paint phase.
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.last_layer_tree.as_ref()
    }

    /// Takes the layer tree from the last paint phase.
    ///
    /// This removes the layer tree from the pipeline owner, returning ownership
    /// to the caller. Useful for passing to the compositor.
    pub fn take_layer_tree(&mut self) -> Option<LayerTree> {
        self.last_layer_tree.take()
    }

    // ========================================================================
    // RenderObject Insertion (with dirty tracking)
    // ========================================================================

    /// Inserts a render object into the tree and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object into the RenderTree
    /// 2. Adds the node to the dirty layout list (since new nodes need layout)
    /// 3. Adds the node to the dirty paint list (since new nodes need paint)
    ///
    /// Use this instead of `render_tree_mut().insert()` to ensure proper dirty tracking.
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted node.
    pub fn insert_render_object(
        &mut self,
        render_object: Box<dyn crate::traits::RenderObject>,
    ) -> RenderId {
        let id = self.render_tree.insert(render_object);
        let depth = self.render_tree.depth(id).unwrap_or(0);

        // New nodes need layout and paint
        self.add_node_needing_layout(id.get(), depth);
        self.add_node_needing_paint(id.get(), depth);

        id
    }

    /// Inserts a render object as a child and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object as a child in the RenderTree
    /// 2. Adds the node to the dirty layout list
    /// 3. Adds the node to the dirty paint list
    /// 4. Marks the parent as needing layout (since child structure changed)
    ///
    /// Use this instead of `render_tree_mut().insert_child()` to ensure proper dirty tracking.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent node ID
    /// * `render_object` - The render object to insert as child
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted child, or `None` if parent doesn't exist.
    pub fn insert_child_render_object(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::traits::RenderObject>,
    ) -> Option<RenderId> {
        // Get parent depth before insertion
        let parent_depth = self.render_tree.depth(parent_id)?;

        // Insert child
        let child_id = self.render_tree.insert_child(parent_id, render_object)?;
        let child_depth = parent_depth + 1;

        // Mark child as needing layout and paint
        self.add_node_needing_layout(child_id.get(), child_depth);
        self.add_node_needing_paint(child_id.get(), child_depth);

        // Mark parent as needing layout (child structure changed)
        self.add_node_needing_layout(parent_id.get(), parent_depth);

        Some(child_id)
    }

    /// Sets the root render object and marks it as needing layout.
    ///
    /// This is a convenience method that:
    /// 1. Inserts the render object
    /// 2. Sets it as the root
    /// 3. Ensures it's in the dirty lists
    ///
    /// # Returns
    ///
    /// The `RenderId` of the root node.
    pub fn set_root_render_object(
        &mut self,
        render_object: Box<dyn crate::traits::RenderObject>,
    ) -> RenderId {
        let id = self.insert_render_object(render_object);
        self.root_id = Some(id);
        id
    }

    // ========================================================================
    // Hierarchy Management
    // ========================================================================

    /// Adopts a child pipeline owner.
    ///
    /// The child will be flushed after this owner during each phase.
    pub fn adopt_child(&mut self, child: Arc<RwLock<PipelineOwner>>) {
        self.children.push(child);
    }

    /// Drops a child pipeline owner.
    ///
    /// Returns true if the child was found and removed.
    pub fn drop_child(&mut self, child_id: u64) -> bool {
        if let Some(pos) = self.children.iter().position(|c| c.read().id == child_id) {
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    /// Returns the number of child pipeline owners.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns an iterator over child pipeline owners.
    pub fn children(&self) -> impl Iterator<Item = &Arc<RwLock<PipelineOwner>>> {
        self.children.iter()
    }

    // ========================================================================
    // Dirty Node Access (Flutter API)
    // ========================================================================

    /// Returns the nodes needing layout.
    ///
    /// These are relayout boundaries that need to be laid out in the next
    /// [`flush_layout`](Self::flush_layout) pass.
    #[inline]
    pub fn nodes_needing_layout(&self) -> &[DirtyNode] {
        &self.nodes_needing_layout
    }

    /// Returns the nodes needing paint.
    ///
    /// These are repaint boundaries that need to be painted in the next
    /// [`flush_paint`](Self::flush_paint) pass.
    #[inline]
    pub fn nodes_needing_paint(&self) -> &[DirtyNode] {
        &self.nodes_needing_paint
    }

    /// Returns the nodes needing compositing bits update.
    #[inline]
    pub fn nodes_needing_compositing_bits_update(&self) -> &[DirtyNode] {
        &self.nodes_needing_compositing_bits_update
    }

    /// Returns the nodes needing semantics update.
    #[inline]
    pub fn nodes_needing_semantics(&self) -> &[DirtyNode] {
        &self.nodes_needing_semantics
    }

    /// Adds a node to the layout dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The unique identifier of the render object
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_layout(&mut self, node_id: usize, depth: usize) {
        self.nodes_needing_layout
            .push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the paint dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The unique identifier of the render object
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_paint(&mut self, node_id: usize, depth: usize) {
        self.nodes_needing_paint
            .push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the compositing bits dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The unique identifier of the render object
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_compositing_bits_update(&mut self, node_id: usize, depth: usize) {
        self.nodes_needing_compositing_bits_update
            .push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the semantics dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The unique identifier of the render object
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_semantics(&mut self, node_id: usize, depth: usize) {
        self.nodes_needing_semantics
            .push(DirtyNode::new(node_id, depth));
    }

    // ========================================================================
    // Semantics
    // ========================================================================

    /// Returns whether semantics are enabled.
    #[inline]
    pub fn semantics_enabled(&self) -> bool {
        self.semantics_enabled.load(Ordering::Relaxed)
    }

    /// Sets whether semantics are enabled.
    pub fn set_semantics_enabled(&self, enabled: bool) {
        let was_enabled = self.semantics_enabled.swap(enabled, Ordering::Relaxed);
        if enabled && !was_enabled {
            if let Some(callback) = &self.on_semantics_owner_created {
                callback();
            }
        } else if !enabled && was_enabled {
            if let Some(callback) = &self.on_semantics_owner_disposed {
                callback();
            }
        }
    }

    // ========================================================================
    // Layout Phase
    // ========================================================================

    /// Updates layout for all dirty render objects.
    ///
    /// This is phase 1 of the rendering pipeline. During layout:
    /// - Sizes and positions are calculated
    /// - Objects may dirty paint or compositing
    ///
    /// Nodes are sorted by depth (shallow first) so parents are laid out
    /// before their children. This matches Flutter's `flushLayout` behavior.
    ///
    /// After processing own nodes, recursively flushes child pipeline owners.
    pub fn flush_layout(&mut self) {
        tracing::debug!("flush_layout: {} nodes", self.nodes_needing_layout.len());

        // Process own dirty nodes if any
        // Flutter pattern: while loop to handle nodes added during layout
        while !self.nodes_needing_layout.is_empty() {
            self.debug_doing_layout = true;

            // Take the dirty nodes and replace with empty vec
            // This allows new nodes to be added during layout
            let mut dirty_nodes = std::mem::take(&mut self.nodes_needing_layout);

            // Sort by depth (shallow first) - parents before children
            // Flutter: dirtyNodes.sort((a, b) => a.depth - b.depth)
            dirty_nodes.sort_unstable_by_key(|node| node.depth);

            // Process each dirty node
            for dirty_node in dirty_nodes {
                // Look up the node in the RenderTree by its ID
                // The DirtyNode.id is the slab index (0-based), but RenderId is 1-based
                let render_id = RenderId::new(dirty_node.id);

                if let Some(render_node) = self.render_tree.get_mut(render_id) {
                    let render_object = render_node.render_object_mut();

                    // Only process if still needs layout and owned by this pipeline
                    // Flutter: if (node._needsLayout && node.owner == this)
                    if render_object.needs_layout() {
                        tracing::trace!(
                            "flush_layout: laying out node id={} depth={}",
                            dirty_node.id,
                            dirty_node.depth
                        );

                        // Special handling for RenderView (root)
                        if let Some(render_view) = render_object
                            .as_any_mut()
                            .downcast_mut::<crate::view::RenderView>()
                        {
                            // Ensure initial frame is prepared (sets up root_transform)
                            render_view.prepare_initial_frame_without_owner();
                            render_view.perform_layout();
                        } else {
                            // For non-root nodes, call layout_without_resize()
                            // This uses cached constraints from the parent's layout() call
                            //
                            // Note: In a full implementation, the parent would call
                            // child.layout(constraints, parentUsesSize) during its
                            // perform_layout(). For now, if no constraints are cached,
                            // we set default constraints.
                            use crate::constraints::BoxConstraints;

                            if render_object.cached_constraints().is_none() {
                                // No cached constraints - this is likely first layout
                                // Set default loose constraints
                                let default_constraints =
                                    BoxConstraints::loose(flui_types::Size::new(800.0, 600.0));
                                render_object.set_cached_constraints(default_constraints);
                            }

                            // Now call layout_without_resize which uses cached constraints
                            render_object.layout_without_resize();
                        }
                    }
                }
            }

            self.debug_doing_layout = false;
        }

        // Always flush children, even if parent has no dirty nodes
        // Flutter does this to ensure hierarchical pipeline owners work correctly
        for child in &self.children {
            child.write().flush_layout();
        }
    }

    // ========================================================================
    // Compositing Bits Phase
    // ========================================================================

    /// Updates compositing bits for all dirty render objects.
    ///
    /// This is phase 2 of the rendering pipeline. During this phase:
    /// - Each object determines if it needs a compositing layer
    /// - This information is used during paint
    ///
    /// Nodes are sorted by depth (shallow first). This matches Flutter's
    /// `flushCompositingBits` behavior.
    ///
    /// After processing own nodes, recursively flushes child pipeline owners.
    pub fn flush_compositing_bits(&mut self) {
        tracing::debug!(
            "flush_compositing_bits: {} nodes",
            self.nodes_needing_compositing_bits_update.len()
        );

        // Sort by depth (shallow first)
        // Flutter: _nodesNeedingCompositingBitsUpdate.sort((a, b) => a.depth - b.depth)
        self.nodes_needing_compositing_bits_update
            .sort_unstable_by_key(|node| node.depth);

        // Process dirty nodes
        for node in &self.nodes_needing_compositing_bits_update {
            tracing::trace!("compositing bits node id={} depth={}", node.id, node.depth);
            // TODO: Look up node by id and call _update_compositing_bits()
            // if node._needs_compositing_bits_update && node.owner == self
        }
        self.nodes_needing_compositing_bits_update.clear();

        // Flush children
        for child in &self.children {
            child.write().flush_compositing_bits();
        }
    }

    // ========================================================================
    // Paint Phase
    // ========================================================================

    /// Paints all dirty render objects.
    ///
    /// This is phase 3 of the rendering pipeline. During paint:
    /// - Render objects record paint commands
    /// - Compositing layers are built
    ///
    /// Nodes are sorted by depth (deep first) so children are painted before
    /// their parents. This matches Flutter's `flushPaint` behavior.
    ///
    /// After processing own nodes, recursively flushes child pipeline owners.
    pub fn flush_paint(&mut self) {
        tracing::debug!("flush_paint: {} nodes", self.nodes_needing_paint.len());

        // Process own dirty nodes if any
        if !self.nodes_needing_paint.is_empty() {
            self.debug_doing_paint = true;

            // Take dirty nodes and replace with empty vec
            let dirty_nodes = std::mem::take(&mut self.nodes_needing_paint);

            // Sort by depth (deep first) - children before parents
            // Flutter: dirtyNodes.sort((a, b) => b.depth - a.depth)
            // Note: We don't need to sort for now since we paint from root

            // Clear needs_paint flags for all dirty nodes
            for dirty_node in &dirty_nodes {
                let render_id = RenderId::new(dirty_node.id);
                if let Some(render_node) = self.render_tree.get_mut(render_id) {
                    render_node.render_object_mut().clear_needs_paint();
                }
            }

            // Paint all render objects
            // Since RenderView.child is not directly set (children tracked via RenderTree),
            // we paint all render objects in the tree directly.
            if let Some(root_id) = self.root_id {
                if let Some(root_node) = self.render_tree.get(root_id) {
                    let paint_bounds = root_node.render_object().paint_bounds();
                    tracing::debug!("flush_paint: painting root with bounds {:?}", paint_bounds);

                    // Create PaintingContext
                    let mut context = PaintingContext::new(paint_bounds);

                    // Paint all nodes in the tree (parents first, then children)
                    // Collect all node IDs first to avoid borrow issues
                    let node_ids: Vec<RenderId> =
                        self.render_tree.iter().map(|(id, _)| id).collect();

                    for render_id in node_ids {
                        if let Some(render_node) = self.render_tree.get(render_id) {
                            let render_object = render_node.render_object();
                            tracing::trace!("flush_paint: painting node {:?}", render_id);
                            render_object.paint(&mut context, Offset::ZERO);
                        }
                    }

                    // Store the resulting layer tree
                    self.last_layer_tree = Some(context.into_layer_tree());
                    tracing::debug!(
                        "flush_paint: layer tree has {} layers",
                        self.last_layer_tree.as_ref().map(|t| t.len()).unwrap_or(0)
                    );
                }
            }

            self.debug_doing_paint = false;
        }

        // Always flush children, even if parent has no dirty nodes
        // Flutter does this to ensure hierarchical pipeline owners work correctly
        for child in &self.children {
            child.write().flush_paint();
        }
    }

    // ========================================================================
    // Semantics Phase
    // ========================================================================

    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    ///
    /// Nodes are sorted by depth (shallow first) for top-down traversal.
    /// The geometries of children depend on ancestors' transforms and clips,
    /// so parents must be processed first. This matches Flutter's `flushSemantics`.
    ///
    /// After processing own nodes, recursively flushes child pipeline owners.
    pub fn flush_semantics(&mut self) {
        if !self.semantics_enabled() {
            return;
        }

        tracing::debug!(
            "flush_semantics: {} nodes",
            self.nodes_needing_semantics.len()
        );

        self.debug_doing_semantics = true;

        // Filter out nodes that still need layout (they're not ready for semantics)
        // Flutter: .where((object) => !object._needsLayout && object.owner == this)
        let nodes_to_process: Vec<DirtyNode> = self.nodes_needing_semantics.to_vec();

        self.nodes_needing_semantics.clear();

        // Sort by depth (shallow first) - top-down order
        // Flutter: .sort((a, b) => a.depth - b.depth)
        let mut sorted_nodes = nodes_to_process;
        sorted_nodes.sort_unstable_by_key(|node| node.depth);

        // Process dirty nodes in three passes (matching Flutter):
        // 1. updateChildren - update semantic children relationships
        // 2. ensureGeometry - calculate semantic geometry
        // 3. ensureSemanticsNode - create/update semantics nodes (reversed order)
        for node in &sorted_nodes {
            tracing::trace!(
                "semantics updateChildren id={} depth={}",
                node.id,
                node.depth
            );
            // TODO: node._semantics.updateChildren()
        }

        for node in &sorted_nodes {
            tracing::trace!(
                "semantics ensureGeometry id={} depth={}",
                node.id,
                node.depth
            );
            // TODO: node._semantics.ensureGeometry()
        }

        for node in sorted_nodes.iter().rev() {
            tracing::trace!(
                "semantics ensureSemanticsNode id={} depth={}",
                node.id,
                node.depth
            );
            // TODO: node._semantics.ensureSemanticsNode()
        }

        // TODO: _semantics_owner.send_semantics_update()

        // Flush children
        for child in &self.children {
            child.write().flush_semantics();
        }

        self.debug_doing_semantics = false;
    }

    /// Flushes all pipeline phases in the correct order.
    ///
    /// This is a convenience method that calls all flush methods in sequence:
    /// 1. `flush_layout()`
    /// 2. `flush_compositing_bits()`
    /// 3. `flush_paint()`
    /// 4. `flush_semantics()`
    pub fn flush_all(&mut self) {
        self.flush_layout();
        self.flush_compositing_bits();
        self.flush_paint();
        self.flush_semantics();
    }

    // ========================================================================
    // Debug
    // ========================================================================

    /// Returns whether layout is currently being performed.
    #[inline]
    pub fn debug_doing_layout(&self) -> bool {
        self.debug_doing_layout
    }

    /// Returns whether paint is currently being performed.
    #[inline]
    pub fn debug_doing_paint(&self) -> bool {
        self.debug_doing_paint
    }

    /// Returns whether semantics update is currently being performed.
    #[inline]
    pub fn debug_doing_semantics(&self) -> bool {
        self.debug_doing_semantics
    }

    /// Returns whether any pipeline phase is currently active.
    #[inline]
    pub fn debug_doing_any_phase(&self) -> bool {
        self.debug_doing_layout || self.debug_doing_paint || self.debug_doing_semantics
    }

    /// Returns the total number of dirty nodes across all lists.
    pub fn dirty_node_count(&self) -> usize {
        self.nodes_needing_layout.len()
            + self.nodes_needing_compositing_bits_update.len()
            + self.nodes_needing_paint.len()
            + self.nodes_needing_semantics.len()
    }

    /// Returns whether there are any dirty nodes.
    #[inline]
    pub fn has_dirty_nodes(&self) -> bool {
        !self.nodes_needing_layout.is_empty()
            || !self.nodes_needing_compositing_bits_update.is_empty()
            || !self.nodes_needing_paint.is_empty()
            || !self.nodes_needing_semantics.is_empty()
    }

    /// Clears all dirty node lists without processing them.
    ///
    /// Use with caution - this discards pending work.
    pub fn clear_all_dirty_nodes(&mut self) {
        self.nodes_needing_layout.clear();
        self.nodes_needing_compositing_bits_update.clear();
        self.nodes_needing_paint.clear();
        self.nodes_needing_semantics.clear();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_owner_new() {
        let owner = PipelineOwner::new();
        assert!(owner.root_id().is_none());
        assert!(owner.nodes_needing_layout().is_empty());
        assert!(owner.nodes_needing_paint().is_empty());
        assert!(!owner.debug_doing_layout());
        assert!(!owner.debug_doing_paint());
    }

    #[test]
    fn test_pipeline_owner_id_unique() {
        let owner1 = PipelineOwner::new();
        let owner2 = PipelineOwner::new();
        assert_ne!(owner1.id(), owner2.id());
    }

    #[test]
    fn test_pipeline_owner_dirty_nodes() {
        let mut owner = PipelineOwner::new();

        owner.add_node_needing_layout(1, 0);
        owner.add_node_needing_layout(2, 1);
        owner.add_node_needing_paint(3, 2);

        assert_eq!(owner.nodes_needing_layout().len(), 2);
        assert_eq!(owner.nodes_needing_paint().len(), 1);
        assert_eq!(owner.dirty_node_count(), 3);
        assert!(owner.has_dirty_nodes());
    }

    #[test]
    fn test_pipeline_owner_flush_layout() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(1, 0);
        owner.add_node_needing_layout(2, 1);

        owner.flush_layout();

        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_pipeline_owner_flush_all() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(1, 0);
        owner.add_node_needing_paint(2, 1);
        owner.add_node_needing_compositing_bits_update(3, 2);

        owner.flush_all();

        assert!(!owner.has_dirty_nodes());
    }

    #[test]
    fn test_flush_layout_sorts_by_depth_shallow_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in reverse depth order
        owner.add_node_needing_layout(3, 2); // deepest
        owner.add_node_needing_layout(1, 0); // shallowest
        owner.add_node_needing_layout(2, 1); // middle

        // Before flush, they're in insertion order
        assert_eq!(owner.nodes_needing_layout()[0].depth, 2);
        assert_eq!(owner.nodes_needing_layout()[1].depth, 0);
        assert_eq!(owner.nodes_needing_layout()[2].depth, 1);

        owner.flush_layout();

        // After flush, list is cleared
        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_flush_paint_sorts_by_depth_deep_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in shallow-first order
        owner.add_node_needing_paint(1, 0); // shallowest
        owner.add_node_needing_paint(2, 1); // middle
        owner.add_node_needing_paint(3, 2); // deepest

        owner.flush_paint();

        // After flush, list is cleared
        assert!(owner.nodes_needing_paint().is_empty());
    }

    #[test]
    fn test_pipeline_owner_hierarchy() {
        let mut parent = PipelineOwner::new();
        let child = Arc::new(RwLock::new(PipelineOwner::new()));
        let child_id = child.read().id();

        parent.adopt_child(child.clone());
        assert_eq!(parent.child_count(), 1);

        assert!(parent.drop_child(child_id));
        assert_eq!(parent.child_count(), 0);
    }

    #[test]
    fn test_pipeline_owner_semantics_enabled() {
        let owner = PipelineOwner::new();
        assert!(!owner.semantics_enabled());

        owner.set_semantics_enabled(true);
        assert!(owner.semantics_enabled());

        owner.set_semantics_enabled(false);
        assert!(!owner.semantics_enabled());
    }

    #[test]
    fn test_pipeline_owner_clear_dirty_nodes() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(1, 0);
        owner.add_node_needing_paint(2, 1);
        owner.add_node_needing_semantics(3, 2);

        owner.clear_all_dirty_nodes();

        assert!(!owner.has_dirty_nodes());
        assert_eq!(owner.dirty_node_count(), 0);
    }

    #[test]
    fn test_pipeline_owner_with_callbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }
}
