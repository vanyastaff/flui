//! Protocol-aware children storage for render objects.
//!
//! This module provides two container types for managing child render objects:
//!
//! 1. **[`Children`]** - Simple children without parent data
//! 2. **[`ChildList`]** - Children with per-child parent data
//!
//! # Choosing the Right Container
//!
//! Use **`Children`** for render objects that don't need layout info per child:
//! - `RenderOpacity` - just needs opacity value
//! - `RenderClipRect` - just needs clip shape
//! - `RenderTransform` - just needs transform matrix
//!
//! Use **`ChildList`** for render objects that store layout info per child:
//! - `RenderFlex` - needs flex factor, fit, offset per child
//! - `RenderStack` - needs alignment, offset per child
//! - `RenderWrap` - needs line break info per child
//!
//! # Examples
//!
//! ## Simple single child
//!
//! ```rust,ignore
//! use flui_rendering::containers::BoxChild;
//!
//! pub struct RenderOpacity {
//!     child: BoxChild,  // Children<BoxProtocol, Optional>
//!     opacity: f32,
//! }
//!
//! impl RenderOpacity {
//!     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!         if let Some(child) = self.child.get() {
//!             ctx.push_opacity(self.opacity);
//!             child.paint(ctx, offset);
//!         }
//!     }
//! }
//! ```
//!
//! ## Multiple children with parent data
//!
//! ```rust,ignore
//! use flui_rendering::containers::FlexChildren;
//!
//! pub struct RenderFlex {
//!     children: FlexChildren,  // ChildList<BoxProtocol, Variable, FlexParentData>
//! }
//!
//! impl RenderFlex {
//!     fn layout(&mut self, constraints: BoxConstraints) -> Size {
//!         for (child, data) in self.children.iter_mut() {
//!             // Access flex factor, compute size, store offset
//!             data.offset = computed_offset;
//!         }
//!         total_size
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
//!         self.children.paint_all(offset, |child, child_offset| {
//!             child.paint(ctx, child_offset);
//!         });
//!     }
//! }
//! ```

use ambassador::{delegatable_trait, Delegate};
use flui_tree::arity::storage::ambassador_impl_ChildrenStorage;
use flui_tree::arity::{
    Arity, ArityError, ArityStorage, ChildrenStorage, Exact, Optional, RuntimeArity, Variable,
};
use std::fmt::Debug;
use std::marker::PhantomData;

use crate::parent_data::BoxParentData;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};
use flui_types::Offset;

// ============================================================================
// SingleChildContainer trait - for Ambassador delegation
// ============================================================================

/// Trait for containers that hold a single optional child.
///
/// This trait enables Ambassador delegation for common single-child operations.
/// Implement this trait to get automatic delegation of child access methods.
///
/// # Ambassador Integration
///
/// Use with `#[delegate(SingleChildContainer<P>)]` to delegate child operations:
///
/// ```rust,ignore
/// #[derive(Delegate)]
/// #[delegate(SingleChildContainer<P>, target = "child")]
/// pub struct MyContainer<P: Protocol> {
///     child: Children<P, Optional>,
///     // ... other fields
/// }
/// ```
#[delegatable_trait]
pub trait SingleChildContainer<P: Protocol> {
    /// Returns a reference to the child, if present.
    fn child(&self) -> Option<&P::Object>;

    /// Returns a mutable reference to the child, if present.
    fn child_mut(&mut self) -> Option<&mut P::Object>;

    /// Sets the child, returning the previous child if any.
    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>>;

    /// Takes the child out of the container.
    fn take_child(&mut self) -> Option<Box<P::Object>>;

    /// Returns `true` if the container has a child.
    fn has_child(&self) -> bool;
}

impl<P: Protocol> SingleChildContainer<P> for Children<P, Optional> {
    #[inline]
    fn child(&self) -> Option<&P::Object> {
        self.get()
    }

    #[inline]
    fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.get_mut()
    }

    #[inline]
    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.set(child)
    }

    #[inline]
    fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.take()
    }

    #[inline]
    fn has_child(&self) -> bool {
        !self.is_empty()
    }
}

// ============================================================================
// Children - Simple container without parent data
// ============================================================================

/// Protocol-aware children container without parent data.
///
/// Delegates storage to [`ArityStorage`] via Ambassador pattern.
/// Use this when you don't need per-child layout information.
///
/// # Type Parameters
///
/// - `P` - Protocol marker ([`BoxProtocol`], [`SliverProtocol`])
/// - `A` - Arity constraint ([`Optional`], [`Variable`], [`Exact<N>`])
///
/// # Type Aliases
///
/// For convenience, use the pre-defined type aliases:
/// - [`BoxChild`] - Single optional box child
/// - [`BoxChildren`] - Variable number of box children
/// - [`SliverChild`] - Single optional sliver child
/// - [`SliverChildren`] - Variable number of sliver children
#[derive(Delegate)]
#[delegate(ChildrenStorage<Box<P::Object>>, target = "storage")]
pub struct Children<P: Protocol, A: Arity> {
    storage: ArityStorage<Box<P::Object>, A>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol, A: Arity> Debug for Children<P, A>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.child_count())
            .finish()
    }
}

impl<P: Protocol, A: Arity> Default for Children<P, A> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, A: Arity> Children<P, A> {
    /// Creates an empty container.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            storage: ArityStorage::new(),
            _protocol: PhantomData,
        }
    }

    /// Creates a container with pre-allocated capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: ArityStorage::with_capacity(capacity),
            _protocol: PhantomData,
        }
    }

    /// Creates a container with a single child.
    #[inline]
    #[must_use]
    pub fn with_child(child: Box<P::Object>) -> Self {
        let mut container = Self::new();
        container.set(child);
        container
    }

    // ========================================================================
    // Single child API (for Optional/Exact<1> arities)
    // ========================================================================

    /// Returns a reference to the child.
    #[inline]
    pub fn get(&self) -> Option<&P::Object> {
        self.single_child().map(|b| b.as_ref())
    }

    /// Returns a mutable reference to the child.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut P::Object> {
        self.single_child_mut().map(|b| b.as_mut())
    }

    /// Sets the child, returning the previous child if any.
    #[inline]
    pub fn set(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.set_single_child(child).ok().flatten()
    }

    /// Takes the child out of the container.
    #[inline]
    pub fn take(&mut self) -> Option<Box<P::Object>> {
        self.take_single_child()
    }

    /// Returns `true` if the container has at least one child.
    #[inline]
    #[must_use]
    pub fn has_child(&self) -> bool {
        !self.is_empty()
    }

    /// Removes all children.
    #[inline]
    pub fn clear(&mut self) {
        let _ = self.clear_children();
    }

    // ========================================================================
    // Multi-child API (for Variable arity)
    // ========================================================================

    /// Returns the number of children.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.child_count()
    }

    /// Returns a reference to the child at the given index.
    #[inline]
    pub fn get_at(&self, index: usize) -> Option<&P::Object> {
        self.get_child(index).map(|b| b.as_ref())
    }

    /// Returns a mutable reference to the child at the given index.
    #[inline]
    pub fn get_at_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.get_child_mut(index).map(|b| b.as_mut())
    }

    /// Returns an iterator over the children.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> {
        self.children_slice().iter().map(|b| b.as_ref())
    }

    /// Returns a mutable iterator over the children.
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.children_slice_mut().iter_mut().map(|b| b.as_mut())
    }
}

// ============================================================================
// Type aliases for Children
// ============================================================================

/// Single optional child (generic over protocol).
pub type Single<P> = Children<P, Optional>;

/// Single optional box child.
pub type BoxChild = Children<BoxProtocol, Optional>;

/// Single required box child.
pub type BoxChildRequired = Children<BoxProtocol, Exact<1>>;

/// Variable number of box children.
pub type BoxChildren = Children<BoxProtocol, Variable>;

/// Single optional sliver child.
pub type SliverChild = Children<SliverProtocol, Optional>;

/// Single required sliver child.
pub type SliverChildRequired = Children<SliverProtocol, Exact<1>>;

/// Variable number of sliver children.
pub type SliverChildren = Children<SliverProtocol, Variable>;

// ============================================================================
// ChildNode - Entry storing child + parent data
// ============================================================================

/// A child paired with its parent data.
///
/// Used internally by [`ChildList`] to store children alongside
/// their layout metadata.
#[derive(Debug)]
pub struct ChildNode<P: Protocol, D: Send + Sync> {
    /// The child render object.
    pub child: Box<P::Object>,
    /// Parent data for this child (offset, flex, alignment, etc.).
    pub data: D,
}

impl<P: Protocol, D: Send + Sync> ChildNode<P, D> {
    /// Creates a new child node.
    #[inline]
    pub fn new(child: Box<P::Object>, data: D) -> Self {
        Self { child, data }
    }
}

impl<P: Protocol, D: Default + Send + Sync> ChildNode<P, D> {
    /// Creates a new child node with default parent data.
    #[inline]
    pub fn with_default(child: Box<P::Object>) -> Self {
        Self {
            child,
            data: D::default(),
        }
    }
}

// ============================================================================
// ChildList - Container with parent data per child
// ============================================================================

/// Children container with per-child parent data.
///
/// Stores children alongside their layout metadata (parent data).
/// Provides methods for iteration, painting, and hit testing.
///
/// # Type Parameters
///
/// - `P` - Protocol marker ([`BoxProtocol`], [`SliverProtocol`])
/// - `A` - Arity constraint (typically [`Variable`])
/// - `D` - Parent data type (`FlexParentData`, `StackParentData`, etc.)
///
/// # Type Aliases
///
/// For convenience, use the pre-defined type aliases:
/// - [`FlexChildren`] - For flex layouts
/// - [`StackChildren`] - For stack layouts
/// - [`WrapChildren`] - For wrap layouts
pub struct ChildList<P: Protocol, A: Arity = Variable, D: Send + Sync = BoxParentData> {
    storage: ArityStorage<ChildNode<P, D>, A>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol, A: Arity, D: Send + Sync + Debug> Debug for ChildList<P, A, D>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildList")
            .field("count", &self.storage.child_count())
            .finish()
    }
}

impl<P: Protocol, A: Arity, D: Default + Send + Sync> Default for ChildList<P, A, D> {
    fn default() -> Self {
        Self::new()
    }
}

// Methods requiring D: Default
impl<P: Protocol, A: Arity, D: Default + Send + Sync> ChildList<P, A, D> {
    /// Creates an empty container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            storage: ArityStorage::new(),
            _protocol: PhantomData,
        }
    }

    /// Creates a container with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: ArityStorage::with_capacity(capacity),
            _protocol: PhantomData,
        }
    }

    /// Adds a child with default parent data.
    pub fn push(&mut self, child: Box<P::Object>) {
        let _ = self.storage.add_child(ChildNode::with_default(child));
    }

    /// Inserts a child with default parent data at the given index.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) -> Result<(), ArityError> {
        self.storage
            .insert_child(index, ChildNode::with_default(child))
    }
}

// Methods not requiring D: Default
impl<P: Protocol, A: Arity, D: Send + Sync> ChildList<P, A, D> {
    /// Creates an empty container (does not require `D: Default`).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            storage: ArityStorage::new(),
            _protocol: PhantomData,
        }
    }

    /// Returns the number of children.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.storage.child_count()
    }

    /// Returns `true` if the container is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    // ========================================================================
    // Child access (without parent data)
    // ========================================================================

    /// Returns a reference to the child at the given index.
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.storage.get_child(index).map(|n| n.child.as_ref())
    }

    /// Returns a mutable reference to the child at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.storage.get_child_mut(index).map(|n| n.child.as_mut())
    }

    /// Returns the first child.
    pub fn first(&self) -> Option<&P::Object> {
        self.storage
            .children_slice()
            .first()
            .map(|n| n.child.as_ref())
    }

    /// Returns the last child.
    pub fn last(&self) -> Option<&P::Object> {
        self.storage
            .children_slice()
            .last()
            .map(|n| n.child.as_ref())
    }

    /// Returns an iterator over the children (without parent data).
    pub fn children(&self) -> impl Iterator<Item = &P::Object> {
        self.storage
            .children_slice()
            .iter()
            .map(|n| n.child.as_ref())
    }

    /// Returns a mutable iterator over the children (without parent data).
    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.storage
            .children_slice_mut()
            .iter_mut()
            .map(|n| n.child.as_mut())
    }

    // ========================================================================
    // Parent data access
    // ========================================================================

    /// Returns a reference to the parent data at the given index.
    pub fn data(&self, index: usize) -> Option<&D> {
        self.storage.get_child(index).map(|n| &n.data)
    }

    /// Returns a mutable reference to the parent data at the given index.
    pub fn data_mut(&mut self, index: usize) -> Option<&mut D> {
        self.storage.get_child_mut(index).map(|n| &mut n.data)
    }

    // ========================================================================
    // Combined access (child + parent data)
    // ========================================================================

    /// Returns references to both child and parent data at the given index.
    pub fn get_with_data(&self, index: usize) -> Option<(&P::Object, &D)> {
        self.storage
            .get_child(index)
            .map(|n| (n.child.as_ref(), &n.data))
    }

    /// Returns mutable references to both child and parent data.
    pub fn get_with_data_mut(&mut self, index: usize) -> Option<(&mut P::Object, &mut D)> {
        self.storage
            .get_child_mut(index)
            .map(|n| (n.child.as_mut(), &mut n.data))
    }

    /// Returns an iterator over (child, data) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&P::Object, &D)> {
        self.storage
            .children_slice()
            .iter()
            .map(|n| (n.child.as_ref(), &n.data))
    }

    /// Returns a mutable iterator over (child, data) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&mut P::Object, &mut D)> {
        self.storage
            .children_slice_mut()
            .iter_mut()
            .map(|n| (n.child.as_mut(), &mut n.data))
    }

    /// Returns a reverse iterator over (child, data) pairs.
    ///
    /// Use this for hit testing (test topmost/last-painted child first).
    pub fn iter_rev(&self) -> impl Iterator<Item = (&P::Object, &D)> {
        self.storage
            .children_slice()
            .iter()
            .rev()
            .map(|n| (n.child.as_ref(), &n.data))
    }

    // ========================================================================
    // Modification with explicit parent data
    // ========================================================================

    /// Adds a child with the given parent data.
    pub fn push_with(&mut self, child: Box<P::Object>, data: D) {
        let _ = self.storage.add_child(ChildNode::new(child, data));
    }

    /// Inserts a child with the given parent data at the specified index.
    pub fn insert_with(
        &mut self,
        index: usize,
        child: Box<P::Object>,
        data: D,
    ) -> Result<(), ArityError> {
        self.storage
            .insert_child(index, ChildNode::new(child, data))
    }

    /// Removes and returns the child node at the given index.
    pub fn remove(&mut self, index: usize) -> Option<ChildNode<P, D>> {
        self.storage.remove_child(index)
    }

    /// Removes and returns just the child at the given index.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<P::Object>> {
        self.storage.remove_child(index).map(|n| n.child)
    }

    /// Removes and returns the last child node.
    pub fn pop(&mut self) -> Option<ChildNode<P, D>> {
        self.storage.pop_child()
    }

    /// Removes all children.
    pub fn clear(&mut self) {
        let _ = self.storage.clear_children();
    }

    // ========================================================================
    // Visitor callbacks
    // ========================================================================

    /// Calls a closure for each (child, data) pair.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &D),
    {
        for node in self.storage.children_slice() {
            f(node.child.as_ref(), &node.data);
        }
    }

    /// Calls a closure for each (child, data) pair with mutable access.
    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut P::Object, &mut D),
    {
        for node in self.storage.children_slice_mut() {
            f(node.child.as_mut(), &mut node.data);
        }
    }

    /// Calls a closure for each (child, data) pair in reverse order.
    ///
    /// Use this for hit testing (test topmost/last-painted child first).
    pub fn for_each_rev<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &D),
    {
        for node in self.storage.children_slice().iter().rev() {
            f(node.child.as_ref(), &node.data);
        }
    }
}

// ============================================================================
// Type aliases for ChildList
// ============================================================================

/// Children with flex parent data.
pub type FlexChildren = ChildList<BoxProtocol, Variable, crate::parent_data::FlexParentData>;

/// Children with stack parent data.
pub type StackChildren = ChildList<BoxProtocol, Variable, crate::parent_data::StackParentData>;

/// Children with wrap parent data.
pub type WrapChildren = ChildList<BoxProtocol, Variable, crate::parent_data::WrapParentData>;

/// Box children with custom parent data.
pub type BoxChildList<D = BoxParentData> = ChildList<BoxProtocol, Variable, D>;

// ============================================================================
// Paint & Hit Test Helpers
// ============================================================================

/// Trait for parent data that contains an offset.
///
/// Implement this for your parent data type to enable
/// [`paint_all`](ChildList::paint_all) and [`hit_test_all`](ChildList::hit_test_all).
pub trait HasOffset {
    /// Returns the paint offset for this child.
    fn offset(&self) -> Offset;
}

impl HasOffset for BoxParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl<A: Arity, D: HasOffset + Send + Sync> ChildList<BoxProtocol, A, D> {
    /// Paints all children using their parent data offsets.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
    ///     self.children.paint_all(offset, |child, child_offset| {
    ///         child.paint(ctx, child_offset);
    ///     });
    /// }
    /// ```
    pub fn paint_all<F>(&self, base_offset: Offset, mut paint_child: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        for (child, data) in self.iter() {
            paint_child(child, base_offset + data.offset());
        }
    }

    /// Hit tests all children in reverse order (front to back).
    ///
    /// Returns `true` if any child was hit.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
    ///     self.children.hit_test_all(result, position)
    /// }
    /// ```
    pub fn hit_test_all(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        for (child, data) in self.iter_rev() {
            let hit = result.add_with_paint_offset(
                Some(data.offset()),
                position,
                |result, transformed| child.hit_test(result, transformed),
            );
            if hit {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_children_default() {
        let container: BoxChild = BoxChild::new();
        assert!(!container.has_child());
        assert_eq!(container.len(), 0);
    }

    #[test]
    fn test_child_list_default() {
        let list: BoxChildList = ChildList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
    }
}
