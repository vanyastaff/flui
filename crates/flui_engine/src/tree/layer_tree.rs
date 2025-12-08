//! LayerTree - Separate tree for Layer storage and compositing
//!
//! This module implements the fourth of FLUI's four trees (View, Element, RenderObject, Layer).
//! Following Flutter's architecture, Layers are stored in a separate tree for compositor operations.
//!
//! # Architecture
//!
//! ```text
//! LayerTree (this file)
//!   ├─ nodes: Slab<LayerNodeStorage>
//!   └─ root: Option<LayerId>
//!
//! LayerNodeStorage (type-erased wrapper)
//!   └─ Box<dyn LayerNode>
//!
//! LayerNode trait (type-erased interface)
//!   └─ implemented by ConcreteLayerNode<Layer>
//!
//! ConcreteLayerNode<Layer> (concrete implementation)
//!   ├─ layer: Layer  (Canvas, ShaderMask, BackdropFilter, Cached)
//!   ├─ needs_compositing: bool
//!   ├─ parent_data: Option<Offset>  (offset from parent)
//!   └─ tree structure (parent, children)
//! ```
//!
//! # Flutter Analogy
//!
//! This corresponds to Flutter's Layer tree used for composition. Unlike Flutter's
//! immutable layers, FLUI layers can be mutable for efficiency while maintaining
//! the same architectural separation.
//!
//! # Layer Types
//!
//! Currently supports:
//! - **CanvasLayer**: Standard canvas drawing commands
//! - **ShaderMaskLayer**: GPU shader masking effects
//! - **BackdropFilterLayer**: Backdrop filtering (frosted glass, blur)
//! - **CachedLayer**: Cached layer for RepaintBoundary optimization
//!
//! Future: Support for container layers (OffsetLayer, TransformLayer, ClipLayer).

use std::any::Any;
use std::fmt;

use slab::Slab;

use flui_foundation::ElementId;
use flui_types::Offset;

use crate::layer::Layer;

// ============================================================================
// LAYER ID
// ============================================================================

/// Unique identifier for LayerNode in LayerTree.
///
/// Uses 1-based indexing (NonZeroUsize) for:
/// - Niche optimization: Option<LayerId> = 8 bytes
/// - 0 reserved for "null" semantics
///
/// # Slab Offset Pattern
///
/// LayerId uses 1-based indexing while Slab uses 0-based:
/// - `LayerId(1)` → `nodes[0]`
/// - `LayerId(2)` → `nodes[1]`
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct LayerId(std::num::NonZeroUsize);

impl LayerId {
    /// Creates a new LayerId from a 1-based index.
    ///
    /// # Panics
    ///
    /// Panics if id is 0 (use NonZeroUsize for safety).
    #[inline]
    pub fn new(id: usize) -> Self {
        Self(std::num::NonZeroUsize::new(id).expect("LayerId cannot be 0"))
    }

    /// Gets the underlying 1-based index.
    #[inline]
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

// ============================================================================
// LAYER NODE (Type-erased interface)
// ============================================================================

/// Type-erased interface for LayerNode operations.
///
/// This trait enables storing different layer types in the same Slab
/// while preserving access to common operations.
///
/// # Design
///
/// Similar to ViewNode and RenderNode, LayerNode provides a type-erased
/// interface for compositor layers.
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
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// Returns mutable reference to the Layer.
    #[inline]
    pub fn layer_mut(&mut self) -> &mut Layer {
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
// TYPE-ERASED WRAPPER (Internal storage)
// ============================================================================

/// Type-erased wrapper for LayerNode storage.
///
/// This is what actually gets stored in the Slab - internal implementation detail.
struct LayerNodeStorage {
    inner: Box<dyn LayerNode>,
}

impl LayerNodeStorage {
    fn new(node: ConcreteLayerNode) -> Self {
        Self {
            inner: Box::new(node),
        }
    }
}

impl fmt::Debug for LayerNodeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

// ============================================================================
// LAYER TREE
// ============================================================================

/// LayerTree - Slab-based storage for compositor layers.
///
/// This is the fourth of FLUI's four trees, corresponding to Flutter's Layer tree
/// used for composition and GPU rendering.
///
/// # Architecture
///
/// ```text
/// LayerTree
///   ├─ nodes: Slab<LayerNodeStorage>  (type-erased storage)
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
/// ```rust,ignore
/// use flui_engine::tree::LayerTree;
/// use flui_engine::layer::{Layer, CanvasLayer};
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
    nodes: Slab<LayerNodeStorage>,

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
    /// ```rust,ignore
    /// let layer = Layer::Canvas(CanvasLayer::new());
    /// let id = tree.insert(layer);
    /// ```
    pub fn insert(&mut self, layer: Layer) -> LayerId {
        let node = ConcreteLayerNode::new(layer);
        let storage = LayerNodeStorage::new(node);
        let slab_index = self.nodes.insert(storage);
        LayerId::new(slab_index + 1) // +1 offset
    }

    /// Returns a reference to a LayerNode (type-erased).
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `LayerId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: LayerId) -> Option<&(dyn LayerNode + '_)> {
        self.nodes
            .get(id.get() - 1)
            .map(|storage| &*storage.inner as &(dyn LayerNode + '_))
    }

    /// Returns a mutable reference to a LayerNode (type-erased).
    #[inline]
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut (dyn LayerNode + '_)> {
        self.nodes
            .get_mut(id.get() - 1)
            .map(|storage| &mut *storage.inner as &mut (dyn LayerNode + '_))
    }

    /// Returns a reference to the concrete LayerNode.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(node) = tree.get_concrete(id) {
    ///     println!("Layer: {:?}", node.layer());
    /// }
    /// ```
    pub fn get_concrete(&self, id: LayerId) -> Option<&ConcreteLayerNode> {
        self.get(id)?.as_any().downcast_ref::<ConcreteLayerNode>()
    }

    /// Returns a mutable reference to the concrete LayerNode.
    pub fn get_concrete_mut(&mut self, id: LayerId) -> Option<&mut ConcreteLayerNode> {
        self.get_mut(id)?
            .as_any_mut()
            .downcast_mut::<ConcreteLayerNode>()
    }

    /// Removes a LayerNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: LayerId) -> Option<Box<dyn LayerNode>> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes
            .try_remove(id.get() - 1)
            .map(|storage| storage.inner)
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
}

impl Default for LayerTree {
    fn default() -> Self {
        Self::new()
    }
}
