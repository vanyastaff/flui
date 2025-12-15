//! Multiple children container with parent data support.
//!
//! This module provides the [`Children`] container which stores multiple
//! render object children along with their parent data.
//!
//! # Parent Data
//!
//! Parent data is metadata that a parent render object stores for each child.
//! Common examples include:
//! - `BoxParentData`: stores the child's paint offset
//! - `FlexParentData`: stores flex factor and fit mode
//! - `StackParentData`: stores positioning constraints
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::containers::Children;
//! use flui_rendering::parent_data::FlexParentData;
//! use flui_rendering::protocol::BoxProtocol;
//!
//! struct RenderFlex {
//!     children: Children<BoxProtocol, FlexParentData>,
//! }
//!
//! impl RenderFlex {
//!     fn layout(&mut self) {
//!         // Iterate with parent data
//!         for (child, data) in self.children.iter_with_data_mut() {
//!             if let Some(flex) = data.flex {
//!                 // Layout flexible child
//!             }
//!         }
//!     }
//! }
//! ```

use crate::parent_data::BoxParentData;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::traits::{BoxHitTestResult, RenderBox};
use flui_types::Offset;
use std::fmt::Debug;
use std::marker::PhantomData;

/// A child entry storing both the render object and its parent data.
#[derive(Debug)]
pub struct ChildEntry<P: Protocol, PD> {
    /// The child render object.
    pub child: Box<P::Object>,
    /// Parent data associated with this child.
    pub data: PD,
}

impl<P: Protocol, PD> ChildEntry<P, PD> {
    /// Creates a new child entry with the given child and parent data.
    pub fn new(child: Box<P::Object>, data: PD) -> Self {
        Self { child, data }
    }
}

impl<P: Protocol, PD: Default> ChildEntry<P, PD> {
    /// Creates a new child entry with default parent data.
    pub fn with_default_data(child: Box<P::Object>) -> Self {
        Self {
            child,
            data: PD::default(),
        }
    }
}

/// Container that stores multiple children of a protocol's object type.
///
/// Each child is stored alongside its parent data, allowing efficient
/// access to both during layout and painting.
///
/// # Type Safety
///
/// Uses `Protocol::Object` to ensure type-safe child storage at compile time.
/// The `PD` parameter specifies the parent data type for metadata storage.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `ContainerRenderObjectMixin` combined with
/// `ContainerParentDataMixin`. In Flutter, parent data is stored on the child
/// render object itself. In FLUI, we store it alongside the child for cleaner
/// ownership semantics.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderFlex {
///     children: BoxChildren<FlexParentData>,
///     direction: Axis,
/// }
///
/// impl RenderFlex {
///     fn layout_children(&mut self, constraints: BoxConstraints) {
///         for (child, data) in self.children.iter_with_data_mut() {
///             // child is &mut dyn RenderBox
///             // data is &mut FlexParentData
///             let size = child.perform_layout(child_constraints);
///             data.offset = computed_offset;
///         }
///     }
/// }
/// ```
pub struct Children<P: Protocol, PD = <P as Protocol>::ParentData> {
    entries: Vec<ChildEntry<P, PD>>,
    _phantom: PhantomData<P>,
}

impl<P: Protocol, PD> Debug for Children<P, PD>
where
    P::Object: Debug,
    PD: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.entries.len())
            .finish()
    }
}

impl<P: Protocol, PD: Default> Default for Children<P, PD> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, PD: Default> Children<P, PD> {
    /// Creates a new empty children container.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Creates a children container with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            _phantom: PhantomData,
        }
    }

    /// Adds a child with default parent data to the end of the container.
    pub fn push(&mut self, child: Box<P::Object>) {
        self.entries.push(ChildEntry::with_default_data(child));
    }

    /// Inserts a child with default parent data at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.entries
            .insert(index, ChildEntry::with_default_data(child));
    }
}

impl<P: Protocol, PD> Children<P, PD> {
    /// Creates a new empty children container without requiring Default for PD.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Returns the number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the container has no children.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // ========================================================================
    // Child-only access (for backward compatibility)
    // ========================================================================

    /// Returns a reference to the child at the given index.
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.entries.get(index).map(|e| &*e.child)
    }

    /// Returns a mutable reference to the child at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.entries.get_mut(index).map(|e| &mut *e.child)
    }

    /// Returns the first child, if any.
    pub fn first(&self) -> Option<&P::Object> {
        self.entries.first().map(|e| &*e.child)
    }

    /// Returns a mutable reference to the first child, if any.
    pub fn first_mut(&mut self) -> Option<&mut P::Object> {
        self.entries.first_mut().map(|e| &mut *e.child)
    }

    /// Returns the last child, if any.
    pub fn last(&self) -> Option<&P::Object> {
        self.entries.last().map(|e| &*e.child)
    }

    /// Returns a mutable reference to the last child, if any.
    pub fn last_mut(&mut self) -> Option<&mut P::Object> {
        self.entries.last_mut().map(|e| &mut *e.child)
    }

    /// Returns an iterator over the children (without parent data).
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> {
        self.entries.iter().map(|e| &*e.child)
    }

    /// Returns a mutable iterator over the children (without parent data).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.entries.iter_mut().map(|e| &mut *e.child)
    }

    // ========================================================================
    // Parent data access
    // ========================================================================

    /// Returns a reference to the parent data at the given index.
    pub fn get_data(&self, index: usize) -> Option<&PD> {
        self.entries.get(index).map(|e| &e.data)
    }

    /// Returns a mutable reference to the parent data at the given index.
    pub fn get_data_mut(&mut self, index: usize) -> Option<&mut PD> {
        self.entries.get_mut(index).map(|e| &mut e.data)
    }

    /// Returns the first child's parent data, if any.
    pub fn first_data(&self) -> Option<&PD> {
        self.entries.first().map(|e| &e.data)
    }

    /// Returns the last child's parent data, if any.
    pub fn last_data(&self) -> Option<&PD> {
        self.entries.last().map(|e| &e.data)
    }

    // ========================================================================
    // Combined child + parent data access
    // ========================================================================

    /// Returns a reference to both child and parent data at the given index.
    pub fn get_with_data(&self, index: usize) -> Option<(&P::Object, &PD)> {
        self.entries.get(index).map(|e| (&*e.child, &e.data))
    }

    /// Returns a mutable reference to both child and parent data at the given index.
    pub fn get_with_data_mut(&mut self, index: usize) -> Option<(&mut P::Object, &mut PD)> {
        self.entries
            .get_mut(index)
            .map(|e| (&mut *e.child, &mut e.data))
    }

    /// Returns an iterator over (child, parent_data) pairs.
    pub fn iter_with_data(&self) -> impl Iterator<Item = (&P::Object, &PD)> {
        self.entries.iter().map(|e| (&*e.child, &e.data))
    }

    /// Returns a mutable iterator over (child, parent_data) pairs.
    pub fn iter_with_data_mut(&mut self) -> impl Iterator<Item = (&mut P::Object, &mut PD)> {
        self.entries
            .iter_mut()
            .map(|e| (&mut *e.child, &mut e.data))
    }

    /// Returns an iterator over child entries.
    pub fn entries(&self) -> impl Iterator<Item = &ChildEntry<P, PD>> {
        self.entries.iter()
    }

    /// Returns a mutable iterator over child entries.
    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut ChildEntry<P, PD>> {
        self.entries.iter_mut()
    }

    // ========================================================================
    // Modification with parent data
    // ========================================================================

    /// Adds a child with specified parent data to the end of the container.
    pub fn push_with_data(&mut self, child: Box<P::Object>, data: PD) {
        self.entries.push(ChildEntry::new(child, data));
    }

    /// Inserts a child with specified parent data at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn insert_with_data(&mut self, index: usize, child: Box<P::Object>, data: PD) {
        self.entries.insert(index, ChildEntry::new(child, data));
    }

    /// Removes and returns the child entry at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn remove(&mut self, index: usize) -> ChildEntry<P, PD> {
        self.entries.remove(index)
    }

    /// Removes and returns just the child at the given index, discarding parent data.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn remove_child(&mut self, index: usize) -> Box<P::Object> {
        self.entries.remove(index).child
    }

    /// Removes the last child entry and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<ChildEntry<P, PD>> {
        self.entries.pop()
    }

    /// Removes the last child and returns just the child, or `None` if empty.
    pub fn pop_child(&mut self) -> Option<Box<P::Object>> {
        self.entries.pop().map(|e| e.child)
    }

    /// Removes all children.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Swaps two children by their indices.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    pub fn swap(&mut self, a: usize, b: usize) {
        self.entries.swap(a, b);
    }

    /// Moves a child from one index to another.
    ///
    /// This is more efficient than removing and re-inserting.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    pub fn move_child(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let entry = self.entries.remove(from);
        let insert_idx = if to > from { to - 1 } else { to };
        self.entries.insert(insert_idx, entry);
    }

    // ========================================================================
    // Enumeration helpers
    // ========================================================================

    /// Returns an iterator with indices: (index, &child, &data).
    pub fn enumerate_with_data(&self) -> impl Iterator<Item = (usize, &P::Object, &PD)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, &*e.child, &e.data))
    }

    /// Returns a mutable iterator with indices: (index, &mut child, &mut data).
    pub fn enumerate_with_data_mut(
        &mut self,
    ) -> impl Iterator<Item = (usize, &mut P::Object, &mut PD)> {
        self.entries
            .iter_mut()
            .enumerate()
            .map(|(i, e)| (i, &mut *e.child, &mut e.data))
    }

    // ========================================================================
    // Reverse iteration (for hit testing - front to back)
    // ========================================================================

    /// Returns a reverse iterator over children.
    pub fn iter_rev(&self) -> impl Iterator<Item = &P::Object> {
        self.entries.iter().rev().map(|e| &*e.child)
    }

    /// Returns a reverse iterator over (child, parent_data) pairs.
    ///
    /// Useful for hit testing where you need to test front-to-back
    /// (last painted child first).
    pub fn iter_with_data_rev(&self) -> impl Iterator<Item = (&P::Object, &PD)> {
        self.entries.iter().rev().map(|e| (&*e.child, &e.data))
    }

    /// Returns a reverse iterator with indices.
    pub fn enumerate_rev(&self) -> impl Iterator<Item = (usize, &P::Object, &PD)> {
        let len = self.entries.len();
        self.entries
            .iter()
            .enumerate()
            .rev()
            .map(move |(i, e)| (len - 1 - i, &*e.child, &e.data))
    }

    // ========================================================================
    // Visitors / Callbacks
    // ========================================================================

    /// Calls a closure for each child with its parent data.
    ///
    /// This is useful for operations like painting where you need to
    /// apply an offset from parent data to each child.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &PD),
    {
        for entry in &self.entries {
            f(&entry.child, &entry.data);
        }
    }

    /// Calls a closure for each child with mutable parent data.
    pub fn for_each_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut P::Object, &mut PD),
    {
        for entry in &mut self.entries {
            f(&mut entry.child, &mut entry.data);
        }
    }

    /// Calls a closure for each child in reverse order (front to back).
    ///
    /// This is the correct order for hit testing, where you want to test
    /// the topmost (last painted) child first.
    pub fn for_each_rev<F>(&self, mut f: F)
    where
        F: FnMut(&P::Object, &PD),
    {
        for entry in self.entries.iter().rev() {
            f(&entry.child, &entry.data);
        }
    }

    /// Finds the first child (in reverse order) that satisfies a predicate.
    ///
    /// Useful for hit testing where you search from front to back.
    pub fn find_rev<F>(&self, mut predicate: F) -> Option<(&P::Object, &PD)>
    where
        F: FnMut(&P::Object, &PD) -> bool,
    {
        for entry in self.entries.iter().rev() {
            if predicate(&entry.child, &entry.data) {
                return Some((&entry.child, &entry.data));
            }
        }
        None
    }

    /// Finds the first child (in reverse order) where the closure returns Some.
    ///
    /// Similar to `Iterator::find_map` but in reverse order.
    pub fn find_map_rev<F, T>(&self, mut f: F) -> Option<T>
    where
        F: FnMut(&P::Object, &PD) -> Option<T>,
    {
        for entry in self.entries.iter().rev() {
            if let Some(result) = f(&entry.child, &entry.data) {
                return Some(result);
            }
        }
        None
    }
}

/// Type alias for multiple Box protocol children with default parent data.
pub type BoxChildren<PD = BoxParentData> = Children<BoxProtocol, PD>;

/// Type alias for multiple Sliver protocol children.
pub type SliverChildren<PD = <SliverProtocol as Protocol>::ParentData> =
    Children<SliverProtocol, PD>;

// ============================================================================
// Hit Testing Helpers for BoxChildren
// ============================================================================

/// Trait for parent data that contains an offset.
///
/// This is used by hit testing helpers to extract the child offset
/// from parent data.
pub trait HasOffset {
    /// Returns the offset stored in this parent data.
    fn offset(&self) -> Offset;
}

impl HasOffset for BoxParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

// Implement HasOffset for other common parent data types
impl HasOffset for crate::parent_data::ContainerBoxParentData {
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

impl HasOffset for crate::parent_data::ListWheelParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::MultiChildLayoutParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::FlowParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::TextParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::TableCellParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl HasOffset for crate::parent_data::ListBodyParentData {
    fn offset(&self) -> Offset {
        self.offset
    }
}

impl<PD: HasOffset> BoxChildren<PD> {
    // ========================================================================
    // Paint Helpers
    // ========================================================================

    /// Default paint implementation for box children.
    ///
    /// Paints each child at its offset from parent data. Children are painted
    /// in order (back to front).
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultPaint`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderFlex {
    ///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
    ///         self.children.paint_children(context, offset);
    ///     }
    /// }
    /// ```
    pub fn paint_children<F>(&self, base_offset: Offset, mut paint_child: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        for (child, data) in self.iter_with_data() {
            let child_offset = base_offset + data.offset();
            paint_child(child, child_offset);
        }
    }

    /// Paints children using a custom offset extractor.
    ///
    /// Use this when the offset is stored differently in your parent data,
    /// or when you need to apply additional transformations.
    pub fn paint_children_with<F, G>(
        &self,
        base_offset: Offset,
        mut get_offset: G,
        mut paint_child: F,
    ) where
        F: FnMut(&dyn RenderBox, Offset),
        G: FnMut(&dyn RenderBox, &PD) -> Offset,
    {
        for (child, data) in self.iter_with_data() {
            let child_offset = base_offset + get_offset(child, data);
            paint_child(child, child_offset);
        }
    }

    /// Visits each child for painting, providing the child and computed offset.
    ///
    /// This is a lower-level method that gives full control over the paint process.
    /// Use this when you need to do additional work per child (e.g., check visibility).
    pub fn visit_children_for_paint<F>(&self, base_offset: Offset, mut visitor: F)
    where
        F: FnMut(&dyn RenderBox, &PD, Offset),
    {
        for (child, data) in self.iter_with_data() {
            let child_offset = base_offset + data.offset();
            visitor(child, data, child_offset);
        }
    }

    // ========================================================================
    // Hit Testing Helpers
    // ========================================================================

    /// Default hit test implementation for box children.
    ///
    /// Tests children in reverse paint order (front to back) and returns
    /// `true` if any child is hit.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultHitTestChildren`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl RenderBox for RenderFlex {
    ///     fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
    ///         self.children.hit_test_children(result, position)
    ///     }
    /// }
    /// ```
    pub fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        for (child, data) in self.iter_with_data_rev() {
            let child_offset = data.offset();
            let hit = result.add_with_paint_offset(
                Some(child_offset),
                position,
                |result, transformed_position| child.hit_test(result, transformed_position),
            );
            if hit {
                return true;
            }
        }
        false
    }

    /// Hit tests children using a custom offset extractor.
    ///
    /// Use this when the offset is stored differently in your parent data,
    /// or when you need to apply additional transformations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // For parent data where offset is computed differently
    /// self.children.hit_test_children_with(result, position, |child, data| {
    ///     data.computed_offset()
    /// })
    /// ```
    pub fn hit_test_children_with<F>(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
        mut get_offset: F,
    ) -> bool
    where
        F: FnMut(&dyn RenderBox, &PD) -> Offset,
    {
        for (child, data) in self.iter_with_data_rev() {
            let child_offset = get_offset(child, data);
            let hit = result.add_with_paint_offset(
                Some(child_offset),
                position,
                |result, transformed_position| child.hit_test(result, transformed_position),
            );
            if hit {
                return true;
            }
        }
        false
    }

    /// Hit tests children and collects all hits (not just the first).
    ///
    /// Unlike `hit_test_children`, this continues testing after finding
    /// a hit, useful for overlapping children where multiple can be hit.
    ///
    /// Returns `true` if any child was hit.
    pub fn hit_test_all_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let mut any_hit = false;
        for (child, data) in self.iter_with_data_rev() {
            let child_offset = data.offset();
            let hit = result.add_with_paint_offset(
                Some(child_offset),
                position,
                |result, transformed_position| child.hit_test(result, transformed_position),
            );
            if hit {
                any_hit = true;
            }
        }
        any_hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parent_data::FlexParentData;
    use flui_types::Offset;

    #[test]
    fn test_children_default_is_empty() {
        let children: BoxChildren = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_children_with_capacity() {
        let children: BoxChildren = Children::with_capacity(10);
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_child_entry_new() {
        // We can't easily create a dyn RenderBox in tests without a concrete type,
        // so we just test the struct exists and compiles
        let _entry_type: Option<ChildEntry<BoxProtocol, BoxParentData>> = None;
    }

    #[test]
    fn test_children_empty() {
        let children: Children<BoxProtocol, FlexParentData> = Children::empty();
        assert!(children.is_empty());
    }

    #[test]
    fn test_get_data() {
        let children: BoxChildren = Children::new();
        assert!(children.get_data(0).is_none());
        assert!(children.first_data().is_none());
        assert!(children.last_data().is_none());
    }

    #[test]
    fn test_move_child_same_index() {
        let mut children: BoxChildren = Children::new();
        // Should not panic when from == to
        children.move_child(0, 0);
    }

    #[test]
    fn test_iter_with_data() {
        let children: BoxChildren = Children::new();
        assert_eq!(children.iter_with_data().count(), 0);
    }

    #[test]
    fn test_iter_rev() {
        let children: BoxChildren = Children::new();
        assert_eq!(children.iter_rev().count(), 0);
        assert_eq!(children.iter_with_data_rev().count(), 0);
    }

    #[test]
    fn test_enumerate_with_data() {
        let children: BoxChildren = Children::new();
        assert_eq!(children.enumerate_with_data().count(), 0);
    }

    #[test]
    fn test_enumerate_rev() {
        let children: BoxChildren = Children::new();
        assert_eq!(children.enumerate_rev().count(), 0);
    }
}
