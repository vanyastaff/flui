//! Unified Children container with compile-time Arity and State guarantees.
//!
//! `Children<N, A, S>` provides type-safe child management:
//!
//! - **N: Node** — node type (defines `N::Id`)
//! - **A: Arity** — child count constraint (Leaf, Single, Optional, Variable)
//! - **S: NodeState** — lifecycle state (Unmounted, Mounted)
//!
//! # Design Philosophy
//!
//! The container uses typestate pattern to enforce correct usage:
//!
//! - **Unmounted**: Mutable, for building tree structure
//! - **Mounted**: Immutable, stores only IDs (actual nodes in tree)
//!
//! Different Arity types provide different APIs:
//!
//! - **Leaf**: No children methods (empty by definition)
//! - **Single**: `set()` / `get()` for exactly one child
//! - **Optional**: `set()` / `get()` / `clear()` for 0 or 1 child
//! - **Variable**: Full `push()` / `pop()` / `iter()` API
//!
//! # Storage Types
//!
//! | Arity    | Unmounted      | Mounted          |
//! |----------|----------------|------------------|
//! | Leaf     | `()`           | `()`             |
//! | Single   | `Option<N>`    | `N::Id`          |
//! | Optional | `Option<N>`    | `Option<N::Id>`  |
//! | Variable | `Vec<N>`       | `Box<[N::Id]>`   |
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_tree::{Children, Node, Leaf, Single, Variable, Unmounted, Mounted};
//!
//! // Single child (e.g., Padding widget)
//! let mut children: Children<Element, Single> = Children::new();
//! children.set(child_element);
//! let mounted: Children<Element, Single, Mounted> = children.mount(parent_id, mount_fn);
//! let child_id = mounted.get();  // Guaranteed to exist!
//!
//! // Variable children (e.g., Column widget)
//! let mut children: Children<Element, Variable> = Children::new();
//! children.push(child1);
//! children.push(child2);
//! let mounted = children.mount(parent_id, mount_fn);
//! for id in mounted.iter() { /* ... */ }
//!
//! // Leaf (e.g., Text widget) — no children methods
//! let children: Children<Element, Leaf> = Children::new();
//! // children.push(x);  // ❌ Compile error!
//! ```

use std::fmt;
use std::marker::PhantomData;

use crate::state::{Mounted, NodeState, Unmounted};
use crate::traits::node::Node;

// ============================================================================
// ARITY STORAGE TRAIT
// ============================================================================

/// Extension of Arity that defines storage types for Children container.
///
/// This trait connects compile-time arity constraints with actual storage:
///
/// - `Unmounted<N>`: Mutable storage for building (Vec, Option, etc.)
/// - `Mounted<N>`: Immutable storage for mounted state (Box<[Id]>, Id, etc.)
pub trait ArityStorage: crate::arity::Arity {
    /// Storage type for unmounted state (mutable, building phase).
    type Unmounted<N: Node>: Default + Send + Sync;

    /// Storage type for mounted state (immutable, in-tree).
    type Mounted<N: Node>: Send + Sync;

    /// Convert unmounted storage to mounted storage.
    ///
    /// # Arguments
    /// - `storage`: The unmounted storage to convert
    /// - `parent`: Parent node ID
    /// - `mount_fn`: Function to mount each child: `(child, parent) -> child_id`
    fn mount_storage<N, F>(
        storage: Self::Unmounted<N>,
        parent: N::Id,
        mount_fn: F,
    ) -> Self::Mounted<N>
    where
        N: Node,
        F: FnMut(N, N::Id) -> N::Id;

    /// Convert mounted storage back to unmounted storage.
    ///
    /// # Arguments
    /// - `storage`: The mounted storage to convert
    /// - `unmount_fn`: Function to unmount each child: `child_id -> child`
    fn unmount_storage<N, F>(storage: Self::Mounted<N>, unmount_fn: F) -> Self::Unmounted<N>
    where
        N: Node,
        F: FnMut(N::Id) -> N;
}

// ============================================================================
// ARITY STORAGE IMPLEMENTATIONS
// ============================================================================

impl ArityStorage for crate::arity::Leaf {
    type Unmounted<N: Node> = ();
    type Mounted<N: Node> = ();

    #[inline]
    fn mount_storage<N, F>((): (), _parent: N::Id, _: F)
    where
        N: Node,
        F: FnMut(N, N::Id) -> N::Id,
    {
    }

    #[inline]
    fn unmount_storage<N, F>((): (), _: F)
    where
        N: Node,
        F: FnMut(N::Id) -> N,
    {
    }
}

impl ArityStorage for crate::arity::Single {
    type Unmounted<N: Node> = Option<N>;
    type Mounted<N: Node> = N::Id; // Guaranteed to exist after mount

    fn mount_storage<N, F>(storage: Option<N>, parent: N::Id, mut mount_fn: F) -> N::Id
    where
        N: Node,
        F: FnMut(N, N::Id) -> N::Id,
    {
        let child = storage.expect("Single: child must be set before mount");
        mount_fn(child, parent)
    }

    fn unmount_storage<N, F>(storage: N::Id, mut unmount_fn: F) -> Option<N>
    where
        N: Node,
        F: FnMut(N::Id) -> N,
    {
        Some(unmount_fn(storage))
    }
}

impl ArityStorage for crate::arity::Optional {
    type Unmounted<N: Node> = Option<N>;
    type Mounted<N: Node> = Option<N::Id>;

    fn mount_storage<N, F>(storage: Option<N>, parent: N::Id, mut mount_fn: F) -> Option<N::Id>
    where
        N: Node,
        F: FnMut(N, N::Id) -> N::Id,
    {
        storage.map(|child| mount_fn(child, parent))
    }

    fn unmount_storage<N, F>(storage: Option<N::Id>, unmount_fn: F) -> Option<N>
    where
        N: Node,
        F: FnMut(N::Id) -> N,
    {
        storage.map(unmount_fn)
    }
}

impl ArityStorage for crate::arity::Variable {
    type Unmounted<N: Node> = Vec<N>;
    type Mounted<N: Node> = Box<[N::Id]>;

    fn mount_storage<N, F>(storage: Vec<N>, parent: N::Id, mut mount_fn: F) -> Box<[N::Id]>
    where
        N: Node,
        F: FnMut(N, N::Id) -> N::Id,
    {
        storage
            .into_iter()
            .map(|child| mount_fn(child, parent))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn unmount_storage<N, F>(storage: Box<[N::Id]>, unmount_fn: F) -> Vec<N>
    where
        N: Node,
        F: FnMut(N::Id) -> N,
    {
        Vec::from(storage).into_iter().map(unmount_fn).collect()
    }
}

// ============================================================================
// STATE SELECTOR TRAIT
// ============================================================================

/// Selects the appropriate storage type based on NodeState.
///
/// This trait bridges Arity storage types with NodeState:
/// - `Unmounted` → `A::Unmounted<N>`
/// - `Mounted` → `A::Mounted<N>`
pub trait StateSelector<N: Node, A: ArityStorage>: NodeState + sealed::StateSealed {
    /// The storage type for this state.
    type Storage: Send + Sync;
}

impl<N: Node, A: ArityStorage> StateSelector<N, A> for Unmounted {
    type Storage = A::Unmounted<N>;
}

impl<N: Node, A: ArityStorage> StateSelector<N, A> for Mounted {
    type Storage = A::Mounted<N>;
}

mod sealed {
    pub trait StateSealed {}
    impl StateSealed for super::Unmounted {}
    impl StateSealed for super::Mounted {}
}

// ============================================================================
// CHILDREN CONTAINER
// ============================================================================

/// Unified children container with compile-time Arity and State guarantees.
///
/// # Type Parameters
///
/// - `N: Node` — node type (Element, RenderElement, etc.)
/// - `A: ArityStorage` — arity constraint (Leaf, Single, Optional, Variable)
/// - `S: NodeState` — state (Unmounted, Mounted)
///
/// # Storage
///
/// The actual storage type depends on both Arity and State:
///
/// ```text
/// Children<N, Leaf, Unmounted>     → ()
/// Children<N, Leaf, Mounted>       → ()
/// Children<N, Single, Unmounted>   → Option<N>
/// Children<N, Single, Mounted>     → N::Id
/// Children<N, Optional, Unmounted> → Option<N>
/// Children<N, Optional, Mounted>   → Option<N::Id>
/// Children<N, Variable, Unmounted> → Vec<N>
/// Children<N, Variable, Mounted>   → Box<[N::Id]>
/// ```
pub struct Children<N, A = crate::arity::Variable, S = Unmounted>
where
    N: Node,
    A: ArityStorage,
    S: StateSelector<N, A>,
{
    storage: S::Storage,
    _phantom: PhantomData<(N, A)>,
}

// ============================================================================
// COMMON IMPL (all states)
// ============================================================================

impl<N, A, S> Children<N, A, S>
where
    N: Node,
    A: ArityStorage,
    S: StateSelector<N, A>,
{
    /// Get runtime arity information.
    #[inline]
    pub fn arity() -> crate::arity::RuntimeArity {
        A::runtime_arity()
    }

    /// Check if currently in mounted state (compile-time constant).
    #[inline]
    pub const fn is_mounted() -> bool {
        S::IS_MOUNTED
    }
}

// ============================================================================
// LEAF — UNMOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Leaf, Unmounted> {
    /// Create new empty leaf children.
    #[inline]
    pub fn new() -> Self {
        Self {
            storage: (),
            _phantom: PhantomData,
        }
    }

    /// Leaf always has 0 children.
    #[inline]
    pub const fn len(&self) -> usize {
        0
    }

    /// Leaf is always empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        true
    }

    /// Mount leaf children (trivial — no children to mount).
    #[inline]
    pub fn mount(self, _parent: N::Id) -> Children<N, crate::arity::Leaf, Mounted> {
        Children {
            storage: (),
            _phantom: PhantomData,
        }
    }
}

impl<N: Node> Default for Children<N, crate::arity::Leaf, Unmounted> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// LEAF — MOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Leaf, Mounted> {
    /// Leaf always has 0 children.
    #[inline]
    pub const fn len(&self) -> usize {
        0
    }

    /// Leaf is always empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        true
    }

    /// Unmount leaf children (trivial).
    #[inline]
    pub fn unmount(self) -> Children<N, crate::arity::Leaf, Unmounted> {
        Children {
            storage: (),
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// SINGLE — UNMOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Single, Unmounted> {
    /// Create new single-child container (initially empty).
    #[inline]
    pub fn new() -> Self {
        Self {
            storage: None,
            _phantom: PhantomData,
        }
    }

    /// Create with child already set.
    #[inline]
    pub fn with_child(child: N) -> Self {
        Self {
            storage: Some(child),
            _phantom: PhantomData,
        }
    }

    /// Set the single child.
    #[inline]
    pub fn set(&mut self, child: N) {
        self.storage = Some(child);
    }

    /// Get reference to the child (if set).
    #[inline]
    pub fn get(&self) -> Option<&N> {
        self.storage.as_ref()
    }

    /// Get mutable reference to the child.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut N> {
        self.storage.as_mut()
    }

    /// Take the child out, leaving None.
    #[inline]
    pub fn take(&mut self) -> Option<N> {
        self.storage.take()
    }

    /// Check if child is set.
    #[inline]
    pub fn is_set(&self) -> bool {
        self.storage.is_some()
    }

    /// Returns 1 if set, 0 otherwise.
    #[inline]
    pub fn len(&self) -> usize {
        usize::from(self.storage.is_some())
    }

    /// Returns true if child is not set.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.is_none()
    }

    /// Mount the single child.
    ///
    /// # Panics
    ///
    /// Panics if child was not set.
    pub fn mount<F>(self, parent: N::Id, mount_fn: F) -> Children<N, crate::arity::Single, Mounted>
    where
        F: FnMut(N, N::Id) -> N::Id,
    {
        let storage = crate::arity::Single::mount_storage(self.storage, parent, mount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }

    /// Try to mount, returns None if child not set.
    pub fn try_mount<F>(
        self,
        parent: N::Id,
        mut mount_fn: F,
    ) -> Option<Children<N, crate::arity::Single, Mounted>>
    where
        F: FnMut(N, N::Id) -> N::Id,
    {
        let child = self.storage?;
        let storage = mount_fn(child, parent);
        Some(Children {
            storage,
            _phantom: PhantomData,
        })
    }
}

impl<N: Node> Default for Children<N, crate::arity::Single, Unmounted> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SINGLE — MOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Single, Mounted> {
    /// Get the single child ID (guaranteed to exist).
    #[inline]
    pub fn get(&self) -> N::Id {
        self.storage
    }

    /// Single always has exactly 1 child when mounted.
    #[inline]
    pub const fn len(&self) -> usize {
        1
    }

    /// Single is never empty when mounted.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Unmount the single child.
    pub fn unmount<F>(self, unmount_fn: F) -> Children<N, crate::arity::Single, Unmounted>
    where
        F: FnMut(N::Id) -> N,
    {
        let storage = crate::arity::Single::unmount_storage(self.storage, unmount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// OPTIONAL — UNMOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Optional, Unmounted> {
    /// Create new optional-child container (initially empty).
    #[inline]
    pub fn new() -> Self {
        Self {
            storage: None,
            _phantom: PhantomData,
        }
    }

    /// Create with child.
    #[inline]
    pub fn with_child(child: N) -> Self {
        Self {
            storage: Some(child),
            _phantom: PhantomData,
        }
    }

    /// Set the optional child.
    #[inline]
    pub fn set(&mut self, child: N) {
        self.storage = Some(child);
    }

    /// Get reference to child (if present).
    #[inline]
    pub fn get(&self) -> Option<&N> {
        self.storage.as_ref()
    }

    /// Get mutable reference to child.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut N> {
        self.storage.as_mut()
    }

    /// Take the child out.
    #[inline]
    pub fn take(&mut self) -> Option<N> {
        self.storage.take()
    }

    /// Clear the child (set to None).
    #[inline]
    pub fn clear(&mut self) {
        self.storage = None;
    }

    /// Check if child is present.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.storage.is_some()
    }

    /// Check if child is absent.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.storage.is_none()
    }

    /// Returns 1 if present, 0 otherwise.
    #[inline]
    pub fn len(&self) -> usize {
        usize::from(self.storage.is_some())
    }

    /// Returns true if no child.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.is_none()
    }

    /// Mount the optional child.
    pub fn mount<F>(
        self,
        parent: N::Id,
        mount_fn: F,
    ) -> Children<N, crate::arity::Optional, Mounted>
    where
        F: FnMut(N, N::Id) -> N::Id,
    {
        let storage = crate::arity::Optional::mount_storage(self.storage, parent, mount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }
}

impl<N: Node> Default for Children<N, crate::arity::Optional, Unmounted> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// OPTIONAL — MOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Optional, Mounted> {
    /// Get the child ID (if present).
    #[inline]
    pub fn get(&self) -> Option<N::Id> {
        self.storage
    }

    /// Check if child is present.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.storage.is_some()
    }

    /// Check if child is absent.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.storage.is_none()
    }

    /// Returns 1 if present, 0 otherwise.
    #[inline]
    pub fn len(&self) -> usize {
        usize::from(self.storage.is_some())
    }

    /// Returns true if no child.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.is_none()
    }

    /// Unmount the optional child.
    pub fn unmount<F>(self, unmount_fn: F) -> Children<N, crate::arity::Optional, Unmounted>
    where
        F: FnMut(N::Id) -> N,
    {
        let storage = crate::arity::Optional::unmount_storage(self.storage, unmount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// VARIABLE — UNMOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Variable, Unmounted> {
    /// Create new variable-children container.
    #[inline]
    pub fn new() -> Self {
        Self {
            storage: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Create with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: Vec::with_capacity(capacity),
            _phantom: PhantomData,
        }
    }

    /// Create from existing Vec.
    #[inline]
    pub fn from_vec(children: Vec<N>) -> Self {
        Self {
            storage: children,
            _phantom: PhantomData,
        }
    }

    /// Push a child to the end.
    #[inline]
    pub fn push(&mut self, child: N) {
        self.storage.push(child);
    }

    /// Pop the last child.
    #[inline]
    pub fn pop(&mut self) -> Option<N> {
        self.storage.pop()
    }

    /// Insert child at index.
    #[inline]
    pub fn insert(&mut self, index: usize, child: N) {
        self.storage.insert(index, child);
    }

    /// Remove child at index.
    #[inline]
    pub fn remove(&mut self, index: usize) -> N {
        self.storage.remove(index)
    }

    /// Get child at index.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&N> {
        self.storage.get(index)
    }

    /// Get mutable child at index.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut N> {
        self.storage.get_mut(index)
    }

    /// Get first child.
    #[inline]
    pub fn first(&self) -> Option<&N> {
        self.storage.first()
    }

    /// Get first child (mutable).
    #[inline]
    pub fn first_mut(&mut self) -> Option<&mut N> {
        self.storage.first_mut()
    }

    /// Get last child.
    #[inline]
    pub fn last(&self) -> Option<&N> {
        self.storage.last()
    }

    /// Get last child (mutable).
    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut N> {
        self.storage.last_mut()
    }

    /// Iterate over children.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &N> + DoubleEndedIterator {
        self.storage.iter()
    }

    /// Iterate over children (mutable).
    #[inline]
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = &mut N> + DoubleEndedIterator {
        self.storage.iter_mut()
    }

    /// Get number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Get capacity.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Reserve capacity.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.storage.reserve(additional);
    }

    /// Clear all children.
    #[inline]
    pub fn clear(&mut self) {
        self.storage.clear();
    }

    /// Get as slice.
    #[inline]
    pub fn as_slice(&self) -> &[N] {
        &self.storage
    }

    /// Get as mutable slice.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [N] {
        &mut self.storage
    }

    /// Retain only children matching predicate.
    #[inline]
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&N) -> bool,
    {
        self.storage.retain(f);
    }

    /// Drain children in range.
    #[inline]
    pub fn drain<R>(&mut self, range: R) -> std::vec::Drain<'_, N>
    where
        R: std::ops::RangeBounds<usize>,
    {
        self.storage.drain(range)
    }

    /// Extend with iterator.
    #[inline]
    pub fn extend<I: IntoIterator<Item = N>>(&mut self, iter: I) {
        self.storage.extend(iter);
    }

    /// Convert to Vec.
    #[inline]
    pub fn into_vec(self) -> Vec<N> {
        self.storage
    }

    /// Mount all children.
    pub fn mount<F>(
        self,
        parent: N::Id,
        mount_fn: F,
    ) -> Children<N, crate::arity::Variable, Mounted>
    where
        F: FnMut(N, N::Id) -> N::Id,
    {
        let storage = crate::arity::Variable::mount_storage(self.storage, parent, mount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }
}

impl<N: Node> Default for Children<N, crate::arity::Variable, Unmounted> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N: Node> From<Vec<N>> for Children<N, crate::arity::Variable, Unmounted> {
    fn from(vec: Vec<N>) -> Self {
        Self::from_vec(vec)
    }
}

impl<'a, N: Node> IntoIterator for &'a Children<N, crate::arity::Variable, Unmounted> {
    type Item = &'a N;
    type IntoIter = std::slice::Iter<'a, N>;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.iter()
    }
}

impl<'a, N: Node> IntoIterator for &'a mut Children<N, crate::arity::Variable, Unmounted> {
    type Item = &'a mut N;
    type IntoIter = std::slice::IterMut<'a, N>;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.iter_mut()
    }
}

impl<N: Node> IntoIterator for Children<N, crate::arity::Variable, Unmounted> {
    type Item = N;
    type IntoIter = std::vec::IntoIter<N>;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.into_iter()
    }
}

// ============================================================================
// VARIABLE — MOUNTED
// ============================================================================

impl<N: Node> Children<N, crate::arity::Variable, Mounted> {
    /// Get child ID at index.
    #[inline]
    pub fn get(&self, index: usize) -> Option<N::Id> {
        self.storage.get(index).copied()
    }

    /// Get first child ID.
    #[inline]
    pub fn first(&self) -> Option<N::Id> {
        self.storage.first().copied()
    }

    /// Get last child ID.
    #[inline]
    pub fn last(&self) -> Option<N::Id> {
        self.storage.last().copied()
    }

    /// Iterate over child IDs.
    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = N::Id> + DoubleEndedIterator + '_ {
        self.storage.iter().copied()
    }

    /// Get number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Get as slice of IDs.
    #[inline]
    pub fn as_slice(&self) -> &[N::Id] {
        &self.storage
    }

    /// Check if contains specific ID.
    #[inline]
    pub fn contains(&self, id: N::Id) -> bool {
        self.storage.contains(&id)
    }

    /// Find position of ID.
    #[inline]
    pub fn position(&self, id: N::Id) -> Option<usize> {
        self.storage.iter().position(|&x| x == id)
    }

    /// Unmount all children.
    pub fn unmount<F>(self, unmount_fn: F) -> Children<N, crate::arity::Variable, Unmounted>
    where
        F: FnMut(N::Id) -> N,
    {
        let storage = crate::arity::Variable::unmount_storage(self.storage, unmount_fn);
        Children {
            storage,
            _phantom: PhantomData,
        }
    }

    /// Convert to Vec of IDs.
    #[inline]
    pub fn into_vec(self) -> Vec<N::Id> {
        Vec::from(self.storage)
    }
}

impl<'a, N: Node> IntoIterator for &'a Children<N, crate::arity::Variable, Mounted> {
    type Item = N::Id;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, N::Id>>;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.iter().copied()
    }
}

// ============================================================================
// DEBUG IMPLEMENTATIONS
// ============================================================================

impl<N, A, S> fmt::Debug for Children<N, A, S>
where
    N: Node,
    A: ArityStorage,
    S: StateSelector<N, A>,
    S::Storage: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Children")
            .field("arity", &A::runtime_arity())
            .field("state", &S::name())
            .field("storage", &self.storage)
            .finish()
    }
}

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// Leaf children (always empty).
pub type LeafChildren<N, S = Unmounted> = Children<N, crate::arity::Leaf, S>;

/// Single child container.
pub type SingleChild<N, S = Unmounted> = Children<N, crate::arity::Single, S>;

/// Optional child container.
pub type OptionalChild<N, S = Unmounted> = Children<N, crate::arity::Optional, S>;

/// Variable children container.
pub type VariableChildren<N, S = Unmounted> = Children<N, crate::arity::Variable, S>;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::Identifier;

    // Mock ID type for testing (must implement Identifier)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct TestId(std::num::NonZeroUsize);

    impl std::fmt::Display for TestId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestId({})", self.0)
        }
    }

    impl Identifier for TestId {
        fn new(value: usize) -> Self {
            Self(std::num::NonZeroUsize::new(value).expect("ID cannot be 0"))
        }

        fn new_checked(value: usize) -> Option<Self> {
            std::num::NonZeroUsize::new(value).map(Self)
        }

        fn get(self) -> usize {
            self.0.get()
        }

        unsafe fn new_unchecked(value: usize) -> Self {
            Self(std::num::NonZeroUsize::new_unchecked(value))
        }
    }

    // Mock node type for testing
    #[derive(Debug, Clone, PartialEq)]
    struct TestNode {
        value: i32,
    }

    impl Node for TestNode {
        type Id = TestId;
    }

    // ========== LEAF TESTS ==========

    #[test]
    fn test_leaf_unmounted() {
        let children: LeafChildren<TestNode> = LeafChildren::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_leaf_mount_unmount() {
        let children: LeafChildren<TestNode> = LeafChildren::new();
        let mounted = children.mount(TestId::new(1));
        assert!(mounted.is_empty());
        assert_eq!(mounted.len(), 0);

        let unmounted = mounted.unmount();
        assert!(unmounted.is_empty());
    }

    // ========== SINGLE TESTS ==========

    #[test]
    fn test_single_unmounted() {
        let mut children: SingleChild<TestNode> = SingleChild::new();
        assert!(!children.is_set());
        assert!(children.is_empty());

        children.set(TestNode { value: 42 });
        assert!(children.is_set());
        assert_eq!(children.len(), 1);
        assert_eq!(children.get().unwrap().value, 42);
    }

    #[test]
    fn test_single_with_child() {
        let children = SingleChild::<TestNode>::with_child(TestNode { value: 99 });
        assert!(children.is_set());
        assert_eq!(children.get().unwrap().value, 99);
    }

    #[test]
    fn test_single_mount_unmount() {
        let mut children: SingleChild<TestNode> = SingleChild::new();
        children.set(TestNode { value: 42 });

        let mut next_id = 1;
        let mounted = children.mount(TestId::new(1), |_child, _parent| {
            next_id += 1;
            TestId::new(next_id)
        });

        assert_eq!(mounted.len(), 1);
        assert!(!mounted.is_empty());
        let child_id = mounted.get();
        assert_eq!(child_id, TestId::new(2));

        let unmounted = mounted.unmount(|id| TestNode {
            value: id.get() as i32 * 10,
        });
        assert!(unmounted.is_set());
        assert_eq!(unmounted.get().unwrap().value, 20);
    }

    #[test]
    fn test_single_try_mount() {
        let children: SingleChild<TestNode> = SingleChild::new();
        let result = children.try_mount(TestId::new(1), |_child, _parent| TestId::new(2));
        assert!(result.is_none());

        let children = SingleChild::<TestNode>::with_child(TestNode { value: 1 });
        let result = children.try_mount(TestId::new(1), |_child, _parent| TestId::new(2));
        assert!(result.is_some());
    }

    #[test]
    #[should_panic(expected = "Single: child must be set before mount")]
    fn test_single_mount_panic_if_not_set() {
        let children: SingleChild<TestNode> = SingleChild::new();
        let _ = children.mount(TestId::new(1), |_child, _parent| TestId::new(2));
    }

    // ========== OPTIONAL TESTS ==========

    #[test]
    fn test_optional_unmounted() {
        let mut children: OptionalChild<TestNode> = OptionalChild::new();
        assert!(children.is_none());
        assert!(children.is_empty());

        children.set(TestNode { value: 42 });
        assert!(children.is_some());
        assert_eq!(children.len(), 1);

        children.clear();
        assert!(children.is_none());
    }

    #[test]
    fn test_optional_mount_empty() {
        let children: OptionalChild<TestNode> = OptionalChild::new();
        let mounted = children.mount(TestId::new(1), |_child, _parent| TestId::new(2));

        assert!(mounted.is_none());
        assert!(mounted.is_empty());
        assert_eq!(mounted.get(), None);
    }

    #[test]
    fn test_optional_mount_with_child() {
        let children = OptionalChild::<TestNode>::with_child(TestNode { value: 42 });
        let mounted = children.mount(TestId::new(1), |_child, _parent| TestId::new(2));

        assert!(mounted.is_some());
        assert_eq!(mounted.len(), 1);
        assert_eq!(mounted.get(), Some(TestId::new(2)));
    }

    // ========== VARIABLE TESTS ==========

    #[test]
    fn test_variable_unmounted() {
        let mut children: VariableChildren<TestNode> = VariableChildren::new();
        assert!(children.is_empty());

        children.push(TestNode { value: 1 });
        children.push(TestNode { value: 2 });
        children.push(TestNode { value: 3 });

        assert_eq!(children.len(), 3);
        assert_eq!(children.first().unwrap().value, 1);
        assert_eq!(children.last().unwrap().value, 3);
        assert_eq!(children.get(1).unwrap().value, 2);

        let popped = children.pop().unwrap();
        assert_eq!(popped.value, 3);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_variable_from_vec() {
        let vec = vec![
            TestNode { value: 1 },
            TestNode { value: 2 },
            TestNode { value: 3 },
        ];
        let children = VariableChildren::<TestNode>::from_vec(vec);
        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_variable_iteration() {
        let mut children: VariableChildren<TestNode> = VariableChildren::new();
        children.push(TestNode { value: 1 });
        children.push(TestNode { value: 2 });
        children.push(TestNode { value: 3 });

        let values: Vec<_> = children.iter().map(|n| n.value).collect();
        assert_eq!(values, vec![1, 2, 3]);

        for node in children.iter_mut() {
            node.value *= 10;
        }

        let values: Vec<_> = children.iter().map(|n| n.value).collect();
        assert_eq!(values, vec![10, 20, 30]);
    }

    #[test]
    fn test_variable_mount_unmount() {
        let mut children: VariableChildren<TestNode> = VariableChildren::new();
        children.push(TestNode { value: 1 });
        children.push(TestNode { value: 2 });
        children.push(TestNode { value: 3 });

        let mut next_id = 1;
        let mounted = children.mount(TestId::new(1), |_child, _parent| {
            next_id += 1;
            TestId::new(next_id)
        });

        assert_eq!(mounted.len(), 3);
        assert_eq!(mounted.first(), Some(TestId::new(2)));
        assert_eq!(mounted.last(), Some(TestId::new(4)));
        assert!(mounted.contains(TestId::new(3)));
        assert_eq!(mounted.position(TestId::new(3)), Some(1));

        let ids: Vec<_> = mounted.iter().collect();
        assert_eq!(ids, vec![TestId::new(2), TestId::new(3), TestId::new(4)]);

        let unmounted = mounted.unmount(|id| TestNode {
            value: id.get() as i32 * 100,
        });
        assert_eq!(unmounted.len(), 3);
        let values: Vec<_> = unmounted.iter().map(|n| n.value).collect();
        assert_eq!(values, vec![200, 300, 400]);
    }

    #[test]
    fn test_variable_retain() {
        let mut children: VariableChildren<TestNode> = VariableChildren::new();
        children.push(TestNode { value: 1 });
        children.push(TestNode { value: 2 });
        children.push(TestNode { value: 3 });
        children.push(TestNode { value: 4 });

        children.retain(|n| n.value % 2 == 0);
        assert_eq!(children.len(), 2);
        let values: Vec<_> = children.iter().map(|n| n.value).collect();
        assert_eq!(values, vec![2, 4]);
    }

    #[test]
    fn test_variable_into_iter() {
        let mut children: VariableChildren<TestNode> = VariableChildren::new();
        children.push(TestNode { value: 1 });
        children.push(TestNode { value: 2 });

        let vec: Vec<_> = children.into_iter().collect();
        assert_eq!(vec.len(), 2);
        assert_eq!(vec[0].value, 1);
    }

    // ========== TYPE ALIAS TESTS ==========

    #[test]
    fn test_type_aliases() {
        let _leaf: LeafChildren<TestNode> = Default::default();
        let _single: SingleChild<TestNode> = Default::default();
        let _optional: OptionalChild<TestNode> = Default::default();
        let _variable: VariableChildren<TestNode> = Default::default();
    }

    // ========== DEBUG TESTS ==========

    #[test]
    fn test_debug_output() {
        let children: VariableChildren<TestNode> = VariableChildren::new();
        let debug_str = format!("{:?}", children);
        assert!(debug_str.contains("Children"));
        assert!(debug_str.contains("Variable"));
        assert!(debug_str.contains("Unmounted"));
    }
}
