//! Ambassador delegation for ChildrenStorage from flui-tree

use ambassador::delegatable_trait_remote;
use flui_tree::arity::{ArityError, RuntimeArity};

#[delegatable_trait_remote]
pub trait ChildrenStorage<T> {
    fn get_child(&self, index: usize) -> Option<&T>;
    fn get_child_mut(&mut self, index: usize) -> Option<&mut T>;
    fn child_count(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn children_slice(&self) -> &[T];
    fn single_child(&self) -> Option<&T>;
    fn single_child_mut(&mut self) -> Option<&mut T>;
    fn set_single_child(&mut self, child: T) -> Result<Option<T>, ArityError>;
    fn take_single_child(&mut self) -> Option<T>;
    fn add_child(&mut self, child: T) -> Result<(), ArityError>;
    fn insert_child(&mut self, index: usize, child: T) -> Result<(), ArityError>;
    fn remove_child(&mut self, index: usize) -> Option<T>;
    fn pop_child(&mut self) -> Option<T>;
    fn clear_children(&mut self) -> Result<(), ArityError>;
    fn reserve(&mut self, additional: usize);
    fn shrink_to_fit(&mut self);
    fn runtime_arity(&self) -> RuntimeArity;
    fn can_add_child(&self) -> bool;
    fn can_remove_child(&self) -> bool;
    fn max_children(&self) -> Option<usize>;
    fn min_children(&self) -> usize;
}

// Re-export the trait and macro for use in delegation
pub use ambassador_impl_ChildrenStorage::ChildrenStorage as DelegatableChildrenStorage;
pub use ambassador_impl_ChildrenStorage;
