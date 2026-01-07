//! Element arity system for compile-time child count constraints.
//!
//! This module provides a type-safe way to express element child constraints
//! at compile time using the arity system from `flui-tree`.
//!
//! # Arity Types
//!
//! - `Leaf` - No children (e.g., text, images)
//! - `Single` - Exactly one child (e.g., StatelessElement, ProxyElement)
//! - `Optional` - Zero or one child
//! - `Variable` - N children (e.g., RenderElement with multiple children)
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::element::{ElementCore, Single};
//!
//! // StatelessElement has exactly one child
//! pub struct StatelessElement<V> {
//!     core: ElementCore<V, Single>,
//! }
//! ```

use super::child_storage::*;

// Re-export arity types from flui-tree for consistency with RenderObject system
pub use flui_tree::{Arity, Leaf, Optional, Single, Variable};

/// Element-specific arity trait linking arity types to child storage.
///
/// This trait extends the base `Arity` trait from `flui-tree` to associate
/// each arity type with a concrete `ElementChildStorage` implementation.
///
/// # Design Pattern
///
/// This follows the same pattern as `RenderObject` arity in `flui_rendering`,
/// providing compile-time guarantees about child count while enabling
/// generic code to work with any arity.
pub trait ElementArity: Arity {
    /// The storage type for managing children of this arity.
    ///
    /// This associated type determines how child elements are stored
    /// and managed based on the arity constraint.
    type Storage: ElementChildStorage;
}

/// Leaf arity - no children allowed.
///
/// Used for elements that cannot have children (terminal nodes in the tree).
impl ElementArity for Leaf {
    type Storage = NoChildStorage;
}

/// Single arity - exactly one child required.
///
/// Used for elements that wrap a single child (e.g., StatelessElement, ProxyElement).
impl ElementArity for Single {
    type Storage = SingleChildStorage;
}

/// Optional arity - zero or one child allowed.
///
/// Used for elements that may or may not have a child.
impl ElementArity for Optional {
    type Storage = OptionalChildStorage;
}

/// Variable arity - N children allowed.
///
/// Used for elements that can have multiple children (e.g., RenderElement).
impl ElementArity for Variable {
    type Storage = VariableChildStorage;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time checks that traits are properly implemented
    fn _assert_element_arity_implemented<A: ElementArity>() {}

    #[test]
    fn test_arity_types_implement_element_arity() {
        _assert_element_arity_implemented::<Leaf>();
        _assert_element_arity_implemented::<Single>();
        _assert_element_arity_implemented::<Optional>();
        _assert_element_arity_implemented::<Variable>();
    }
}
