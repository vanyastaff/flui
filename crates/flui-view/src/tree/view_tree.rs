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

use flui_foundation::{ElementId, Key};

use crate::view_mode::ViewMode;
use crate::view_object::ViewObject;
use super::lifecycle::ViewLifecycle;

// ============================================================================
// VIEW OBJECT ID
// ============================================================================

/// Unique identifier for ViewObject in ViewObjectTree.
///
/// Uses 1-based indexing (NonZeroUsize) for:
/// - Niche optimization: Option<ViewObjectId> = 8 bytes
/// - 0 reserved for "null" semantics
///
/// # Slab Offset Pattern
///
/// ViewObjectId uses 1-based indexing while Slab uses 0-based:
/// - `ViewObjectId(1)` → `nodes[0]`
/// - `ViewObjectId(2)` → `nodes[1]`
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ViewObjectId(std::num::NonZeroUsize);

impl ViewObjectId {
    /// Creates a new ViewObjectId from a 1-based index.
    ///
    /// # Panics
    ///
    /// Panics if id is 0 (use NonZeroUsize for safety).
    #[inline]
    pub fn new(id: usize) -> Self {
        Self(std::num::NonZeroUsize::new(id).expect("ViewObjectId cannot be 0"))
    }

    /// Gets the underlying 1-based index.
    #[inline]
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

// ============================================================================
// VIEW NODE TRAIT (Type-erased interface)
// ============================================================================

/// Type-erased trait for ViewNode operations.
///
/// This trait enables storing different ViewNode<V> types in the same Slab
/// while preserving access to common operations.
///
/// # Design
///
/// Similar to how RenderObject trait works for RenderObjects, ViewNodeTrait
/// provides a type-erased interface for ViewNodes of different concrete types.
pub trait ViewNodeTrait: Send + Sync + fmt::Debug {
    // ========== Tree Structure ==========

    fn parent(&self) -> Option<ViewObjectId>;
    fn set_parent(&mut self, parent: Option<ViewObjectId>);

    fn children(&self) -> &[ViewObjectId];
    fn add_child(&mut self, child: ViewObjectId);
    fn remove_child(&mut self, child: ViewObjectId);

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
// VIEW NODE (Generic implementation)
// ============================================================================

/// Generic ViewNode - stores ViewObject inline for zero-cost abstraction.
///
/// # Type Parameters
///
/// * `V` - Concrete ViewObject type (e.g., StatelessView, StatefulView)
///
/// # Design
///
/// ViewNode is generic over the ViewObject type, enabling static dispatch
/// for all ViewObject operations until type erasure at storage boundary.
///
/// ```rust,ignore
/// // Concrete type - no vtable!
/// let node = ViewNode {
///     object: MyView { text: "Hello" },  // Inline storage
///     mode: ViewMode::Stateless,
/// };
///
/// // Static dispatch
/// node.object.build(ctx);  // Direct call, can inline!
/// ```
#[derive(Debug)]
pub struct ViewNode<V: ViewObject> {
    // ========== Tree Structure ==========
    parent: Option<ViewObjectId>,
    children: Vec<ViewObjectId>,

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

impl<V: ViewObject> ViewNode<V> {
    /// Creates a new ViewNode with the given ViewObject and mode.
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

    /// Creates a ViewNode with a key.
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
// VIEW NODE TRAIT IMPL (Generic → Type-erased)
// ============================================================================

impl<V: ViewObject + 'static> ViewNodeTrait for ViewNode<V> {
    fn parent(&self) -> Option<ViewObjectId> {
        self.parent
    }

    fn set_parent(&mut self, parent: Option<ViewObjectId>) {
        self.parent = parent;
    }

    fn children(&self) -> &[ViewObjectId] {
        &self.children
    }

    fn add_child(&mut self, child: ViewObjectId) {
        self.children.push(child);
    }

    fn remove_child(&mut self, child: ViewObjectId) {
        self.children.retain(|&id| id != child);
    }

    fn view_object(&self) -> &dyn ViewObject {
        &self.object  // ✅ Uses existing ViewObject trait
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
// VIEW NODE BOX (Type-erased wrapper)
// ============================================================================

/// Type-erased wrapper for ViewNode storage.
///
/// This is what actually gets stored in the Slab.
struct ViewNodeBox {
    inner: Box<dyn ViewNodeTrait>,
}

impl ViewNodeBox {
    fn new<V: ViewObject + 'static>(node: ViewNode<V>) -> Self {
        Self {
            inner: Box::new(node),
        }
    }
}

impl fmt::Debug for ViewNodeBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

// ============================================================================
// VIEW OBJECT TREE
// ============================================================================

/// ViewObjectTree - Slab-based storage for ViewObjects.
///
/// This is the first of FLUI's four trees, corresponding to Flutter's Widget tree.
///
/// # Architecture
///
/// ```text
/// ViewObjectTree
///   ├─ nodes: Slab<ViewNodeBox>  (type-erased storage)
///   └─ root: Option<ViewObjectId>
/// ```
///
/// # Thread Safety
///
/// ViewObjectTree itself is not thread-safe. Use `Arc<RwLock<ViewObjectTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::tree::ViewObjectTree;
///
/// let mut tree = ViewObjectTree::new();
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
pub struct ViewObjectTree {
    /// Slab storage for ViewNodes (0-based indexing internally)
    nodes: Slab<ViewNodeBox>,

    /// Root ViewObject ID (None if tree is empty)
    root: Option<ViewObjectId>,
}

impl ViewObjectTree {
    /// Creates a new empty ViewObjectTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a ViewObjectTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root ViewObject ID.
    #[inline]
    pub fn root(&self) -> Option<ViewObjectId> {
        self.root
    }

    /// Set the root ViewObject ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ViewObjectId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a ViewObject exists in the tree.
    #[inline]
    pub fn contains(&self, id: ViewObjectId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of ViewObjects in the tree.
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
    /// Returns the ViewObjectId of the inserted object.
    ///
    /// # Generic
    ///
    /// This method is generic over the ViewObject type, enabling zero-cost
    /// insertion without requiring the caller to box the ViewObject.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `ViewObjectId(1)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let view = MyView { text: "Hello" };
    /// let id = tree.insert(view, ViewMode::Stateless);
    /// ```
    pub fn insert<V: ViewObject + 'static>(
        &mut self,
        object: V,
        mode: ViewMode,
    ) -> ViewObjectId {
        let node = ViewNode::new(object, mode);
        let boxed = ViewNodeBox::new(node);
        let slab_index = self.nodes.insert(boxed);
        ViewObjectId::new(slab_index + 1)  // +1 offset
    }

    /// Returns a reference to a ViewNode (type-erased).
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `ViewObjectId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: ViewObjectId) -> Option<&dyn ViewNodeTrait> {
        self.nodes
            .get(id.get() - 1)
            .map(|node| &*node.inner)
    }

    /// Returns a mutable reference to a ViewNode (type-erased).
    #[inline]
    pub fn get_mut(&mut self, id: ViewObjectId) -> Option<&mut dyn ViewNodeTrait> {
        self.nodes
            .get_mut(id.get() - 1)
            .map(|node| &mut *node.inner)
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
    pub fn get_concrete<V: ViewObject + 'static>(&self, id: ViewObjectId) -> Option<&ViewNode<V>> {
        self.get(id)?
            .as_any()
            .downcast_ref::<ViewNode<V>>()
    }

    /// Returns a mutable reference to the concrete ViewNode type.
    pub fn get_concrete_mut<V: ViewObject + 'static>(
        &mut self,
        id: ViewObjectId,
    ) -> Option<&mut ViewNode<V>> {
        self.get_mut(id)?
            .as_any_mut()
            .downcast_mut::<ViewNode<V>>()
    }

    /// Removes a ViewObject from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: ViewObjectId) -> Option<Box<dyn ViewNodeTrait>> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        self.nodes
            .try_remove(id.get() - 1)
            .map(|node| node.inner)
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent ViewObject.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: ViewObjectId, child_id: ViewObjectId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent ViewObject.
    pub fn remove_child(&mut self, parent_id: ViewObjectId, child_id: ViewObjectId) {
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

impl Default for ViewObjectTree {
    fn default() -> Self {
        Self::new()
    }
}
