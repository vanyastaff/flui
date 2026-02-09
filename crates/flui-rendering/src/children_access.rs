//! Children access layer for type-safe child iteration.
//!
//! This module provides `ChildrenAccess`, which provides closure-based iteration
//! over children with type-safe `ChildHandle` instances. This design solves borrow
//! checker issues by avoiding iterator return types that would hold mutable borrows.
//!
//! # Design Rationale
//!
//! Instead of returning `impl Iterator<Item = ChildHandle>` (which would hold
//! a mutable borrow of the tree), we use closure-based methods like `for_each`,
//! `map`, `fold`, etc. This allows the closure to receive a fresh `ChildHandle`
//! for each child without lifetime issues.
//!
//! # Example
//!
//! ```ignore
//! fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
//!     let mut y_offset = 0.0;
//!
//!     ctx.children.for_each(|mut child| {
//!         let size = child.layout(constraints);
//!         child.set_offset(Offset::new(0.0, y_offset));
//!         y_offset += size.height;
//!     });
//!
//!     Size::new(constraints.max_width, y_offset)
//! }
//! ```

use std::marker::PhantomData;

use flui_foundation::RenderId;
use flui_types::{Offset, Size};

use crate::arity::{Arity, Leaf, Optional, Single, Variable};
use crate::child_handle::ChildHandle;
use crate::parent_data::ParentData;

// ============================================================================
// ChildState - Per-child state storage
// ============================================================================

/// State stored for each child (size, offset, parent_data).
///
/// This is the mutable state that `ChildHandle` references.
pub struct ChildState<P: ParentData + Default> {
    /// Render ID of this child.
    pub id: RenderId,
    /// Computed size after layout.
    pub size: Size,
    /// Position offset set by parent.
    pub offset: Offset,
    /// Parent data for this child.
    pub parent_data: P,
}

impl<P: ParentData + Default> ChildState<P> {
    /// Creates a new child state with default values.
    pub fn new(id: RenderId) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: P::default(),
        }
    }

    /// Creates a new child state with specific parent data.
    pub fn with_parent_data(id: RenderId, parent_data: P) -> Self {
        Self {
            id,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data,
        }
    }
}

// ============================================================================
// ChildrenAccess - Main Type
// ============================================================================

/// Type-safe access to children as `ChildHandle` instances.
///
/// `ChildrenAccess` is parameterized by:
/// - `A`: Arity (Leaf, Optional, Single, Variable)
/// - `P`: ParentData type
///
/// Different methods are available depending on the Arity type.
/// Phase-specific behavior is now determined by the context type that
/// contains this `ChildrenAccess` (e.g., `BoxLayoutContext`, `BoxPaintContext`).
///
/// # Design
///
/// This type stores child states and creates `ChildHandle` instances on-demand
/// via closure-based iteration. This avoids borrow checker issues that would
/// occur if we tried to return iterators of handles.
pub struct ChildrenAccess<'a, A: Arity, P: ParentData + Default> {
    /// Child states (owned, mutable).
    children: &'a mut [ChildState<P>],

    /// Phantom data for arity type parameter.
    _phantom: PhantomData<A>,
}

// ============================================================================
// Common Methods (Available for All Arities)
// ============================================================================

impl<'a, A: Arity, P: ParentData + Default> ChildrenAccess<'a, A, P> {
    /// Creates a new children accessor.
    #[inline]
    pub fn new(children: &'a mut [ChildState<P>]) -> Self {
        Self {
            children,
            _phantom: PhantomData,
        }
    }

    /// Returns the number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns whether there are no children.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

// ============================================================================
// Leaf Arity (No children - no additional methods)
// ============================================================================

impl<'a, P: ParentData + Default> ChildrenAccess<'a, Leaf, P> {
    // Intentionally empty - Leaf has no children to access
}

// ============================================================================
// Optional Arity (0 or 1 child)
// ============================================================================

impl<'a, P: ParentData + Default> ChildrenAccess<'a, Optional, P> {
    /// Returns a handle to the optional child, if present.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if let Some(mut child) = ctx.children.get() {
    ///     let size = child.layout(constraints);
    ///     child.set_offset(offset);
    /// }
    /// ```
    pub fn get(&mut self) -> Option<ChildHandle<'_, P>> {
        self.children.first_mut().map(|state| {
            ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data)
        })
    }

    /// Returns whether a child is present.
    #[inline]
    pub fn is_some(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns whether no child is present.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.children.is_empty()
    }

    /// Executes a closure if child is present.
    ///
    /// # Example
    ///
    /// ```ignore
    /// ctx.children.if_some(|mut child| {
    ///     child.layout(constraints);
    ///     child.set_offset(Offset::ZERO);
    /// });
    /// ```
    pub fn if_some<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(ChildHandle<'_, P>) -> R,
    {
        self.get().map(f)
    }
}

// ============================================================================
// Single Arity (Exactly 1 child)
// ============================================================================

impl<'a, P: ParentData + Default> ChildrenAccess<'a, Single, P> {
    /// Returns a handle to the single child.
    ///
    /// This method always succeeds because Single arity guarantees exactly 1 child.
    ///
    /// # Panics
    ///
    /// Panics if invariant is violated (should never happen if arity is enforced).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut child = ctx.children.get();
    /// let size = child.layout(constraints);
    /// child.set_offset(Offset::new(padding.left, padding.top));
    /// ```
    pub fn get(&mut self) -> ChildHandle<'_, P> {
        let state = &mut self.children[0];
        ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data)
    }
}

// ============================================================================
// Variable Arity (0+ children)
// ============================================================================

impl<'a, P: ParentData + Default> ChildrenAccess<'a, Variable, P> {
    /// Returns a handle to the child at the given index.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if let Some(mut first) = ctx.children.get(0) {
    ///     first.layout(special_constraints);
    /// }
    /// ```
    pub fn get(&mut self, index: usize) -> Option<ChildHandle<'_, P>> {
        self.children.get_mut(index).map(|state| {
            ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data)
        })
    }

    /// Iterates over all children using a closure.
    ///
    /// This is the primary way to iterate over variable children.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut y_offset = 0.0;
    /// ctx.children.for_each(|mut child| {
    ///     let size = child.layout(constraints);
    ///     child.set_offset(Offset::new(0.0, y_offset));
    ///     y_offset += size.height;
    /// });
    /// ```
    pub fn for_each<F>(&mut self, mut f: F)
    where
        F: FnMut(ChildHandle<'_, P>),
    {
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            f(handle);
        }
    }

    /// Iterates over children with indices.
    ///
    /// # Example
    ///
    /// ```ignore
    /// ctx.children.for_each_indexed(|index, mut child| {
    ///     println!("Laying out child {}", index);
    ///     child.layout(constraints);
    /// });
    /// ```
    pub fn for_each_indexed<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, ChildHandle<'_, P>),
    {
        for (index, state) in self.children.iter_mut().enumerate() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            f(index, handle);
        }
    }

    /// Maps over children, collecting results.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let sizes: Vec<Size> = ctx.children.map(|mut child| {
    ///     child.layout(constraints)
    /// });
    /// ```
    pub fn map<T, F>(&mut self, mut f: F) -> Vec<T>
    where
        F: FnMut(ChildHandle<'_, P>) -> T,
    {
        let mut results = Vec::with_capacity(self.children.len());
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            results.push(f(handle));
        }
        results
    }

    /// Folds over children, accumulating a value.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let total_height = ctx.children.fold(0.0, |acc, mut child| {
    ///     acc + child.layout(constraints).height
    /// });
    /// ```
    pub fn fold<T, F>(&mut self, init: T, mut f: F) -> T
    where
        F: FnMut(T, ChildHandle<'_, P>) -> T,
    {
        let mut acc = init;
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            acc = f(acc, handle);
        }
        acc
    }

    /// Returns true if any child matches the predicate.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if ctx.children.any(|child| child.parent_data().flex > 0.0) {
    ///     // Has flexible children
    /// }
    /// ```
    pub fn any<F>(&mut self, mut predicate: F) -> bool
    where
        F: FnMut(ChildHandle<'_, P>) -> bool,
    {
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            if predicate(handle) {
                return true;
            }
        }
        false
    }

    /// Returns true if all children match the predicate.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if ctx.children.all(|child| child.size().width <= max_width) {
    ///     // All children fit
    /// }
    /// ```
    pub fn all<F>(&mut self, mut predicate: F) -> bool
    where
        F: FnMut(ChildHandle<'_, P>) -> bool,
    {
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            if !predicate(handle) {
                return false;
            }
        }
        true
    }

    /// Counts children matching a predicate.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let flex_count = ctx.children.count(|child| child.parent_data().flex > 0.0);
    /// ```
    pub fn count<F>(&mut self, mut predicate: F) -> usize
    where
        F: FnMut(ChildHandle<'_, P>) -> bool,
    {
        let mut count = 0;
        for state in self.children.iter_mut() {
            let handle =
                ChildHandle::new(state.id, state.size, state.offset, &mut state.parent_data);
            if predicate(handle) {
                count += 1;
            }
        }
        count
    }

    /// Sums a value from each child.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let total_flex: f32 = ctx.children.sum(|child| child.parent_data().flex);
    /// ```
    pub fn sum<T, F>(&mut self, mut f: F) -> T
    where
        T: std::iter::Sum + Default,
        F: FnMut(ChildHandle<'_, P>) -> T,
    {
        self.map(|child| f(child)).into_iter().sum()
    }
}

// ============================================================================
// Debug
// ============================================================================

impl<A: Arity, P: ParentData + Default> std::fmt::Debug for ChildrenAccess<'_, A, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildrenAccess")
            .field("arity", &std::any::type_name::<A>())
            .field("parent_data", &std::any::type_name::<P>())
            .field("child_count", &self.children.len())
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parent_data::BoxParentData;

    #[test]
    fn test_children_access_empty() {
        let mut children: Vec<ChildState<BoxParentData>> = vec![];
        let access: ChildrenAccess<'_, Variable, BoxParentData> =
            ChildrenAccess::new(&mut children);

        assert!(access.is_empty());
        assert_eq!(access.len(), 0);
    }

    #[test]
    fn test_optional_none() {
        let mut children: Vec<ChildState<BoxParentData>> = vec![];
        let mut access: ChildrenAccess<'_, Optional, BoxParentData> =
            ChildrenAccess::new(&mut children);

        assert!(access.is_none());
        assert!(access.get().is_none());
    }

    #[test]
    fn test_optional_some() {
        let mut children = vec![ChildState::new(RenderId::new(1))];
        let mut access: ChildrenAccess<'_, Optional, BoxParentData> =
            ChildrenAccess::new(&mut children);

        assert!(access.is_some());
        let child = access.get();
        assert!(child.is_some());
    }

    #[test]
    fn test_single_get() {
        let mut children = vec![ChildState::new(RenderId::new(1))];
        let mut access: ChildrenAccess<'_, Single, BoxParentData> =
            ChildrenAccess::new(&mut children);

        let child = access.get();
        assert_eq!(child.id().get(), 1);
    }

    #[test]
    fn test_variable_for_each() {
        let mut children = vec![
            ChildState::new(RenderId::new(1)),
            ChildState::new(RenderId::new(2)),
            ChildState::new(RenderId::new(3)),
        ];
        let mut access: ChildrenAccess<'_, Variable, BoxParentData> =
            ChildrenAccess::new(&mut children);

        let mut count = 0;
        access.for_each(|_child| {
            count += 1;
        });

        assert_eq!(count, 3);
    }

    #[test]
    fn test_variable_map() {
        let mut children = vec![
            ChildState::new(RenderId::new(1)),
            ChildState::new(RenderId::new(2)),
            ChildState::new(RenderId::new(3)),
        ];
        let mut access: ChildrenAccess<'_, Variable, BoxParentData> =
            ChildrenAccess::new(&mut children);

        let ids: Vec<usize> = access.map(|child| child.id().get());
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_variable_fold() {
        let mut children = vec![
            ChildState::new(RenderId::new(1)),
            ChildState::new(RenderId::new(2)),
            ChildState::new(RenderId::new(3)),
        ];
        let mut access: ChildrenAccess<'_, Variable, BoxParentData> =
            ChildrenAccess::new(&mut children);

        let sum: usize = access.fold(0, |acc, child| acc + child.id().get());
        assert_eq!(sum, 6);
    }
}
