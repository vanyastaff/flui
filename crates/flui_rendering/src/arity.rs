//! Arity system for compile-time child count constraints.
//!
//! This module re-exports the Arity types from `flui-tree` for use in
//! the rendering system. Arity determines how many children a render
//! object can have at compile time.
//!
//! # Arity Types
//!
//! | Type       | Children | Use Case                        |
//! |------------|----------|---------------------------------|
//! | `Leaf`     | 0        | Text, Image, ColoredBox         |
//! | `Single`   | 1        | Padding, Center, Align          |
//! | `Optional` | 0 or 1   | Container, DecoratedBox         |
//! | `Variable` | 0+       | Row, Column, Stack, Flex        |
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::arity::{Arity, Leaf, Single, Variable};
//! use flui_rendering::traits::RenderBox;
//!
//! // Leaf - no children
//! struct RenderText { text: String }
//! impl RenderBox for RenderText {
//!     type Arity = Leaf;
//!     // ...
//! }
//!
//! // Single - exactly one child
//! struct RenderPadding { padding: EdgeInsets }
//! impl RenderBox for RenderPadding {
//!     type Arity = Single;
//!     // ...
//! }
//!
//! // Variable - any number of children
//! struct RenderColumn { children: Vec<...> }
//! impl RenderBox for RenderColumn {
//!     type Arity = Variable;
//!     // ...
//! }
//! ```

// Re-export from flui-tree
pub use flui_tree::{
    Arity, ArityStorage, ArityStorageView, ChildrenAccess as TreeChildrenAccess, Leaf, Optional,
    Single, Variable,
};
