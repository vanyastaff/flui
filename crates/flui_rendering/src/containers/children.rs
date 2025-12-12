//! Multiple children container with arity support

use std::marker::PhantomData;

use ambassador::Delegate;
use flui_tree::arity::{Arity, ArityStorage, ChildrenStorage, Variable};

use crate::parent_data::ParentData;
use crate::protocol::Protocol;

/// Container for multiple children of a specific protocol with arity validation
///
/// `Children<P, PD, A>` stores children using ArityStorage from flui-tree, providing
/// compile-time validation of child count constraints.
///
/// Uses ambassador to delegate ChildrenStorage trait to internal storage.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `PD`: The parent data type (defaults to P::ParentData)
/// - `A`: The arity constraint (Variable by default for N children)
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
#[derive(Debug, Delegate)]
#[delegate(ChildrenStorage<Box<P::Object>, A>, target = "storage")]
pub struct Children<P: Protocol, PD: ParentData = P::ParentData, A: Arity = Variable> {
    storage: ArityStorage<Box<P::Object>, A>,
    _phantom: PhantomData<(P, PD)>,
}

impl<P: Protocol, PD: ParentData, A: Arity> Children<P, PD, A> {
    /// Creates a new empty container
    pub fn new() -> Self {
        Self {
            storage: ArityStorage::new(),
            _phantom: PhantomData,
        }
    }

    /// Creates a container with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: ArityStorage::with_capacity(capacity),
            _phantom: PhantomData,
        }
    }

    /// Returns the number of children
    pub fn len(&self) -> usize {
        self.storage.child_count()
    }

    /// Returns whether the container is empty
    pub fn is_empty(&self) -> bool {
        self.storage.child_count() == 0
    }

    /// Returns a reference to the child at the given index
    pub fn get(&self, index: usize) -> Option<&P::Object> {
        self.storage.get_child(index).map(|b| &**b)
    }

    /// Returns a mutable reference to the child at the given index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.storage.get_child_mut(index).map(|b| &mut **b)
    }

    /// Returns an iterator over references to the children
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> + '_ {
        self.storage.iter_children().map(|b| &**b)
    }

    /// Returns an iterator over mutable references to the children
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> + '_ {
        self.storage.iter_children_mut().map(|b| &mut **b)
    }

    /// Adds a child to the end of the container
    ///
    /// # Panics
    ///
    /// Panics if the arity constraint is violated
    pub fn push(&mut self, child: Box<P::Object>) {
        self.storage
            .add_child(child)
            .expect("Arity constraint violated: cannot add child");
    }

    /// Inserts a child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index > len or arity constraint is violated.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.storage
            .insert_child(index, child)
            .expect("Arity constraint violated: cannot insert child");
    }

    /// Removes and returns the child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index is out of bounds or arity constraint is violated.
    pub fn remove(&mut self, index: usize) -> Box<P::Object> {
        self.storage
            .remove_child(index)
            .expect("Arity constraint violated: cannot remove child")
    }

    /// Removes and returns the last child
    pub fn pop(&mut self) -> Option<Box<P::Object>> {
        if self.storage.child_count() > 0 {
            self.storage.remove_child(self.storage.child_count() - 1)
        } else {
            None
        }
    }

    /// Clears all children
    ///
    /// # Panics
    ///
    /// Panics if arity constraint requires minimum children
    pub fn clear(&mut self) {
        while self.storage.child_count() > 0 {
            let _ = self.storage.remove_child(0);
        }
    }

    /// Returns the current capacity (if backed by Vec)
    pub fn capacity(&self) -> usize {
        self.storage.capacity().unwrap_or(0)
    }

    /// Reserves capacity for at least `additional` more children
    pub fn reserve(&mut self, additional: usize) {
        self.storage.reserve(additional);
    }

    /// Swaps two children
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds
    pub fn swap(&mut self, a: usize, b: usize) {
        self.storage
            .swap_children(a, b)
            .expect("Cannot swap children: index out of bounds");
    }
}

impl<P: Protocol, PD: ParentData, A: Arity> Default for Children<P, PD, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, PD: ParentData, A: Arity> FromIterator<Box<P::Object>> for Children<P, PD, A> {
    fn from_iter<T: IntoIterator<Item = Box<P::Object>>>(iter: T) -> Self {
        let mut container = Self::new();
        for child in iter {
            container.push(child);
        }
        container
    }
}

// ChildrenStorage is automatically delegated to `storage` via ambassador

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
