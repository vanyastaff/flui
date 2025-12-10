//! LayerTree - Slab-based storage for compositor layers
//!
//! This module provides the LayerTree struct and LayerNode
//! for managing the compositor layer hierarchy.

use slab::Slab;

use flui_foundation::{ElementId, LayerId};
use flui_types::Offset;

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
    offset: Option<Offset>,

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
    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Returns reference to the Layer.
    #[inline]
    pub fn get_layer(&self) -> &Layer {
        &self.layer
    }

    /// Returns mutable reference to the Layer.
    #[inline]
    pub fn get_layer_mut(&mut self) -> &mut Layer {
        &mut self.layer
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
    pub fn offset(&self) -> Option<Offset> {
        self.offset
    }

    /// Sets the offset from parent.
    #[inline]
    pub fn set_offset(&mut self, offset: Option<Offset>) {
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
/// This is the fourth of FLUI's five trees, corresponding to Flutter's Layer tree
/// used for composition and GPU rendering.
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
/// use flui_layer::{LayerTree, Layer, CanvasLayer, LayerNode};
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
    /// use flui_layer::{LayerTree, Layer, CanvasLayer};
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
        self.get(id).map(|node| node.layer())
    }

    /// Returns a mutable reference to the Layer directly.
    #[inline]
    pub fn get_layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.get_mut(id).map(|node| node.layer_mut())
    }

    /// Removes a LayerNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
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
        self.get(id).map(|node| node.children())
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
    /// This is the core operation used by Flutter's PaintingContext when composing
    /// layers during painting. It's typically called in two scenarios:
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::CanvasLayer;

    #[test]
    fn test_layer_tree_new() {
        let tree = LayerTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_layer_tree_with_capacity() {
        let tree = LayerTree::with_capacity(100);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_layer_tree_insert() {
        let mut tree = LayerTree::new();
        let layer = Layer::Canvas(CanvasLayer::new());
        let id = tree.insert(layer);

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
        assert_eq!(id.get(), 1); // First ID should be 1
    }

    #[test]
    fn test_layer_tree_get() {
        let mut tree = LayerTree::new();
        let layer = Layer::Canvas(CanvasLayer::new());
        let id = tree.insert(layer);

        let node = tree.get(id);
        assert!(node.is_some());
        assert!(node.unwrap().layer().is_canvas());
    }

    #[test]
    fn test_layer_tree_get_layer() {
        let mut tree = LayerTree::new();
        let layer = Layer::Canvas(CanvasLayer::new());
        let id = tree.insert(layer);

        let layer = tree.get_layer(id);
        assert!(layer.is_some());
        assert!(layer.unwrap().is_canvas());
    }

    #[test]
    fn test_layer_tree_remove() {
        let mut tree = LayerTree::new();
        let layer = Layer::Canvas(CanvasLayer::new());
        let id = tree.insert(layer);

        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
        assert!(tree.is_empty());
    }

    #[test]
    fn test_layer_tree_parent_child() {
        let mut tree = LayerTree::new();

        let parent_layer = Layer::Canvas(CanvasLayer::new());
        let child_layer = Layer::Canvas(CanvasLayer::new());

        let parent_id = tree.insert(parent_layer);
        let child_id = tree.insert(child_layer);

        tree.add_child(parent_id, child_id);

        // Check parent has child
        let children = tree.children(parent_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child_id);

        // Check child has parent
        let parent = tree.parent(child_id);
        assert_eq!(parent, Some(parent_id));
    }

    #[test]
    fn test_layer_tree_remove_child() {
        let mut tree = LayerTree::new();

        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 1);

        tree.remove_child(parent_id, child_id);
        assert_eq!(tree.children(parent_id).unwrap().len(), 0);
        assert!(tree.parent(child_id).is_none());
    }

    #[test]
    fn test_layer_tree_set_root() {
        let mut tree = LayerTree::new();
        let id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        assert!(tree.root().is_none());
        tree.set_root(Some(id));
        assert_eq!(tree.root(), Some(id));
    }

    #[test]
    fn test_layer_tree_clear() {
        let mut tree = LayerTree::new();
        let id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        tree.set_root(Some(id));

        tree.clear();
        assert!(tree.is_empty());
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_layer_tree_iter() {
        let mut tree = LayerTree::new();
        let id1 = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let id2 = tree.insert(Layer::Canvas(CanvasLayer::new()));

        let ids: Vec<_> = tree.layer_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_layer_node_with_element_id() {
        let element_id = ElementId::new(42);
        let node = LayerNode::new(Layer::Canvas(CanvasLayer::new())).with_element_id(element_id);

        assert_eq!(node.element_id(), Some(element_id));
    }

    #[test]
    fn test_layer_node_with_offset() {
        let offset = Offset::new(10.0, 20.0);
        let node = LayerNode::new(Layer::Canvas(CanvasLayer::new())).with_offset(offset);

        assert_eq!(node.offset(), Some(offset));
    }

    #[test]
    fn test_layer_node_needs_compositing() {
        let mut node = LayerNode::new(Layer::Canvas(CanvasLayer::new()));

        assert!(node.needs_compositing()); // Default is true

        node.set_needs_compositing(false);
        assert!(!node.needs_compositing());
    }

    // ========== Layer Composition Tests ==========

    #[test]
    fn test_clear_children() {
        let mut tree = LayerTree::new();

        // Create parent with multiple children
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child3_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);
        tree.add_child(parent_id, child3_id);

        // Verify children were added
        assert_eq!(tree.children(parent_id).unwrap().len(), 3);

        // Clear all children
        tree.clear_children(parent_id);

        // Verify children were cleared
        assert_eq!(tree.children(parent_id).unwrap().len(), 0);

        // Verify children still exist in tree (not removed, just unlinked)
        assert!(tree.contains(child1_id));
        assert!(tree.contains(child2_id));
        assert!(tree.contains(child3_id));

        // Verify children no longer have parent reference
        assert!(tree.parent(child1_id).is_none());
        assert!(tree.parent(child2_id).is_none());
        assert!(tree.parent(child3_id).is_none());
    }

    #[test]
    fn test_append_layer() {
        let mut tree = LayerTree::new();

        // Create container layer
        let container_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Create picture layer
        let picture_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Append to container (Flutter PaintingContext pattern)
        tree.append_layer(container_id, picture_id);

        // Verify layer was appended
        let children = tree.children(container_id).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], picture_id);

        // Verify parent-child relationship
        assert_eq!(tree.parent(picture_id), Some(container_id));
    }

    #[test]
    fn test_append_layer_multiple_times() {
        let mut tree = LayerTree::new();

        let container_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer3_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Append layers one by one
        tree.append_layer(container_id, layer1_id);
        tree.append_layer(container_id, layer2_id);
        tree.append_layer(container_id, layer3_id);

        // Verify all layers were appended in order
        let children = tree.children(container_id).unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0], layer1_id);
        assert_eq!(children[1], layer2_id);
        assert_eq!(children[2], layer3_id);
    }

    #[test]
    fn test_append_layers_bulk() {
        let mut tree = LayerTree::new();

        let container_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let layer3_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Append multiple layers at once
        tree.append_layers(container_id, &[layer1_id, layer2_id, layer3_id]);

        // Verify all layers were appended in order
        let children = tree.children(container_id).unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0], layer1_id);
        assert_eq!(children[1], layer2_id);
        assert_eq!(children[2], layer3_id);
    }

    #[test]
    fn test_append_layers_empty() {
        let mut tree = LayerTree::new();

        let container_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Append empty slice - should be no-op
        tree.append_layers(container_id, &[]);

        // Verify no children were added
        let children = tree.children(container_id).unwrap();
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_layer_composition_integration() {
        // Simulate PaintingContext workflow:
        // 1. Create container layer (e.g., OffsetLayer)
        // 2. Record and append picture layers
        // 3. Clear and rebuild

        let mut tree = LayerTree::new();

        // Step 1: Create container
        let container_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Step 2: Append some picture layers
        let picture1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let picture2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        tree.append_layers(container_id, &[picture1_id, picture2_id]);

        assert_eq!(tree.children(container_id).unwrap().len(), 2);

        // Step 3: Clear and rebuild (simulating repaint)
        tree.clear_children(container_id);
        assert_eq!(tree.children(container_id).unwrap().len(), 0);

        // Append new layers
        let new_picture1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let new_picture2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let new_picture3_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        tree.append_layers(container_id, &[new_picture1_id, new_picture2_id, new_picture3_id]);

        // Verify new structure
        let children = tree.children(container_id).unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0], new_picture1_id);
        assert_eq!(children[1], new_picture2_id);
        assert_eq!(children[2], new_picture3_id);

        // Old picture layers should still exist (just unlinked)
        assert!(tree.contains(picture1_id));
        assert!(tree.contains(picture2_id));
    }
}
