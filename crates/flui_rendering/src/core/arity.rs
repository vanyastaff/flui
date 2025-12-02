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
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_children_access() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);

        assert_eq!(
            accessor.as_slice().first().copied(),
            Some(ElementId::new(1))
        );
        assert_eq!(accessor.as_slice().last().copied(), Some(ElementId::new(3)));
        assert_eq!(accessor.as_slice().get(1).copied(), Some(ElementId::new(2)));
        assert!(accessor.as_slice().contains(&ElementId::new(2)));
        assert!(!accessor.as_slice().contains(&ElementId::new(99)));
    }

    #[test]
    fn test_optional_child() {
        let children = [ElementId::new(42)];
        let accessor = Optional::from_slice(&children);
        assert_eq!(accessor.get().copied(), Some(ElementId::new(42)));

        let empty: [ElementId; 0] = [];
        let empty_accessor = Optional::from_slice(&empty);
        assert_eq!(empty_accessor.get().copied(), None);
    }

    #[test]
    fn test_single_child() {
        let children = [ElementId::new(42)];
        let accessor = Single::from_slice(&children);
        assert_eq!(
            accessor.as_slice().first().copied(),
            Some(ElementId::new(42))
        );
    }

    #[test]
    fn test_variable_children() {
        let children = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let accessor = Variable::from_slice(&children);
        assert_eq!(accessor.len(), 3);
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
