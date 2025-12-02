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

// Core arity trait and markers
pub use flui_tree::arity::{
    AccessPattern, Arity, AtLeast, Exact, Leaf, Optional, Range, RuntimeArity, Single, Variable,
};

// GAT-based accessor types
pub use flui_tree::arity::{
    ChildrenAccess, FixedChildren, NoChildren, OptionalChild, SliceChildren,
};
