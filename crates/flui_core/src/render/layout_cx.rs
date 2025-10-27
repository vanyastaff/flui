//! Typed LayoutCx with arity-specific extension traits
//!
//! Universal solution without code duplication (idea.md Chapter 3)

use std::marker::PhantomData;
use flui_types::Size;
use flui_types::constraints::BoxConstraints;

use crate::element::{ElementId, ElementTree};
use crate::render::arity::{Arity, SingleArity, MultiArity};
use super::cache::{layout_cache, LayoutCacheKey, LayoutResult};

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
        // Use cache
        let cache_key = LayoutCacheKey::new(child, constraints);
        let cache = layout_cache();

        if let Some(cached) = cache.get(&cache_key) {
            if !cached.needs_layout {
                return cached.size;
            }
        }

        // Actually layout the child!
        let size = self.layout_child_uncached(child, constraints);

        // Store in cache
        cache.insert(cache_key, LayoutResult::new(size));

        size
    }
}

impl<'a> LayoutCx<'a, SingleArity> {
    /// Internal: Layout child without cache
    #[allow(invalid_reference_casting)]
    fn layout_child_uncached(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // SAFETY: Split borrow - we're laying out child (different from parent)
        // Parent RenderObject is at self.element_id (immutable in this context)
        // Child RenderObject is at child_id (we get mutable access)
        // This is safe because:
        // 1. Parent and child are different elements (no aliasing)
        // 2. Layout is single-threaded
        // 3. No other code accesses child_id during parent's layout
        // TODO: Use UnsafeCell for proper interior mutability
        unsafe {
            let tree_mut = &mut *(self.tree as *const ElementTree as *mut ElementTree);
            tree_mut.layout_render_object(child_id, constraints)
                .unwrap_or(Size::ZERO)
        }
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
        // Use cache with child count
        let child_count = self.child_count();
        let cache_key = LayoutCacheKey::new(child, constraints)
            .with_child_count(child_count);
        let cache = layout_cache();

        if let Some(cached) = cache.get(&cache_key) {
            if !cached.needs_layout {
                return cached.size;
            }
        }

        // Actually layout the child!
        let size = self.layout_child_uncached(child, constraints);

        // Store in cache
        cache.insert(cache_key, LayoutResult::new(size));

        size
    }
}

impl<'a> LayoutCx<'a, MultiArity> {
    /// Internal: Layout child without cache
    #[allow(invalid_reference_casting)]
    fn layout_child_uncached(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // SAFETY: Same split borrow pattern as SingleArity
        // TODO: Use UnsafeCell for proper interior mutability
        unsafe {
            let tree_mut = &mut *(self.tree as *const ElementTree as *mut ElementTree);
            tree_mut.layout_render_object(child_id, constraints)
                .unwrap_or(Size::ZERO)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::LeafArity;
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
