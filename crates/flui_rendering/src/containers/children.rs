//! Multiple children container with parent data support.

use crate::parent_data::BoxParentData;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use std::fmt::Debug;
use std::marker::PhantomData;

/// Container that stores multiple children of a protocol's object type.
///
/// # Type Safety
///
/// Uses `Protocol::Object` to ensure type-safe child storage at compile time.
/// The `PD` parameter specifies the parent data type for metadata storage.
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
///         for child in self.children.iter_mut() {
///             // child is &mut dyn RenderBox
///             let size = child.perform_layout(child_constraints);
///         }
///     }
/// }
/// ```
pub struct Children<P: Protocol, PD = <P as Protocol>::ParentData> {
    children: Vec<Box<P::Object>>,
    _phantom: PhantomData<(P, PD)>,
}

impl<P: Protocol, PD> Debug for Children<P, PD>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.children.len())
            .finish()
    }
}

impl<P: Protocol, PD> Default for Children<P, PD> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, PD> Children<P, PD> {
    /// Creates a new empty children container.
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Creates a children container with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            children: Vec::with_capacity(capacity),
            _phantom: PhantomData,
        }
    }

    /// Returns the number of children.
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns `true` if the container has no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns a reference to the child at the given index.
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.children.get(index).map(|b| &**b)
    }

    /// Returns a mutable reference to the child at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.children.get_mut(index).map(|b| &mut **b)
    }

    /// Returns the first child, if any.
    pub fn first(&self) -> Option<&P::Object> {
        self.children.first().map(|b| &**b)
    }

    /// Returns a mutable reference to the first child, if any.
    pub fn first_mut(&mut self) -> Option<&mut P::Object> {
        self.children.first_mut().map(|b| &mut **b)
    }

    /// Returns the last child, if any.
    pub fn last(&self) -> Option<&P::Object> {
        self.children.last().map(|b| &**b)
    }

    /// Returns a mutable reference to the last child, if any.
    pub fn last_mut(&mut self) -> Option<&mut P::Object> {
        self.children.last_mut().map(|b| &mut **b)
    }

    /// Returns an iterator over the children.
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> {
        self.children.iter().map(|b| &**b)
    }

    /// Returns a mutable iterator over the children.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> {
        self.children.iter_mut().map(|b| &mut **b)
    }

    /// Adds a child to the end of the container.
    pub fn push(&mut self, child: Box<P::Object>) {
        self.children.push(child);
    }

    /// Inserts a child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.children.insert(index, child);
    }

    /// Removes and returns the child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= len`.
    pub fn remove(&mut self, index: usize) -> Box<P::Object> {
        self.children.remove(index)
    }

    /// Removes the last child and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<Box<P::Object>> {
        self.children.pop()
    }

    /// Removes all children.
    pub fn clear(&mut self) {
        self.children.clear();
    }

    /// Swaps two children by their indices.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    pub fn swap(&mut self, a: usize, b: usize) {
        self.children.swap(a, b);
    }
}

/// Type alias for multiple Box protocol children with default parent data.
pub type BoxChildren<PD = BoxParentData> = Children<BoxProtocol, PD>;

/// Type alias for multiple Sliver protocol children.
pub type SliverChildren<PD = <SliverProtocol as Protocol>::ParentData> =
    Children<SliverProtocol, PD>;

#[cfg(test)]
mod tests {
    use super::*;

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
}
