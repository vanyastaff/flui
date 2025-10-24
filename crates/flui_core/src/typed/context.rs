//! Typed contexts for layout and paint operations
//!
//! **NOTE**: This is a proof-of-concept implementation demonstrating the typed arity system.
//! It is intentionally simplified and does not yet integrate with ElementTree/RenderContext.

use std::marker::PhantomData;

use flui_types::{Offset, Size};
use flui_types::constraints::BoxConstraints;

use crate::ElementId;
use super::arity::{LeafArity, SingleArity, MultiArity};
use super::render_object::RenderObject;

/// Typed layout context - specialized by RenderObject arity
pub struct LayoutCx<'a, R: RenderObject> {
    constraints: BoxConstraints,
    children: Vec<ElementId>,
    child: Option<ElementId>,
    _phantom: PhantomData<&'a R>,
}

impl<'a, R: RenderObject> LayoutCx<'a, R> {
    /// Create a new layout context (proof-of-concept constructor)
    pub fn new_with_constraints(constraints: BoxConstraints) -> Self {
        Self {
            constraints,
            children: Vec::new(),
            child: None,
            _phantom: PhantomData,
        }
    }

    /// Get the constraints
    pub fn constraints(&self) -> BoxConstraints {
        self.constraints
    }
}

// ===== LeafArity specialization =====
// No child access methods - leaf objects have no children

impl<'a, R> LayoutCx<'a, R>
where
    R: RenderObject<Arity = LeafArity>,
{
    // Leaf has no special methods - only constraints() from base impl
}

// ===== SingleArity specialization =====

impl<'a, R> LayoutCx<'a, R>
where
    R: RenderObject<Arity = SingleArity>,
{
    /// Get the single child (only available for SingleArity!)
    ///
    /// This method demonstrates the compile-time safety: it's only
    /// available when R::Arity = SingleArity.
    pub fn child(&self) -> ElementId {
        self.child.expect("Single arity must have exactly one child")
    }

    /// Layout the single child (stub for proof-of-concept)
    pub fn layout_single_child(&self, _child: ElementId, constraints: BoxConstraints) -> Size {
        // In real implementation, this would recursively layout the child
        constraints.smallest()
    }
}

// ===== MultiArity specialization =====

impl<'a, R> LayoutCx<'a, R>
where
    R: RenderObject<Arity = MultiArity>,
{
    /// Get all children (only available for MultiArity!)
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get child count
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Layout a child (stub for proof-of-concept)
    pub fn layout_child(&self, _child: ElementId, constraints: BoxConstraints) -> Size {
        // In real implementation, this would recursively layout the child
        constraints.smallest()
    }
}

// ========== PaintCx ==========

/// Typed paint context - specialized by RenderObject arity
pub struct PaintCx<'a, R: RenderObject> {
    painter: &'a egui::Painter,
    offset: Offset,
    children: Vec<ElementId>,
    child: Option<ElementId>,
    _phantom: PhantomData<R>,
}

impl<'a, R: RenderObject> PaintCx<'a, R> {
    /// Create a new paint context (proof-of-concept constructor)
    pub fn new_with_painter(painter: &'a egui::Painter, offset: Offset) -> Self {
        Self {
            painter,
            offset,
            children: Vec::new(),
            child: None,
            _phantom: PhantomData,
        }
    }

    /// Get the painter
    pub fn painter(&self) -> &egui::Painter {
        self.painter
    }

    /// Get the offset
    pub fn offset(&self) -> Offset {
        self.offset
    }
}

// ===== LeafArity specialization =====

impl<'a, R> PaintCx<'a, R>
where
    R: RenderObject<Arity = LeafArity>,
{
    // Leaf has no special methods - can only paint itself
}

// ===== SingleArity specialization =====

impl<'a, R> PaintCx<'a, R>
where
    R: RenderObject<Arity = SingleArity>,
{
    /// Get the single child (only available for SingleArity!)
    pub fn child(&self) -> ElementId {
        self.child.expect("Single arity must have exactly one child")
    }

    /// Paint the single child (stub for proof-of-concept)
    pub fn paint_single_child(&self, _child: ElementId) {
        // In real implementation, this would recursively paint the child
    }

    /// Paint the single child at offset (stub)
    pub fn paint_single_child_at(&self, _child: ElementId, _offset: Offset) {
        // In real implementation, this would recursively paint the child
    }
}

// ===== MultiArity specialization =====

impl<'a, R> PaintCx<'a, R>
where
    R: RenderObject<Arity = MultiArity>,
{
    /// Get all children (only available for MultiArity!)
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Paint a child (stub for proof-of-concept)
    pub fn paint_child(&self, _child: ElementId) {
        // In real implementation, this would recursively paint the child
    }

    /// Paint a child at offset (stub)
    pub fn paint_child_at(&self, _child: ElementId, _offset: Offset) {
        // In real implementation, this would recursively paint the child
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed::render_object::RenderObject;

    // Demonstrate that different arities get different methods

    #[derive(Debug)]
    struct TestLeaf;

    impl RenderObject for TestLeaf {
        type Arity = LeafArity;

        fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
            // Can access constraints
            let _c = cx.constraints();

            // These would be compile errors:
            // let _child = cx.child(); // ERROR!
            // let _children = cx.children(); // ERROR!

            Size::ZERO
        }

        fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
            // Leaf paint - no child methods available
        }
    }

    #[derive(Debug)]
    struct TestSingle;

    impl RenderObject for TestSingle {
        type Arity = SingleArity;

        fn layout<'a>(&mut self, _cx: &mut LayoutCx<'a, Self>) -> Size {
            // Can access single child
            // let _child = cx.child(); // Would work if we had a child set

            // This would be compile error:
            // let _children = cx.children(); // ERROR!

            Size::ZERO
        }

        fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
            // Single paint - cx.child() available
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl RenderObject for TestMulti {
        type Arity = MultiArity;

        fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
            // Can access multiple children
            let _children = cx.children();
            let _count = cx.child_count();

            // This would be compile error:
            // let _child = cx.child(); // ERROR!

            Size::ZERO
        }

        fn paint<'a>(&self, _cx: &mut PaintCx<'a, Self>) {
            // Multi paint - cx.children() available
        }
    }

    #[test]
    fn test_contexts_compile() {
        // Just verify that all types compile correctly
        let _leaf = TestLeaf;
        let _single = TestSingle;
        let _multi = TestMulti;
    }
}
