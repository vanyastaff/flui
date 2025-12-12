//! TypedChildren container for protocol-specific child management
//!
//! This module provides `TypedChildren<P, A>` - a protocol-aware wrapper around
//! ArityStorage that integrates arity validation with protocol-specific types.

use std::marker::PhantomData;

use flui_tree::arity::{Arity, ArityStorage, ChildrenStorage};

use crate::protocol::Protocol;

/// TypedChildren container for protocol-specific child storage with arity validation
///
/// `TypedChildren<P, A>` combines Protocol-specific types with arity constraints,
/// providing a type-safe container for render object children.
///
/// Uses ambassador to delegate ChildrenStorage trait to internal storage.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `A`: The arity constraint (Variable by default for N children)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::TypedChildren;
/// use flui_rendering::protocol::BoxProtocol;
/// use flui_tree::arity::Exact;
///
/// // Exactly one child
/// let mut children: TypedChildren<BoxProtocol, Exact<1>> = TypedChildren::new();
/// children.set_single_child(Box::new(child));
///
/// // Access child through protocol Object type
/// if let Some(child) = children.single_child() {
///     // child is &Box<dyn RenderBox>
/// }
/// ```
pub struct TypedChildren<P: Protocol, A: Arity>
where
    P::Object: Send + Sync,
{
    storage: ArityStorage<Box<P::Object>, A>,
    _phantom: PhantomData<P>,
}

// Manual Debug impl since P::Object doesn't require Debug
impl<P: Protocol, A: Arity> std::fmt::Debug for TypedChildren<P, A>
where
    P::Object: Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedChildren")
            .field("protocol", &P::name())
            .field("arity", &self.storage.runtime_arity())
            .field("child_count", &self.storage.child_count())
            .finish()
    }
}

impl<P: Protocol, A: Arity> TypedChildren<P, A>
where
    P::Object: Send + Sync,
{
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
        self.storage.is_empty()
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

    /// Clears all children
    ///
    /// # Panics
    ///
    /// Panics if arity constraint requires minimum children
    pub fn clear(&mut self) {
        let _ = self.storage.clear_children();
    }

    /// Reserves capacity for at least `additional` more children
    pub fn reserve(&mut self, additional: usize) {
        self.storage.reserve(additional);
    }

    // ChildrenStorage trait methods forwarded to storage

    /// Add child to the end
    pub fn add_child(&mut self, child: Box<P::Object>) -> Result<(), flui_tree::arity::ArityError> {
        self.storage.add_child(child)
    }

    /// Insert child at specific index
    pub fn insert_child(&mut self, index: usize, child: Box<P::Object>) -> Result<(), flui_tree::arity::ArityError> {
        self.storage.insert_child(index, child)
    }

    /// Remove child at index
    pub fn remove_child(&mut self, index: usize) -> Option<Box<P::Object>> {
        self.storage.remove_child(index)
    }

    /// Remove and return the last child
    pub fn pop_child(&mut self) -> Option<Box<P::Object>> {
        self.storage.pop_child()
    }

    /// Clear all children
    pub fn clear_children(&mut self) -> Result<(), flui_tree::arity::ArityError> {
        self.storage.clear_children()
    }

    /// Get single child (for Optional, Exact<1>)
    pub fn single_child(&self) -> Option<&Box<P::Object>> {
        self.storage.single_child()
    }

    /// Get mutable single child
    pub fn single_child_mut(&mut self) -> Option<&mut Box<P::Object>> {
        self.storage.single_child_mut()
    }

    /// Set single child
    pub fn set_single_child(&mut self, child: Box<P::Object>) -> Result<Option<Box<P::Object>>, flui_tree::arity::ArityError> {
        self.storage.set_single_child(child)
    }
}

impl<P: Protocol, A: Arity> Default for TypedChildren<P, A>
where
    P::Object: Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, A: Arity> FromIterator<Box<P::Object>> for TypedChildren<P, A>
where
    P::Object: Send + Sync,
{
    fn from_iter<T: IntoIterator<Item = Box<P::Object>>>(iter: T) -> Self {
        let mut container = Self::new();
        for child in iter {
            let _ = container.storage.add_child(child);
        }
        container
    }
}

// ChildrenStorage is automatically delegated to `storage` via ambassador

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;
    use flui_tree::arity::{Exact, Optional, Variable};

    #[test]
    fn test_new() {
        let container: TypedChildren<BoxProtocol, Variable> = TypedChildren::new();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_with_capacity() {
        let container: TypedChildren<BoxProtocol, Variable> = TypedChildren::with_capacity(10);
        assert_eq!(container.len(), 0);
    }

    #[test]
    fn test_debug() {
        let container: TypedChildren<BoxProtocol, Exact<1>> = TypedChildren::new();
        let debug_str = format!("{:?}", container);
        assert!(debug_str.contains("TypedChildren"));
        assert!(debug_str.contains("child_count"));
    }
}
