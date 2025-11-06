//! RenderBuilder - chainable API for render objects
//!
//! This module provides builder types that wrap LeafRender, SingleRender,
//! and MultiRender objects, providing a chainable API and automatic tree
//! management.
//!
//! # Philosophy
//!
//! RenderBuilder eliminates the boilerplate of manually:
//! - Choosing RenderNode variant (Leaf/Single/Multi)
//! - Inserting children into the tree
//! - Wrapping in RenderElement
//! - Converting to Element enum
//!
//! # Example
//!
//! **Old way:**
//! ```rust,ignore
//! fn build(self, ctx: &mut BuildContext) -> (Element, State) {
//!     let (child_id, state) = if let Some(child) = self.child {
//!         let (elem, state) = child.build_any(ctx);
//!         let id = ctx.tree().write().insert(elem.into_element());
//!         (Some(id), Some(state))
//!     } else {
//!         (None, None)
//!     };
//!
//!     let render_node = RenderNode::Single {
//!         render: Box::new(RenderPadding::new(self.padding)),
//!         child: child_id,
//!     };
//!
//!     (Element::Render(RenderElement::new(render_node)), state)
//! }
//! ```
//!
//! **New way:**
//! ```rust,ignore
//! fn build(self) -> impl IntoElement {
//!     RenderPadding::new(self.padding)
//!         .with_child(self.child)  // ← That's it!
//! }
//! ```

use crate::element::{Element, RenderElement};
use crate::foundation::ElementId;
use crate::render::{LeafRender, MultiRender, RenderNode, SingleRender};
use super::{IntoElement, AnyElement};

/// Builder for LeafRender objects (no children)
///
/// Provides a simple wrapper for leaf render objects that
/// automatically creates the correct RenderNode::Leaf variant.
///
/// # Example
///
/// ```rust,ignore
/// impl View for Text {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         LeafRenderBuilder::new(RenderParagraph::new(&self.text))
///     }
/// }
/// ```
#[derive(Debug)]
pub struct LeafRenderBuilder<R: LeafRender> {
    render: R,
}

impl<R: LeafRender> LeafRenderBuilder<R> {
    /// Create a new leaf render builder
    pub fn new(render: R) -> Self {
        Self { render }
    }
}

impl<R: LeafRender<Metadata = ()>> IntoElement for LeafRenderBuilder<R> {
    fn into_element(self) -> Element {
        // Create RenderNode::Leaf
        let render_node = RenderNode::Leaf(Box::new(self.render));

        // Wrap in RenderElement
        let render_element = RenderElement::new(render_node);

        // Convert to Element enum
        Element::Render(render_element)
    }
}

/// Builder for SingleRender objects (one child)
///
/// Provides chainable `.with_child()` method that automatically
/// handles tree insertion and RenderNode creation.
///
/// # Example
///
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         SingleRenderBuilder::new(RenderPadding::new(self.padding))
///             .with_child(self.child)  // ← Automatic tree management
///     }
/// }
/// ```
#[derive(Debug)]
pub struct SingleRenderBuilder<R: SingleRender> {
    render: R,
    child: Option<AnyElement>,
}

impl<R: SingleRender> SingleRenderBuilder<R> {
    /// Create a new single render builder
    pub fn new(render: R) -> Self {
        Self {
            render,
            child: None,
        }
    }

    /// Add a child (chainable)
    ///
    /// This method accepts any `impl IntoElement` and stores it
    /// for later conversion. The tree insertion happens in `into_element()`.
    pub fn with_child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(AnyElement::new(child));
        self
    }

    /// Add an optional child (chainable)
    ///
    /// Convenience method for handling `Option<impl IntoElement>`.
    /// If `None`, the builder remains unchanged.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// SingleRenderBuilder::new(render)
    ///     .with_optional_child(self.child)  // child: Option<Box<dyn AnyView>>
    /// ```
    pub fn with_optional_child(mut self, child: Option<impl IntoElement>) -> Self {
        if let Some(c) = child {
            self.child = Some(AnyElement::new(c));
        }
        self
    }
}

impl<R: SingleRender<Metadata = ()>> IntoElement for SingleRenderBuilder<R> {
    fn into_element(self) -> Element {
        // Convert optional child to ElementId
        let child_id = self.child.map(|child| {
            let element = child.into_element_inner();
            insert_into_tree(element)
        });

        // Create RenderNode::Single (child can be None)
        let render_node = RenderNode::Single {
            render: Box::new(self.render),
            child: child_id,
        };

        // Wrap in RenderElement
        let render_element = RenderElement::new(render_node);

        // Convert to Element enum
        Element::Render(render_element)
    }
}

/// Builder for MultiRender objects (multiple children)
///
/// Provides chainable `.with_children()` method that accepts
/// an iterator of IntoElement items.
///
/// # Example
///
/// ```rust,ignore
/// impl View for Column {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         MultiRenderBuilder::new(RenderFlex::column())
///             .with_children(self.children.into_iter())
///     }
/// }
/// ```
#[derive(Debug)]
pub struct MultiRenderBuilder<R: MultiRender> {
    render: R,
    children: Vec<AnyElement>,
}

impl<R: MultiRender> MultiRenderBuilder<R> {
    /// Create a new multi render builder
    pub fn new(render: R) -> Self {
        Self {
            render,
            children: Vec::new(),
        }
    }

    /// Add children from an iterator (chainable)
    pub fn with_children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator,
        I::Item: IntoElement,
    {
        self.children = children
            .into_iter()
            .map(|child| AnyElement::new(child))
            .collect();
        self
    }

    /// Add a single child (chainable)
    ///
    /// Convenience method for adding one child at a time.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(AnyElement::new(child));
        self
    }
}

impl<R: MultiRender<Metadata = ()>> IntoElement for MultiRenderBuilder<R> {
    fn into_element(self) -> Element {
        // Convert all children to Elements
        let child_ids: Vec<ElementId> = self
            .children
            .into_iter()
            .map(|child| {
                let element = child.into_element_inner();
                insert_into_tree(element)
            })
            .collect();

        // Create RenderNode::Multi
        let render_node = RenderNode::Multi {
            render: Box::new(self.render),
            children: child_ids,
        };

        // Wrap in RenderElement
        let render_element = RenderElement::new(render_node);

        // Convert to Element enum
        Element::Render(render_element)
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Insert an element into the tree and return its ID
///
/// Uses thread-local BuildContext to access the tree and insert the element.
fn insert_into_tree(element: Element) -> ElementId {
    use super::build_context::current_build_context;

    // Get BuildContext from thread-local
    let ctx = current_build_context();

    // Insert element into tree
    ctx.tree().write().insert(element)
}

// ============================================================================
// Extension traits for convenience
// ============================================================================

/// Extension trait for LeafRender - adds `.into_builder()` method
///
/// This allows writing:
/// ```rust,ignore
/// RenderText::new("Hello").into_builder()
/// ```
pub trait LeafRenderExt: LeafRender + Sized {
    /// Convert into a LeafRenderBuilder
    fn into_builder(self) -> LeafRenderBuilder<Self> {
        LeafRenderBuilder::new(self)
    }
}

// Blanket implementation
impl<R: LeafRender> LeafRenderExt for R {}

/// Extension trait for SingleRender - adds `.into_builder()` method
pub trait SingleRenderExt: SingleRender + Sized {
    /// Convert into a SingleRenderBuilder
    fn into_builder(self) -> SingleRenderBuilder<Self> {
        SingleRenderBuilder::new(self)
    }
}

// Blanket implementation
impl<R: SingleRender> SingleRenderExt for R {}

/// Extension trait for MultiRender - adds `.into_builder()` method
pub trait MultiRenderExt: MultiRender + Sized {
    /// Convert into a MultiRenderBuilder
    fn into_builder(self) -> MultiRenderBuilder<Self> {
        MultiRenderBuilder::new(self)
    }
}

// Blanket implementation
impl<R: MultiRender> MultiRenderExt for R {}

// ============================================================================
// Tuple syntax for convenience
// ============================================================================

/// Implement IntoElement for (SingleRender, child) tuples
///
/// This allows writing:
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderPadding::new(self.padding), self.child)
///         //  ↑ Tuple automatically creates SingleRenderBuilder
///     }
/// }
/// ```
impl<R: SingleRender<Metadata = ()>, C: IntoElement> IntoElement for (R, C) {
    fn into_element(self) -> Element {
        // Manually inline to avoid trait bound issues
        let child_element = AnyElement::new(self.1).into_element_inner();
        let child_id = insert_into_tree(child_element);

        let render_node = RenderNode::Single {
            render: Box::new(self.0),
            child: Some(child_id),
        };

        Element::Render(RenderElement::new(render_node))
    }
}

// ============================================================================
// Direct IntoElement for RenderObjects (most convenient)
// ============================================================================

// NOTE: We CANNOT impl IntoElement for all LeafRender because it conflicts
// with the blanket impl for View. Users must use LeafRenderBuilder::new()
// or the .into_builder() extension method.
//
// This is a Rust limitation - we can't have both:
// - impl<V: View> IntoElement for V
// - impl<R: LeafRender> IntoElement for R
//
// Because a type could implement both traits.

// Note: We CAN'T do direct impl for SingleRender and MultiRender either because
// they need children. Use builders or tuple syntax for those.
