//! PipelineOwner manages the rendering pipeline.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use flui_foundation::RenderId;
use flui_layer::LayerTree;
use flui_types::Offset;
use parking_lot::RwLock;

use crate::context::CanvasContext;
use crate::storage::RenderTree;

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
        render_object: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
    ) -> RenderId {
        use flui_tree::traits::TreeWrite;
        let node = crate::storage::RenderNode::new_box(render_object);
        let id = self.render_tree.insert(node);
        let depth = self.render_tree.depth(id).unwrap_or(0) as usize;

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
        render_object: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth before insertion
        let parent_depth = self.render_tree.depth(parent_id)?;

        // Insert child (using Box protocol)
        let child_id = self
            .render_tree
            .insert_box_child(parent_id, render_object)?;
        let child_depth = parent_depth + 1;

        // Mark child as needing layout and paint
        self.add_node_needing_layout(child_id.get(), child_depth as usize);
        self.add_node_needing_paint(child_id.get(), child_depth as usize);

        // Mark parent as needing layout (child structure changed)
        self.add_node_needing_layout(parent_id.get(), parent_depth as usize);

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
        render_object: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
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
    /// # Synchronous Child Layout
    ///
    /// With interior mutability (RwLock on RenderNode), parent's `perform_layout`
    /// can call `layout_child()` which triggers synchronous child layout through
    /// the RenderTree. The child is laid out immediately and returns its size.
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

            tracing::debug!(
                "flush_layout: sorted order (shallow-first) = {:?}",
                dirty_nodes
                    .iter()
                    .map(|n| (n.id, n.depth))
                    .collect::<Vec<_>>()
            );

            // Process each dirty node
            for dirty_node in dirty_nodes {
                // Look up the node in the RenderTree by its ID
                // The DirtyNode.id is the slab index (0-based), but RenderId is 1-based
                let render_id = RenderId::new(dirty_node.id);

                // Layout this node with synchronous child layout support
                self.layout_node_with_children(render_id);
            }

            self.debug_doing_layout = false;
        }

        // Always flush children, even if parent has no dirty nodes
        // Flutter does this to ensure hierarchical pipeline owners work correctly
        for child in &self.children {
            child.write().flush_layout();
        }
    }

    /// Lays out a single node with depth-first child layout.
    ///
    /// This method follows Flutter's layout model:
    /// 1. Propagate constraints to children
    /// 2. Layout children first (depth-first) so their sizes are available
    /// 3. Sync child sizes to parent's ChildState
    /// 4. Layout parent using child sizes via `layout_child()` calls
    ///
    /// This ensures that when parent's `perform_layout` calls `layout_child()`,
    /// the child's size is already cached and available.
    ///
    /// # Interior Mutability
    ///
    /// Uses RwLock on RenderNode to allow parent to hold a lock while
    /// children are being laid out through separate locks.
    fn layout_node_with_children(&mut self, render_id: RenderId) {
        // Check if node exists and needs layout
        let needs_layout = {
            if let Some(render_node) = self.render_tree.get(render_id) {
                render_node.needs_layout()
            } else {
                return;
            }
        };

        if !needs_layout {
            return;
        }

        tracing::trace!(
            "layout_node_with_children: laying out node id={:?}",
            render_id
        );

        // STEP 1: Get children IDs and propagate constraints
        let children: Vec<RenderId> = {
            if let Some(render_node) = self.render_tree.get(render_id) {
                render_node.children().to_vec()
            } else {
                Vec::new()
            }
        };

        // STEP 2: Layout children FIRST (depth-first)
        // This ensures child sizes are available when parent's perform_layout runs
        for child_id in &children {
            let child_needs_layout = {
                if let Some(child_node) = self.render_tree.get(*child_id) {
                    child_node.needs_layout()
                } else {
                    false
                }
            };

            if child_needs_layout {
                // Propagate constraints from parent to child
                self.propagate_constraints_to_child(render_id, *child_id);

                // Recursively layout the child (depth-first)
                self.layout_node_with_children(*child_id);
            }

            // STEP 3: Sync child size to parent's ChildState BEFORE parent layout
            self.sync_child_size_to_parent(*child_id);
        }

    }

    /// Propagates constraints from parent to child.
    ///
    /// This is called before laying out a child to ensure it has proper constraints.
    /// We pass loose constraints (same max, zero min) so children can size themselves
    /// within the parent's bounds. This matches Flutter's typical behavior where
    /// parents like Center/Align give children loose constraints.
    fn propagate_constraints_to_child(&self, _parent_id: RenderId, _child_id: RenderId) {
    }

    /// Syncs a child's size to its parent's ChildState.
    ///
    /// After a child is laid out, this method updates the parent's internal
    /// ChildState with the child's resulting size. This allows the parent's
    /// `layout_child()` to return the correct size.
    fn sync_child_size_to_parent(&mut self, _child_id: RenderId) {
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
        //
        // Note: Full compositing bits update is not yet implemented.
        // This would require:
        // 1. PipelineOwner to hold a reference to RenderTree
        // 2. Look up each render object by ID
        // 3. Call render_object.update_compositing_bits()
        //
        // Currently we just clear the list - compositing works but
        // may not be optimally batched.
        for node in &self.nodes_needing_compositing_bits_update {
            tracing::trace!(
                "compositing bits update: node id={} depth={} (batching not implemented)",
                node.id,
                node.depth
            );
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
                if let Some(render_node) = self.render_tree.get(render_id) {
                    render_node.clear_needs_paint();
                }
            }

            // Paint render tree recursively starting from root.
            // Each parent paints itself, then paints children with accumulated offset.
            if let Some(root_id) = self.root_id {
                if let Some(root_node) = self.render_tree.get(root_id) {
                    let paint_bounds = root_node.paint_bounds();
                    tracing::debug!("flush_paint: painting root with bounds {:?}", paint_bounds);

                    // Create CanvasContext
                    let mut context = CanvasContext::new(paint_bounds);

                    // Paint recursively from root with offset accumulation
                    self.paint_node_recursive(&mut context, root_id, Offset::ZERO);

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

    /// Recursively paints a node and its children with accumulated offset.
    ///
    /// This follows Flutter's approach where each parent:
    /// 1. Paints itself at the given offset
    /// 2. For each child, adds child's offset and recursively paints
    ///
    /// # Repaint Boundaries
    ///
    /// When a child is a repaint boundary (`is_repaint_boundary() == true`),
    /// it creates its own `OffsetLayer` to isolate its painting. The offset
    /// is stored in the layer rather than accumulated, allowing the subtree
    /// to be cached and reused when only the offset changes.
    ///
    /// The tree structure (parent-child relationships) is stored in RenderTree,
    /// while child offsets are stored in each render object's internal state
    /// (set during layout via position_child).
    fn paint_node_recursive(&self, context: &mut CanvasContext, node_id: RenderId, offset: Offset) {
        // Get the render node and collect info for painting
        let (is_repaint_boundary, children_with_offsets, paint_alpha, paint_transform): (
            bool,
            Vec<(RenderId, Offset)>,
            Option<u8>,
            Option<flui_types::Matrix4>,
        ) = {
            if let Some(render_node) = self.render_tree.get(node_id) {
                let render_object = render_node.box_render_object();

                // Get children from tree structure (RenderNode stores parent-child relationships)
                let tree_children = render_node.children();

                let is_boundary = render_object.is_repaint_boundary();
                let alpha = render_object.paint_alpha();
                let transform = render_object.paint_transform();

                tracing::debug!(
                    "paint_node_recursive: node_id={:?}, offset=({}, {}), is_repaint_boundary={}, tree_children={}, ro_child_count={}, alpha={:?}",
                    node_id,
                    offset.dx,
                    offset.dy,
                    is_boundary,
                    tree_children.len(),
                    render_object.child_count(),
                    alpha
                );

                // Paint this node at the accumulated offset
                render_object.paint(context, offset);

                // For each child in the tree, get its offset from the render object
                // The render object stores offsets via position_child during layout
                let children: Vec<_> = tree_children
                    .iter()
                    .enumerate()
                    .map(|(i, &child_id)| {
                        // Get offset from render object (set during layout)
                        let child_offset = render_object.child_offset(i);
                        tracing::debug!(
                            "  child[{}]: id={:?}, offset=({}, {})",
                            i,
                            child_id,
                            child_offset.dx,
                            child_offset.dy
                        );
                        (child_id, child_offset)
                    })
                    .collect();

                (is_boundary, children, alpha, transform)
            } else {
                return;
            }
        };

        // Helper closure to paint all children
        let paint_children = |ctx: &mut CanvasContext, base_offset: Offset| {
            for (child_id, child_offset) in &children_with_offsets {
                // Check if child is a repaint boundary
                let child_is_repaint_boundary = {
                    if let Some(child_node) = self.render_tree.get(*child_id) {
                        child_node.box_render_object().is_repaint_boundary()
                    } else {
                        false
                    }
                };

                if child_is_repaint_boundary {
                    // For repaint boundaries, create a new OffsetLayer
                    let child_accumulated_offset = base_offset + *child_offset;

                    ctx.paint_child_with_offset(child_accumulated_offset, |child_ctx| {
                        self.paint_node_recursive(child_ctx, *child_id, Offset::ZERO);
                    });
                } else {
                    // Normal case: accumulate offset and paint directly
                    let child_accumulated_offset = base_offset + *child_offset;
                    self.paint_node_recursive(ctx, *child_id, child_accumulated_offset);
                }
            }
        };

        // Apply effect layers (opacity, transform) around children
        if let Some(alpha) = paint_alpha {
            // Skip painting children entirely if fully transparent
            if alpha == 0 {
                // Don't paint children at all
            } else {
                // Wrap children in opacity layer
                // The offset is where this node is positioned. Children are painted
                // relative to this node, so we pass Offset::ZERO for children's base,
                // but the OpacityLayer itself is positioned at `offset`.
                context.push_opacity(offset, alpha, |opacity_ctx| {
                    if let Some(transform) = paint_transform {
                        // Apply transform layer inside opacity
                        opacity_ctx.push_transform(
                            true,
                            Offset::ZERO,
                            &transform,
                            |transform_ctx| {
                                paint_children(transform_ctx, Offset::ZERO);
                            },
                            None,
                        );
                    } else {
                        // Children paint relative to the opacity layer's origin
                        paint_children(opacity_ctx, Offset::ZERO);
                    }
                });
            }
        } else if let Some(transform) = paint_transform {
            // Apply transform layer
            context.push_transform(
                true,
                offset,
                &transform,
                |transform_ctx| {
                    paint_children(transform_ctx, Offset::ZERO);
                },
                None,
            );
        } else {
            // No effect layers - paint children directly
            paint_children(context, offset);
        }

        // Track that this was a repaint boundary for future reference
        if is_repaint_boundary {
            if let Some(render_node) = self.render_tree.get(node_id) {
                let mut render_object = render_node.box_render_object_mut();
                render_object.set_was_repaint_boundary(true);
            }
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

        // Semantics system is not yet implemented
        if !nodes_to_process.is_empty() {
            unimplemented!(
                "Semantics system not yet implemented - requires full semantics integration. \
                 {} nodes need semantics updates",
                nodes_to_process.len()
            );
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
