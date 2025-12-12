//! Multiple children storage for render objects
//!
//! Flutter equivalent: `ContainerRenderObjectMixin<ChildType, ParentDataType>`

use flui_foundation::RenderId;

use crate::protocol::Protocol;

/// Multiple children storage with parent data (Flutter: ContainerRenderObjectMixin)
///
/// # Type Parameters
///
/// - `P`: Protocol type (BoxProtocol or SliverProtocol)
/// - `PD`: Parent data type (default: ())
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{Children, BoxProtocol};
///
/// #[derive(Default, Clone, Debug)]
/// struct FlexParentData {
///     flex: f32,
///     offset: Offset,
/// }
///
/// struct RenderFlex {
///     children: Children<BoxProtocol, FlexParentData>,
///     direction: Axis,
/// }
/// ```
#[derive(Debug)]
pub struct Children<P: Protocol, PD = ()> {
    items: Vec<(RenderId, PD)>,
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Protocol, PD> Children<P, PD> {
    /// Create empty children storage
    #[inline]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create with capacity
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Number of children
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Add child with parent data
    #[inline]
    pub fn push(&mut self, child: RenderId, parent_data: PD) {
        self.items.push((child, parent_data));
    }

    /// Remove last child
    #[inline]
    pub fn pop(&mut self) -> Option<(RenderId, PD)> {
        self.items.pop()
    }

    /// Get child at index
    #[inline]
    pub fn get(&self, index: usize) -> Option<&(RenderId, PD)> {
        self.items.get(index)
    }

    /// Get mutable child at index
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut (RenderId, PD)> {
        self.items.get_mut(index)
    }

    /// Get child ID at index
    #[inline]
    pub fn get_id(&self, index: usize) -> Option<RenderId> {
        self.items.get(index).map(|(id, _)| *id)
    }

    /// Get parent data at index
    #[inline]
    pub fn get_parent_data(&self, index: usize) -> Option<&PD> {
        self.items.get(index).map(|(_, pd)| pd)
    }

    /// Get mutable parent data at index
    #[inline]
    pub fn get_parent_data_mut(&mut self, index: usize) -> Option<&mut PD> {
        self.items.get_mut(index).map(|(_, pd)| pd)
    }

    /// Iterate over children
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &(RenderId, PD)> {
        self.items.iter()
    }

    /// Iterate mutably over children
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (RenderId, PD)> {
        self.items.iter_mut()
    }

    /// Iterate over child IDs
    #[inline]
    pub fn iter_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.items.iter().map(|(id, _)| *id)
    }

    /// Iterate over (child, parent_data) pairs
    #[inline]
    pub fn iter_with_data(&self) -> impl Iterator<Item = (RenderId, &PD)> + '_ {
        self.items.iter().map(|(id, pd)| (*id, pd))
    }

    /// Clear all children
    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Insert child at index
    #[inline]
    pub fn insert(&mut self, index: usize, child: RenderId, parent_data: PD) {
        self.items.insert(index, (child, parent_data));
    }

    /// Remove child at index
    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<(RenderId, PD)> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    /// Swap two children
    #[inline]
    pub fn swap(&mut self, a: usize, b: usize) {
        self.items.swap(a, b);
    }

    /// Retain children matching predicate
    #[inline]
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&(RenderId, PD)) -> bool,
    {
        self.items.retain(|item| f(item));
    }

    /// Reserve capacity
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }
}

impl<P: Protocol, PD> Default for Children<P, PD> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, PD: Clone> Clone for Children<P, PD> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

// ============================================================================
// Type Aliases
// ============================================================================

/// Multiple Box children (Flutter: ContainerRenderObjectMixin<RenderBox, ParentDataType>)
pub type BoxChildren<PD = ()> = Children<crate::protocol::BoxProtocol, PD>;

/// Multiple Sliver children (Flutter: ContainerRenderObjectMixin<RenderSliver, ParentDataType>)
pub type SliverChildren<PD = ()> = Children<crate::protocol::SliverProtocol, PD>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;
    use flui_types::Offset;

    #[derive(Debug, Clone, PartialEq)]
    struct TestParentData {
        offset: Offset,
        flex: f32,
    }

    #[test]
    fn test_children_basic() {
        let mut children: Children<BoxProtocol, TestParentData> = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);

        let id1 = RenderId::new(1);
        let pd1 = TestParentData {
            offset: Offset::ZERO,
            flex: 1.0,
        };
        children.push(id1, pd1.clone());

        assert_eq!(children.len(), 1);
        assert_eq!(children.get_id(0), Some(id1));
        assert_eq!(children.get_parent_data(0), Some(&pd1));
    }

    #[test]
    fn test_children_iteration() {
        let mut children: Children<BoxProtocol, f32> = Children::new();

        children.push(RenderId::new(1), 1.0);
        children.push(RenderId::new(2), 2.0);
        children.push(RenderId::new(3), 3.0);

        let ids: Vec<_> = children.iter_ids().collect();
        assert_eq!(ids.len(), 3);

        let data: Vec<_> = children.iter_with_data().map(|(_, pd)| *pd).collect();
        assert_eq!(data, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_children_manipulation() {
        let mut children: Children<BoxProtocol, ()> = Children::new();

        children.push(RenderId::new(1), ());
        children.push(RenderId::new(2), ());
        children.push(RenderId::new(3), ());

        children.insert(1, RenderId::new(10), ());
        assert_eq!(children.len(), 4);

        let removed = children.remove(1);
        assert_eq!(removed, Some((RenderId::new(10), ())));
        assert_eq!(children.len(), 3);
    }
}
