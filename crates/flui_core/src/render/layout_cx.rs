//! Typed LayoutCx with arity-specific extension traits
//!
//! Universal solution without code duplication (idea.md Chapter 3)

use std::marker::PhantomData;
use flui_types::Size;
use flui_types::constraints::BoxConstraints;

use crate::element::{ElementId, ElementTree};
use crate::render::arity::{Arity, SingleArity, MultiArity};

/// Typed layout context
///
/// **Universal design with extension traits**:
/// - Base impl provides common methods for ALL arities
/// - Extension traits (SingleChild, MultiChild) add arity-specific methods
/// - No code duplication!
///
/// ```text
/// LayoutCx<A>
///   ├─ Base methods (all arities): constraints(), etc.
///   ├─ + SingleChild trait (only SingleArity): child(), layout_child()
///   └─ + MultiChild trait (only MultiArity): children(), layout_child()
/// ```
pub struct LayoutCx<'a, A: Arity> {
    /// Element tree reference
    tree: &'a ElementTree,

    /// Current element ID
    element_id: ElementId,

    /// Layout constraints
    constraints: BoxConstraints,

    /// Phantom data for arity type
    _phantom: PhantomData<A>,
}

// ========== Base Implementation (ALL Arities) ==========

impl<'a, A: Arity> LayoutCx<'a, A> {
    /// Create a new layout context
    pub fn new(tree: &'a ElementTree, element_id: ElementId, constraints: BoxConstraints) -> Self {
        Self {
            tree,
            element_id,
            constraints,
            _phantom: PhantomData,
        }
    }

    /// Get the constraints
    pub fn constraints(&self) -> BoxConstraints {
        self.constraints
    }

    /// Get current element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get tree reference
    pub fn tree(&self) -> &ElementTree {
        self.tree
    }

    /// Get parent data for a child element
    ///
    /// Reads the ParentData attached to a child RenderElement and downcasts it
    /// to the requested concrete type. Returns `None` if:
    /// - The child doesn't exist
    /// - The child is not a RenderElement
    /// - The child has no parent data attached
    /// - The parent data cannot be downcast to type `T`
    ///
    /// # Type Parameters
    ///
    /// - `T`: The concrete ParentData type to downcast to (e.g., `FlexParentData`, `StackParentData`)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In RenderFlex::layout()
    /// for child in children {
    ///     if let Some(flex_data) = cx.parent_data::<FlexParentData>(child) {
    ///         if flex_data.flex > 0 {
    ///             // Child is flexible - allocate space proportionally
    ///             flexible_children.push((child, flex_data.flex));
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// ```rust,ignore
    /// // In RenderStack::layout()
    /// if let Some(stack_data) = cx.parent_data::<StackParentData>(child) {
    ///     if stack_data.is_positioned() {
    ///         // Child is positioned - use absolute positioning
    ///         let constraints = compute_positioned_constraints(stack_data, parent_size);
    ///         cx.layout_child(child, constraints);
    ///     }
    /// }
    /// ```
    pub fn parent_data<T>(&self, child_id: ElementId) -> Option<&T>
    where
        T: crate::render::ParentData + 'static,
    {
        self.tree
            .get(child_id)?
            .parent_data()?
            .downcast_ref::<T>()
    }
}

// ========== SingleChild Extension Trait ==========

/// Extension trait for single-child layout operations
///
/// Only available for `LayoutCx<SingleArity>`.
/// This provides `.child()` and `.layout_child()` methods.
pub trait SingleChild {
    /// Get the single child element ID
    fn child(&self) -> ElementId;

    /// Layout the single child
    fn layout_child(&self, child: ElementId, constraints: BoxConstraints) -> Size;
}

impl<'a> SingleChild for LayoutCx<'a, SingleArity> {
    fn child(&self) -> ElementId {
        let children = self.tree.children(self.element_id);
        assert_eq!(children.len(), 1, "SingleArity must have exactly one child");
        children[0]
    }

    fn layout_child(&self, child: ElementId, constraints: BoxConstraints) -> Size {
        // Layout child - RenderState caching is handled in layout_render_object()
        // No need for global cache here since RenderState provides per-object caching
        self.layout_child_uncached(child, constraints)
    }
}

impl<'a> LayoutCx<'a, SingleArity> {
    /// Internal: Layout child without cache
    fn layout_child_uncached(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // Safe: ElementTree::layout_render_object uses RefCell for interior mutability
        // Parent RenderObject is at self.element_id (immutable via self reference)
        // Child RenderObject is at child_id (borrowed mutably through RefCell)
        // RefCell provides runtime borrow checking to prevent aliasing
        self.tree
            .layout_render_object(child_id, constraints)
            .unwrap_or(Size::ZERO)
    }
}

// ========== MultiChild Extension Trait ==========

/// Extension trait for multi-child layout operations
///
/// Only available for `LayoutCx<MultiArity>`.
/// This provides `.children()` and `.layout_child()` methods.
pub trait MultiChild {
    /// Get all child element IDs
    fn children(&self) -> Vec<ElementId>;

    /// Get child count
    fn child_count(&self) -> usize {
        self.tree().child_count(self.element_id())
    }

    /// Get element tree
    fn tree(&self) -> &ElementTree;

    /// Get element ID
    fn element_id(&self) -> ElementId;

    /// Layout a child
    fn layout_child(&self, child: ElementId, constraints: BoxConstraints) -> Size;
}

impl<'a> MultiChild for LayoutCx<'a, MultiArity> {
    fn children(&self) -> Vec<ElementId> {
        self.tree.children(self.element_id)
    }

    fn tree(&self) -> &ElementTree {
        self.tree
    }

    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn layout_child(&self, child: ElementId, constraints: BoxConstraints) -> Size {
        // Layout child - RenderState caching is handled in layout_render_object()
        // No need for global cache here since RenderState provides per-object caching
        self.layout_child_uncached(child, constraints)
    }
}

impl<'a> LayoutCx<'a, MultiArity> {
    /// Internal: Layout child without cache
    fn layout_child_uncached(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // Safe: ElementTree::layout_render_object uses RefCell for interior mutability
        // Same safety reasoning as SingleArity version
        self.tree
            .layout_render_object(child_id, constraints)
            .unwrap_or(Size::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::arity::LeafArity;
    use crate::{RenderObject, PaintCx};
    use flui_engine::{BoxedLayer, ContainerLayer};

    // Test RenderObject for tests
    #[derive(Debug)]
    struct TestRender;

    impl RenderObject for TestRender {
        type Arity = LeafArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(10.0, 10.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_layout_cx_creation() {
        let tree = ElementTree::new();
        let cx = LayoutCx::<LeafArity>::new(&tree, 0, BoxConstraints::tight(Size::ZERO));

        assert_eq!(cx.constraints(), BoxConstraints::tight(Size::ZERO));
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_single_child_extension() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let cx = LayoutCx::<SingleArity>::new(&tree, parent_id, BoxConstraints::tight(Size::ZERO));

        // SingleChild trait methods available!
        assert_eq!(cx.child(), child_id);
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_multi_child_extension() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child1_id = tree.insert(Some(parent_id), Box::new(TestRender));
        let child2_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let cx = LayoutCx::<MultiArity>::new(&tree, parent_id, BoxConstraints::tight(Size::ZERO));

        // MultiChild trait methods available!
        assert_eq!(cx.children(), &[child1_id, child2_id]);
        assert_eq!(cx.child_count(), 2);
    }
}
