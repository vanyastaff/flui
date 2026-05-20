//! LayerTree - Slab-based storage for compositor layers
//!
//! This module provides the LayerTree struct and LayerNode
//! for managing the compositor layer hierarchy.

use flui_foundation::{ElementId, LayerId};
use flui_types::{Offset, geometry::Pixels};
use slab::Slab;

use crate::layer::Layer;

// ============================================================================
// LAYER NODE
// ============================================================================

/// A node in the LayerTree that wraps a Layer with tree structure metadata.
///
/// # Design
///
/// Unlike ViewTree and RenderTree which are generic over the object type,
/// LayerNode is concrete because Layer is already an enum that encompasses
/// all layer types. This simplifies the API while maintaining the same
/// architectural pattern.
#[derive(Debug)]
pub struct LayerNode {
    // ========== Tree Structure ==========
    parent: Option<LayerId>,
    children: Vec<LayerId>,

    // ========== Layer ==========
    /// The compositor layer (Canvas, ShaderMask, etc.)
    layer: Layer,

    // ========== Metadata ==========
    /// Whether this layer needs compositing
    needs_compositing: bool,

    /// Offset from parent (parent data)
    offset: Option<Offset<Pixels>>,

    /// Associated ElementId (for cross-tree references)
    element_id: Option<ElementId>,
}

impl LayerNode {
    /// Creates a new LayerNode with the given Layer.
    pub fn new(layer: Layer) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            layer,
            needs_compositing: true, // Default: layers need compositing
            offset: None,
            element_id: None,
        }
    }

    /// Creates a LayerNode with an associated ElementId.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Creates a LayerNode with an offset.
    pub fn with_offset(mut self, offset: Offset<Pixels>) -> Self {
        self.offset = Some(offset);
        self
    }

    // ========== Tree Structure ==========

    /// Gets the parent LayerId.
    #[inline]
    pub fn parent(&self) -> Option<LayerId> {
        self.parent
    }

    /// Sets the parent LayerId.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<LayerId>) {
        self.parent = parent;
    }

    /// Gets all children LayerIds.
    #[inline]
    pub fn children(&self) -> &[LayerId] {
        &self.children
    }

    /// Adds a child to this layer node.
    #[inline]
    pub fn add_child(&mut self, child: LayerId) {
        self.children.push(child);
    }

    /// Removes a child from this layer node.
    #[inline]
    pub fn remove_child(&mut self, child: LayerId) {
        self.children.retain(|&id| id != child);
    }

    /// Clears all children from this layer node.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    // ========== Layer Access ==========

    /// Returns reference to the Layer.
    #[inline]
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// Returns mutable reference to the Layer.
    #[inline]
    pub fn layer_mut(&mut self) -> &mut Layer {
        &mut self.layer
    }

    // ========== Metadata ==========

    /// Gets whether this layer needs compositing.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    /// Sets whether this layer needs compositing.
    #[inline]
    pub fn set_needs_compositing(&mut self, needs: bool) {
        self.needs_compositing = needs;
    }

    /// Gets the offset from parent (parent data).
    #[inline]
    pub fn offset(&self) -> Option<Offset<Pixels>> {
        self.offset
    }

    /// Sets the offset from parent.
    #[inline]
    pub fn set_offset(&mut self, offset: Option<Offset<Pixels>>) {
        self.offset = offset;
    }

    /// Gets the associated ElementId (for cross-tree references).
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated ElementId.
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
}

// ============================================================================
// LAYER TREE
// ============================================================================

/// LayerTree - Slab-based storage for compositor layers.
///
/// This is the fourth of FLUI's five trees, corresponding to Flutter's Layer
/// tree used for composition and GPU rendering.
///
/// # Architecture
///
/// ```text
/// LayerTree
///   ├─ nodes: Slab<LayerNode>  (direct storage)
///   └─ root: Option<LayerId>
/// ```
///
/// # Thread Safety
///
/// LayerTree itself is not thread-safe. Use `Arc<RwLock<LayerTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust
/// use flui_layer::{CanvasLayer, Layer, LayerNode, LayerTree};
/// use flui_tree::TreeRead;
///
/// let mut tree = LayerTree::new();
///
/// // Insert canvas layer
/// let canvas_layer = Layer::Canvas(CanvasLayer::new());
/// let id = tree.insert(canvas_layer);
///
/// // Access layer
/// let node = tree.get(id).unwrap();
/// assert!(node.needs_compositing());
/// ```
#[derive(Debug)]
pub struct LayerTree {
    /// Slab storage for LayerNodes (0-based indexing internally)
    nodes: Slab<LayerNode>,

    /// Root LayerNode ID (None if tree is empty)
    root: Option<LayerId>,
}

impl LayerTree {
    /// Creates a new empty LayerTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a LayerTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root LayerNode ID.
    #[inline]
    pub fn root(&self) -> Option<LayerId> {
        self.root
    }

    /// Set the root LayerNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<LayerId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a LayerNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: LayerId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of LayerNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a Layer into the tree.
    ///
    /// Returns the LayerId of the inserted node.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `LayerId(1)`
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_layer::{CanvasLayer, Layer, LayerTree};
    ///
    /// let mut tree = LayerTree::new();
    /// let layer = Layer::Canvas(CanvasLayer::new());
    /// let id = tree.insert(layer);
    /// ```
    pub fn insert(&mut self, layer: Layer) -> LayerId {
        let node = LayerNode::new(layer);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1) // +1 offset
    }

    /// Inserts a Layer with an associated ElementId.
    pub fn insert_with_element(&mut self, layer: Layer, element_id: ElementId) -> LayerId {
        let node = LayerNode::new(layer).with_element_id(element_id);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1)
    }

    /// Returns a reference to a LayerNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `LayerId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: LayerId) -> Option<&LayerNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a LayerNode.
    #[inline]
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut LayerNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Returns a reference to the Layer directly.
    #[inline]
    pub fn get_layer(&self, id: LayerId) -> Option<&Layer> {
        self.get(id).map(LayerNode::layer)
    }

    /// Returns a mutable reference to the Layer directly.
    #[inline]
    pub fn get_layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.get_mut(id).map(LayerNode::layer_mut)
    }

    /// Removes a LayerNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree
    /// cleanup.
    pub fn remove(&mut self, id: LayerId) -> Option<LayerNode> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent LayerNode.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: LayerId, child_id: LayerId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent LayerNode.
    pub fn remove_child(&mut self, parent_id: LayerId, child_id: LayerId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns the parent of a node.
    pub fn parent(&self, id: LayerId) -> Option<LayerId> {
        self.get(id)?.parent()
    }

    /// Returns the children of a node.
    pub fn children(&self, id: LayerId) -> Option<&[LayerId]> {
        self.get(id).map(LayerNode::children)
    }

    /// Clears all children from a parent node.
    ///
    /// This is used by Flutter's `pushLayer` when reusing layers - old children
    /// are removed before adding new content.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Flutter pattern: reuse layer, clear old children
    /// if let Some(old_layer) = reusable_layer {
    ///     tree.clear_children(old_layer_id);
    /// }
    /// ```
    pub fn clear_children(&mut self, parent_id: LayerId) {
        // First, get the list of children to clear their parent references
        let children_to_clear: Vec<LayerId> = if let Some(parent) = self.get(parent_id) {
            parent.children().to_vec()
        } else {
            return;
        };

        // Clear parent's children list
        if let Some(parent) = self.get_mut(parent_id) {
            parent.clear_children();
        }

        // Clear parent reference from each child
        for child_id in children_to_clear {
            if let Some(child) = self.get_mut(child_id) {
                child.set_parent(None);
            }
        }
    }

    // ========== Layer Composition (Flutter PaintingContext Pattern) ==========

    /// Appends a layer as a child of a container layer.
    ///
    /// This is the core operation used by Flutter's PaintingContext when
    /// composing layers during painting. It's typically called in two
    /// scenarios:
    ///
    /// 1. **After stopRecordingIfNeeded()**: Append the finished PictureLayer
    /// 2. **In pushLayer()**: Append a container layer before painting into it
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void _appendLayer(Layer layer) {
    ///   _containerLayer.append(layer);
    /// }
    /// ```
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_layer::{LayerTree, Layer, PictureLayer};
    ///
    /// let mut tree = LayerTree::new();
    ///
    /// // Create container layer (e.g., OffsetLayer)
    /// let container = Layer::Offset(OffsetLayer::zero());
    /// let container_id = tree.insert(container);
    ///
    /// // Record some drawing commands
    /// let mut canvas = Canvas::new();
    /// canvas.draw_rect(rect, &paint);
    /// let picture = canvas.finish();
    ///
    /// // Create picture layer
    /// let picture_layer = Layer::Picture(PictureLayer::new(picture));
    /// let picture_id = tree.insert(picture_layer);
    ///
    /// // Append to container (Flutter: _containerLayer.append(layer))
    /// tree.append_layer(container_id, picture_id);
    /// ```
    ///
    /// # Usage in PaintingContext
    ///
    /// ```rust,ignore
    /// impl PaintingContext {
    ///     fn stop_recording_if_needed(&mut self) {
    ///         if let Some(current_layer) = self.current_layer.take() {
    ///             // Finish recording
    ///             let picture = self.canvas.finish();
    ///             let picture_layer = PictureLayer::new(picture);
    ///             let layer_id = self.layer_tree.insert(Layer::Picture(picture_layer));
    ///
    ///             // Append to container (THIS METHOD)
    ///             self.layer_tree.append_layer(self.container_layer, layer_id);
    ///         }
    ///     }
    ///
    ///     fn push_layer<F>(&mut self, layer: Layer, painter: F, offset: Offset)
    ///     where
    ///         F: FnOnce(&mut PaintingContext, Offset),
    ///     {
    ///         self.stop_recording_if_needed();
    ///
    ///         // Insert and append container layer (THIS METHOD)
    ///         let layer_id = self.layer_tree.insert(layer);
    ///         self.layer_tree.append_layer(self.container_layer, layer_id);
    ///
    ///         // Create child context and paint
    ///         let mut child_context = PaintingContext::new(layer_id, ...);
    ///         painter(&mut child_context, offset);
    ///         child_context.stop_recording_if_needed();
    ///     }
    /// }
    /// ```
    pub fn append_layer(&mut self, container_id: LayerId, child_id: LayerId) {
        self.add_child(container_id, child_id);
    }

    /// Appends multiple layers to a container in order.
    ///
    /// This is a convenience method for bulk appending, which is common when
    /// building complex layer hierarchies.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.append_layers(container_id, &[layer1_id, layer2_id, layer3_id]);
    /// ```
    pub fn append_layers(&mut self, container_id: LayerId, children: &[LayerId]) {
        for &child_id in children {
            self.append_layer(container_id, child_id);
        }
    }

    // ========== Iteration ==========

    /// Returns an iterator over all LayerIds in the tree.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.nodes.iter().map(|(index, _)| LayerId::new(index + 1))
    }

    /// Returns the raw slab iterator for zero-cost iteration.
    ///
    /// Used internally by tree trait implementations.
    #[inline]
    pub(crate) fn iter_slab(&self) -> slab::Iter<'_, LayerNode> {
        self.nodes.iter()
    }

    /// Returns an iterator over all (LayerId, &LayerNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (LayerId, &LayerNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (LayerId::new(index + 1), node))
    }

    /// Returns a mutable iterator over all (LayerId, &mut LayerNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (LayerId, &mut LayerNode)> + '_ {
        self.nodes
            .iter_mut()
            .map(|(index, node)| (LayerId::new(index + 1), node))
    }
}

impl Default for LayerTree {
    fn default() -> Self {
        Self::new()
    }
}
