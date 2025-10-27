//! Typed PaintCx with arity-specific extension traits
//!
//! Universal solution without code duplication (idea.md Chapter 3)

use std::marker::PhantomData;
use flui_types::Offset;
use flui_engine::BoxedLayer;

use crate::element::{ElementId, ElementTree};
use crate::render::arity::{Arity, SingleArity, MultiArity};

/// Typed paint context
///
/// **Universal design with extension traits**:
/// - Base impl provides common methods for ALL arities
/// - Extension traits (SingleChildPaint, MultiChildPaint) add arity-specific methods
/// - No code duplication!
pub struct PaintCx<'a, A: Arity> {
    /// Element tree reference
    tree: &'a ElementTree,

    /// Current element ID
    element_id: ElementId,

    /// Painting offset
    offset: Offset,

    /// Phantom data for arity type
    _phantom: PhantomData<A>,
}

// ========== Base Implementation (ALL Arities) ==========

impl<'a, A: Arity> PaintCx<'a, A> {
    /// Create a new paint context
    pub fn new(tree: &'a ElementTree, element_id: ElementId, offset: Offset) -> Self {
        Self {
            tree,
            element_id,
            offset,
            _phantom: PhantomData,
        }
    }

    /// Get the painting offset
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Get current element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get tree reference
    pub fn tree(&self) -> &ElementTree {
        self.tree
    }

    /// Create a new context with different offset
    pub fn with_offset(&self, offset: Offset) -> Self {
        Self {
            tree: self.tree,
            element_id: self.element_id,
            offset,
            _phantom: PhantomData,
        }
    }
}

// ========== SingleChildPaint Extension Trait ==========

/// Extension trait for single-child paint operations
///
/// Only available for `PaintCx<SingleArity>`.
/// This provides `.child()` and `.capture_child_layer()` methods.
pub trait SingleChildPaint {
    /// Get the single child element ID
    fn child(&self) -> ElementId;

    /// Capture the child's layer for composition
    fn capture_child_layer(&self, child: ElementId) -> BoxedLayer;
}

impl<'a> SingleChildPaint for PaintCx<'a, SingleArity> {
    fn child(&self) -> ElementId {
        let children = self.tree.children(self.element_id);
        assert_eq!(children.len(), 1, "SingleArity must have exactly one child");
        children[0]
    }

    fn capture_child_layer(&self, child: ElementId) -> BoxedLayer {
        // Actually paint the child!
        self.capture_child_layer_uncached(child)
    }
}

impl<'a> PaintCx<'a, SingleArity> {
    /// Internal: Paint child without cache
    #[allow(invalid_reference_casting)]
    fn capture_child_layer_uncached(&self, child_id: ElementId) -> BoxedLayer {
        // SAFETY: Split borrow - we're painting child (different from parent)
        // Parent RenderObject is at self.element_id (immutable in this context)
        // Child RenderObject is at child_id (we get mutable access)
        // This is safe because:
        // 1. Parent and child are different elements (no aliasing)
        // 2. Paint is single-threaded
        // 3. No other code accesses child_id during parent's paint
        // TODO: Use UnsafeCell for proper interior mutability
        unsafe {
            let tree_ref = &*(self.tree as *const ElementTree);
            tree_ref.paint_render_object(child_id, self.offset)
                .unwrap_or_else(|| Box::new(flui_engine::ContainerLayer::new()))
        }
    }
}

// ========== MultiChildPaint Extension Trait ==========

/// Extension trait for multi-child paint operations
///
/// Only available for `PaintCx<MultiArity>`.
/// This provides `.children()` and `.capture_child_layers()` methods.
pub trait MultiChildPaint {
    /// Get all child element IDs
    fn children(&self) -> Vec<ElementId>;

    /// Capture a single child's layer
    fn capture_child_layer(&self, child: ElementId) -> BoxedLayer;

    /// Capture all children's layers
    fn capture_child_layers(&self) -> Vec<BoxedLayer> {
        self.children()
            .iter()
            .map(|&child| self.capture_child_layer(child))
            .collect()
    }
}

impl<'a> MultiChildPaint for PaintCx<'a, MultiArity> {
    fn children(&self) -> Vec<ElementId> {
        self.tree.children(self.element_id)
    }

    fn capture_child_layer(&self, child: ElementId) -> BoxedLayer {
        // Actually paint the child!
        self.capture_child_layer_uncached(child)
    }
}

impl<'a> PaintCx<'a, MultiArity> {
    /// Internal: Paint child without cache
    #[allow(invalid_reference_casting)]
    fn capture_child_layer_uncached(&self, child_id: ElementId) -> BoxedLayer {
        // SAFETY: Same split borrow pattern as SingleArity
        // TODO: Use UnsafeCell for proper interior mutability
        unsafe {
            let tree_ref = &*(self.tree as *const ElementTree);
            tree_ref.paint_render_object(child_id, self.offset)
                .unwrap_or_else(|| Box::new(flui_engine::ContainerLayer::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::LeafArity;
    use crate::{RenderObject, LayoutCx};
    use flui_types::Size;
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
    fn test_paint_cx_creation() {
        let tree = ElementTree::new();
        let cx = PaintCx::<LeafArity>::new(&tree, 0, Offset::ZERO);

        assert_eq!(cx.offset(), Offset::ZERO);
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_single_child_paint_extension() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let cx = PaintCx::<SingleArity>::new(&tree, parent_id, Offset::ZERO);

        // SingleChildPaint trait methods available!
        assert_eq!(cx.child(), child_id);

        let _layer = cx.capture_child_layer(child_id);
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_multi_child_paint_extension() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child1_id = tree.insert(Some(parent_id), Box::new(TestRender));
        let child2_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let cx = PaintCx::<MultiArity>::new(&tree, parent_id, Offset::ZERO);

        // MultiChildPaint trait methods available!
        assert_eq!(cx.children(), &[child1_id, child2_id]);

        let layers = cx.capture_child_layers();
        assert_eq!(layers.len(), 2);
    }
}
