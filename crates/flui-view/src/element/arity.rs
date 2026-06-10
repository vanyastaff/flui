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

// Re-export arity types from flui-tree for consistency with RenderObject system
pub use flui_tree::{Arity, Leaf, Optional, Single, Variable};

/// Element-specific arity marker trait.
///
/// E3 (atomic box→arena swap): this used to carry a `type Storage:
/// ElementChildStorage` associated type that owned the per-element box
/// child graph. The slab-resident [`ElementTree`](crate::tree::ElementTree)
/// is now the single element graph, so there is no per-element storage to
/// associate. The trait remains as the compile-time child-count constraint
/// (`Leaf` = 0, `Single` = 1, `Optional` = 0..=1, `Variable` = N) layered
/// over `flui_tree::Arity`; it simply no longer carries storage.
///
/// This mirrors `RenderObject` arity in `flui_rendering`, providing
/// compile-time guarantees about child count while letting generic code
/// work with any arity.
pub trait ElementArity: Arity {}

/// Leaf arity - no children allowed.
///
/// Used for elements that cannot have children (terminal nodes in the tree).
impl ElementArity for Leaf {}

/// Single arity - exactly one child required.
///
/// Used for elements that wrap a single child (e.g., StatelessElement,
/// ProxyElement).
impl ElementArity for Single {}

/// Optional arity - zero or one child allowed.
///
/// Used for elements that may or may not have a child.
impl ElementArity for Optional {}

/// Variable arity - N children allowed.
///
/// Used for elements that can have multiple children (e.g., RenderElement).
impl ElementArity for Variable {}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time checks that traits are properly implemented
    fn assert_element_arity_implemented<A: ElementArity>() {}

    #[test]
    fn test_arity_types_implement_element_arity() {
        assert_element_arity_implemented::<Leaf>();
        assert_element_arity_implemented::<Single>();
        assert_element_arity_implemented::<Optional>();
        assert_element_arity_implemented::<Variable>();
    }
}
