//! RenderNode - unified render tree node
//!
//! Defines the `RenderNode` enum which wraps three object-safe traits:
//! - LeafRender
//! - SingleRender
//! - MultiRender

use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

use super::render_traits::{LeafRender, MultiRender, SingleRender};
use crate::element::ElementId;
use crate::pipeline::ElementTree;

/// Unified render tree node enum
///
/// Three variants for three child count patterns:
/// - `Leaf`: No children
/// - `Single`: Exactly one child
/// - `Multi`: Multiple children
///
/// # GAT and Trait Objects
///
/// Each render trait has a `Metadata` associated type. To use with trait objects,
/// we specify `Metadata = ()` as the default. Render objects can still have their
/// own parent data types - they just need to implement the trait with `Metadata = ()`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::render::{RenderNode, LeafRender};
///
/// let paragraph = Paragraph::new("Hello");
/// let render = RenderNode::new_leaf(Box::new(paragraph));
///
/// let size = render.layout(tree, constraints);
/// let layer = render.paint(tree, offset);
/// ```
#[derive(Debug)]
pub enum RenderNode {
    /// Leaf (no children)
    ///
    /// Metadata fixed to () for object safety. Individual render objects can
    /// store parent data internally if needed.
    Leaf(Box<dyn LeafRender<Metadata = ()>>),

    /// Single child
    Single {
        /// Render object (Metadata fixed to () for object safety)
        render: Box<dyn SingleRender<Metadata = ()>>,
        /// Child element ID (None if not yet mounted)
        child: Option<ElementId>,
    },

    /// Multiple children
    Multi {
        /// Render object (Metadata fixed to () for object safety)
        render: Box<dyn MultiRender<Metadata = ()>>,
        /// Child element IDs
        children: Vec<ElementId>,
    },
}

impl RenderNode {
    // ========== Constructors ==========

    /// Create leaf render
    pub fn new_leaf(render: Box<dyn LeafRender<Metadata = ()>>) -> Self {
        Self::Leaf(render)
    }

    /// Create single-child render
    pub fn new_single(render: Box<dyn SingleRender<Metadata = ()>>, child: ElementId) -> Self {
        Self::Single { render, child: Some(child) }
    }

    /// Create multi-child render
    pub fn new_multi(render: Box<dyn MultiRender<Metadata = ()>>, children: Vec<ElementId>) -> Self {
        Self::Multi { render, children }
    }

    /// Create leaf render (alias for widget convenience)
    pub fn leaf(render: Box<dyn LeafRender<Metadata = ()>>) -> Self {
        Self::new_leaf(render)
    }

    /// Create single-child render without ElementId (for widgets)
    ///
    /// The element framework will set the child ElementId later during mounting.
    pub fn single(render: Box<dyn SingleRender<Metadata = ()>>) -> Self {
        Self::Single {
            render,
            child: None,
        }
    }

    /// Create multi-child render without ElementIds (for widgets)
    ///
    /// The element framework will set children ElementIds later during mounting.
    pub fn multi(render: Box<dyn MultiRender<Metadata = ()>>) -> Self {
        Self::Multi {
            render,
            children: Vec::new(),
        }
    }

    // ========== Queries ==========

    /// Get arity
    ///
    /// Returns:
    /// - `Some(0)` for Leaf
    /// - `Some(1)` for Single
    /// - `None` for Multi
    pub fn arity(&self) -> Option<usize> {
        match self {
            Self::Leaf(_) => Some(0),
            Self::Single { .. } => Some(1),
            Self::Multi { .. } => None,
        }
    }

    /// Get debug name
    pub fn debug_name(&self) -> &'static str {
        match self {
            Self::Leaf(r) => r.debug_name(),
            Self::Single { render: r, .. } => r.debug_name(),
            Self::Multi { render: r, .. } => r.debug_name(),
        }
    }

    /// Get child (Single only)
    ///
    /// # Returns
    ///
    /// Returns `Some(ElementId)` if this is a Single variant with a mounted child,
    /// `None` if Single but not yet mounted, or `None` if not a Single variant.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(child_id) = render.child() {
    ///     // Process child
    /// }
    /// ```
    pub fn child(&self) -> Option<ElementId> {
        match self {
            Self::Single { child, .. } => *child,
            _ => None,
        }
    }

    /// Set child (Single only)
    ///
    /// # Returns
    ///
    /// Returns `true` if child was set (Single variant), `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if render.set_child(new_child_id) {
    ///     // Child was set successfully
    /// }
    /// ```
    pub fn set_child(&mut self, new_child: ElementId) -> bool {
        match self {
            Self::Single { child, .. } => {
                *child = Some(new_child);
                true
            }
            _ => false,
        }
    }

    /// Get children (Multi only)
    ///
    /// # Returns
    ///
    /// Returns `Some(&[ElementId])` if this is a Multi variant, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(children) = render.children() {
    ///     for child_id in children {
    ///         // Process each child
    ///     }
    /// }
    /// ```
    pub fn children(&self) -> Option<&[ElementId]> {
        match self {
            Self::Multi { children, .. } => Some(children),
            _ => None,
        }
    }

    /// Set children (Multi only)
    ///
    /// # Returns
    ///
    /// Returns `true` if children were set (Multi variant), `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if render.set_children(&new_children) {
    ///     // Children were set successfully
    /// }
    /// ```
    pub fn set_children(&mut self, new_children: &[ElementId]) -> bool {
        match self {
            Self::Multi { children, .. } => {
                children.clear();
                children.extend_from_slice(new_children);
                true
            }
            _ => false,
        }
    }

    /// Check if leaf
    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf(_))
    }

    /// Check if single
    pub fn is_single(&self) -> bool {
        matches!(self, Self::Single { .. })
    }

    /// Check if multi
    pub fn is_multi(&self) -> bool {
        matches!(self, Self::Multi { .. })
    }

    // ========== Layout ==========

    /// Perform layout
    ///
    /// # Single Child Handling
    ///
    /// For Single variant, if child is None (not yet mounted), returns zero size
    /// constrained by the given constraints.
    pub fn layout(&mut self, tree: &ElementTree, constraints: BoxConstraints) -> Size {
        match self {
            Self::Leaf(r) => r.layout(constraints),
            Self::Single {
                render: r, child, ..
            } => {
                if let Some(child_id) = child {
                    r.layout(tree, *child_id, constraints)
                } else {
                    // Child not yet mounted - return zero size
                    constraints.constrain(Size::ZERO)
                }
            }
            Self::Multi {
                render: r,
                children,
                ..
            } => r.layout(tree, children, constraints),
        }
    }

    // ========== Paint ==========

    /// Perform paint
    ///
    /// # Single Child Handling
    ///
    /// For Single variant, if child is None (not yet mounted), returns an empty
    /// container layer.
    pub fn paint(&self, tree: &ElementTree, offset: Offset) -> BoxedLayer {
        match self {
            Self::Leaf(r) => r.paint(offset),
            Self::Single {
                render: r, child, ..
            } => {
                if let Some(child_id) = child {
                    r.paint(tree, *child_id, offset)
                } else {
                    // Child not yet mounted - return empty layer
                    Box::new(flui_engine::ContainerLayer::new())
                }
            }
            Self::Multi {
                render: r,
                children,
                ..
            } => r.paint(tree, children, offset),
        }
    }

    // ========== Intrinsics ==========

    /// Compute intrinsic width
    pub fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        match self {
            Self::Leaf(r) => r.intrinsic_width(height),
            Self::Single { render: r, .. } => r.intrinsic_width(height),
            Self::Multi { render: r, .. } => r.intrinsic_width(height),
        }
    }

    /// Compute intrinsic height
    pub fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        match self {
            Self::Leaf(r) => r.intrinsic_height(width),
            Self::Single { render: r, .. } => r.intrinsic_height(width),
            Self::Multi { render: r, .. } => r.intrinsic_height(width),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    #[derive(Debug)]
    struct TestLeaf;

    impl LeafRender for TestLeaf {
        type Metadata = ();

        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _offset: Offset) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestSingle;

    impl SingleRender for TestSingle {
        type Metadata = ();

        fn layout(
            &mut self,
            _tree: &ElementTree,
            _child_id: ElementId,
            constraints: BoxConstraints,
        ) -> Size {
            constraints.constrain(Size::new(200.0, 200.0))
        }

        fn paint(&self, _tree: &ElementTree, _child_id: ElementId, _offset: Offset) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl MultiRender for TestMulti {
        type Metadata = ();

        fn layout(
            &mut self,
            _tree: &ElementTree,
            _children: &[ElementId],
            constraints: BoxConstraints,
        ) -> Size {
            constraints.constrain(Size::new(300.0, 300.0))
        }

        fn paint(
            &self,
            _tree: &ElementTree,
            _children: &[ElementId],
            _offset: Offset,
        ) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_leaf() {
        let render = RenderNode::new_leaf(Box::new(TestLeaf));
        assert!(render.is_leaf());
        assert_eq!(render.arity(), Some(0));
    }

    #[test]
    fn test_single() {
        let render = RenderNode::new_single(Box::new(TestSingle), 1);
        assert!(render.is_single());
        assert_eq!(render.arity(), Some(1));
        assert_eq!(render.child(), Some(1));
    }

    #[test]
    fn test_multi() {
        let children = vec![1, 2];
        let render = RenderNode::new_multi(Box::new(TestMulti), children.clone());
        assert!(render.is_multi());
        assert_eq!(render.arity(), None);
        assert_eq!(render.children(), Some(&children[..]));
    }

    #[test]
    fn test_set_child() {
        let mut render = RenderNode::new_single(Box::new(TestSingle), 1);
        assert!(render.set_child(2));
        assert_eq!(render.child(), Some(2));
    }

    #[test]
    fn test_set_children() {
        let mut render = RenderNode::new_multi(Box::new(TestMulti), vec![]);
        let new_children = vec![1];
        assert!(render.set_children(&new_children));
        assert_eq!(render.children(), Some(&new_children[..]));
    }

    #[test]
    fn test_debug_names() {
        let leaf = RenderNode::new_leaf(Box::new(TestLeaf));
        let single = RenderNode::new_single(Box::new(TestSingle), 1);
        let multi = RenderNode::new_multi(Box::new(TestMulti), vec![]);

        assert!(leaf.debug_name().contains("TestLeaf"));
        assert!(single.debug_name().contains("TestSingle"));
        assert!(multi.debug_name().contains("TestMulti"));
    }
}
