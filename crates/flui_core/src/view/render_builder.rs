//! Render builders - convenient API for creating render elements
//!
//! This module provides builder types that simplify creating render elements
//! from render objects. Builders handle tree insertion and RenderNode creation
//! automatically.
//!
//! # Unified API (v0.1.0)
//!
//! With the unified `Render` trait, we still provide three builder types
//! based on child count for API convenience:
//!
//! - `RenderBuilder::leaf()` - For renders with no children
//! - `RenderBuilder::single()` - For renders with one child
//! - `RenderBuilder::multi()` - For renders with multiple children
//!
//! # Examples
//!
//! ## Leaf Render (No Children)
//!
//! ```rust,ignore
//! impl View for Text {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::leaf(RenderParagraph::new(&self.text))
//!     }
//! }
//! ```
//!
//! ## Single-Child Render
//!
//! ```rust,ignore
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::single(RenderPadding::new(self.padding))
//!             .with_child(self.child)
//!     }
//! }
//! ```
//!
//! ## Multi-Child Render
//!
//! ```rust,ignore
//! impl View for Column {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::multi(RenderFlex::column())
//!             .with_children(self.children)
//!     }
//! }
//! ```

use super::{AnyElement, IntoElement};
use crate::element::{Element, RenderElement};
use crate::render::Render;

/// Builder for creating render elements
///
/// Provides static methods to create builders for different child count patterns.
pub struct RenderBuilder;

impl RenderBuilder {
    /// Create a leaf render builder (no children)
    ///
    /// # Parameters
    ///
    /// - `render`: Any type implementing `Render` trait
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::leaf(RenderParagraph::new("Hello"))
    /// ```
    pub fn leaf(render: impl Render) -> LeafRenderBuilder {
        LeafRenderBuilder {
            render: Box::new(render),
        }
    }

    /// Create a single-child render builder
    ///
    /// # Parameters
    ///
    /// - `render`: Any type implementing `Render` trait
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::single(RenderPadding::new(EdgeInsets::all(10.0)))
    ///     .with_child(child_view)
    /// ```
    pub fn single(render: impl Render) -> SingleRenderBuilder {
        SingleRenderBuilder {
            render: Box::new(render),
            child: None,
        }
    }

    /// Create a multi-child render builder
    ///
    /// # Parameters
    ///
    /// - `render`: Any type implementing `Render` trait
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::multi(RenderFlex::column())
    ///     .with_children(vec![child1, child2, child3])
    /// ```
    pub fn multi(render: impl Render) -> MultiRenderBuilder {
        MultiRenderBuilder {
            render: Box::new(render),
            children: Vec::new(),
        }
    }
}

// ============================================================================
// LeafRenderBuilder
// ============================================================================

/// Builder for leaf render objects (no children)
///
/// Created via `RenderBuilder::leaf()`. Automatically creates a RenderElement
/// with no children.
///
/// # Examples
///
/// ```rust,ignore
/// impl View for Text {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         RenderBuilder::leaf(RenderParagraph::new(&self.text))
///     }
/// }
/// ```
pub struct LeafRenderBuilder {
    render: Box<dyn Render>,
}

impl IntoElement for LeafRenderBuilder {
    fn into_element(self) -> Element {
        // Wrap render object in RenderElement (no children)
        let render_element = RenderElement::new(self.render);

        // Convert to Element enum
        Element::Render(render_element)
    }
}

// ============================================================================
// SingleRenderBuilder
// ============================================================================

/// Builder for single-child render objects
///
/// Created via `RenderBuilder::single()`. Provides chainable `.with_child()`
/// method that automatically handles tree insertion.
///
/// # Examples
///
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         RenderBuilder::single(RenderPadding::new(self.padding))
///             .with_child(self.child)
///     }
/// }
/// ```
pub struct SingleRenderBuilder {
    render: Box<dyn Render>,
    child: Option<AnyElement>,
}

impl SingleRenderBuilder {
    /// Add a child (chainable)
    ///
    /// This method accepts any `impl IntoElement` and stores it
    /// for later conversion. The tree insertion happens in `into_element()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::single(render)
    ///     .with_child(Text::new("Hello"))
    /// ```
    pub fn with_child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(AnyElement::new(child));
        self
    }

    /// Add an optional child (chainable)
    ///
    /// Convenience method for handling `Option<impl IntoElement>`.
    /// If `None`, the builder remains unchanged.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::single(render)
    ///     .with_optional_child(self.child)  // child: Option<Box<dyn View>>
    /// ```
    pub fn with_optional_child(mut self, child: Option<impl IntoElement>) -> Self {
        if let Some(child) = child {
            self.child = Some(AnyElement::new(child));
        }
        self
    }
}

impl IntoElement for SingleRenderBuilder {
    fn into_element(self) -> Element {
        // Convert child to element and get its ID
        let child_element = self.child.map(|c| c.into_element());

        // Create RenderElement with render object
        let render_element = RenderElement::new(self.render);

        // If we have a child element, we need to handle it
        // This is a simplified version - full implementation would require
        // element tree integration
        if let Some(_child_elem) = child_element {
            // TODO: Store child for later mounting
            // This requires changes to RenderElement or element tree
        }

        // Convert to Element enum
        Element::Render(render_element)
    }
}

// ============================================================================
// MultiRenderBuilder
// ============================================================================

/// Builder for multi-child render objects
///
/// Created via `RenderBuilder::multi()`. Provides chainable `.with_children()`
/// and `.add_child()` methods for flexible child management.
///
/// # Examples
///
/// ```rust,ignore
/// impl View for Column {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         RenderBuilder::multi(RenderFlex::column())
///             .with_children(self.children)
///     }
/// }
/// ```
pub struct MultiRenderBuilder {
    render: Box<dyn Render>,
    children: Vec<AnyElement>,
}

impl MultiRenderBuilder {
    /// Set all children at once (chainable)
    ///
    /// Replaces any existing children with the provided vec.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::multi(render)
    ///     .with_children(vec![child1, child2, child3])
    /// ```
    pub fn with_children(mut self, children: Vec<impl IntoElement>) -> Self {
        self.children = children.into_iter().map(AnyElement::new).collect();
        self
    }

    /// Add a single child (chainable)
    ///
    /// Appends the child to the existing children list.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::multi(render)
    ///     .add_child(Text::new("First"))
    ///     .add_child(Text::new("Second"))
    /// ```
    pub fn add_child(mut self, child: impl IntoElement) -> Self {
        self.children.push(AnyElement::new(child));
        self
    }

    /// Add an optional child (chainable)
    ///
    /// If `Some`, appends the child. If `None`, does nothing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::multi(render)
    ///     .add_optional_child(optional_header)
    ///     .with_children(body_children)
    ///     .add_optional_child(optional_footer)
    /// ```
    pub fn add_optional_child(mut self, child: Option<impl IntoElement>) -> Self {
        if let Some(child) = child {
            self.children.push(AnyElement::new(child));
        }
        self
    }
}

impl IntoElement for MultiRenderBuilder {
    fn into_element(self) -> Element {
        // Convert children to elements
        let _child_elements: Vec<Element> = self
            .children
            .into_iter()
            .map(|c| c.into_element())
            .collect();

        // Create RenderElement with render object (children will be set during mounting)
        // TODO: This needs to be handled by the element tree mounting logic
        let render_element = RenderElement::new(self.render);

        // Convert to Element enum
        Element::Render(render_element)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Arity, LayoutContext, PaintContext};
    use flui_engine::{BoxedLayer, ContainerLayer};
    use flui_types::{constraints::BoxConstraints, Offset, Size};

    #[derive(Debug)]
    struct TestLeafRender;

    impl Render for TestLeafRender {
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
    struct TestSingleRender;

    impl Render for TestSingleRender {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            ctx.constraints.biggest()
        }

        fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }
    }

    #[derive(Debug)]
    struct TestMultiRender;

    impl Render for TestMultiRender {
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
    fn test_leaf_builder() {
        let builder = RenderBuilder::leaf(TestLeafRender);
        let _element = builder.into_element();
    }

    #[test]
    fn test_single_builder() {
        let builder = RenderBuilder::single(TestSingleRender);
        let _element = builder.into_element();
    }

    #[test]
    fn test_multi_builder() {
        let builder = RenderBuilder::multi(TestMultiRender);
        let _element = builder.into_element();
    }
}
