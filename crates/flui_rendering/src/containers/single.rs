//! Single child container with arity support

use std::marker::PhantomData;

use flui_tree::arity::{Arity, ArityStorage, ChildrenStorage, Optional};

use crate::protocol::Protocol;

/// Container for children of a specific protocol with arity validation
///
/// `Single<P, A>` stores children using ArityStorage from flui-tree, providing
/// compile-time validation of child count constraints.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `A`: The arity constraint (Optional by default for 0 or 1 child)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::Single;
/// use flui_rendering::protocol::BoxProtocol;
/// use flui_tree::arity::Optional;
///
/// // Optional child (0 or 1)
/// let mut container: Single<BoxProtocol> = Single::new();
/// container.set_child(Box::new(my_render_box));
///
/// if let Some(child) = container.child() {
///     // child is &dyn RenderBox - no downcasting needed!
///     let size = child.size();
/// }
/// ```
#[derive(Debug)]
pub struct Single<P: Protocol, A: Arity = Optional> {
    storage: ArityStorage<Box<P::Object>, A>,
    _phantom: PhantomData<P>,
}

impl<P: Protocol, A: Arity> Single<P, A> {
    /// Creates a new empty container
    pub fn new() -> Self {
        Self {
            storage: ArityStorage::new(),
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the child, if any
    pub fn child(&self) -> Option<&P::Object> {
        self.storage.get_single().map(|b| &**b)
    }

    /// Returns a mutable reference to the child, if any
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.storage.get_single_mut().map(|b| &mut **b)
    }

    /// Sets the child, replacing any existing child
    ///
    /// # Panics
    ///
    /// Panics if the arity constraint doesn't allow a single child
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.storage
            .set_single_child(child)
            .expect("Arity constraint violated: cannot set single child");
    }

    /// Takes the child, leaving None in its place
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.storage.remove_child(0)
    }

    /// Returns whether this container has a child
    pub fn has_child(&self) -> bool {
        self.storage.child_count() > 0
    }

    /// Clears the child
    pub fn clear(&mut self) {
        if self.storage.child_count() > 0 {
            let _ = self.storage.remove_child(0);
        }
    }
}

impl<P: Protocol, A: Arity> Default for Single<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

// Implement ChildrenStorage to enable delegation
impl<P: Protocol, A: Arity> ChildrenStorage<Box<P::Object>, A> for Single<P, A> {
    fn child_count(&self) -> usize {
        self.storage.child_count()
    }

    fn get_child(&self, index: usize) -> Option<&Box<P::Object>> {
        self.storage.get_child(index)
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut Box<P::Object>> {
        self.storage.get_child_mut(index)
    }

    fn add_child(&mut self, child: Box<P::Object>) -> Result<(), Box<P::Object>> {
        self.storage.add_child(child)
    }

    fn insert_child(
        &mut self,
        index: usize,
        child: Box<P::Object>,
    ) -> Result<(), Box<P::Object>> {
        self.storage.insert_child(index, child)
    }

    fn remove_child(&mut self, index: usize) -> Option<Box<P::Object>> {
        self.storage.remove_child(index)
    }

    fn set_single_child(&mut self, child: Box<P::Object>) -> Result<(), Box<P::Object>> {
        self.storage.set_single_child(child)
    }

    fn get_single(&self) -> Option<&Box<P::Object>> {
        self.storage.get_single()
    }

    fn get_single_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.storage.get_single_mut()
    }

    fn iter_children(&self) -> impl Iterator<Item = &Box<P::Object>> {
        self.storage.iter_children()
    }

    fn iter_children_mut(&mut self) -> impl Iterator<Item = &mut Box<P::Object>> {
        self.storage.iter_children_mut()
    }

    fn capacity(&self) -> Option<usize> {
        self.storage.capacity()
    }

    fn reserve(&mut self, additional: usize) {
        self.storage.reserve(additional)
    }

    fn swap_children(&mut self, a: usize, b: usize) -> Result<(), &'static str> {
        self.storage.swap_children(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;

    #[test]
    fn test_new() {
        let container: Single<BoxProtocol> = Single::new();
        assert!(!container.has_child());
        assert!(container.child().is_none());
    }

    #[test]
    fn test_clear() {
        let mut container: Single<BoxProtocol> = Single::new();
        // We can't easily test with real RenderBox here,
        // but we can test the API
        assert!(!container.has_child());
        container.clear();
        assert!(!container.has_child());
    }
}
