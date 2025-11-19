//! IntoElement implementations for render objects
//!
//! This module provides `IntoElement` implementations for `Render<A>` and `SliverRender<A>`
//! traits, enabling direct conversion of render objects to Elements with compile-time
//! arity validation.
//!
//! # Architecture
//!
//! ```text
//! Render<Leaf>     + ()           → RenderElement (no children)
//! Render<Single>   + child        → RenderElement (1 child)
//! Render<Variable> + Vec<children> → RenderElement (N children)
//! ```
//!
//! # Required RenderElement Constructors
//!
//! This module requires the following constructors to be added to `RenderElement`:
//!
//! ```rust,ignore
//! impl RenderElement {
//!     /// Create from type-erased DynRenderObject (Box protocol)
//!     pub fn new_dyn(render: Box<dyn DynRenderObject>) -> Self { ... }
//!
//!     /// Create from type-erased DynRenderObject with children (Box protocol)
//!     pub fn new_dyn_with_children(
//!         render: Box<dyn DynRenderObject>,
//!         children: Vec<Element>
//!     ) -> Self { ... }
//!
//!     /// Create from type-erased DynRenderObject (Sliver protocol)
//!     pub fn new_sliver_dyn(render: Box<dyn DynRenderObject>) -> Self { ... }
//!
//!     /// Create from type-erased DynRenderObject with children (Sliver protocol)
//!     pub fn new_sliver_dyn_with_children(
//!         render: Box<dyn DynRenderObject>,
//!         children: Vec<Element>
//!     ) -> Self { ... }
//! }
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Tuple syntax with compile-time arity validation
//!         (RenderPadding::new(self.padding), self.child)
//!     }
//! }
//! ```

use crate::element::{Element, RenderElement};
use crate::render::arity::{Leaf, Optional, Single, Variable};
use crate::render::traits::{Render, SliverRender};
use crate::render::wrappers::{BoxRenderObjectWrapper, SliverRenderObjectWrapper};
use crate::view::into_element::{AnyElement, IntoElement};
use crate::view::AnyView;

// ============================================================================
// Box Protocol - Leaf Renders (0 children)
// ============================================================================

/// Tuple implementation for (Render<Leaf>, ()) - leaf render case
///
/// For renders with no children:
/// ```rust,ignore
/// impl View for Text {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderParagraph::new(&self.text), ())
///     }
/// }
/// ```
impl<R> IntoElement for (R, ())
where
    R: Render<Leaf>,
{
    fn into_element(self) -> Element {
        let (render, _) = self;
        let wrapped = BoxRenderObjectWrapper::<Leaf, R>::new(render);
        Element::Render(RenderElement::new_dyn(Box::new(wrapped)))
    }
}

// ============================================================================
// Box Protocol - Optional Child Renders (0-1 children)
// ============================================================================

/// Tuple implementation for (Render<Optional>, Option<AnyElement>)
///
/// For renders with optional child:
/// ```rust,ignore
/// impl View for Container {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderDecoratedBox::new(self.decoration), self.child)
///     }
/// }
/// ```
impl<R> IntoElement for (R, Option<AnyElement>)
where
    R: Render<Optional>,
{
    fn into_element(self) -> Element {
        let (render, child) = self;
        let wrapped = BoxRenderObjectWrapper::<Optional, R>::new(render);

        let children: Vec<Element> = child.into_iter().map(|c| c.into_element()).collect();

        if children.is_empty() {
            Element::Render(RenderElement::new_dyn(Box::new(wrapped)))
        } else {
            Element::Render(RenderElement::new_dyn_with_children(
                Box::new(wrapped),
                children,
            ))
        }
    }
}

/// Convenience implementation for (Render<Optional>, Option<Box<dyn AnyView>>)
impl<R> IntoElement for (R, Option<Box<dyn AnyView>>)
where
    R: Render<Optional>,
{
    fn into_element(self) -> Element {
        let (render, child) = self;
        let child_element = child.map(AnyElement::new);
        (render, child_element).into_element()
    }
}

// ============================================================================
// Box Protocol - Single Child Renders (exactly 1 child)
// ============================================================================

/// Tuple implementation for (Render<Single>, AnyElement) - single child
///
/// For renders with exactly one child:
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderPadding::new(self.padding), self.child.into_any())
///     }
/// }
/// ```
impl<R> IntoElement for (R, AnyElement)
where
    R: Render<Single>,
{
    fn into_element(self) -> Element {
        let (render, child) = self;
        let wrapped = BoxRenderObjectWrapper::<Single, R>::new(render);

        let children = vec![child.into_element()];
        Element::Render(RenderElement::new_dyn_with_children(
            Box::new(wrapped),
            children,
        ))
    }
}

/// Convenience implementation for (Render<Single>, Box<dyn AnyView>)
impl<R> IntoElement for (R, Box<dyn AnyView>)
where
    R: Render<Single>,
{
    fn into_element(self) -> Element {
        let (render, child) = self;
        (render, AnyElement::new(child)).into_element()
    }
}

// ============================================================================
// Box Protocol - Variable Children Renders (0+ children)
// ============================================================================

/// Tuple implementation for (Render<Variable>, Vec<AnyElement>) - multi-child
///
/// For renders with any number of children:
/// ```rust,ignore
/// impl View for Column {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderFlex::column(), self.children)
///     }
/// }
/// ```
impl<R> IntoElement for (R, Vec<AnyElement>)
where
    R: Render<Variable>,
{
    fn into_element(self) -> Element {
        let (render, children) = self;
        let wrapped = BoxRenderObjectWrapper::<Variable, R>::new(render);

        let child_elements: Vec<Element> = children.into_iter().map(|c| c.into_element()).collect();

        if child_elements.is_empty() {
            Element::Render(RenderElement::new_dyn(Box::new(wrapped)))
        } else {
            Element::Render(RenderElement::new_dyn_with_children(
                Box::new(wrapped),
                child_elements,
            ))
        }
    }
}

/// Convenience implementation for (Render<Variable>, Vec<Box<dyn AnyView>>)
impl<R> IntoElement for (R, Vec<Box<dyn AnyView>>)
where
    R: Render<Variable>,
{
    fn into_element(self) -> Element {
        let (render, children) = self;
        let child_elements: Vec<AnyElement> = children.into_iter().map(AnyElement::new).collect();
        (render, child_elements).into_element()
    }
}

// ============================================================================
// Sliver Protocol - Leaf Slivers (0 children)
// ============================================================================

/// Wrapper for SliverRender objects to avoid trait overlap
///
/// Since Rust doesn't support negative trait bounds, we use this wrapper
/// to disambiguate sliver from box renders.
///
/// # Usage
///
/// ```rust,ignore
/// impl View for SliverToBoxAdapter {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Sliver(RenderSliverToBoxAdapter::new(), self.child)
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Sliver<S, C>(pub S, pub C);

/// Helper function to create Sliver wrapper with ergonomic syntax
///
/// # Examples
///
/// ```rust,ignore
/// // Leaf sliver
/// sliver(RenderSliverFillRemaining::new(), ())
///
/// // Single child sliver
/// sliver(RenderSliverPadding::new(padding), child)
///
/// // Multi-child sliver
/// sliver(RenderSliverList::new(), children)
/// ```
#[inline]
pub fn sliver<S, C>(render: S, children: C) -> Sliver<S, C> {
    Sliver(render, children)
}

/// Sliver implementation for (SliverRender<Leaf>, ()) - leaf sliver
impl<S> IntoElement for Sliver<S, ()>
where
    S: SliverRender<Leaf>,
{
    fn into_element(self) -> Element {
        let Sliver(render, _) = self;
        let wrapped = SliverRenderObjectWrapper::<Leaf, S>::new(render);
        Element::Render(RenderElement::new_sliver_dyn(Box::new(wrapped)))
    }
}

// ============================================================================
// Sliver Protocol - Optional Child Slivers (0-1 children)
// ============================================================================

/// Sliver implementation for (SliverRender<Optional>, Option<AnyElement>)
impl<S> IntoElement for Sliver<S, Option<AnyElement>>
where
    S: SliverRender<Optional>,
{
    fn into_element(self) -> Element {
        let Sliver(render, child) = self;
        let wrapped = SliverRenderObjectWrapper::<Optional, S>::new(render);

        let children: Vec<Element> = child.into_iter().map(|c| c.into_element()).collect();

        if children.is_empty() {
            Element::Render(RenderElement::new_sliver_dyn(Box::new(wrapped)))
        } else {
            Element::Render(RenderElement::new_sliver_dyn_with_children(
                Box::new(wrapped),
                children,
            ))
        }
    }
}

/// Convenience implementation for Sliver<SliverRender<Optional>, Option<Box<dyn AnyView>>>
impl<S> IntoElement for Sliver<S, Option<Box<dyn AnyView>>>
where
    S: SliverRender<Optional>,
{
    fn into_element(self) -> Element {
        let Sliver(render, child) = self;
        let child_element = child.map(AnyElement::new);
        Sliver(render, child_element).into_element()
    }
}

// ============================================================================
// Sliver Protocol - Single Child Slivers (exactly 1 child)
// ============================================================================

/// Sliver implementation for (SliverRender<Single>, AnyElement)
impl<S> IntoElement for Sliver<S, AnyElement>
where
    S: SliverRender<Single>,
{
    fn into_element(self) -> Element {
        let Sliver(render, child) = self;
        let wrapped = SliverRenderObjectWrapper::<Single, S>::new(render);

        let children = vec![child.into_element()];
        Element::Render(RenderElement::new_sliver_dyn_with_children(
            Box::new(wrapped),
            children,
        ))
    }
}

/// Convenience implementation for Sliver<SliverRender<Single>, Box<dyn AnyView>>
impl<S> IntoElement for Sliver<S, Box<dyn AnyView>>
where
    S: SliverRender<Single>,
{
    fn into_element(self) -> Element {
        let Sliver(render, child) = self;
        Sliver(render, AnyElement::new(child)).into_element()
    }
}

// ============================================================================
// Sliver Protocol - Variable Children Slivers (0+ children)
// ============================================================================

/// Sliver implementation for (SliverRender<Variable>, Vec<AnyElement>)
impl<S> IntoElement for Sliver<S, Vec<AnyElement>>
where
    S: SliverRender<Variable>,
{
    fn into_element(self) -> Element {
        let Sliver(render, children) = self;
        let wrapped = SliverRenderObjectWrapper::<Variable, S>::new(render);

        let child_elements: Vec<Element> = children.into_iter().map(|c| c.into_element()).collect();

        if child_elements.is_empty() {
            Element::Render(RenderElement::new_sliver_dyn(Box::new(wrapped)))
        } else {
            Element::Render(RenderElement::new_sliver_dyn_with_children(
                Box::new(wrapped),
                child_elements,
            ))
        }
    }
}

/// Convenience implementation for Sliver<SliverRender<Variable>, Vec<Box<dyn AnyView>>>
impl<S> IntoElement for Sliver<S, Vec<Box<dyn AnyView>>>
where
    S: SliverRender<Variable>,
{
    fn into_element(self) -> Element {
        let Sliver(render, children) = self;
        let child_elements: Vec<AnyElement> = children.into_iter().map(AnyElement::new).collect();
        Sliver(render, child_elements).into_element()
    }
}

// ============================================================================
// Sealed Trait Extensions
// ============================================================================

/// Add sealed implementations for render tuples
///
/// These need to be added to the sealed module in view/into_element.rs
pub mod sealed_extensions {
    use super::*;
    use crate::view::into_element::sealed_into_element::Sealed;

    // Box protocol tuple implementations
    impl<R: Render<Leaf>> Sealed for (R, ()) {}
    impl<R: Render<Optional>> Sealed for (R, Option<AnyElement>) {}
    impl<R: Render<Optional>> Sealed for (R, Option<Box<dyn AnyView>>) {}
    impl<R: Render<Single>> Sealed for (R, AnyElement) {}
    impl<R: Render<Single>> Sealed for (R, Box<dyn AnyView>) {}
    impl<R: Render<Variable>> Sealed for (R, Vec<AnyElement>) {}
    impl<R: Render<Variable>> Sealed for (R, Vec<Box<dyn AnyView>>) {}

    // Sliver protocol wrapper implementations
    impl<S: SliverRender<Leaf>> Sealed for Sliver<S, ()> {}
    impl<S: SliverRender<Optional>> Sealed for Sliver<S, Option<AnyElement>> {}
    impl<S: SliverRender<Optional>> Sealed for Sliver<S, Option<Box<dyn AnyView>>> {}
    impl<S: SliverRender<Single>> Sealed for Sliver<S, AnyElement> {}
    impl<S: SliverRender<Single>> Sealed for Sliver<S, Box<dyn AnyView>> {}
    impl<S: SliverRender<Variable>> Sealed for Sliver<S, Vec<AnyElement>> {}
    impl<S: SliverRender<Variable>> Sealed for Sliver<S, Vec<Box<dyn AnyView>>> {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::hit_test::BoxHitTestResult;
    use crate::render::protocol::{
        BoxGeometry, BoxHitTestContext, BoxLayoutContext, BoxPaintContext,
    };
    use flui_types::Size;

    // Mock leaf render for testing
    #[derive(Debug)]
    struct MockLeafRender {
        size: Size,
    }

    impl Render<Leaf> for MockLeafRender {
        fn layout(&mut self, ctx: &BoxLayoutContext<Leaf>) -> BoxGeometry {
            BoxGeometry {
                size: ctx.constraints.constrain(self.size),
            }
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<Leaf>) {
            // No-op
        }

        fn hit_test(&self, ctx: &BoxHitTestContext<Leaf>, _result: &mut BoxHitTestResult) -> bool {
            ctx.position.dx >= 0.0
                && ctx.position.dy >= 0.0
                && ctx.position.dx <= ctx.size.width
                && ctx.position.dy <= ctx.size.height
        }
    }

    #[test]
    fn test_leaf_render_into_element() {
        let render = MockLeafRender {
            size: Size::new(100.0, 50.0),
        };
        let _element = (render, ()).into_element();
        // Element created successfully
    }

    // Mock single-child render for testing
    #[derive(Debug)]
    struct MockSingleRender;

    impl Render<Single> for MockSingleRender {
        fn layout(&mut self, ctx: &BoxLayoutContext<Single>) -> BoxGeometry {
            let child = ctx.children().single();
            let child_size = ctx.layout_child(child, ctx.constraints);
            BoxGeometry { size: child_size }
        }

        fn paint(&self, ctx: &mut BoxPaintContext<Single>) {
            let child = ctx.children().single();
            ctx.paint_child(child, ctx.offset);
        }

        fn hit_test(
            &self,
            _ctx: &BoxHitTestContext<Single>,
            _result: &mut BoxHitTestResult,
        ) -> bool {
            true
        }
    }

    // Note: Full integration tests require BuildContext setup
    // These are structural tests only
}
