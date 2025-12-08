//! ViewObjectTree - Separate tree for ViewObject storage
//!
//! This module implements the first of FLUI's four trees (View, Element, RenderObject, Layer).
//! Following Flutter's architecture, ViewObjects are stored in a separate tree from Elements.
//!
//! # Architecture
//!
//! ```text
//! ViewObjectTree (this file)
//!   ├─ nodes: Slab<ViewNodeBox>
//!   └─ root: Option<ViewObjectId>
//!
//! ViewNodeBox (type-erased wrapper)
//!   └─ Box<dyn ViewNodeTrait>
//!
//! ViewNode<V: ViewObject> (generic, zero-cost)
//!   ├─ object: V  (inline, no Box!)
//!   ├─ mode: ViewMode (runtime enum)
//!   └─ tree structure (parent, children)
//! ```
//!
//! # Flutter Analogy
//!
//! This corresponds to Flutter's Widget tree, but with mutable ViewObjects.
//! Unlike Flutter Widgets (which are immutable), FLUI ViewObjects can be mutable
//! for efficiency, but are still stored separately from Elements for architectural clarity.
//!
//! # Generic Design
//!
//! ViewNode is generic over the concrete ViewObject type, providing zero-cost
//! abstraction. Type erasure happens only at the storage boundary (Slab).
//!
//! ```rust,ignore
//! // Generic node - static dispatch inside!
//! let node = ViewNode {
//!     object: MyStatelessView { text: "Hello" },  // Concrete type
//!     mode: ViewMode::Stateless,
//! };
//!
//! // Type erasure only when inserting
//! let id = tree.insert(node);  // Box<dyn ViewNodeTrait>
//! ```

use std::any::Any;
use std::fmt;

use slab::Slab;

use flui_foundation::Key;

use crate::view_mode::ViewMode;
use crate::view_object::ViewObject;
use crate::ViewLifecycle;

// ============================================================================
// VIEW OBJECT ID
// ============================================================================

/// Unique identifier for ViewNode in ViewTree.
///
/// Uses 1-based indexing (NonZeroUsize) for:
/// - Niche optimization: Option<ViewId> = 8 bytes
/// - 0 reserved for "null" semantics
///
/// # Slab Offset Pattern
///
/// ViewId uses 1-based indexing while Slab uses 0-based:
/// - `ViewId(1)` → `nodes[0]`
/// - `ViewId(2)` → `nodes[1]`
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ViewId(std::num::NonZeroUsize);

impl ViewId {
    /// Creates a new ViewId from a 1-based index.
    ///
    /// # Panics
    ///
    /// Panics if id is 0 (use NonZeroUsize for safety).
    #[inline]
    pub fn new(id: usize) -> Self {
        Self(std::num::NonZeroUsize::new(id).expect("ViewId cannot be 0"))
    }

    /// Gets the underlying 1-based index.
    #[inline]
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

// ============================================================================
// VIEW NODE (Type-erased interface)
// ============================================================================

/// Type-erased interface for ViewNode operations.
///
/// This trait enables storing different ConcreteViewNode<V> types in the same Slab
/// while preserving access to common operations.
///
/// # Design
///
/// Similar to how RenderObject trait works for RenderObjects, ViewNode
/// provides a type-erased interface for view nodes of different concrete types.
///
/// Like Flutter's Widget, this is the base abstraction for all view nodes.
pub trait ViewNode: Send + Sync + fmt::Debug {
    // ========== Tree Structure ==========

    fn parent(&self) -> Option<ViewId>;
    fn set_parent(&mut self, parent: Option<ViewId>);

    fn children(&self) -> &[ViewId];
    fn add_child(&mut self, child: ViewId);
    fn remove_child(&mut self, child: ViewId);

    // ========== ViewObject Access ==========

    /// Returns reference to ViewObject as trait object.
    ///
    /// Uses the existing ViewObject trait for type erasure.
    fn view_object(&self) -> &dyn ViewObject;

    /// Returns mutable reference to ViewObject as trait object.
    fn view_object_mut(&mut self) -> &mut dyn ViewObject;

    // ========== Metadata ==========

    fn mode(&self) -> ViewMode;
    fn lifecycle(&self) -> ViewLifecycle;
    fn set_lifecycle(&mut self, lifecycle: ViewLifecycle);

    fn key(&self) -> Option<&Key>;

    // ========== Downcasting ==========

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// CONCRETE VIEW NODE (Generic implementation)
// ============================================================================

/// Concrete ViewNode - stores ViewObject inline for zero-cost abstraction.
///
/// # Type Parameters
///
/// * `V` - Concrete ViewObject type (e.g., StatelessView, StatefulView)
///
/// # Design
///
/// ConcreteViewNode is generic over the ViewObject type, enabling static dispatch
/// for all ViewObject operations until type erasure at storage boundary.
///
/// ```rust,ignore
/// // Concrete type - no vtable!
/// let node = ConcreteViewNode {
///     object: MyView { text: "Hello" },  // Inline storage
///     mode: ViewMode::Stateless,
/// };
///
/// // Static dispatch
/// node.object.build(ctx);  // Direct call, can inline!
/// ```
#[derive(Debug)]
pub struct ConcreteViewNode<V: ViewObject> {
    // ========== Tree Structure ==========
    parent: Option<ViewId>,
    children: Vec<ViewId>,

    // ========== ViewObject (Generic, inline!) ==========
    /// The ViewObject - stored inline, no Box!
    ///
    /// This is the key difference from current ViewElement which uses Box<dyn ViewObject>.
    /// By storing the concrete type inline, we get:
    /// - Zero-cost abstraction (static dispatch)
    /// - No heap allocation for the ViewObject itself
    /// - Better cache locality
    object: V,

    // ========== Metadata ==========
    /// View mode (Stateless, Stateful, Provider, etc.)
    ///
    /// This is a runtime value, not a generic parameter.
    mode: ViewMode,

    /// Current lifecycle state
    lifecycle: ViewLifecycle,

    /// Optional key for reconciliation
    key: Option<Key>,
}

impl<V: ViewObject> ConcreteViewNode<V> {
    /// Creates a new ConcreteViewNode with the given ViewObject and mode.
    pub fn new(object: V, mode: ViewMode) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            object,
            mode,
            lifecycle: ViewLifecycle::Initial,
            key: None,
        }
    }

    /// Creates a ConcreteViewNode with a key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Returns reference to the concrete ViewObject.
    ///
    /// This provides zero-cost access to the concrete type without downcasting.
    #[inline]
    pub fn object(&self) -> &V {
        &self.object
    }

    /// Returns mutable reference to the concrete ViewObject.
    #[inline]
    pub fn object_mut(&mut self) -> &mut V {
        &mut self.object
    }
}

// ============================================================================
// VIEW NODE IMPL (Generic → Type-erased)
// ============================================================================

impl<V: ViewObject + fmt::Debug + 'static> ViewNode for ConcreteViewNode<V> {
    fn parent(&self) -> Option<ViewId> {
        self.parent
    }

    fn set_parent(&mut self, parent: Option<ViewId>) {
        self.parent = parent;
    }

    fn children(&self) -> &[ViewId] {
        &self.children
    }

    fn add_child(&mut self, child: ViewId) {
        self.children.push(child);
    }

    fn remove_child(&mut self, child: ViewId) {
        self.children.retain(|&id| id != child);
    }

    fn view_object(&self) -> &dyn ViewObject {
        &self.object // ✅ Uses existing ViewObject trait
    }

    fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut self.object
    }

    fn mode(&self) -> ViewMode {
        self.mode
    }

    fn lifecycle(&self) -> ViewLifecycle {
        self.lifecycle
    }

    fn set_lifecycle(&mut self, lifecycle: ViewLifecycle) {
        self.lifecycle = lifecycle;
    }

    fn key(&self) -> Option<&Key> {
        self.key.as_ref()
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

/// Type-erased wrapper for ViewNode storage.
///
/// This is what actually gets stored in the Slab - internal implementation detail.
struct ViewNodeStorage {
    inner: Box<dyn ViewNode>,
}

impl ViewNodeStorage {
    fn new<V: ViewObject + fmt::Debug + 'static>(node: ConcreteViewNode<V>) -> Self {
        Self {
            inner: Box::new(node),
        }
    }
}

impl fmt::Debug for ViewNodeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

// ============================================================================
// VIEW TREE
// ============================================================================

/// ViewTree - Slab-based storage for view nodes.
///
/// This is the first of FLUI's four trees, corresponding to Flutter's Widget tree.
///
/// # Architecture
///
/// ```text
/// ViewTree
///   ├─ nodes: Slab<ViewNodeStorage>  (type-erased storage)
///   └─ root: Option<ViewId>
/// ```
///
/// # Thread Safety
///
/// ViewTree itself is not thread-safe. Use `Arc<RwLock<ViewTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::tree::ViewTree;
///
/// let mut tree = ViewTree::new();
///
/// // Insert stateless view
/// let view = MyStatelessView { text: "Hello" };
/// let id = tree.insert(view, ViewMode::Stateless);
///
/// // Access via type-erased interface
/// let node = tree.get(id).unwrap();
/// assert_eq!(node.mode(), ViewMode::Stateless);
///
/// // Build
/// node.view_object_mut().build(ctx);
/// ```
#[derive(Debug)]
pub struct ViewTree {
    /// Slab storage for ViewNodes (0-based indexing internally)
    nodes: Slab<ViewNodeStorage>,

    /// Root ViewNode ID (None if tree is empty)
    root: Option<ViewId>,
}

impl ViewTree {
    /// Creates a new empty ViewTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a ViewTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root ViewNode ID.
    #[inline]
    pub fn root(&self) -> Option<ViewId> {
        self.root
    }

    /// Set the root ViewNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ViewId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a ViewNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: ViewId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of ViewNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Inserts a ViewObject into the tree.
    ///
    /// Returns the ViewId of the inserted node.
    ///
    /// # Generic
    ///
    /// This method is generic over the ViewObject type, enabling zero-cost
    /// insertion without requiring the caller to box the ViewObject.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `ViewId(1)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let view = MyView { text: "Hello" };
    /// let id = tree.insert(view, ViewMode::Stateless);
    /// ```
    pub fn insert<V: ViewObject + fmt::Debug + 'static>(
        &mut self,
        object: V,
        mode: ViewMode,
    ) -> ViewId {
        let node = ConcreteViewNode::new(object, mode);
        let storage = ViewNodeStorage::new(node);
        let slab_index = self.nodes.insert(storage);
        ViewId::new(slab_index + 1) // +1 offset
    }

    /// Returns a reference to a ViewNode (type-erased).
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `ViewId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: ViewId) -> Option<&(dyn ViewNode + '_)> {
        self.nodes
            .get(id.get() - 1)
            .map(|storage| &*storage.inner as &(dyn ViewNode + '_))
    }

    /// Returns a mutable reference to a ViewNode (type-erased).
    #[inline]
    pub fn get_mut(&mut self, id: ViewId) -> Option<&mut (dyn ViewNode + '_)> {
        self.nodes
            .get_mut(id.get() - 1)
            .map(|storage| &mut *storage.inner as &mut (dyn ViewNode + '_))
    }

    /// Returns a reference to the concrete ViewNode type.
    ///
    /// This requires knowing the concrete type V at the call site.
    /// Returns None if the ID is invalid or the type doesn't match.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(node) = tree.get_concrete::<MyView>(id) {
    ///     println!("Text: {}", node.object().text);
    /// }
    /// ```
    pub fn get_concrete<V: ViewObject + fmt::Debug + 'static>(
        &self,
        id: ViewId,
    ) -> Option<&ConcreteViewNode<V>> {
        self.get(id)?.as_any().downcast_ref::<ConcreteViewNode<V>>()
    }

    /// Returns a mutable reference to the concrete ViewNode type.
    pub fn get_concrete_mut<V: ViewObject + fmt::Debug + 'static>(
        &mut self,
        id: ViewId,
    ) -> Option<&mut ConcreteViewNode<V>> {
        self.get_mut(id)?
            .as_any_mut()
            .downcast_mut::<ConcreteViewNode<V>>()
    }

    /// Removes a ViewNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: ViewId) -> Option<Box<dyn ViewNode>> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes
            .try_remove(id.get() - 1)
            .map(|storage| storage.inner)
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent ViewNode.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: ViewId, child_id: ViewId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent ViewNode.
    pub fn remove_child(&mut self, parent_id: ViewId, child_id: ViewId) {
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

impl Default for ViewTree {
    fn default() -> Self {
        Self::new()
    }
}
