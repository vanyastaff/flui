//! LayerTree - Slab-based storage for compositor layers
//!
//! This module provides the LayerTree struct and LayerNode trait
//! for managing the compositor layer hierarchy.

use std::any::Any;
use std::fmt;

use slab::Slab;

use flui_foundation::{ElementId, LayerId};
use flui_types::Offset;

use crate::layer::Layer;

// ============================================================================
// LAYER NODE TRAIT
// ============================================================================

/// Type-erased interface for LayerNode operations.
///
/// This trait enables storing different layer types in the same Slab
/// while preserving access to common operations.
pub trait LayerNode: Send + Sync + fmt::Debug {
    // ========== Tree Structure ==========

    /// Gets the parent LayerId.
    fn parent(&self) -> Option<LayerId>;

    /// Sets the parent LayerId.
    fn set_parent(&mut self, parent: Option<LayerId>);

    /// Gets all children LayerIds.
    fn children(&self) -> &[LayerId];

    /// Adds a child to this layer node.
    fn add_child(&mut self, child: LayerId);

    /// Removes a child from this layer node.
    fn remove_child(&mut self, child: LayerId);

    /// Clears all children from this layer node.
    fn clear_children(&mut self);

    // ========== Layer Access ==========

    /// Returns reference to the concrete Layer.
    fn layer(&self) -> &Layer;

    /// Returns mutable reference to the concrete Layer.
    fn layer_mut(&mut self) -> &mut Layer;

    // ========== Metadata ==========

    /// Gets whether this layer needs compositing.
    fn needs_compositing(&self) -> bool;

    /// Sets whether this layer needs compositing.
    fn set_needs_compositing(&mut self, needs: bool);

    /// Gets the offset from parent (parent data).
    fn offset(&self) -> Option<Offset>;

    /// Sets the offset from parent.
    fn set_offset(&mut self, offset: Option<Offset>);

    /// Gets the associated ElementId (for cross-tree references).
    fn element_id(&self) -> Option<ElementId>;

    /// Sets the associated ElementId.
    fn set_element_id(&mut self, element_id: Option<ElementId>);

    // ========== Downcasting ==========

    /// Downcast to Any for concrete type access.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to Any (mutable) for concrete type access.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// CONCRETE LAYER NODE
// ============================================================================

/// Concrete LayerNode - stores Layer directly.
///
/// # Design
///
/// Unlike ViewTree and RenderTree which are generic over the object type,
/// LayerNode is concrete because Layer is already an enum that encompasses
/// all layer types. This simplifies the API while maintaining the same
/// architectural pattern.
#[derive(Debug)]
pub struct ConcreteLayerNode {
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

impl ConcreteLayerNode {
    /// Creates a new ConcreteLayerNode with the given Layer.
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

    /// Creates a ConcreteLayerNode with an associated ElementId.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Creates a ConcreteLayerNode with an offset.
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
}

// ============================================================================
// LAYER NODE IMPL
// ============================================================================

impl LayerNode for ConcreteLayerNode {
    fn parent(&self) -> Option<LayerId> {
        self.parent
    }

    fn set_parent(&mut self, parent: Option<LayerId>) {
        self.parent = parent;
    }

    fn children(&self) -> &[LayerId] {
        &self.children
    }

    fn add_child(&mut self, child: LayerId) {
        self.children.push(child);
    }

    fn remove_child(&mut self, child: LayerId) {
        self.children.retain(|&id| id != child);
    }

    fn clear_children(&mut self) {
        self.children.clear();
    }

    fn layer(&self) -> &Layer {
        &self.layer
    }

    fn layer_mut(&mut self) -> &mut Layer {
        &mut self.layer
    }

    fn needs_compositing(&self) -> bool {
        self.needs_compositing
    }

    fn set_needs_compositing(&mut self, needs: bool) {
        self.needs_compositing = needs;
    }

    fn offset(&self) -> Option<Offset> {
        self.offset
    }

    fn set_offset(&mut self, offset: Option<Offset>) {
        self.offset = offset;
    }

    fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
///   ├─ nodes: Slab<ConcreteLayerNode>  (direct storage)
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
    nodes: Slab<ConcreteLayerNode>,

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
        let node = ConcreteLayerNode::new(layer);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1) // +1 offset
    }

    /// Inserts a Layer with an associated ElementId.
    pub fn insert_with_element(&mut self, layer: Layer, element_id: ElementId) -> LayerId {
        let node = ConcreteLayerNode::new(layer).with_element_id(element_id);
        let slab_index = self.nodes.insert(node);
        LayerId::new(slab_index + 1)
    }

    /// Returns a reference to a LayerNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `LayerId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: LayerId) -> Option<&ConcreteLayerNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a LayerNode.
    #[inline]
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut ConcreteLayerNode> {
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
    pub fn remove(&mut self, id: LayerId) -> Option<ConcreteLayerNode> {
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

    // ========== Iteration ==========

    /// Returns an iterator over all LayerIds in the tree.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.nodes.iter().map(|(index, _)| LayerId::new(index + 1))
    }

    /// Returns an iterator over all (LayerId, &ConcreteLayerNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (LayerId, &ConcreteLayerNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (LayerId::new(index + 1), node))
    }

    /// Returns a mutable iterator over all (LayerId, &mut ConcreteLayerNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (LayerId, &mut ConcreteLayerNode)> + '_ {
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
    fn test_concrete_layer_node_with_element_id() {
        let element_id = ElementId::new(42);
        let node =
            ConcreteLayerNode::new(Layer::Canvas(CanvasLayer::new())).with_element_id(element_id);

        assert_eq!(node.element_id(), Some(element_id));
    }

    #[test]
    fn test_concrete_layer_node_with_offset() {
        let offset = Offset::new(10.0, 20.0);
        let node = ConcreteLayerNode::new(Layer::Canvas(CanvasLayer::new())).with_offset(offset);

        assert_eq!(node.offset(), Some(offset));
    }

    #[test]
    fn test_layer_node_needs_compositing() {
        let mut node = ConcreteLayerNode::new(Layer::Canvas(CanvasLayer::new()));

        assert!(node.needs_compositing()); // Default is true

        node.set_needs_compositing(false);
        assert!(!node.needs_compositing());
    }
}
