//! Typed Arity system for RenderObjects
//!
//! This module implements compile-time child count constraints through the type system.
//! Instead of runtime checks for child count, we use generic types to enforce
//! correct usage at compile time.

use crate::ElementId;

/// Marker trait for RenderObject arity (number of children)
///
/// This trait is implemented by:
/// - `LeafArity`: No children allowed
/// - `SingleArity`: Exactly one child required
/// - `MultiArity`: Zero or more children allowed
///
/// The arity type is used to specialize `LayoutCx` and `PaintCx` methods,
/// ensuring compile-time safety for child access patterns.
pub trait RenderArity: Send + Sync + 'static {
    /// Human-readable name for debugging
    fn name() -> &'static str;

    /// Optional compile-time child count constraint
    const CHILD_COUNT: Option<usize> = None;
}

/// Leaf arity: RenderObject with no children
///
/// Examples: `RenderParagraph`, `RenderImage`
///
/// When a RenderObject has `type Arity = LeafArity`, the context will:
/// - NOT provide `.child()` or `.children()` methods
/// - Prevent accidentally trying to layout/paint children
#[derive(Debug, Clone, Copy)]
pub struct LeafArity;

impl RenderArity for LeafArity {
    fn name() -> &'static str {
        "Leaf"
    }

    const CHILD_COUNT: Option<usize> = Some(0);
}

/// Single arity: RenderObject with exactly one child
///
/// Examples: `RenderOpacity`, `RenderPadding`, `RenderTransform`
///
/// When a RenderObject has `type Arity = SingleArity`, the context will:
/// - Provide `.child()` method to get the single child
/// - NOT provide `.children()` iterator (compile error)
#[derive(Debug, Clone, Copy)]
pub struct SingleArity;

impl RenderArity for SingleArity {
    fn name() -> &'static str {
        "Single"
    }

    const CHILD_COUNT: Option<usize> = Some(1);
}

/// Multi arity: RenderObject with zero or more children
///
/// Examples: `RenderFlex`, `RenderStack`, `RenderWrap`
///
/// When a RenderObject has `type Arity = MultiArity`, the context will:
/// - Provide `.children()` method to iterate children
/// - NOT provide `.child()` method (compile error)
#[derive(Debug, Clone, Copy)]
pub struct MultiArity;

impl RenderArity for MultiArity {
    fn name() -> &'static str {
        "Multi"
    }

    const CHILD_COUNT: Option<usize> = None; // Variable count
}

/// Trait for accessing children in a type-safe way
///
/// This is implemented by the context types based on the arity.
pub trait ChildAccess {
    /// Get single child (only available for SingleArity)
    fn child(&self) -> ElementId {
        unimplemented!("child() is only available for SingleArity")
    }

    /// Get all children (only available for MultiArity)
    fn children(&self) -> &[ElementId] {
        unimplemented!("children() is only available for MultiArity")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arity_names() {
        assert_eq!(LeafArity::name(), "Leaf");
        assert_eq!(SingleArity::name(), "Single");
        assert_eq!(MultiArity::name(), "Multi");
    }

    #[test]
    fn test_child_counts() {
        assert_eq!(LeafArity::CHILD_COUNT, Some(0));
        assert_eq!(SingleArity::CHILD_COUNT, Some(1));
        assert_eq!(MultiArity::CHILD_COUNT, None);
    }
}
