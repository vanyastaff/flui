//! Type aliases for common arity storage patterns.
//!
//! This module provides convenient type aliases for the most commonly used
//! arity storage configurations.

use super::arity_storage::ArityStorage;
use super::types::{Exact, Leaf, Optional, Variable};

/// Type alias for single-child storage (`Exact<1>`).
///
/// Use this for containers that must have exactly one child,
/// like `Padding`, `Align`, `Transform`, etc.
///
/// # Example
///
/// ```
/// use flui_tree::arity::{SingleChildStorage, ChildrenStorage};
///
/// let mut storage: SingleChildStorage<u32> = SingleChildStorage::new();
/// storage.set_single_child(42).unwrap();
/// assert_eq!(storage.single_child(), Some(&42));
/// ```
pub type SingleChildStorage<T> = ArityStorage<T, Exact<1>>;

/// Type alias for optional-child storage (`Optional`).
///
/// Use this for containers that can have zero or one child,
/// like `SizedBox`, `Container`, `ColoredBox`.
///
/// # Example
///
/// ```
/// use flui_tree::arity::{OptionalChildStorage, ChildrenStorage};
///
/// let mut storage: OptionalChildStorage<u32> = OptionalChildStorage::new();
/// assert!(storage.is_empty());
/// storage.add_child(10).unwrap();
/// assert_eq!(storage.single_child(), Some(&10));
/// ```
pub type OptionalChildStorage<T> = ArityStorage<T, Optional>;

/// Type alias for variable-children storage (`Variable`).
///
/// Use this for containers that can have any number of children,
/// like `Flex`, `Stack`, `Column`, `Row`.
///
/// # Example
///
/// ```
/// use flui_tree::arity::{VariableChildrenStorage, ChildrenStorage};
///
/// let mut storage: VariableChildrenStorage<u32> = VariableChildrenStorage::new();
/// storage.add_child(1).unwrap();
/// storage.add_child(2).unwrap();
/// storage.add_child(3).unwrap();
/// assert_eq!(storage.child_count(), 3);
/// ```
pub type VariableChildrenStorage<T> = ArityStorage<T, Variable>;

/// Type alias for leaf storage (no children).
///
/// Use this for nodes that cannot have children,
/// like `Text`, `Image`, `Spacer`.
///
/// # Example
///
/// ```
/// use flui_tree::arity::{LeafStorage, ChildrenStorage};
///
/// let storage: LeafStorage<u32> = LeafStorage::new();
/// assert!(storage.is_empty());
/// assert!(storage.child_count() == 0);
/// ```
pub type LeafStorage<T> = ArityStorage<T, Leaf>;
