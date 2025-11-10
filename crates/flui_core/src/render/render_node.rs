//! RenderNode - unified render tree node
//!
//! This module defines the `RenderNode` struct which wraps the unified `Render` trait
//! and manages children storage inline for optimal performance.

use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use super::{Arity, Children, LayoutContext, PaintContext, Render};
use crate::element::ElementTree;

/// Unified render tree node
///
/// Wraps any render object implementing the unified `Render` trait.
/// Stores children inline for zero-cost access during layout and paint.
///
/// # Architecture
///
/// ```text
/// RenderNode {
///     render: Box<dyn Render>,  // ← Type-erased render object
///     children: Children,        // ← Inline storage (None/Single/Multi)
/// }
/// ```
///
/// # Performance
///
/// - **Inline children storage**: No separate HashMap or Vec lookup
/// - **Single trait dispatch**: One trait instead of three
/// - **Cache-friendly**: Children stored contiguously with render object
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::render::{RenderNode, Children};
///
/// // Leaf render (no children)
/// let node = RenderNode::new(Box::new(RenderParagraph::new("Hello")), Children::None);
///
/// // Single-child render
/// let node = RenderNode::new(Box::new(RenderPadding::new(EdgeInsets::all(10.0))), Children::from_single(child_id));
///
/// // Multi-child render
/// let node = RenderNode::new(Box::new(RenderFlex::row()), Children::from_multi(vec![id1, id2, id3]));
/// ```
#[derive(Debug)]
pub struct RenderNode {
    /// The render object (unified trait)
    ///
    /// Type-erased to `Box<dyn Render>` for storage in the element tree.
    /// Uses trait object for polymorphism while keeping element enum small.
    render: Box<dyn Render>,

    /// Children storage (inline for performance)
    ///
    /// Stores children directly in the node to avoid separate lookups.
    /// Three variants:
    /// - `Children::None` for leaf nodes (0 bytes overhead)
    /// - `Children::Single(id)` for single-child (8 bytes)
    /// - `Children::Multi(vec)` for multi-child (24 bytes)
    children: Children,
}

impl RenderNode {
    // ========== Constructors ==========

    /// Create a new render node
    ///
    /// Validates that the children count matches the render object's arity.
    ///
    /// # Parameters
    ///
    /// - `render`: The render object (must implement `Render` trait)
    /// - `children`: Children enum (None/Single/Multi)
    ///
    /// # Panics
    ///
    /// Panics if arity validation fails (children count doesn't match expected arity).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf node
    /// let node = RenderNode::new(
    ///     Box::new(RenderParagraph::new("Hello")),
    ///     Children::None
    /// );
    ///
    /// // Single-child node
    /// let node = RenderNode::new(
    ///     Box::new(RenderPadding::new(EdgeInsets::all(10.0))),
    ///     Children::from_single(child_id)
    /// );
    /// ```
    pub fn new(render: Box<dyn Render>, children: Children) -> Self {
        // Validate arity
        let arity = render.arity();
        if let Err(e) = arity.validate(children.len()) {
            panic!("RenderNode arity validation failed: {}", e);
        }

        Self { render, children }
    }

    /// Create leaf render node (no children)
    ///
    /// Convenience constructor for leaf nodes.
    /// Validates that the render object has `Arity::Exact(0)`.
    ///
    /// # Panics
    ///
    /// Panics if the render object doesn't have `Arity::Exact(0)`.
    pub fn leaf(render: Box<dyn Render>) -> Self {
        Self::new(render, Children::None)
    }

    /// Create single-child render node (without child ID yet)
    ///
    /// Used during element mounting when the child ID isn't known yet.
    /// The child ID will be set later via `set_children()`.
    ///
    /// # Panics
    ///
    /// Panics if the render object doesn't have `Arity::Exact(1)`.
    pub fn single(render: Box<dyn Render>) -> Self {
        Self::new(render, Children::None) // Temporary, will be updated
    }

    /// Create multi-child render node (without children yet)
    ///
    /// Used during element mounting when the children aren't known yet.
    /// Children will be set later via `set_children()`.
    pub fn multi(render: Box<dyn Render>) -> Self {
        Self::new(render, Children::from_multi(Vec::new()))
    }

    // ========== Queries ==========

    /// Get arity of this render object
    ///
    /// Returns the expected child count pattern.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let arity = node.arity();
    /// match arity {
    ///     Arity::Exact(0) => println!("Leaf node"),
    ///     Arity::Exact(1) => println!("Single-child node"),
    ///     Arity::Variable => println!("Multi-child node"),
    ///     _ => {}
    /// }
    /// ```
    pub fn arity(&self) -> Arity {
        self.render.arity()
    }

    /// Get debug name of the render object
    ///
    /// Returns a human-readable name for diagnostics.
    pub fn debug_name(&self) -> &'static str {
        self.render.debug_name()
    }

    /// Downcast render object to access metadata
    ///
    /// Allows parent render objects to downcast children to access metadata.
    /// Used by layouts like Flex and Stack to query child-specific metadata.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Access FlexItemMetadata from child
    /// if let Some(flex_item) = node.as_any().downcast_ref::<RenderFlexItem>() {
    ///     let flex_factor = flex_item.metadata.flex;
    ///     // Use flex factor in layout calculations...
    /// }
    /// ```
    pub fn as_any(&self) -> &dyn std::any::Any {
        self.render.as_any()
    }

    /// Get children reference
    ///
    /// Returns a reference to the children enum.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let children = node.children();
    /// match children {
    ///     Children::None => println!("No children"),
    ///     Children::Single(id) => println!("One child: {:?}", id),
    ///     Children::Multi(ids) => println!("{} children", ids.len()),
    /// }
    /// ```
    pub fn children(&self) -> &Children {
        &self.children
    }

    /// Set children (with arity validation)
    ///
    /// Updates the children of this render node.
    /// Validates that the new children count matches the arity.
    ///
    /// # Parameters
    ///
    /// - `children`: New children enum
    ///
    /// # Panics
    ///
    /// Panics if arity validation fails.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Set single child
    /// node.set_children(Children::from_single(child_id));
    ///
    /// // Set multiple children
    /// node.set_children(Children::from_multi(vec![id1, id2, id3]));
    /// ```
    pub fn set_children(&mut self, children: Children) {
        let arity = self.render.arity();
        if let Err(e) = arity.validate(children.len()) {
            panic!("RenderNode set_children arity validation failed: {}", e);
        }
        self.children = children;
    }

    /// Check if this is a leaf node (no children)
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Check if this is a single-child node
    pub fn is_single(&self) -> bool {
        matches!(self.children, Children::Single(_))
    }

    /// Check if this is a multi-child node
    pub fn is_multi(&self) -> bool {
        matches!(self.children, Children::Multi(_))
    }

    // ========== Layout ==========

    /// Perform layout
    ///
    /// Calls the render object's `layout` method with a context
    /// containing the element tree, children, and constraints.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `constraints`: Layout constraints from parent
    ///
    /// # Returns
    ///
    /// The computed size (must satisfy constraints).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let size = node.layout(&tree, BoxConstraints::tight(Size::new(100.0, 100.0)));
    /// ```
    pub fn layout(&mut self, tree: &ElementTree, constraints: BoxConstraints) -> Size {
        let ctx = LayoutContext::new(tree, &self.children, constraints);
        self.render.layout(&ctx)
    }

    // ========== Paint ==========

    /// Perform paint
    ///
    /// Calls the render object's `paint` method with a context
    /// containing the element tree, children, and offset.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `offset`: Paint offset in parent's coordinate space
    ///
    /// # Returns
    ///
    /// A boxed layer containing the painted content.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let layer = node.paint(&tree, Offset::new(10.0, 20.0));
    /// ```
    pub fn paint(&self, tree: &ElementTree, offset: Offset) -> BoxedLayer {
        let ctx = PaintContext::new(tree, &self.children, offset);
        self.render.paint(&ctx)
    }

    // ========== Intrinsics ==========

    /// Compute intrinsic width
    ///
    /// Delegates to the render object's `intrinsic_width` method.
    pub fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        self.render.intrinsic_width(height)
    }

    /// Compute intrinsic height
    ///
    /// Delegates to the render object's `intrinsic_height` method.
    pub fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        self.render.intrinsic_height(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    #[derive(Debug)]
    struct TestLeaf;

    impl Render for TestLeaf {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            ctx.constraints.constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn arity(&self) -> Arity {
            Arity::Exact(0)
        }
    }

    #[derive(Debug)]
    struct TestSingle;

    impl Render for TestSingle {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            let child_id = ctx.children.single();
            ctx.layout_child(child_id, ctx.constraints)
        }

        fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
            let child_id = ctx.children.single();
            ctx.paint_child(child_id, ctx.offset)
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl Render for TestMulti {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            ctx.constraints.biggest()
        }

        fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn arity(&self) -> Arity {
            Arity::Variable
        }
    }

    #[test]
    fn test_leaf_node() {
        let node = RenderNode::new(Box::new(TestLeaf), Children::None);
        assert!(node.is_leaf());
        assert_eq!(node.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_single_node() {
        let node = RenderNode::new(Box::new(TestSingle), Children::from_single(1));
        assert!(node.is_single());
        assert_eq!(node.arity(), Arity::Exact(1));
    }

    #[test]
    fn test_multi_node() {
        let children = vec![1, 2, 3];
        let node = RenderNode::new(Box::new(TestMulti), Children::from_multi(children.clone()));
        assert!(node.is_multi());
        assert_eq!(node.arity(), Arity::Variable);
        assert_eq!(node.children().as_slice(), &children[..]);
    }

    #[test]
    #[should_panic(expected = "arity validation failed")]
    fn test_arity_validation_fail() {
        // Try to create leaf with children
        RenderNode::new(Box::new(TestLeaf), Children::from_single(1));
    }

    #[test]
    fn test_set_children() {
        let mut node = RenderNode::new(Box::new(TestMulti), Children::from_multi(vec![]));
        node.set_children(Children::from_multi(vec![1, 2]));
        assert_eq!(node.children().len(), 2);
    }

    #[test]
    fn test_debug_name() {
        let node = RenderNode::new(Box::new(TestLeaf), Children::None);
        let name = node.debug_name();
        assert!(name.contains("TestLeaf"));
    }
}
