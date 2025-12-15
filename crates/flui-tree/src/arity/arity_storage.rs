//! Arity-aware storage enum for children management.
//!
//! This module provides `ArityStorage<T, A>` - a compile-time arity-validated
//! storage enum that adapts its internal representation based on arity constraints.
//!
//! # Design
//!
//! `ArityStorage` uses an enum to provide optimal storage for different arity types:
//!
//! | Arity | Storage | Size (Box<dyn RenderBox>) |
//! |-------|---------|---------------------------|
//! | `Leaf` | `()` | 0 bytes |
//! | `Optional` | `Option<T>` | 24 bytes |
//! | `Exact<1>` | `SmallVec<[T; 1]>` | 32 bytes |
//! | `Exact<N>` | `SmallVec<[T; N]>` | ~32 bytes |
//! | `Variable` | `Vec<T>` | 24 bytes |
//! | `Range<MIN, MAX>` | `Vec<T>` | 24 bytes |
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_arity::{ArityStorage, Exact, Optional, Variable};
//!
//! // Single child storage
//! let mut storage: ArityStorage<Element, Exact<1>> = ArityStorage::new();
//! storage.set_single_child(element)?;
//!
//! // Optional child storage
//! let mut storage: ArityStorage<Element, Optional> = ArityStorage::new();
//! storage.add_child(element)?;  // Can add if empty
//! storage.clear_children()?;     // Can clear
//!
//! // Variable children storage
//! let mut storage: ArityStorage<Element, Variable> = ArityStorage::new();
//! storage.add_child(elem1)?;
//! storage.add_child(elem2)?;
//! for child in storage.iter() { /* ... */ }
//! ```
//!
//! # Delegation
//!
//! `ArityStorage` implements `ChildrenStorage<T>` and can be used with Ambassador:
//!
//! ```rust,ignore
//! #[derive(Delegate)]
//! #[delegate(ChildrenStorage<Box<dyn RenderBox>>, target = "storage")]
//! pub struct Proxy<A: Arity> {
//!     storage: ArityStorage<Box<dyn RenderBox>, A>,
//! }
//! ```

use smallvec::SmallVec;
use std::marker::PhantomData;

use super::accessors::{NoChildren, OptionalChild, SliceChildren};
use super::error::ArityError;
use super::runtime::RuntimeArity;
use super::storage::ChildrenStorage;
use super::traits::Arity;

// ============================================================================
// ARITY STORAGE ENUM
// ============================================================================

/// Arity-aware storage enum that adapts to compile-time arity constraints.
///
/// This enum provides optimal storage for different arity types while
/// maintaining type safety through the `A: Arity` parameter.
///
/// # Type Parameters
///
/// - `T`: Element type (e.g., `Box<dyn RenderBox>`, `Element`)
/// - `A`: Arity constraint (e.g., `Exact<1>`, `Optional`, `Variable`)
///
/// # Variants
///
/// The enum has different variants for different storage strategies:
///
/// - **Leaf**: Zero-sized, no storage needed
/// - **Optional**: `Option<T>`, 0 or 1 element
/// - **Exact**: `SmallVec<[T; 4]>`, inline up to 4 elements
/// - **Variable**: `Vec<T>`, heap-allocated dynamic size
/// - **Range**: `Vec<T>`, heap-allocated with bounds checking
///
/// # Memory Layout
///
/// All variants are wrapped in a single enum, so the size is determined by
/// the largest variant plus a discriminant tag (typically 1-8 bytes).
#[derive(Debug)]
pub enum ArityStorage<T, A: Arity> {
    /// Leaf arity: no children (zero-sized).
    Leaf(PhantomData<A>),

    /// Optional arity: 0 or 1 child.
    Optional(Option<T>),

    /// Exact arity: exactly N children (inline up to 4).
    /// Uses SmallVec for cache-friendly storage.
    Exact(SmallVec<[T; 4]>),

    /// Variable arity: any number of children.
    Variable(Vec<T>),

    /// Range arity: MIN to MAX children.
    Range(Vec<T>),
}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl<T, A: Arity> ArityStorage<T, A> {
    /// Create new empty storage based on arity type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let storage: ArityStorage<Element, Exact<1>> = ArityStorage::new();
    /// let storage: ArityStorage<Element, Optional> = ArityStorage::new();
    /// let storage: ArityStorage<Element, Variable> = ArityStorage::new();
    /// ```
    pub fn new() -> Self {
        match A::runtime_arity() {
            RuntimeArity::Exact(0) => ArityStorage::Leaf(PhantomData),
            RuntimeArity::Optional => ArityStorage::Optional(None),
            RuntimeArity::Exact(_) => ArityStorage::Exact(SmallVec::new()),
            RuntimeArity::Variable => ArityStorage::Variable(Vec::new()),
            RuntimeArity::AtLeast(_) => ArityStorage::Variable(Vec::new()),
            RuntimeArity::Range(_, _) => ArityStorage::Range(Vec::new()),
            RuntimeArity::Never => ArityStorage::Leaf(PhantomData),
        }
    }

    /// Create storage with capacity hint (for Variable, Range).
    ///
    /// This pre-allocates space to avoid reallocations.
    pub fn with_capacity(capacity: usize) -> Self {
        match A::runtime_arity() {
            RuntimeArity::Exact(0) => ArityStorage::Leaf(PhantomData),
            RuntimeArity::Optional => ArityStorage::Optional(None),
            RuntimeArity::Exact(_) => ArityStorage::Exact(SmallVec::new()),
            RuntimeArity::Variable => ArityStorage::Variable(Vec::with_capacity(capacity)),
            RuntimeArity::AtLeast(_) => ArityStorage::Variable(Vec::with_capacity(capacity)),
            RuntimeArity::Range(_, _) => ArityStorage::Range(Vec::with_capacity(capacity)),
            RuntimeArity::Never => ArityStorage::Leaf(PhantomData),
        }
    }
}

impl<T, A: Arity> Default for ArityStorage<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// HELPER METHODS
// ============================================================================

impl<T, A: Arity> ArityStorage<T, A> {
    /// Get the inner slice for all variants.
    fn inner_slice(&self) -> &[T] {
        match self {
            ArityStorage::Leaf(_) => &[],
            ArityStorage::Optional(opt) => match opt {
                Some(ref child) => std::slice::from_ref(child),
                None => &[],
            },
            ArityStorage::Exact(vec) => vec.as_slice(),
            ArityStorage::Variable(vec) => vec.as_slice(),
            ArityStorage::Range(vec) => vec.as_slice(),
        }
    }
}

// ============================================================================
// CHILDREN STORAGE IMPLEMENTATION
// ============================================================================

impl<T: Send + Sync, A: Arity> ChildrenStorage<T> for ArityStorage<T, A> {
    fn get_child(&self, index: usize) -> Option<&T> {
        match self {
            ArityStorage::Leaf(_) => None,
            ArityStorage::Optional(opt) => {
                if index == 0 {
                    opt.as_ref()
                } else {
                    None
                }
            }
            ArityStorage::Exact(vec) => vec.get(index),
            ArityStorage::Variable(vec) => vec.get(index),
            ArityStorage::Range(vec) => vec.get(index),
        }
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut T> {
        match self {
            ArityStorage::Leaf(_) => None,
            ArityStorage::Optional(opt) => {
                if index == 0 {
                    opt.as_mut()
                } else {
                    None
                }
            }
            ArityStorage::Exact(vec) => vec.get_mut(index),
            ArityStorage::Variable(vec) => vec.get_mut(index),
            ArityStorage::Range(vec) => vec.get_mut(index),
        }
    }

    fn child_count(&self) -> usize {
        match self {
            ArityStorage::Leaf(_) => 0,
            ArityStorage::Optional(opt) => {
                if opt.is_some() {
                    1
                } else {
                    0
                }
            }
            ArityStorage::Exact(vec) => vec.len(),
            ArityStorage::Variable(vec) => vec.len(),
            ArityStorage::Range(vec) => vec.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.child_count() == 0
    }

    fn children_slice(&self) -> &[T] {
        self.inner_slice()
    }

    fn children_slice_mut(&mut self) -> &mut [T] {
        match self {
            ArityStorage::Leaf(_) => &mut [],
            ArityStorage::Optional(opt) => {
                if let Some(ref mut child) = opt {
                    std::slice::from_mut(child)
                } else {
                    &mut []
                }
            }
            ArityStorage::Exact(vec) => vec.as_mut_slice(),
            ArityStorage::Variable(vec) => vec.as_mut_slice(),
            ArityStorage::Range(vec) => vec.as_mut_slice(),
        }
    }

    fn single_child(&self) -> Option<&T> {
        self.get_child(0)
    }

    fn single_child_mut(&mut self) -> Option<&mut T> {
        self.get_child_mut(0)
    }

    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError> {
        match self {
            ArityStorage::Leaf(_) => Err(ArityError::TooManyChildren {
                arity: RuntimeArity::Exact(0),
                attempted: 1,
            }),
            ArityStorage::Optional(opt) => Ok(opt.replace(child)),
            ArityStorage::Exact(vec) => {
                if vec.is_empty() {
                    vec.push(child);
                    Ok(None)
                } else {
                    Ok(Some(std::mem::replace(&mut vec[0], child)))
                }
            }
            ArityStorage::Variable(vec) => {
                if vec.is_empty() {
                    vec.push(child);
                    Ok(None)
                } else {
                    Ok(Some(std::mem::replace(&mut vec[0], child)))
                }
            }
            ArityStorage::Range(vec) => {
                if vec.is_empty() {
                    vec.push(child);
                    Ok(None)
                } else {
                    Ok(Some(std::mem::replace(&mut vec[0], child)))
                }
            }
        }
    }

    fn take_single_child(&mut self) -> Option<T> {
        match self {
            ArityStorage::Leaf(_) => None,
            ArityStorage::Optional(opt) => opt.take(),
            ArityStorage::Exact(vec) => {
                if vec.is_empty() {
                    None
                } else {
                    Some(vec.remove(0))
                }
            }
            ArityStorage::Variable(vec) => {
                if vec.is_empty() {
                    None
                } else {
                    Some(vec.remove(0))
                }
            }
            ArityStorage::Range(vec) => {
                if vec.is_empty() {
                    None
                } else {
                    Some(vec.remove(0))
                }
            }
        }
    }

    fn add_child(&mut self, child: T) -> Result<(), ArityError> {
        let current_count = self.child_count();
        let runtime_arity = self.runtime_arity();

        // Validate against arity constraints using can_add_child logic
        if !self.can_add_child() {
            return Err(ArityError::TooManyChildren {
                arity: runtime_arity,
                attempted: current_count + 1,
            });
        }

        match self {
            ArityStorage::Leaf(_) => Err(ArityError::TooManyChildren {
                arity: RuntimeArity::Exact(0),
                attempted: 1,
            }),
            ArityStorage::Optional(opt) => {
                if opt.is_none() {
                    *opt = Some(child);
                    Ok(())
                } else {
                    Err(ArityError::TooManyChildren {
                        arity: RuntimeArity::Optional,
                        attempted: 2,
                    })
                }
            }
            ArityStorage::Exact(vec) => {
                vec.push(child);
                Ok(())
            }
            ArityStorage::Variable(vec) => {
                vec.push(child);
                Ok(())
            }
            ArityStorage::Range(vec) => {
                vec.push(child);
                Ok(())
            }
        }
    }

    fn insert_child(&mut self, index: usize, child: T) -> Result<(), ArityError> {
        let current_count = self.child_count();

        if index > current_count {
            return Err(ArityError::InvalidChildCount {
                arity: self.runtime_arity(),
                actual: index,
            });
        }

        // Validate arity constraints using can_add_child logic
        let runtime_arity = self.runtime_arity();
        if !self.can_add_child() {
            return Err(ArityError::TooManyChildren {
                arity: runtime_arity,
                attempted: current_count + 1,
            });
        }

        match self {
            ArityStorage::Leaf(_) => Err(ArityError::TooManyChildren {
                arity: RuntimeArity::Exact(0),
                attempted: 1,
            }),
            ArityStorage::Optional(_) => self.add_child(child),
            ArityStorage::Exact(vec) => {
                vec.insert(index, child);
                Ok(())
            }
            ArityStorage::Variable(vec) => {
                vec.insert(index, child);
                Ok(())
            }
            ArityStorage::Range(vec) => {
                vec.insert(index, child);
                Ok(())
            }
        }
    }

    fn remove_child(&mut self, index: usize) -> Option<T> {
        let current_count = self.child_count();

        if index >= current_count {
            return None;
        }

        // Validate arity constraints using can_remove_child logic
        if !self.can_remove_child() {
            return None; // Cannot remove - would violate arity
        }

        match self {
            ArityStorage::Leaf(_) => None,
            ArityStorage::Optional(opt) => {
                if index == 0 {
                    opt.take()
                } else {
                    None
                }
            }
            ArityStorage::Exact(vec) => {
                if index < vec.len() {
                    Some(vec.remove(index))
                } else {
                    None
                }
            }
            ArityStorage::Variable(vec) => {
                if index < vec.len() {
                    Some(vec.remove(index))
                } else {
                    None
                }
            }
            ArityStorage::Range(vec) => {
                if index < vec.len() {
                    Some(vec.remove(index))
                } else {
                    None
                }
            }
        }
    }

    fn pop_child(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let current_count = self.child_count();
        let runtime_arity = self.runtime_arity();

        // Validate arity constraints
        if !runtime_arity.validate(current_count - 1) {
            return None;
        }

        match self {
            ArityStorage::Leaf(_) => None,
            ArityStorage::Optional(opt) => opt.take(),
            ArityStorage::Exact(vec) => vec.pop(),
            ArityStorage::Variable(vec) => vec.pop(),
            ArityStorage::Range(vec) => vec.pop(),
        }
    }

    fn clear_children(&mut self) -> Result<(), ArityError> {
        let runtime_arity = self.runtime_arity();

        // Check if clearing is allowed
        if !runtime_arity.validate(0) {
            return Err(ArityError::TooFewChildren {
                arity: runtime_arity,
                attempted: 0,
            });
        }

        match self {
            ArityStorage::Leaf(_) => Ok(()),
            ArityStorage::Optional(opt) => {
                *opt = None;
                Ok(())
            }
            ArityStorage::Exact(vec) => {
                vec.clear();
                Ok(())
            }
            ArityStorage::Variable(vec) => {
                vec.clear();
                Ok(())
            }
            ArityStorage::Range(vec) => {
                vec.clear();
                Ok(())
            }
        }
    }

    fn reserve(&mut self, additional: usize) {
        match self {
            ArityStorage::Variable(vec) => vec.reserve(additional),
            ArityStorage::Range(vec) => vec.reserve(additional),
            ArityStorage::Exact(vec) => vec.reserve(additional),
            _ => {} // No-op for other variants
        }
    }

    fn shrink_to_fit(&mut self) {
        match self {
            ArityStorage::Variable(vec) => vec.shrink_to_fit(),
            ArityStorage::Range(vec) => vec.shrink_to_fit(),
            ArityStorage::Exact(vec) => vec.shrink_to_fit(),
            _ => {} // No-op for other variants
        }
    }

    fn runtime_arity(&self) -> RuntimeArity {
        A::runtime_arity()
    }

    fn can_add_child(&self) -> bool {
        let count = self.child_count();
        match self.runtime_arity() {
            RuntimeArity::Exact(n) => count < n,
            RuntimeArity::Optional => count < 1,
            RuntimeArity::AtLeast(_) => true,
            RuntimeArity::Variable => true,
            RuntimeArity::Range(_, max) => count < max,
            RuntimeArity::Never => false,
        }
    }

    fn can_remove_child(&self) -> bool {
        if self.is_empty() {
            return false;
        }

        let count = self.child_count();
        match self.runtime_arity() {
            RuntimeArity::Exact(n) => count > n,
            RuntimeArity::Optional => true,
            RuntimeArity::AtLeast(min) => count > min,
            RuntimeArity::Variable => true,
            RuntimeArity::Range(min, _) => count > min,
            RuntimeArity::Never => false,
        }
    }

    fn max_children(&self) -> Option<usize> {
        match self.runtime_arity() {
            RuntimeArity::Exact(n) => Some(n),
            RuntimeArity::Optional => Some(1),
            RuntimeArity::AtLeast(_) => None,
            RuntimeArity::Variable => None,
            RuntimeArity::Range(_, max) => Some(max),
            RuntimeArity::Never => Some(0),
        }
    }

    fn min_children(&self) -> usize {
        match self.runtime_arity() {
            RuntimeArity::Exact(n) => n,
            RuntimeArity::Optional => 0,
            RuntimeArity::AtLeast(min) => min,
            RuntimeArity::Variable => 0,
            RuntimeArity::Range(min, _) => min,
            RuntimeArity::Never => 0,
        }
    }
}

// ============================================================================
// VIEW ENUM (for as_view() return type)
// ============================================================================

/// View enum that wraps different accessor types.
///
/// This allows creating read-only views of ArityStorage.
#[derive(Debug, Clone)]
pub enum ArityStorageView<'a, T> {
    Leaf(NoChildren<T>),
    Optional(OptionalChild<'a, T>),
    Slice(SliceChildren<'a, T>),
}

impl<'a, T: 'a> ArityStorageView<'a, T> {
    /// Get the underlying slice.
    pub fn as_slice(&self) -> &'a [T] {
        match self {
            ArityStorageView::Leaf(_) => &[],
            ArityStorageView::Optional(v) => v.children,
            ArityStorageView::Slice(v) => v.children,
        }
    }

    /// Get the number of children.
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Iterate over children.
    pub fn iter(&self) -> std::slice::Iter<'a, T> {
        self.as_slice().iter()
    }
}

impl<T, A: Arity> ArityStorage<T, A> {
    /// Create a read-only view of this storage.
    pub fn as_view(&self) -> ArityStorageView<'_, T> {
        match self {
            ArityStorage::Leaf(_) => ArityStorageView::Leaf(NoChildren(PhantomData)),
            ArityStorage::Optional(opt) => {
                let slice = if let Some(ref child) = opt {
                    std::slice::from_ref(child)
                } else {
                    &[]
                };
                ArityStorageView::Optional(OptionalChild { children: slice })
            }
            ArityStorage::Exact(vec) => ArityStorageView::Slice(SliceChildren {
                children: vec.as_slice(),
            }),
            ArityStorage::Variable(vec) => ArityStorageView::Slice(SliceChildren {
                children: vec.as_slice(),
            }),
            ArityStorage::Range(vec) => ArityStorageView::Slice(SliceChildren {
                children: vec.as_slice(),
            }),
        }
    }
}

// ============================================================================
// SPECIALIZED CONSTRUCTORS
// ============================================================================

impl<T, A: Arity> ArityStorage<T, A> {
    /// Create storage from iterator (for Variable, Range).
    pub fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Send + Sync,
    {
        match A::runtime_arity() {
            RuntimeArity::Variable => ArityStorage::Variable(iter.into_iter().collect()),
            RuntimeArity::AtLeast(_) => ArityStorage::Variable(iter.into_iter().collect()),
            RuntimeArity::Range(_, _) => ArityStorage::Range(iter.into_iter().collect()),
            _ => {
                // For other arities, create empty and try to add
                let mut storage = Self::new();
                for item in iter {
                    let _ = storage.add_child(item);
                }
                storage
            }
        }
    }

    /// Retain only children matching predicate (for Variable, Range).
    ///
    /// # Errors
    ///
    /// May return `ArityError` if retaining would violate arity constraints.
    pub fn retain<F>(&mut self, mut f: F) -> Result<(), ArityError>
    where
        F: FnMut(&T) -> bool,
    {
        match self {
            ArityStorage::Leaf(_) => Ok(()),
            ArityStorage::Optional(opt) => {
                if let Some(ref child) = opt {
                    if !f(child) {
                        *opt = None;
                    }
                }
                Ok(())
            }
            ArityStorage::Exact(vec) => {
                vec.retain(|item| f(item));

                // Validate result against arity
                let runtime_arity = A::runtime_arity();
                if !runtime_arity.validate(vec.len()) {
                    Err(ArityError::InvalidChildCount {
                        arity: runtime_arity,
                        actual: vec.len(),
                    })
                } else {
                    Ok(())
                }
            }
            ArityStorage::Variable(vec) => {
                vec.retain(|item| f(item));
                Ok(())
            }
            ArityStorage::Range(vec) => {
                vec.retain(|item| f(item));

                // Validate result against arity
                let runtime_arity = A::runtime_arity();
                if !runtime_arity.validate(vec.len()) {
                    Err(ArityError::InvalidChildCount {
                        arity: runtime_arity,
                        actual: vec.len(),
                    })
                } else {
                    Ok(())
                }
            }
        }
    }
}

// Implement FromIterator
impl<T: Send + Sync, A: Arity> FromIterator<T> for ArityStorage<T, A> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_iter(iter)
    }
}

// ============================================================================
// CLONE IMPLEMENTATION
// ============================================================================

impl<T: Clone, A: Arity> Clone for ArityStorage<T, A> {
    fn clone(&self) -> Self {
        match self {
            ArityStorage::Leaf(phantom) => ArityStorage::Leaf(*phantom),
            ArityStorage::Optional(opt) => ArityStorage::Optional(opt.clone()),
            ArityStorage::Exact(vec) => ArityStorage::Exact(vec.clone()),
            ArityStorage::Variable(vec) => ArityStorage::Variable(vec.clone()),
            ArityStorage::Range(vec) => ArityStorage::Range(vec.clone()),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::{Exact, Leaf, Optional, Variable};

    #[test]
    fn test_leaf_storage() {
        let mut storage: ArityStorage<i32, Leaf> = ArityStorage::new();
        assert_eq!(storage.child_count(), 0);
        assert!(storage.is_empty());
        assert!(storage.add_child(1).is_err());
    }

    #[test]
    fn test_optional_storage() {
        let mut storage: ArityStorage<i32, Optional> = ArityStorage::new();
        assert_eq!(storage.child_count(), 0);

        storage.add_child(42).unwrap();
        assert_eq!(storage.child_count(), 1);
        assert_eq!(storage.single_child(), Some(&42));

        assert!(storage.add_child(99).is_err());

        let taken = storage.take_single_child();
        assert_eq!(taken, Some(42));
        assert!(storage.is_empty());
    }

    #[test]
    fn test_exact_storage() {
        let mut storage: ArityStorage<i32, Exact<3>> = ArityStorage::new();
        assert_eq!(storage.child_count(), 0);

        storage.add_child(1).unwrap();
        storage.add_child(2).unwrap();
        storage.add_child(3).unwrap();

        assert_eq!(storage.child_count(), 3);
        assert!(storage.add_child(4).is_err());

        let view = storage.as_view();
        assert_eq!(view.len(), 3);
    }

    #[test]
    fn test_variable_storage() {
        let mut storage: ArityStorage<i32, Variable> = ArityStorage::new();

        storage.add_child(1).unwrap();
        storage.add_child(2).unwrap();
        storage.add_child(3).unwrap();

        assert_eq!(storage.child_count(), 3);

        let removed = storage.remove_child(1);
        assert_eq!(removed, Some(2));
        assert_eq!(storage.child_count(), 2);

        storage.clear_children().unwrap();
        assert!(storage.is_empty());
    }

    #[test]
    fn test_storage_view() {
        let mut storage: ArityStorage<i32, Variable> = ArityStorage::new();
        storage.add_child(10).unwrap();
        storage.add_child(20).unwrap();
        storage.add_child(30).unwrap();

        let view = storage.as_view();
        assert_eq!(view.len(), 3);

        let sum: i32 = view.iter().sum();
        assert_eq!(sum, 60);
    }

    #[test]
    fn test_from_iter() {
        let vec = vec![1, 2, 3, 4, 5];
        let storage: ArityStorage<i32, Variable> = ArityStorage::from_iter(vec);

        assert_eq!(storage.child_count(), 5);
        assert_eq!(storage.get_child(2), Some(&3));
    }

    #[test]
    fn test_children_slice() {
        let mut storage: ArityStorage<i32, Variable> = ArityStorage::new();
        storage.add_child(1).unwrap();
        storage.add_child(2).unwrap();
        storage.add_child(3).unwrap();

        assert_eq!(storage.children_slice(), &[1, 2, 3]);
    }

    #[test]
    fn test_retain() {
        let mut storage: ArityStorage<i32, Variable> = ArityStorage::from_iter(vec![1, 2, 3, 4, 5]);
        storage.retain(|x| *x % 2 == 0).unwrap();
        assert_eq!(storage.children_slice(), &[2, 4]);
    }
}
