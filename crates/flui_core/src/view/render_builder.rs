//! Render builder - convenient API for creating render elements
//!
//! This module provides a unified builder for creating render elements
//! from render objects. The builder handles tree insertion and child
//! management automatically.
//!
//! # Unified API (v0.2.0)
//!
//! With the unified `Render` trait, we now have a single builder for all cases:
//!
//! ```rust,ignore
//! // Leaf render (no children)
//! RenderBuilder::new(RenderText::new("Hello"))
//!
//! // Single-child render
//! RenderBuilder::new(RenderPadding::new(padding))
//!     .child(child_view)
//!
//! // Multi-child render
//! RenderBuilder::new(RenderColumn::new())
//!     .child(child1)
//!     .child(child2)
//!     .child(child3)
//!
//! // Or use .children() for vec
//! RenderBuilder::new(RenderColumn::new())
//!     .children(vec![child1, child2, child3])
//! ```
//!
//! # Examples
//!
//! ## Leaf Render (No Children)
//!
//! ```rust,ignore
//! impl View for Text {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::new(RenderParagraph::new(&self.text))
//!     }
//! }
//! ```
//!
//! ## Single-Child Render
//!
//! ```rust,ignore
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::new(RenderPadding::new(self.padding))
//!             .maybe_child(self.child)
//!     }
//! }
//! ```
//!
//! ## Multi-Child Render
//!
//! ```rust,ignore
//! impl View for Column {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderBuilder::new(RenderFlex::column())
//!             .children(self.children)
//!     }
//! }
//! ```

use super::{AnyElement, IntoElement};
use crate::element::{Element, RenderElement};
use crate::render::Render;

/// Unified builder for creating render elements
///
/// Works with any render object (leaf, single-child, or multi-child).
/// Arity checking is performed at runtime by RenderElement.set_children().
pub struct RenderBuilder {
    render: Box<dyn Render>,
    children: Vec<AnyElement>,
}

impl RenderBuilder {
    /// Create a new render builder
    ///
    /// Works with any render object - leaf, single-child, or multi-child.
    /// Arity checking happens at runtime when mounting.
    ///
    /// # Parameters
    ///
    /// - `render`: Any type implementing `Render` trait
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf render (no children)
    /// RenderBuilder::new(RenderText::new("Hello"))
    ///
    /// // Single-child render
    /// RenderBuilder::new(RenderPadding::new(padding))
    ///     .child(child_view)
    ///
    /// // Multi-child render
    /// RenderBuilder::new(RenderColumn::new())
    ///     .children(vec![child1, child2])
    /// ```
    pub fn new(render: impl Render) -> Self {
        Self {
            render: Box::new(render),
            children: Vec::new(),
        }
    }

    /// Add a single child (chainable)
    ///
    /// Can be called multiple times to add multiple children.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::new(render)
    ///     .child(Text::new("First"))
    ///     .child(Text::new("Second"))
    /// ```
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(AnyElement::new(child));
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
    /// RenderBuilder::new(render)
    ///     .maybe_child(self.optional_child)
    /// ```
    pub fn maybe_child(mut self, child: Option<impl IntoElement>) -> Self {
        if let Some(child) = child {
            self.children.push(AnyElement::new(child));
        }
        self
    }

    /// Set all children at once (chainable)
    ///
    /// Replaces any existing children with the provided vec.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// RenderBuilder::new(render)
    ///     .children(vec![child1, child2, child3])
    /// ```
    pub fn children(mut self, children: Vec<impl IntoElement>) -> Self {
        self.children = children.into_iter().map(AnyElement::new).collect();
        self
    }
}

impl IntoElement for RenderBuilder {
    fn into_element(self) -> Element {
        // Convert children to elements
        let child_elements: Vec<Element> = self
            .children
            .into_iter()
            .map(|c| c.into_element())
            .collect();

        // Create RenderElement with unmounted children if any
        let render_element = if child_elements.is_empty() {
            RenderElement::new(self.render)
        } else {
            RenderElement::new_with_children(self.render, child_elements)
        };

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
        let builder = RenderBuilder::new(TestLeafRender);
        let _element = builder.into_element();
    }

    #[test]
    fn test_single_builder() {
        let builder = RenderBuilder::new(TestSingleRender);
        let _element = builder.into_element();
    }

    #[test]
    fn test_multi_builder() {
        let builder = RenderBuilder::new(TestMultiRender);
        let _element = builder.into_element();
    }

    #[test]
    fn test_builder_with_child() {
        let builder = RenderBuilder::new(TestSingleRender)
            .child(RenderBuilder::new(TestLeafRender));
        let _element = builder.into_element();
    }

    #[test]
    fn test_builder_with_children() {
        let builder = RenderBuilder::new(TestMultiRender)
            .children(vec![
                RenderBuilder::new(TestLeafRender),
                RenderBuilder::new(TestLeafRender),
            ]);
        let _element = builder.into_element();
    }
}
