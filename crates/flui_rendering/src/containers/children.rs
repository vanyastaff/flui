//! Type-safe children storage for render objects.
//!
//! This module provides Arity-based containers for managing child render objects
//! with compile-time guarantees about child count.
//!
//! # Architecture
//!
//! ```text
//! Arity System (compile-time guarantees)
//! ├── Leaf        → 0 children      (RenderColoredBox, RenderImage)
//! ├── Optional    → 0 or 1 child    (RenderPadding, RenderOpacity)
//! ├── Exact<1>    → exactly 1       (RenderConstrainedBox)
//! ├── Exact<N>    → exactly N       (RenderSplitView)
//! └── Variable    → 0..N children   (RenderFlex, RenderStack)
//! ```
//!
//! # Container Types
//!
//! | Container | Use Case | Example |
//! |-----------|----------|---------|
//! | [`Child`] | Single child without parentData | `RenderOpacity`, `RenderClipRect` |
//! | [`ChildList`] | Multiple children with parentData | `RenderFlex`, `RenderStack` |
//!
//! # Quick Reference
//!
//! ```rust,ignore
//! // Single optional child (most common for proxy-like objects)
//! struct RenderOpacity {
//!     child: BoxChild,  // = Child<BoxProtocol, Optional>
//!     opacity: f32,
//! }
//!
//! // Multiple children with layout data
//! struct RenderFlex {
//!     children: FlexChildren,  // = ChildList<BoxProtocol, Variable, FlexParentData>
//!     direction: Axis,
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
// Child - Type-safe single/optional child container
// ============================================================================

/// Type-safe child container with Arity-based compile-time guarantees.
///
/// This is the primary container for render objects that have a single child
/// (or optionally no child). The Arity parameter enforces child count at compile time.
///
/// # Type Parameters
///
/// - `P` - Protocol ([`BoxProtocol`] or [`SliverProtocol`])
/// - `A` - Arity constraint ([`Optional`], [`Exact<1>`], [`Leaf`])
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::containers::{BoxChild, BoxChildRequired};
///
/// // Optional child (0 or 1)
/// struct RenderOpacity {
///     child: BoxChild,  // Child<BoxProtocol, Optional>
/// }
///
/// // Required child (exactly 1)
/// struct RenderConstrainedBox {
///     child: BoxChildRequired,  // Child<BoxProtocol, Exact<1>>
/// }
/// ```
#[derive(Delegate)]
#[delegate(ChildrenStorage<Box<P::Object>>, target = "storage")]
pub struct Child<P: Protocol, A: Arity = Optional> {
    storage: ArityStorage<Box<P::Object>, A>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol, A: Arity> Debug for Child<P, A>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("has_child", &self.has_child())
            .finish()
    }
}

impl<P: Protocol, A: Arity> Default for Child<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, A: Arity> Child<P, A> {
    /// Creates an empty container.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            storage: ArityStorage::new(),
            _protocol: PhantomData,
        }
    }

    /// Creates a container with the given child.
    #[inline]
    #[must_use]
    pub fn with(child: Box<P::Object>) -> Self {
        let mut container = Self::new();
        let _ = container.storage.set_single_child(child);
        container
    }

    /// Returns the child as a trait object reference.
    #[inline]
    pub fn get(&self) -> Option<&P::Object> {
        self.storage.single_child().map(|b| b.as_ref())
    }

    /// Returns the child as a mutable trait object reference.
    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut P::Object> {
        self.storage.single_child_mut().map(|b| b.as_mut())
    }

    /// Returns the child as a boxed reference.
    #[inline]
    pub fn get_boxed(&self) -> Option<&Box<P::Object>> {
        self.storage.single_child()
    }

    /// Returns the child as a mutable boxed reference.
    #[inline]
    pub fn get_boxed_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.storage.single_child_mut()
    }

    /// Sets the child, returning the previous child if any.
    #[inline]
    pub fn set(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.storage.set_single_child(child).ok().flatten()
    }

    /// Takes the child out of the container.
    #[inline]
    pub fn take(&mut self) -> Option<Box<P::Object>> {
        self.storage.take_single_child()
    }

    /// Returns `true` if the container has a child.
    #[inline]
    #[must_use]
    pub fn has_child(&self) -> bool {
        !self.storage.is_empty()
    }

    /// Clears the child.
    #[inline]
    pub fn clear(&mut self) {
        let _ = self.storage.clear_children();
    }
}

// ============================================================================
// Type Aliases - Single Child
// ============================================================================

/// Single optional Box child (0 or 1).
///
/// Use for render objects that may or may not have a child.
///
/// ```rust,ignore
/// struct RenderOpacity {
///     child: BoxChild,
///     opacity: f32,
/// }
/// ```
pub type BoxChild = Child<BoxProtocol, Optional>;

/// Single required Box child (exactly 1).
///
/// Use for render objects that must have exactly one child.
///
/// ```rust,ignore
/// struct RenderConstrainedBox {
///     child: BoxChildRequired,
///     constraints: BoxConstraints,
/// }
/// ```
pub type BoxChildRequired = Child<BoxProtocol, Exact<1>>;

/// Single optional Sliver child (0 or 1).
pub type SliverChild = Child<SliverProtocol, Optional>;

/// Single required Sliver child (exactly 1).
pub type SliverChildRequired = Child<SliverProtocol, Exact<1>>;

// ============================================================================
// ChildList - Multiple children with parent data
// ============================================================================

/// A child entry with associated parent data.
///
/// Parent data stores layout information like offset, flex factor, etc.
/// This is set by the parent during layout and read during paint/hit-test.
#[derive(Debug)]
pub struct ChildEntry<P: Protocol, D: Send + Sync> {
    /// The child render object.
    pub child: Box<P::Object>,
    /// Parent data for this child.
    pub data: D,
}

impl<P: Protocol, D: Send + Sync> ChildEntry<P, D> {
    /// Creates a new entry with the given child and data.
    #[inline]
    pub fn new(child: Box<P::Object>, data: D) -> Self {
        Self { child, data }
    }
}

impl<P: Protocol, D: Default + Send + Sync> ChildEntry<P, D> {
    /// Creates a new entry with default parent data.
    #[inline]
    pub fn with_default_data(child: Box<P::Object>) -> Self {
        Self {
            child,
            data: D::default(),
        }
    }
}

/// Multiple children container with per-child parent data.
///
/// This is the container for render objects with variable children count,
/// where each child has associated layout data (offset, flex factor, etc.).
///
/// # Type Parameters
///
/// - `P` - Protocol ([`BoxProtocol`] or [`SliverProtocol`])
/// - `A` - Arity constraint (typically [`Variable`])
/// - `D` - Parent data type (`FlexParentData`, `StackParentData`, etc.)
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::containers::FlexChildren;
///
/// struct RenderFlex {
///     children: FlexChildren,
///     direction: Axis,
/// }
///
/// impl RenderFlex {
///     fn layout(&mut self) {
///         for (child, data) in self.children.iter_mut() {
///             // Layout child, then set offset in data
///             data.offset = computed_offset;
///         }
///     }
/// }
/// ```
pub struct ChildList<P: Protocol, A: Arity = Variable, D: Send + Sync = BoxParentData> {
    storage: ArityStorage<ChildEntry<P, D>, A>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol, A: Arity, D: Send + Sync + Debug> Debug for ChildList<P, A, D>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildList")
            .field("len", &self.len())
            .finish()
    }
}

impl<P: Protocol, A: Arity, D: Default + Send + Sync> Default for ChildList<P, A, D> {
    fn default() -> Self {
        Self::new()
    }
}

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
        let _ = self.storage.add_child(ChildEntry::with_default_data(child));
    }

    /// Inserts a child with default parent data at the given index.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) -> Result<(), ArityError> {
        self.storage
            .insert_child(index, ChildEntry::with_default_data(child))
    }
}

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
    // Child Access
    // ========================================================================

    /// Returns the child at the given index.
    pub fn child(&self, index: usize) -> Option<&P::Object> {
        self.storage.get_child(index).map(|e| e.child.as_ref())
    }

    /// Returns a mutable reference to the child at the given index.
    pub fn child_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.storage.get_child_mut(index).map(|e| e.child.as_mut())
    }

    /// Returns the first child.
    pub fn first(&self) -> Option<&P::Object> {
        self.storage
            .children_slice()
            .first()
            .map(|e| e.child.as_ref())
    }

    /// Returns the last child.
    pub fn last(&self) -> Option<&P::Object> {
        self.storage
            .children_slice()
            .last()
            .map(|e| e.child.as_ref())
    }

    // ========================================================================
    // Parent Data Access
    // ========================================================================

    /// Returns the parent data at the given index.
    pub fn data(&self, index: usize) -> Option<&D> {
        self.storage.get_child(index).map(|e| &e.data)
    }

    /// Returns a mutable reference to the parent data at the given index.
    pub fn data_mut(&mut self, index: usize) -> Option<&mut D> {
        self.storage.get_child_mut(index).map(|e| &mut e.data)
    }

    // ========================================================================
    // Combined Access
    // ========================================================================

    /// Returns both child and data at the given index.
    pub fn get(&self, index: usize) -> Option<(&P::Object, &D)> {
        self.storage
            .get_child(index)
            .map(|e| (e.child.as_ref(), &e.data))
    }

    /// Returns mutable references to both child and data at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<(&mut P::Object, &mut D)> {
        self.storage
            .get_child_mut(index)
            .map(|e| (e.child.as_mut(), &mut e.data))
    }

    /// Alias for [`get`] - returns both child and data.
    #[inline]
    pub fn get_with_data(&self, index: usize) -> Option<(&P::Object, &D)> {
        self.get(index)
    }

    /// Alias for [`get_mut`] - returns mutable child and data.
    #[inline]
    pub fn get_with_data_mut(&mut self, index: usize) -> Option<(&mut P::Object, &mut D)> {
        self.get_mut(index)
    }

    // ========================================================================
    // Iteration
    // ========================================================================

    /// Iterates over (child, data) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&P::Object, &D)> {
        self.storage
            .children_slice()
            .iter()
            .map(|e| (e.child.as_ref(), &e.data))
    }

    /// Iterates mutably over (child, data) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&mut P::Object, &mut D)> {
        self.storage
            .children_slice_mut()
            .iter_mut()
            .map(|e| (e.child.as_mut(), &mut e.data))
    }

    /// Iterates over children only.
    pub fn children(&self) -> impl Iterator<Item = &P::Object> {
        self.storage
            .children_slice()
            .iter()
            .map(|e| e.child.as_ref())
    }

    /// Iterates mutably over children only.
    pub fn children_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.storage
            .children_slice_mut()
            .iter_mut()
            .map(|e| e.child.as_mut())
    }

    /// Iterates in reverse order (for hit testing - front to back).
    pub fn iter_rev(&self) -> impl Iterator<Item = (&P::Object, &D)> {
        self.storage
            .children_slice()
            .iter()
            .rev()
            .map(|e| (e.child.as_ref(), &e.data))
    }

    // ========================================================================
    // Modification
    // ========================================================================

    /// Adds a child with the given parent data.
    pub fn push_with(&mut self, child: Box<P::Object>, data: D) {
        let _ = self.storage.add_child(ChildEntry::new(child, data));
    }

    /// Inserts a child with data at the specified index.
    pub fn insert_with(
        &mut self,
        index: usize,
        child: Box<P::Object>,
        data: D,
    ) -> Result<(), ArityError> {
        self.storage
            .insert_child(index, ChildEntry::new(child, data))
    }

    /// Removes and returns the entry at the given index.
    pub fn remove(&mut self, index: usize) -> Option<ChildEntry<P, D>> {
        self.storage.remove_child(index)
    }

    /// Removes and returns only the child at the given index.
    pub fn remove_child(&mut self, index: usize) -> Option<Box<P::Object>> {
        self.storage.remove_child(index).map(|e| e.child)
    }

    /// Removes and returns the last entry.
    pub fn pop(&mut self) -> Option<ChildEntry<P, D>> {
        self.storage.pop_child()
    }

    /// Removes all children.
    pub fn clear(&mut self) {
        let _ = self.storage.clear_children();
    }

    // ========================================================================
    // Visitor Callbacks
    // ========================================================================

    /// Calls a closure for each (child, data) pair.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &D),
    {
        for entry in self.storage.children_slice() {
            f(entry.child.as_ref(), &entry.data);
        }
    }

    /// Calls a closure for each (child, data) pair mutably.
    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut P::Object, &mut D),
    {
        for entry in self.storage.children_slice_mut() {
            f(entry.child.as_mut(), &mut entry.data);
        }
    }

    /// Calls a closure for each pair in reverse order.
    pub fn for_each_rev<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &D),
    {
        for entry in self.storage.children_slice().iter().rev() {
            f(entry.child.as_ref(), &entry.data);
        }
    }
}

// ============================================================================
// Type Aliases - Multiple Children
// ============================================================================

/// Variable number of Box children (no parent data).
pub type BoxChildren = ChildList<BoxProtocol, Variable, ()>;

/// Variable number of Sliver children (no parent data).
pub type SliverChildren = ChildList<SliverProtocol, Variable, ()>;

/// Flex layout children with FlexParentData.
pub type FlexChildren = ChildList<BoxProtocol, Variable, crate::parent_data::FlexParentData>;

/// Stack layout children with StackParentData.
pub type StackChildren = ChildList<BoxProtocol, Variable, crate::parent_data::StackParentData>;

/// Wrap layout children with WrapParentData.
pub type WrapChildren = ChildList<BoxProtocol, Variable, crate::parent_data::WrapParentData>;

/// Generic Box children with custom parent data.
pub type BoxChildList<D = BoxParentData> = ChildList<BoxProtocol, Variable, D>;

/// Generic Sliver children with custom parent data.
pub type SliverChildList<D = crate::parent_data::SliverPhysicalParentData> =
    ChildList<SliverProtocol, Variable, D>;

// ============================================================================
// Paint & Hit Test Helpers
// ============================================================================

/// Trait for parent data that contains a paint offset.
///
/// Implement this to enable [`ChildList::paint_all`] and [`ChildList::hit_test_all`].
pub trait HasOffset {
    /// Returns the paint offset for this child.
    fn offset(&self) -> Offset;
}

impl HasOffset for BoxParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::FlexParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::StackParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::WrapParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl<A: Arity, D: HasOffset + Send + Sync> ChildList<BoxProtocol, A, D> {
    /// Paints all children at their parent data offsets.
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
// Delegation Traits (for Ambassador)
// ============================================================================

/// Trait for single-child containers (used by Ambassador delegation).
#[delegatable_trait]
pub trait SingleChildContainer<T> {
    /// Returns the child if present.
    fn child(&self) -> Option<&T>;
    /// Returns mutable child if present.
    fn child_mut(&mut self) -> Option<&mut T>;
    /// Sets the child.
    fn set_child(&mut self, child: T) -> Option<T>;
    /// Takes the child.
    fn take_child(&mut self) -> Option<T>;
    /// Returns true if has child.
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}

impl<P: Protocol> SingleChildContainer<Box<P::Object>> for Child<P, Optional> {
    fn child(&self) -> Option<&Box<P::Object>> {
        self.get_boxed()
    }

    fn child_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.get_boxed_mut()
    }

    fn set_child(&mut self, child: Box<P::Object>) -> Option<Box<P::Object>> {
        self.set(child)
    }

    fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.take()
    }
}

/// Trait for multi-child containers (used by Ambassador delegation).
#[delegatable_trait]
pub trait MultiChildContainer<T> {
    /// Returns the number of children.
    fn len(&self) -> usize;
    /// Returns true if empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Returns child at index.
    fn get(&self, index: usize) -> Option<&T>;
    /// Returns mutable child at index.
    fn get_mut(&mut self, index: usize) -> Option<&mut T>;
    /// Adds a child.
    fn push(&mut self, child: T);
    /// Removes child at index.
    fn remove(&mut self, index: usize) -> Option<T>;
    /// Clears all children.
    fn clear(&mut self);
}

/// Trait for multi-child containers with parent data.
#[delegatable_trait]
pub trait MultiChildContainerWithData<T, D>: MultiChildContainer<T> {
    /// Returns data at index.
    fn data(&self, index: usize) -> Option<&D>;
    /// Returns mutable data at index.
    fn data_mut(&mut self, index: usize) -> Option<&mut D>;
    /// Returns both child and data.
    fn get_with_data(&self, index: usize) -> Option<(&T, &D)>;
    /// Returns mutable child and data.
    fn get_with_data_mut(&mut self, index: usize) -> Option<(&mut T, &mut D)>;
    /// Adds child with data.
    fn push_with_data(&mut self, child: T, data: D);
}

impl<P: Protocol, A: Arity, D: Default + Send + Sync> MultiChildContainer<Box<P::Object>>
    for ChildList<P, A, D>
{
    fn len(&self) -> usize {
        ChildList::len(self)
    }

    fn get(&self, index: usize) -> Option<&Box<P::Object>> {
        self.storage.get_child(index).map(|e| &e.child)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut Box<P::Object>> {
        self.storage.get_child_mut(index).map(|e| &mut e.child)
    }

    fn push(&mut self, child: Box<P::Object>) {
        ChildList::push(self, child);
    }

    fn remove(&mut self, index: usize) -> Option<Box<P::Object>> {
        ChildList::remove_child(self, index)
    }

    fn clear(&mut self) {
        ChildList::clear(self);
    }
}

impl<P: Protocol, A: Arity, D: Default + Send + Sync> MultiChildContainerWithData<Box<P::Object>, D>
    for ChildList<P, A, D>
{
    fn data(&self, index: usize) -> Option<&D> {
        ChildList::data(self, index)
    }

    fn data_mut(&mut self, index: usize) -> Option<&mut D> {
        ChildList::data_mut(self, index)
    }

    fn get_with_data(&self, index: usize) -> Option<(&Box<P::Object>, &D)> {
        self.storage.get_child(index).map(|e| (&e.child, &e.data))
    }

    fn get_with_data_mut(&mut self, index: usize) -> Option<(&mut Box<P::Object>, &mut D)> {
        self.storage
            .get_child_mut(index)
            .map(|e| (&mut e.child, &mut e.data))
    }

    fn push_with_data(&mut self, child: Box<P::Object>, data: D) {
        self.push_with(child, data);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::RenderSliver;

    #[test]
    fn test_box_child_default() {
        let child: BoxChild = BoxChild::new();
        assert!(!child.has_child());
    }

    #[test]
    fn test_child_list_default() {
        let list: FlexChildren = ChildList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_box_child_type_alias() {
        let _: BoxChild = Child::new();
        let _: BoxChildRequired = Child::new();
    }

    #[test]
    fn test_sliver_child_type_alias() {
        let _: SliverChild = Child::new();
        let _: SliverChildRequired = Child::new();
    }

    #[test]
    fn test_child_list_type_aliases() {
        let _: FlexChildren = ChildList::new();
        let _: StackChildren = ChildList::new();
        let _: WrapChildren = ChildList::new();
    }

    #[test]
    fn test_single_child_container_trait() {
        fn accepts_single<T, C: SingleChildContainer<T>>(c: &C) -> bool {
            c.has_child()
        }

        let child: BoxChild = Child::new();
        assert!(!accepts_single::<Box<dyn RenderBox>, _>(&child));
    }

    #[test]
    fn test_multi_child_container_trait() {
        fn accepts_multi<T, C: MultiChildContainer<T>>(c: &C) -> usize {
            c.len()
        }

        let list: FlexChildren = ChildList::new();
        assert_eq!(accepts_multi::<Box<dyn RenderBox>, _>(&list), 0);
    }
}
