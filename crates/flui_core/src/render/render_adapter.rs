//! Adapter for legacy Render trait
//!
//! This module provides adapters to convert from the old `Render` trait
//! (with Arity generics) to the new `LeafRender`/`SingleRender`/`MultiRender` traits.
//!
//! This enables backward compatibility during migration.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Old Render implementation
//! impl Render for RenderParagraph {
//!     type Arity = LeafArity;
//!     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size { ... }
//!     fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer { ... }
//! }
//!
//! // Convert to new architecture
//! let render = Render::from_legacy(RenderParagraph::new("Hello"));
//! ```

use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

use crate::element::{ElementId, ElementTree};
use crate::render::{
    LeafArity, SingleArity, MultiArity,
    LayoutCx, PaintCx, Render,
};

use super::render_node::RenderNode;
use super::render_traits::{LeafRender, SingleRender, MultiRender};

// ========== Leaf Adapter ==========

/// Adapter for LeafArity Renders
///
/// Converts a `Render<Arity = LeafArity>` to `LeafRender`.
#[derive(Debug)]
pub struct LeafAdapter<T> {
    inner: T,
}

impl<T> LeafAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> LeafRender for LeafAdapter<T>
where
    T: Render<Arity = LeafArity> + Send + Sync + std::fmt::Debug + 'static,
{
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Create a fake ElementTree and ElementId for the old API
        // The old API requires these, but LeafArity doesn't use them
        let tree = ElementTree::new();
        let element_id = 0; // ElementId is just usize

        let mut cx = LayoutCx::<LeafArity>::new(&tree, element_id, constraints);
        self.inner.layout(&mut cx)
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        let tree = ElementTree::new();
        let element_id = 0; // ElementId is just usize

        let cx = PaintCx::<LeafArity>::new(&tree, element_id, offset);
        self.inner.paint(&cx)
    }

    fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn debug_name(&self) -> &'static str {
        self.inner.debug_name()
    }
}

// ========== Single Adapter ==========

/// Adapter for SingleArity Renders
///
/// Converts a `Render<Arity = SingleArity>` to `SingleRender`.
#[derive(Debug)]
pub struct SingleAdapter<T> {
    inner: T,
}

impl<T> SingleAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> SingleRender for SingleAdapter<T>
where
    T: Render<Arity = SingleArity> + Send + Sync + std::fmt::Debug + 'static,
{
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        let mut cx = LayoutCx::<SingleArity>::new(tree, child_id, constraints);
        self.inner.layout(&mut cx)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        let cx = PaintCx::<SingleArity>::new(tree, child_id, offset);
        self.inner.paint(&cx)
    }

    fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn debug_name(&self) -> &'static str {
        self.inner.debug_name()
    }
}

// ========== Multi Adapter ==========

/// Adapter for MultiArity Renders
///
/// Converts a `Render<Arity = MultiArity>` to `MultiRender`.
#[derive(Debug)]
pub struct MultiAdapter<T> {
    inner: T,
}

impl<T> MultiAdapter<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> MultiRender for MultiAdapter<T>
where
    T: Render<Arity = MultiArity> + Send + Sync + std::fmt::Debug + 'static,
{
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        // MultiArity uses first child as element_id
        let element_id = children.first().copied().unwrap_or(0);
        let mut cx = LayoutCx::<MultiArity>::new(tree, element_id, constraints);
        self.inner.layout(&mut cx)
    }

    fn paint(&self, tree: &ElementTree, children: &[ElementId], offset: Offset) -> BoxedLayer {
        let element_id = children.first().copied().unwrap_or(0);
        let cx = PaintCx::<MultiArity>::new(tree, element_id, offset);
        self.inner.paint(&cx)
    }

    fn intrinsic_width(&self, height: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: Option<f32>) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn debug_name(&self) -> &'static str {
        self.inner.debug_name()
    }
}

// ========== Render Extension Methods ==========

impl RenderNode {
    /// Create Render from a legacy LeafArity Render
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let render = Render::from_legacy_leaf(RenderParagraph::new("Hello"));
    /// ```
    pub fn from_legacy_leaf<T>(render_object: T) -> Self
    where
        T: Render<Arity = LeafArity> + Send + Sync + std::fmt::Debug + 'static,
    {
        Self::new_leaf(Box::new(LeafAdapter::new(render_object)))
    }

    /// Create Render from a legacy SingleArity Render
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let child_id = ElementId::new(42);
    /// let render = Render::from_legacy_single(RenderOpacity::new(0.5), child_id);
    /// ```
    pub fn from_legacy_single<T>(render_object: T, child: ElementId) -> Self
    where
        T: Render<Arity = SingleArity> + Send + Sync + std::fmt::Debug + 'static,
    {
        Self::new_single(Box::new(SingleAdapter::new(render_object)), child)
    }

    /// Create Render from a legacy MultiArity Render
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let children = vec![ElementId::new(1), ElementId::new(2)];
    /// let render = Render::from_legacy_multi(RenderFlex::new(), children);
    /// ```
    pub fn from_legacy_multi<T>(render_object: T, children: Vec<ElementId>) -> Self
    where
        T: Render<Arity = MultiArity> + Send + Sync + std::fmt::Debug + 'static,
    {
        Self::new_multi(Box::new(MultiAdapter::new(render_object)), children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    // Test Renders using old API
    #[derive(Debug)]
    struct TestLeafRender;

    impl Render for TestLeafRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestSingleRender;

    impl Render for TestSingleRender {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(200.0, 200.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestMultiRender;

    impl Render for TestMultiRender {
        type Arity = MultiArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(300.0, 300.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_leaf_adapter() {
        let render = Render::from_legacy_leaf(TestLeafRender);

        let tree = ElementTree::new();
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

        if let Render::Leaf(mut leaf) = render {
            let size = leaf.layout(constraints);
            assert_eq!(size, Size::new(50.0, 50.0));
        } else {
            panic!("Expected Leaf variant");
        }
    }

    #[test]
    fn test_single_adapter() {
        let child_id = 42; // ElementId is just usize
        let render = Render::from_legacy_single(TestSingleRender, child_id);

        let tree = ElementTree::new();
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

        if let Render::Single { mut render, child } = render {
            assert_eq!(child, child_id);
            let size = render.layout(&tree, child, constraints);
            assert_eq!(size, Size::new(50.0, 50.0));
        } else {
            panic!("Expected Single variant");
        }
    }

    #[test]
    fn test_multi_adapter() {
        let children = vec![1, 2, 3]; // ElementId is just usize
        let render = Render::from_legacy_multi(TestMultiRender, children.clone());

        let tree = ElementTree::new();
        let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

        if let Render::Multi { mut render, children: c } = render {
            assert_eq!(c, children);
            let size = render.layout(&tree, &c, constraints);
            assert_eq!(size, Size::new(50.0, 50.0));
        } else {
            panic!("Expected Multi variant");
        }
    }

    #[test]
    fn test_adapter_debug_names() {
        let leaf = Render::from_legacy_leaf(TestLeafRender);
        let child_id = 42; // ElementId is just usize
        let single = Render::from_legacy_single(TestSingleRender, child_id);

        assert!(leaf.debug_name().contains("TestLeafRender"));
        assert!(single.debug_name().contains("TestSingleRender"));
    }
}
