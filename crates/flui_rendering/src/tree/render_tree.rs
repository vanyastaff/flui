//! RenderTree - Separate tree for RenderObject storage
//!
//! This module implements the third of FLUI's four trees (View, Element, RenderObject, Layer).
//! Following Flutter's architecture, RenderObjects are stored in a separate tree from Elements.
//!
//! # Architecture
//!
//! ```text
//! RenderTree (this file)
//!   ├─ nodes: Slab<RenderNodeStorage>
//!   └─ root: Option<RenderId>
//!
//! RenderNodeStorage (type-erased wrapper)
//!   └─ Box<dyn RenderNode>
//!
//! RenderNode trait (type-erased interface)
//!   └─ implemented by ConcreteRenderNode<R>
//!
//! ConcreteRenderNode<R: RenderObject> (generic, zero-cost)
//!   ├─ object: R  (inline, no Box!)
//!   ├─ lifecycle: RenderLifecycle (runtime enum)
//!   ├─ cached_size: Option<Size>
//!   └─ tree structure (parent, children)
//! ```
//!
//! # Flutter Analogy
//!
//! This corresponds to Flutter's RenderObject tree. Like Flutter, FLUI separates
//! the render tree from the element tree for architectural clarity and performance.
//!
//! # Generic Design
//!
//! RenderNode is generic over the concrete RenderObject type, providing zero-cost
//! abstraction. Type erasure happens only at the storage boundary (Slab).
//!
//! ```rust,ignore
//! // Generic node - static dispatch inside!
//! let node = ConcreteRenderNode {
//!     object: RenderPadding { padding: EdgeInsets::all(8.0) },  // Concrete type
//!     lifecycle: RenderLifecycle::Initial,
//! };
//!
//! // Type erasure only when inserting
//! let id = tree.insert(node);  // Box<dyn RenderNode>
//! ```

use std::any::Any;
use std::fmt;

use slab::Slab;

use flui_foundation::ElementId;
use flui_types::Size;

use crate::core::RenderLifecycle;
use crate::core::RenderObject;

// ============================================================================
// RENDER ID
// ============================================================================

/// Unique identifier for RenderNode in RenderTree.
///
/// Uses 1-based indexing (NonZeroUsize) for:
/// - Niche optimization: Option<RenderId> = 8 bytes
/// - 0 reserved for "null" semantics
///
/// # Slab Offset Pattern
///
/// RenderId uses 1-based indexing while Slab uses 0-based:
/// - `RenderId(1)` → `nodes[0]`
/// - `RenderId(2)` → `nodes[1]`
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RenderId(std::num::NonZeroUsize);

impl RenderId {
    /// Creates a new RenderId from a 1-based index.
    ///
    /// # Panics
    ///
    /// Panics if id is 0 (use NonZeroUsize for safety).
    #[inline]
    pub fn new(id: usize) -> Self {
        Self(std::num::NonZeroUsize::new(id).expect("RenderId cannot be 0"))
    }

    /// Gets the underlying 1-based index.
    #[inline]
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

// ============================================================================
// RENDER NODE (Type-erased interface)
// ============================================================================

/// Type-erased interface for RenderNode operations.
///
/// This trait enables storing different ConcreteRenderNode<R> types in the same Slab
/// while preserving access to common operations.
///
/// # Design
///
/// Similar to how ViewNode trait works for ViewObjects, RenderNode
/// provides a type-erased interface for render nodes of different concrete types.
///
/// Like Flutter's RenderObject tree, this is the base abstraction for all render nodes.
pub trait RenderNode: Send + Sync + fmt::Debug {
    // ========== Tree Structure ==========

    /// Gets the parent RenderId.
    fn parent(&self) -> Option<RenderId>;

    /// Sets the parent RenderId.
    fn set_parent(&mut self, parent: Option<RenderId>);

    /// Gets all children RenderIds.
    fn children(&self) -> &[RenderId];

    /// Adds a child to this render node.
    fn add_child(&mut self, child: RenderId);

    /// Removes a child from this render node.
    fn remove_child(&mut self, child: RenderId);

    // ========== RenderObject Access ==========

    /// Returns reference to RenderObject as trait object.
    ///
    /// Uses the existing RenderObject trait for type erasure.
    fn render_object(&self) -> &dyn RenderObject;

    /// Returns mutable reference to RenderObject as trait object.
    fn render_object_mut(&mut self) -> &mut dyn RenderObject;

    // ========== Metadata ==========

    /// Gets the current lifecycle state.
    fn lifecycle(&self) -> RenderLifecycle;

    /// Sets the lifecycle state.
    fn set_lifecycle(&mut self, lifecycle: RenderLifecycle);

    /// Gets the cached size from last layout, if any.
    fn cached_size(&self) -> Option<Size>;

    /// Sets the cached size.
    fn set_cached_size(&mut self, size: Option<Size>);

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
// CONCRETE RENDER NODE (Generic implementation)
// ============================================================================

/// Concrete RenderNode - stores RenderObject inline for zero-cost abstraction.
///
/// # Type Parameters
///
/// * `R` - Concrete RenderObject type (e.g., RenderPadding, RenderFlex)
///
/// # Design
///
/// ConcreteRenderNode is generic over the RenderObject type, enabling static dispatch
/// for all RenderObject operations until type erasure at storage boundary.
///
/// ```rust,ignore
/// // Concrete type - no vtable!
/// let node = ConcreteRenderNode {
///     object: RenderPadding { padding: EdgeInsets::all(8.0) },  // Inline storage
///     lifecycle: RenderLifecycle::Initial,
/// };
///
/// // Static dispatch
/// node.object.perform_layout(ctx);  // Direct call, can inline!
/// ```
#[derive(Debug)]
pub struct ConcreteRenderNode<R: RenderObject> {
    // ========== Tree Structure ==========
    parent: Option<RenderId>,
    children: Vec<RenderId>,

    // ========== RenderObject (Generic, inline!) ==========
    /// The RenderObject - stored inline, no Box!
    ///
    /// This is the key difference from type-erased storage.
    /// By storing the concrete type inline, we get:
    /// - Zero-cost abstraction (static dispatch)
    /// - No heap allocation for the RenderObject itself
    /// - Better cache locality
    object: R,

    // ========== Metadata ==========
    /// Current lifecycle state
    lifecycle: RenderLifecycle,

    /// Cached size from last layout (optimization)
    cached_size: Option<Size>,

    /// Associated ElementId (for cross-tree references)
    element_id: Option<ElementId>,
}

impl<R: RenderObject> ConcreteRenderNode<R> {
    /// Creates a new ConcreteRenderNode with the given RenderObject.
    pub fn new(object: R) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            object,
            lifecycle: RenderLifecycle::Detached,
            cached_size: None,
            element_id: None,
        }
    }

    /// Creates a ConcreteRenderNode with an associated ElementId.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Returns reference to the concrete RenderObject.
    ///
    /// This provides zero-cost access to the concrete type without downcasting.
    #[inline]
    pub fn object(&self) -> &R {
        &self.object
    }

    /// Returns mutable reference to the concrete RenderObject.
    #[inline]
    pub fn object_mut(&mut self) -> &mut R {
        &mut self.object
    }
}

// ============================================================================
// RENDER NODE IMPL (Generic → Type-erased)
// ============================================================================

impl<R: RenderObject + fmt::Debug + 'static> RenderNode for ConcreteRenderNode<R> {
    fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    fn children(&self) -> &[RenderId] {
        &self.children
    }

    fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    fn remove_child(&mut self, child: RenderId) {
        self.children.retain(|&id| id != child);
    }

    fn render_object(&self) -> &dyn RenderObject {
        &self.object  // ✅ Uses existing RenderObject trait
    }

    fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut self.object
    }

    fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    fn set_lifecycle(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = lifecycle;
    }

    fn cached_size(&self) -> Option<Size> {
        self.cached_size
    }

    fn set_cached_size(&mut self, size: Option<Size>) {
        self.cached_size = size;
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

/// Type-erased wrapper for RenderNode storage.
///
/// This is what actually gets stored in the Slab - internal implementation detail.
struct RenderNodeStorage {
    inner: Box<dyn RenderNode>,
}

impl RenderNodeStorage {
    fn new<R: RenderObject + fmt::Debug + 'static>(node: ConcreteRenderNode<R>) -> Self {
        Self {
            inner: Box::new(node),
        }
    }
}

impl fmt::Debug for RenderNodeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

// ============================================================================
// RENDER TREE
// ============================================================================

/// RenderTree - Slab-based storage for render nodes.
///
/// This is the third of FLUI's four trees, corresponding to Flutter's RenderObject tree.
///
/// # Architecture
///
/// ```text
/// RenderTree
///   ├─ nodes: Slab<RenderNodeStorage>  (type-erased storage)
///   └─ root: Option<RenderId>
/// ```
///
/// # Thread Safety
///
/// RenderTree itself is not thread-safe. Use `Arc<RwLock<RenderTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::tree::RenderTree;
///
/// let mut tree = RenderTree::new();
///
/// // Insert render object
/// let render = RenderPadding::new(EdgeInsets::all(8.0));
/// let id = tree.insert(render);
///
/// // Access via type-erased interface
/// let node = tree.get(id).unwrap();
/// assert_eq!(node.lifecycle(), RenderLifecycle::Initial);
///
/// // Perform layout
/// node.render_object_mut().perform_layout(element_id, constraints, &mut layout_child);
/// ```
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for RenderNodes (0-based indexing internally)
    nodes: Slab<RenderNodeStorage>,

    /// Root RenderNode ID (None if tree is empty)
    root: Option<RenderId>,
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root RenderNode ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Set the root RenderNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a RenderNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of RenderNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a RenderObject into the tree.
    ///
    /// Returns the RenderId of the inserted node.
    ///
    /// # Generic
    ///
    /// This method is generic over the RenderObject type, enabling zero-cost
    /// insertion without requiring the caller to box the RenderObject.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `RenderId(1)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let render = RenderPadding::new(EdgeInsets::all(8.0));
    /// let id = tree.insert(render);
    /// ```
    pub fn insert<R: RenderObject + fmt::Debug + 'static>(
        &mut self,
        object: R,
    ) -> RenderId {
        let node = ConcreteRenderNode::new(object);
        let storage = RenderNodeStorage::new(node);
        let slab_index = self.nodes.insert(storage);
        RenderId::new(slab_index + 1)  // +1 offset
    }

    /// Returns a reference to a RenderNode (type-erased).
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `RenderId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&(dyn RenderNode + '_)> {
        self.nodes
            .get(id.get() - 1)
            .map(|storage| &*storage.inner as &(dyn RenderNode + '_))
    }

    /// Returns a mutable reference to a RenderNode (type-erased).
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut (dyn RenderNode + '_)> {
        self.nodes
            .get_mut(id.get() - 1)
            .map(|storage| &mut *storage.inner as &mut (dyn RenderNode + '_))
    }

    /// Returns a reference to the concrete RenderNode type.
    ///
    /// This requires knowing the concrete type R at the call site.
    /// Returns None if the ID is invalid or the type doesn't match.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(node) = tree.get_concrete::<RenderPadding>(id) {
    ///     println!("Padding: {:?}", node.object().padding);
    /// }
    /// ```
    pub fn get_concrete<R: RenderObject + fmt::Debug + 'static>(
        &self,
        id: RenderId,
    ) -> Option<&ConcreteRenderNode<R>> {
        self.get(id)?
            .as_any()
            .downcast_ref::<ConcreteRenderNode<R>>()
    }

    /// Returns a mutable reference to the concrete RenderNode type.
    pub fn get_concrete_mut<R: RenderObject + fmt::Debug + 'static>(
        &mut self,
        id: RenderId,
    ) -> Option<&mut ConcreteRenderNode<R>> {
        self.get_mut(id)?
            .as_any_mut()
            .downcast_mut::<ConcreteRenderNode<R>>()
    }

    /// Removes a RenderNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: RenderId) -> Option<Box<dyn RenderNode>> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes
            .try_remove(id.get() - 1)
            .map(|storage| storage.inner)
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent RenderNode.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent RenderNode.
    pub fn remove_child(&mut self, parent_id: RenderId, child_id: RenderId) {
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

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}
