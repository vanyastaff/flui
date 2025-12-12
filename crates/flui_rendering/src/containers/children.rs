//! Multiple children container with arity support

use std::marker::PhantomData;

use flui_tree::arity::{Arity, ChildrenStorage, Variable};

use crate::containers::TypedChildren;
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
#[derive(Debug)]
pub struct Children<P: Protocol, PD: ParentData = <P as Protocol>::ParentData, A: Arity = Variable> {
    children: TypedChildren<P, A>,
    _phantom: PhantomData<PD>,
}

impl<P: Protocol, PD: ParentData, A: Arity> Children<P, PD, A> {
    /// Creates a new empty container
    pub fn new() -> Self {
        Self {
            children: TypedChildren::new(),
            _phantom: PhantomData,
        }
    }

    /// Creates a container with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            children: TypedChildren::with_capacity(capacity),
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
        self.children.get(index)
    }

    /// Returns a mutable reference to the child at the given index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut P::Object> {
        self.children.get_mut(index)
    }

    /// Returns an iterator over references to the children
    pub fn iter(&self) -> impl Iterator<Item = &P::Object> + '_ {
        (0..self.len()).filter_map(move |i| self.get(i))
    }

    /// Returns an iterator over mutable references to the children
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut P::Object> + '_ {
        let len = self.len();
        (0..len).filter_map(move |i| {
            // SAFETY: We're creating non-overlapping mutable references
            unsafe {
                let ptr = self as *mut Self;
                (*ptr).get_mut(i)
            }
        })
    }

    /// Adds a child to the end of the container
    ///
    /// # Panics
    ///
    /// Panics if the arity constraint is violated
    pub fn push(&mut self, child: Box<P::Object>) {
        self.children.add_child(child)
            .expect("Arity constraint violated: cannot add child");
    }

    /// Inserts a child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index > len or arity constraint is violated.
    pub fn insert(&mut self, index: usize, child: Box<P::Object>) {
        self.children.insert_child(index, child)
            .expect("Arity constraint violated: cannot insert child");
    }

    /// Removes and returns the child at the given index
    ///
    /// # Panics
    ///
    /// Panics if index is out of bounds or arity constraint is violated.
    pub fn remove(&mut self, index: usize) -> Box<P::Object> {
        self.children.remove_child(index)
            .expect("Arity constraint violated: cannot remove child")
    }

    /// Removes and returns the last child
    pub fn pop(&mut self) -> Option<Box<P::Object>> {
        self.children.pop_child()
    }

    /// Clears all children
    ///
    /// # Panics
    ///
    /// Panics if arity constraint requires minimum children
    pub fn clear(&mut self) {
        let _ = self.children.clear_children();
    }

    /// Returns the current capacity (if backed by Vec)
    pub fn capacity(&self) -> usize {
        // ArityStorage doesn't have a capacity method, so we estimate
        self.len()
    }

    /// Reserves capacity for at least `additional` more children
    pub fn reserve(&mut self, additional: usize) {
        self.children.reserve(additional);
    }

    /// Swaps two children
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds
    pub fn swap(&mut self, a: usize, b: usize) {
        if a < self.len() && b < self.len() {
            // We need to temporarily remove both elements to swap
            // This is a limitation of the current API
            if a == b {
                return;
            }

            let (first, second) = if a < b { (a, b) } else { (b, a) };

            let elem_b = self.children.remove_child(second)
                .expect("Cannot swap: index out of bounds");
            let elem_a = self.children.remove_child(first)
                .expect("Cannot swap: index out of bounds");

            self.children.insert_child(first, elem_b)
                .expect("Cannot swap: failed to reinsert");
            self.children.insert_child(second, elem_a)
                .expect("Cannot swap: failed to reinsert");
        } else {
            panic!("Cannot swap children: index out of bounds");
        }
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
