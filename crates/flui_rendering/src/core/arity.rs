//! Arity system re-exports from flui-tree.
//!
//! This module re-exports the unified arity system from `flui-tree` for use
//! in rendering operations. The arity system provides compile-time child count
//! validation through GAT-based accessors.
//!
//! # Arity Types
//!
//! | Arity | Child Count | Use Cases |
//! |-------|-------------|-----------|
//! | [`Leaf`] | 0 | Text, Image, Spacer |
//! | [`Optional`] | 0-1 | Container, SizedBox |
//! | [`Single`] | 1 | Padding, Transform, Align |
//! | [`Variable`] | 0+ | Flex, Stack, Column, Row |
//! | [`Exact<N>`] | N | Grid cells, custom layouts |
//! | [`AtLeast<N>`] | N+ | TabBar, MenuBar |
//! | [`Range<MIN, MAX>`] | MIN-MAX | Carousel, PageView |
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_rendering::core::{Single, Variable, Leaf};
//!
//! // Single child - exactly one child required
//! impl RenderBox<Single> for RenderPadding { ... }
//!
//! // Variable children - any number of children
//! impl RenderBox<Variable> for RenderFlex { ... }
//!
//! // Leaf - no children
//! impl RenderBox<Leaf> for RenderText { ... }
//! ```

use flui_foundation::ElementId;

// ============================================================================
// CORE RE-EXPORTS FROM FLUI-TREE
// ============================================================================

// Core arity trait and markers
pub use flui_tree::arity::{
    // Access pattern hints
    AccessPattern,
    // Core trait
    Arity,
    // Arity markers
    AtLeast,
    Exact,
    Leaf,
    Optional,
    Range,
    // Runtime arity for dynamic dispatch
    RuntimeArity,
    Single,
    Variable,
};

// GAT-based accessor types
pub use flui_tree::arity::{
    // Core accessor trait
    ChildrenAccess,
    // Concrete accessor implementations
    FixedChildren,
    NoChildren,
    OptionalChild,
    SliceChildren,
};

// ============================================================================
// CONVENIENCE TYPE ALIASES
// ============================================================================

/// Children accessor for Leaf arity (no children).
pub type LeafChildren = NoChildren<ElementId>;

/// Children accessor for Optional arity (0 or 1 child).
pub type OptionalChildren<'a> = OptionalChild<'a, ElementId>;

/// Children accessor for Single arity (exactly 1 child).
pub type SingleChildren<'a> = FixedChildren<'a, ElementId, 1>;

/// Children accessor for Variable arity (any number of children).
pub type VariableChildren<'a> = SliceChildren<'a, ElementId>;

/// Children accessor for Exact<N> arity (exactly N children).
pub type ExactChildren<'a, const N: usize> = FixedChildren<'a, ElementId, N>;

// ============================================================================
// RENDERING-SPECIFIC EXTENSION
// ============================================================================

/// Extension trait for children accessors with ElementId-specific operations.
///
/// Provides convenience methods for working with `ElementId` children
/// that are commonly needed in rendering operations.
pub trait RenderChildrenExt<'a>: ChildrenAccess<'a, ElementId> {
    /// Returns an iterator over ElementIds by value (copied).
    #[inline]
    fn element_ids(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.iter().copied()
    }

    /// Returns the first ElementId, if any.
    #[inline]
    fn first_id(&self) -> Option<ElementId> {
        self.as_slice().first().copied()
    }

    /// Returns the last ElementId, if any.
    #[inline]
    fn last_id(&self) -> Option<ElementId> {
        self.as_slice().last().copied()
    }

    /// Returns ElementId at the given index, if valid.
    #[inline]
    fn get_id(&self, index: usize) -> Option<ElementId> {
        self.as_slice().get(index).copied()
    }

    /// Checks if the accessor contains the given ElementId.
    #[inline]
    fn contains(&self, id: ElementId) -> bool {
        self.as_slice().contains(&id)
    }

    /// Counts ElementIds matching the predicate.
    #[inline]
    fn count_matching<F>(&self, predicate: F) -> usize
    where
        F: Fn(&ElementId) -> bool,
    {
        self.as_slice().iter().filter(|id| predicate(id)).count()
    }
}

// Blanket implementation for all accessors over ElementId
impl<'a, A> RenderChildrenExt<'a> for A where A: ChildrenAccess<'a, ElementId> {}

// ============================================================================
// ARITY-SPECIFIC EXTENSIONS
// ============================================================================

/// Extension trait for Optional arity with ElementId convenience methods.
pub trait OptionalChildExt<'a> {
    /// Gets the optional ElementId, if present.
    fn child_id(&self) -> Option<ElementId>;
}

impl<'a> OptionalChildExt<'a> for OptionalChild<'a, ElementId> {
    #[inline]
    fn child_id(&self) -> Option<ElementId> {
        self.get().copied()
    }
}

/// Extension trait for Single arity with ElementId convenience methods.
pub trait SingleChildExt<'a> {
    /// Gets the single child ElementId.
    fn single_child(&self) -> Option<ElementId>;
}

impl<'a> SingleChildExt<'a> for FixedChildren<'a, ElementId, 1> {
    #[inline]
    fn single_child(&self) -> Option<ElementId> {
        self.as_slice().first().copied()
    }
}

/// Extension trait for Variable arity with ElementId convenience methods.
pub trait VariableChildExt<'a> {
    /// Returns the number of children.
    fn child_count(&self) -> usize;
}

impl<'a> VariableChildExt<'a> for SliceChildren<'a, ElementId> {
    #[inline]
    fn child_count(&self) -> usize {
        self.len()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_children_ext() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);

        assert_eq!(accessor.first_id(), Some(ElementId::new(1)));
        assert_eq!(accessor.last_id(), Some(ElementId::new(3)));
        assert_eq!(accessor.get_id(1), Some(ElementId::new(2)));
        assert!(accessor.contains(ElementId::new(2)));
        assert!(!accessor.contains(ElementId::new(99)));

        let count = accessor.count_matching(|id| id.get() > 1);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_optional_child_ext() {
        let children = [ElementId::new(42)];
        let accessor = Optional::from_slice(&children);
        assert_eq!(accessor.child_id(), Some(ElementId::new(42)));

        let empty: [ElementId; 0] = [];
        let empty_accessor = Optional::from_slice(&empty);
        assert_eq!(empty_accessor.child_id(), None);
    }

    #[test]
    fn test_single_child_ext() {
        let children = [ElementId::new(42)];
        let accessor = Single::from_slice(&children);
        assert_eq!(accessor.single_child(), Some(ElementId::new(42)));
    }

    #[test]
    fn test_variable_child_ext() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);
        assert_eq!(accessor.child_count(), 3);
    }

    #[test]
    fn test_type_aliases() {
        let _: LeafChildren = NoChildren::new();

        let children = [ElementId::new(1)];
        let _: OptionalChildren = Optional::from_slice(&children);
        let _: SingleChildren = Single::from_slice(&children);

        let children = [ElementId::new(1), ElementId::new(2)];
        let _: VariableChildren = Variable::from_slice(&children);
        let _: ExactChildren<2> = Exact::<2>::from_slice(&children);
    }
}
