//! Mutable storage trait for children with arity-based implementations.
//!
//! This module provides `ChildrenStorage<T>` - a trait for owning and mutating
//! children collections. It complements `ChildrenAccess<'a, T>` which provides
//! read-only views.
//!
//! # Design Philosophy
//!
//! - **ChildrenAccess**: Read-only views (borrows, `Copy`, cheap)
//! - **ChildrenStorage**: Mutable ownership (owns data, enables delegation)
//!
//! # Key Differences
//!
//! | Aspect | ChildrenAccess | ChildrenStorage |
//! |--------|----------------|-----------------|
//! | Ownership | Borrows (`&'a [T]`) | Owns data |
//! | Lifetime | Has `'a` parameter | No lifetime |
//! | Methods | `as_slice()`, `iter()` | `set()`, `add()`, `remove()` |
//! | Copy | Yes (cheap view) | No (owns data) |
//! | Use Case | Iteration, queries | Modification |
//! | Delegation | Not suitable | Perfect for Proxy |
//!
//! # Usage with Proxy
//!
//! ```rust,ignore
//! use ambassador::Delegate;
//!
//! #[derive(Delegate)]
//! #[delegate(ChildrenStorage<Box<P::Object>>, target = "storage")]
//! pub struct Proxy<P: Protocol, A: Arity> {
//!     storage: ArityStorage<Box<P::Object>, A>,  // Delegates here!
//!     // ...
//! }
//!
//! impl RenderProxyBox for Proxy<BoxProtocol> {
//!     fn child(&self) -> Option<&dyn RenderBox> {
//!         self.single_child()  // Uses delegated method!
//!             .map(|b| b.as_ref() as &dyn RenderBox)
//!     }
//! }
//! ```

use ambassador::delegatable_trait;

use super::accessors::SliceChildren;
use super::error::ArityError;
use super::runtime::RuntimeArity;

// ============================================================================
// CHILDREN STORAGE TRAIT (Ambassador-compatible)
// ============================================================================

/// Trait for mutable storage of children (owns data).
///
/// This trait provides mutation operations for children storage and can be
/// delegated to via Ambassador. It complements `ChildrenAccess` which provides
/// read-only views.
///
/// # Design
///
/// All methods work with owned `T` values, allowing the storage to manage
/// lifecycle, attach/detach children, and support undo/redo operations.
///
/// # Ambassador Compatibility
///
/// This trait is designed for delegation via Ambassador. All methods are
/// simple (no generics, no `impl Trait` returns) to ensure compatibility.
///
/// ```rust,ignore
/// #[derive(Delegate)]
/// #[delegate(ChildrenStorage<Box<dyn RenderBox>>, target = "storage")]
/// pub struct Proxy<A: Arity> {
///     storage: ArityStorage<Box<dyn RenderBox>, A>,
/// }
/// ```
///
/// # Extension Methods
///
/// For generic operations like `retain`, `find`, `any`, `all`, use the
/// `ChildrenStorageExt` extension trait which provides these through
/// the slice access.
#[delegatable_trait]
pub trait ChildrenStorage<T> {
    // ========================================================================
    // CORE ACCESS (immutable)
    // ========================================================================

    /// Get immutable reference to child at index.
    ///
    /// Returns `None` if index is out of bounds.
    fn get_child(&self, index: usize) -> Option<&T>;

    /// Get mutable reference to child at index.
    ///
    /// Returns `None` if index is out of bounds.
    fn get_child_mut(&mut self, index: usize) -> Option<&mut T>;

    /// Get the number of children.
    fn child_count(&self) -> usize;

    /// Check if storage is empty.
    fn is_empty(&self) -> bool;

    /// Get children as a slice for iteration.
    ///
    /// This is the primary way to iterate over children.
    fn children_slice(&self) -> &[T];

    // ========================================================================
    // SINGLE CHILD ACCESS (for Exact<1>, Optional)
    // ========================================================================

    /// Get single child reference (for Exact<1>, Optional).
    ///
    /// Returns the first child if present, or `None` otherwise.
    fn single_child(&self) -> Option<&T>;

    /// Get mutable single child reference (for Exact<1>, Optional).
    ///
    /// Returns the first child if present, or `None` otherwise.
    fn single_child_mut(&mut self) -> Option<&mut T>;

    /// Set single child (for Exact<1>, Optional).
    ///
    /// Replaces existing child if present. Returns the old child if any.
    ///
    /// # Errors
    ///
    /// May return `ArityError` if the arity doesn't support setting a child
    /// (e.g., Leaf arity, or Variable when full).
    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError>;

    /// Take single child out (for Exact<1>, Optional).
    ///
    /// Removes and returns the child if present.
    fn take_single_child(&mut self) -> Option<T>;

    // ========================================================================
    // MULTI-CHILD OPERATIONS (for Variable, Range, AtLeast)
    // ========================================================================

    /// Add child to storage (for Variable, Range, AtLeast).
    ///
    /// # Errors
    ///
    /// Returns `ArityError::TooManyChildren` if the arity limit is exceeded.
    fn add_child(&mut self, child: T) -> Result<(), ArityError>;

    /// Insert child at specific index (for Variable, Range).
    ///
    /// # Errors
    ///
    /// Returns `ArityError` if index is out of bounds or arity limit exceeded.
    fn insert_child(&mut self, index: usize, child: T) -> Result<(), ArityError>;

    /// Remove child at index (for Variable, Range, Optional).
    ///
    /// Returns the removed child if successful, `None` if index out of bounds.
    fn remove_child(&mut self, index: usize) -> Option<T>;

    /// Remove and return the last child (for Variable).
    ///
    /// Returns `None` if storage is empty.
    fn pop_child(&mut self) -> Option<T>;

    /// Clear all children.
    ///
    /// # Errors
    ///
    /// May return `ArityError` if the arity requires minimum children (e.g., Exact<1>).
    fn clear_children(&mut self) -> Result<(), ArityError>;

    // ========================================================================
    // CAPACITY OPERATIONS
    // ========================================================================

    /// Reserve capacity for additional children (for Variable).
    ///
    /// This is a hint for optimization - implementations may ignore it.
    fn reserve(&mut self, additional: usize);

    /// Shrink capacity to fit current children count (for Variable).
    fn shrink_to_fit(&mut self);

    // ========================================================================
    // ARITY INFORMATION
    // ========================================================================

    /// Get runtime arity information.
    fn runtime_arity(&self) -> RuntimeArity;

    /// Check if can add more children.
    fn can_add_child(&self) -> bool;

    /// Check if can remove children.
    fn can_remove_child(&self) -> bool;

    /// Get maximum children capacity.
    fn max_children(&self) -> Option<usize>;

    /// Get minimum required children.
    fn min_children(&self) -> usize;
}

// ============================================================================
// EXTENSION TRAIT (for generic operations - NOT delegatable)
// ============================================================================

/// Extension trait for `ChildrenStorage` providing generic operations.
///
/// These methods use generics and cannot be delegated via Ambassador,
/// but they work with any type implementing `ChildrenStorage`.
pub trait ChildrenStorageExt<T>: ChildrenStorage<T> {
    /// Iterate over children references.
    #[inline]
    fn iter(&self) -> std::slice::Iter<'_, T> {
        self.children_slice().iter()
    }

    /// Find first child matching predicate.
    fn find<P>(&self, mut predicate: P) -> Option<&T>
    where
        P: FnMut(&T) -> bool,
    {
        self.children_slice().iter().find(|item| predicate(item))
    }

    /// Check if any child matches predicate.
    fn any<P>(&self, mut predicate: P) -> bool
    where
        P: FnMut(&T) -> bool,
    {
        self.children_slice().iter().any(|item| predicate(item))
    }

    /// Check if all children match predicate.
    fn all<P>(&self, mut predicate: P) -> bool
    where
        P: FnMut(&T) -> bool,
    {
        self.children_slice().iter().all(|item| predicate(item))
    }

    /// Create a read-only view accessor.
    fn as_view(&self) -> SliceChildren<'_, T> {
        SliceChildren {
            children: self.children_slice(),
        }
    }
}

// Blanket implementation for all ChildrenStorage types
impl<T, S: ChildrenStorage<T> + ?Sized> ChildrenStorageExt<T> for S {}

// ============================================================================
// BLANKET IMPLEMENTATIONS FOR COMMON TYPES
// ============================================================================

impl<T: Send + Sync> ChildrenStorage<T> for Vec<T> {
    fn get_child(&self, index: usize) -> Option<&T> {
        self.get(index)
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut T> {
        self.get_mut(index)
    }

    fn child_count(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }

    fn children_slice(&self) -> &[T] {
        self.as_slice()
    }

    fn single_child(&self) -> Option<&T> {
        self.first()
    }

    fn single_child_mut(&mut self) -> Option<&mut T> {
        self.first_mut()
    }

    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError> {
        if self.is_empty() {
            self.push(child);
            Ok(None)
        } else {
            Ok(Some(std::mem::replace(&mut self[0], child)))
        }
    }

    fn take_single_child(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            Some(self.remove(0))
        }
    }

    fn add_child(&mut self, child: T) -> Result<(), ArityError> {
        self.push(child);
        Ok(())
    }

    fn insert_child(&mut self, index: usize, child: T) -> Result<(), ArityError> {
        if index > self.len() {
            Err(ArityError::InvalidChildCount {
                arity: RuntimeArity::Variable,
                actual: index,
            })
        } else {
            self.insert(index, child);
            Ok(())
        }
    }

    fn remove_child(&mut self, index: usize) -> Option<T> {
        if index < self.len() {
            Some(self.remove(index))
        } else {
            None
        }
    }

    fn pop_child(&mut self) -> Option<T> {
        self.pop()
    }

    fn clear_children(&mut self) -> Result<(), ArityError> {
        self.clear();
        Ok(())
    }

    fn reserve(&mut self, additional: usize) {
        Vec::reserve(self, additional);
    }

    fn shrink_to_fit(&mut self) {
        Vec::shrink_to_fit(self);
    }

    fn runtime_arity(&self) -> RuntimeArity {
        RuntimeArity::Variable
    }

    fn can_add_child(&self) -> bool {
        true
    }

    fn can_remove_child(&self) -> bool {
        !self.is_empty()
    }

    fn max_children(&self) -> Option<usize> {
        None
    }

    fn min_children(&self) -> usize {
        0
    }
}

impl<T: Send + Sync> ChildrenStorage<T> for Option<T> {
    fn get_child(&self, index: usize) -> Option<&T> {
        if index == 0 {
            self.as_ref()
        } else {
            None
        }
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut T> {
        if index == 0 {
            self.as_mut()
        } else {
            None
        }
    }

    fn child_count(&self) -> usize {
        if self.is_some() {
            1
        } else {
            0
        }
    }

    fn is_empty(&self) -> bool {
        self.is_none()
    }

    fn children_slice(&self) -> &[T] {
        match self {
            Some(ref child) => std::slice::from_ref(child),
            None => &[],
        }
    }

    fn single_child(&self) -> Option<&T> {
        self.as_ref()
    }

    fn single_child_mut(&mut self) -> Option<&mut T> {
        self.as_mut()
    }

    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError> {
        Ok(self.replace(child))
    }

    fn take_single_child(&mut self) -> Option<T> {
        self.take()
    }

    fn add_child(&mut self, child: T) -> Result<(), ArityError> {
        if self.is_none() {
            *self = Some(child);
            Ok(())
        } else {
            Err(ArityError::TooManyChildren {
                arity: RuntimeArity::Optional,
                attempted: 2,
            })
        }
    }

    fn insert_child(&mut self, index: usize, child: T) -> Result<(), ArityError> {
        if index == 0 {
            self.add_child(child)
        } else {
            Err(ArityError::InvalidChildCount {
                arity: RuntimeArity::Optional,
                actual: index,
            })
        }
    }

    fn remove_child(&mut self, index: usize) -> Option<T> {
        if index == 0 {
            self.take()
        } else {
            None
        }
    }

    fn pop_child(&mut self) -> Option<T> {
        self.take()
    }

    fn clear_children(&mut self) -> Result<(), ArityError> {
        *self = None;
        Ok(())
    }

    fn reserve(&mut self, _additional: usize) {
        // No-op for Option
    }

    fn shrink_to_fit(&mut self) {
        // No-op for Option
    }

    fn runtime_arity(&self) -> RuntimeArity {
        RuntimeArity::Optional
    }

    fn can_add_child(&self) -> bool {
        self.is_none()
    }

    fn can_remove_child(&self) -> bool {
        self.is_some()
    }

    fn max_children(&self) -> Option<usize> {
        Some(1)
    }

    fn min_children(&self) -> usize {
        0
    }
}

// Implementation for fixed-size arrays [T; N]
impl<T: Send + Sync, const N: usize> ChildrenStorage<T> for [T; N] {
    fn get_child(&self, index: usize) -> Option<&T> {
        self.get(index)
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut T> {
        self.get_mut(index)
    }

    fn child_count(&self) -> usize {
        N
    }

    fn is_empty(&self) -> bool {
        N == 0
    }

    fn children_slice(&self) -> &[T] {
        self.as_slice()
    }

    fn single_child(&self) -> Option<&T> {
        self.first()
    }

    fn single_child_mut(&mut self) -> Option<&mut T> {
        self.first_mut()
    }

    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError> {
        if N >= 1 {
            Ok(Some(std::mem::replace(&mut self[0], child)))
        } else {
            Err(ArityError::TooManyChildren {
                arity: RuntimeArity::Exact(0),
                attempted: 1,
            })
        }
    }

    fn take_single_child(&mut self) -> Option<T> {
        // Cannot take from array - would leave uninitialized
        None
    }

    fn add_child(&mut self, _child: T) -> Result<(), ArityError> {
        Err(ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(N),
            attempted: N + 1,
        })
    }

    fn insert_child(&mut self, _index: usize, _child: T) -> Result<(), ArityError> {
        Err(ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(N),
            attempted: N + 1,
        })
    }

    fn remove_child(&mut self, _index: usize) -> Option<T> {
        // Cannot remove from array - would leave uninitialized
        None
    }

    fn pop_child(&mut self) -> Option<T> {
        // Cannot pop from array
        None
    }

    fn clear_children(&mut self) -> Result<(), ArityError> {
        if N == 0 {
            Ok(())
        } else {
            Err(ArityError::TooFewChildren {
                arity: RuntimeArity::Exact(N),
                attempted: 0,
            })
        }
    }

    fn reserve(&mut self, _additional: usize) {
        // No-op for arrays
    }

    fn shrink_to_fit(&mut self) {
        // No-op for arrays
    }

    fn runtime_arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(N)
    }

    fn can_add_child(&self) -> bool {
        false // Fixed-size array
    }

    fn can_remove_child(&self) -> bool {
        false // Fixed-size array
    }

    fn max_children(&self) -> Option<usize> {
        Some(N)
    }

    fn min_children(&self) -> usize {
        N
    }
}

// Empty tuple for Leaf arity
impl ChildrenStorage<()> for () {
    fn get_child(&self, _index: usize) -> Option<&()> {
        None
    }

    fn get_child_mut(&mut self, _index: usize) -> Option<&mut ()> {
        None
    }

    fn child_count(&self) -> usize {
        0
    }

    fn is_empty(&self) -> bool {
        true
    }

    fn children_slice(&self) -> &[()] {
        &[]
    }

    fn single_child(&self) -> Option<&()> {
        None
    }

    fn single_child_mut(&mut self) -> Option<&mut ()> {
        None
    }

    fn set_single_child(&mut self, _child: ()) -> Result<Option<()>, ArityError> {
        Err(ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(0),
            attempted: 1,
        })
    }

    fn take_single_child(&mut self) -> Option<()> {
        None
    }

    fn add_child(&mut self, _child: ()) -> Result<(), ArityError> {
        Err(ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(0),
            attempted: 1,
        })
    }

    fn insert_child(&mut self, _index: usize, _child: ()) -> Result<(), ArityError> {
        Err(ArityError::TooManyChildren {
            arity: RuntimeArity::Exact(0),
            attempted: 1,
        })
    }

    fn remove_child(&mut self, _index: usize) -> Option<()> {
        None
    }

    fn pop_child(&mut self) -> Option<()> {
        None
    }

    fn clear_children(&mut self) -> Result<(), ArityError> {
        Ok(())
    }

    fn reserve(&mut self, _additional: usize) {
        // No-op
    }

    fn shrink_to_fit(&mut self) {
        // No-op
    }

    fn runtime_arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(0)
    }

    fn can_add_child(&self) -> bool {
        false
    }

    fn can_remove_child(&self) -> bool {
        false
    }

    fn max_children(&self) -> Option<usize> {
        Some(0)
    }

    fn min_children(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::accessors::ChildrenAccess;

    #[test]
    fn test_vec_storage() {
        let mut storage: Vec<i32> = Vec::new();
        assert_eq!(storage.child_count(), 0);
        assert!(storage.is_empty());

        storage.add_child(1).unwrap();
        storage.add_child(2).unwrap();
        storage.add_child(3).unwrap();

        assert_eq!(storage.child_count(), 3);
        assert_eq!(storage.get_child(1), Some(&2));

        let removed = storage.remove_child(1);
        assert_eq!(removed, Some(2));
        assert_eq!(storage.child_count(), 2);
    }

    #[test]
    fn test_option_storage() {
        let mut storage: Option<i32> = None;
        assert_eq!(storage.child_count(), 0);
        assert!(storage.can_add_child());

        storage.add_child(42).unwrap();
        assert_eq!(storage.child_count(), 1);
        assert!(!storage.can_add_child());
        assert_eq!(storage.single_child(), Some(&42));

        let err = storage.add_child(99);
        assert!(err.is_err());

        let taken = storage.take_single_child();
        assert_eq!(taken, Some(42));
        assert!(storage.is_empty());
    }

    #[test]
    fn test_array_storage() {
        let mut storage: [i32; 3] = [1, 2, 3];
        assert_eq!(storage.child_count(), 3);
        assert_eq!(storage.get_child(1), Some(&2));

        // Cannot add to fixed array
        assert!(storage.add_child(4).is_err());

        // Can access slice
        assert_eq!(storage.children_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_empty_tuple_storage() {
        let mut storage = ();
        assert_eq!(storage.child_count(), 0);
        assert!(!storage.can_add_child());
        assert!(storage.add_child(()).is_err());
    }

    #[test]
    fn test_storage_extension() {
        let storage: Vec<i32> = vec![1, 2, 3, 4, 5];

        // Use extension methods
        let sum: i32 = storage.iter().sum();
        assert_eq!(sum, 15);

        let found = storage.find(|x| *x > 3);
        assert_eq!(found, Some(&4));

        let has_even = storage.any(|x| *x % 2 == 0);
        assert!(has_even);

        // Create view
        let view = storage.as_view();
        assert_eq!(view.len(), 5);
    }

    #[test]
    fn test_children_slice() {
        let storage: Vec<i32> = vec![10, 20, 30];
        let slice = storage.children_slice();
        assert_eq!(slice, &[10, 20, 30]);

        let opt: Option<i32> = Some(42);
        let slice = opt.children_slice();
        assert_eq!(slice, &[42]);

        let empty: Option<i32> = None;
        let slice = empty.children_slice();
        assert_eq!(slice, &[]);
    }
}
