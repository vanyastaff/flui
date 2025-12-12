//! Multiple children container

use std::marker::PhantomData;

use crate::parent_data::ParentData;
use crate::protocol::Protocol;

/// Container for multiple children of a specific protocol with parent data
///
/// `Children<P, PD>` stores a vector of children using the protocol's object type,
/// along with parent data for each child.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `PD`: The parent data type (defaults to P::ParentData)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::Children;
/// use flui_rendering::protocol::BoxProtocol;
/// use flui_rendering::parent_data::BoxParentData;
///
/// let mut container: Children<BoxProtocol> = Children::new();
/// container.push(Box::new(child1));
/// container.push(Box::new(child2));
///
/// for child in container.iter() {
///     // child is &dyn RenderBox
///     let size = child.size();
/// }
/// ```
#[derive(Debug)]
pub struct Children<P: Protocol, PD: ParentData = P::ParentData> {
    children: Vec<Box<P::Object>>,
    _phantom: PhantomData<(P, PD)>,
}

impl<P: Protocol, PD: ParentData> Children<P, PD> {
    /// Creates a new empty container
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Creates a container with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            children: Vec::with_capacity(capacity),
            _phantom: PhantomData,
        }
    }

    /// Returns the number of children
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns whether the container is empty
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns a reference to the child at the given index
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.children.get(index).map(|b| &**b)
    }

    /// Returns a mutable reference to the child at the given index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.children.get_mut(index).map(|b| &mut **b)
    }

    /// Returns an iterator over references to the children
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> + '_ {
        self.children.iter().map(|b| &**b)
    }

    /// Returns an iterator over mutable references to the children
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> + '_ {
        self.children.iter_mut().map(|b| &mut **b)
    }

    /// Adds a child to the end of the container
    pub fn push(&mut self, child: Box<P::Object>) {
        self.children.push(child);
    }

    /// Inserts a child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index > len.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.children.insert(index, child);
    }

    /// Removes and returns the child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index is out of bounds.
    pub fn remove(&mut self, index: usize) -> Box<P::Object> {
        self.children.remove(index)
    }

    /// Removes and returns the last child
    pub fn pop(&mut self) -> Option<Box<P::Object>> {
        self.children.pop()
    }

    /// Clears all children
    pub fn clear(&mut self) {
        self.children.clear();
    }

    /// Returns the current capacity
    pub fn capacity(&self) -> usize {
        self.children.capacity()
    }

    /// Reserves capacity for at least `additional` more children
    pub fn reserve(&mut self, additional: usize) {
        self.children.reserve(additional);
    }

    /// Shrinks the capacity to fit the current number of children
    pub fn shrink_to_fit(&mut self) {
        self.children.shrink_to_fit();
    }

    /// Swaps two children
    pub fn swap(&mut self, a: usize, b: usize) {
        self.children.swap(a, b);
    }
}

impl<P: Protocol, PD: ParentData> Default for Children<P, PD> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, PD: ParentData> FromIterator<Box<P::Object>> for Children<P, PD> {
    fn from_iter<T: IntoIterator<Item = Box<P::Object>>>(iter: T) -> Self {
        Self {
            children: iter.into_iter().collect(),
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;

    #[test]
    fn test_new() {
        let container: Children<BoxProtocol> = Children::new();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_with_capacity() {
        let container: Children<BoxProtocol> = Children::with_capacity(10);
        assert_eq!(container.len(), 0);
        assert!(container.capacity() >= 10);
    }
}
