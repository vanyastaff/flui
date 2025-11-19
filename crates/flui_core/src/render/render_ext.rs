//! Extension traits for ergonomic render object construction
//!
//! Provides fluent API for specifying children when building render objects,
//! making the code more readable and reducing boilerplate.
//!
//! # Design Philosophy
//!
//! Instead of using tuple syntax directly:
//! ```rust,ignore
//! (RenderPadding::new(padding), Some(child))  // Verbose
//! ```
//!
//! Use fluent builder API:
//! ```rust,ignore
//! RenderPadding::new(padding).child(child)  // Clear and concise
//! ```
//!
//! # Architecture
//!
//! ```text
//! RenderObject ──→ RenderExt/SliverExt ──→ Builder Wrapper ──→ IntoElement ──→ Element
//! ```
//!
//! The extension traits provide methods that return lightweight wrapper types,
//! which then implement `IntoElement` to create the final `Element`.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_core::render::{RenderExt, SliverExt};
//!
//! // Leaf render (no children)
//! impl View for Text {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderParagraph::new(&self.text).leaf()
//!     }
//! }
//!
//! // Single child
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderPadding::new(self.padding).child(self.child)
//!     }
//! }
//!
//! // Optional child
//! impl View for Container {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderContainer::new().maybe_child(self.child)  // child: Option<T>
//!     }
//! }
//!
//! // Multiple children
//! impl View for Column {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderFlex::column().children(self.children)
//!     }
//! }
//!
//! // Sliver renders - same method names!
//! impl View for SliverPadding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderSliverPadding::new(self.padding).child(self.child)
//!     }
//! }
//! ```
//!
//! # Comparison with Tuple Syntax
//!
//! | Tuple Syntax | Builder API | Improvement |
//! |--------------|-------------|-------------|
//! | `(render, ())` | `render.leaf()` | Explicit intent |
//! | `(render, Some(child))` | `render.child(child)` | No Option wrapping |
//! | `(render, self.child)` | `render.maybe_child(self.child)` | Handles Option<T> |
//! | `(render, vec![a, b])` | `render.children(vec![a, b])` | Same verbosity |
//!
//! # Type Safety
//!
//! All builder types are zero-cost abstractions that enforce arity at compile time:
//! - `WithLeaf<R>` - Requires `R: Render<Leaf>` or `R: SliverRender<Leaf>`
//! - `WithChild<R, C>` - Requires `R: Render<Single>` or `R: SliverRender<Single>`
//! - `WithOptionalChild<R, C>` - Requires `R: Render<Single>` or `R: SliverRender<Single>`
//! - `WithChildren<R, C>` - Requires `R: Render<Variable>` or `R: SliverRender<Variable>`

use crate::element::{Element, RenderElement};
use crate::view::into_element::IntoElement;

// ============================================================================
// Box Protocol Extension Trait
// ============================================================================

/// Extension trait for ergonomic box render object construction
///
/// Provides fluent methods for specifying children count when building render objects.
/// All methods return wrapper types that implement `IntoElement`.
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for all types, so you can use it
/// on any render object without manual implementation.
///
/// # Methods
///
/// - `leaf()` - No children (for `Render<Leaf>`)
/// - `child(c)` - Single required child (for `Render<Single>`)
/// - `maybe_child(opt)` - Optional child (for `Render<Single>`)
/// - `children(vec)` - Multiple children (for `Render<Variable>`)
///
/// # Thread Safety
///
/// All wrappers are `Send + Sync` if the underlying render object is.
pub trait RenderExt: Sized {
    /// Add a single required child
    ///
    /// Use this for render objects that always have exactly one child:
    /// - Padding
    /// - Transform
    /// - Opacity
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for Padding {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderPadding::new(self.padding).child(self.child)
    ///     }
    /// }
    /// ```
    ///
    /// # Type Flexibility
    ///
    /// Accepts any `C: IntoElement`:
    /// - `View` types (e.g., `Text::new("Hello")`)
    /// - `AnyElement` (type-erased)
    /// - `Box<dyn AnyView>`
    ///
    /// # Compile-Time Enforcement
    ///
    /// This method is only available when `Self: Render<Single>`.
    fn child<C: IntoElement>(self, child: C) -> WithChild<Self, C>
    where
        Self: crate::render::traits::Render<crate::render::Single>,
    {
        WithChild {
            render: self,
            child,
        }
    }

    /// Add an optional child
    ///
    /// Use this for render objects that may or may not have a child:
    /// - Containers with optional content
    /// - Conditional rendering wrappers
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for Container {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderContainer::new().maybe_child(self.child)
    ///     }
    /// }
    /// ```
    ///
    /// # Difference from `child()`
    ///
    /// - `child(c)` - Takes `C: IntoElement` directly
    /// - `maybe_child(opt)` - Takes `Option<C: IntoElement>`
    ///
    /// Both work with `Render<Single>`, but `maybe_child` handles the Option
    /// automatically.
    ///
    /// # Compile-Time Enforcement
    ///
    /// This method is only available when `Self: Render<Single>`.
    fn maybe_child<C: IntoElement>(self, child: Option<C>) -> WithOptionalChild<Self, C>
    where
        Self: crate::render::traits::Render<crate::render::Single>,
    {
        WithOptionalChild {
            render: self,
            child,
        }
    }

    /// Add multiple children
    ///
    /// Use this for render objects that accept variable number of children:
    /// - Flex layouts (Row, Column)
    /// - Stack
    /// - Wrap
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for Column {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderFlex::column().children(self.children)
    ///     }
    /// }
    /// ```
    ///
    /// # Type Flexibility
    ///
    /// Accepts `Vec<C>` where `C: IntoElement`:
    /// - `Vec<AnyElement>` (common pattern)
    /// - `Vec<Box<dyn AnyView>>` (type-erased views)
    /// - `Vec<SomeView>` (homogeneous views)
    ///
    /// # Compile-Time Enforcement
    ///
    /// This method is only available when `Self: Render<Variable>`.
    fn children<C: IntoElement>(self, children: Vec<C>) -> WithChildren<Self, C>
    where
        Self: crate::render::traits::Render<crate::render::Variable>,
    {
        WithChildren {
            render: self,
            children,
        }
    }
}

// Blanket implementation for all types
impl<R> RenderExt for R {}

// ============================================================================
// Sliver Protocol Extension Trait
// ============================================================================

/// Extension trait for ergonomic sliver render object construction
///
/// Provides the same fluent API as `RenderExt`, but for sliver-based render objects.
/// Uses the same method names for consistency and better DX.
///
/// # Design
///
/// Instead of prefixing methods with `sliver_`, we use the same names as `RenderExt`.
/// The compiler automatically selects the correct trait based on whether your type
/// implements `Render<A>` or `SliverRender<A>`.
///
/// # Example
///
/// ```rust,ignore
/// // Box render
/// RenderPadding::new(padding).child(content)  // Uses RenderExt
///
/// // Sliver render - same method name!
/// RenderSliverPadding::new(padding).child(content)  // Uses SliverExt
/// ```
///
/// # Methods
///
/// - `leaf()` - No children (for `SliverRender<Leaf>`)
/// - `child(c)` - Single required child (for `SliverRender<Single>`)
/// - `maybe_child(opt)` - Optional child (for `SliverRender<Single>`)
/// - `children(vec)` - Multiple children (for `SliverRender<Variable>`)
pub trait SliverExt: Sized {
    /// Add a single required sliver child
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for SliverPadding {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderSliverPadding::new(self.padding).child(self.sliver)
    ///     }
    /// }
    /// ```
    fn child<C: IntoElement>(self, child: C) -> SliverWithChild<Self, C>
    where
        Self: crate::render::traits::SliverRender<crate::render::Single>,
    {
        SliverWithChild {
            render: self,
            child,
        }
    }

    /// Add an optional sliver child
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for SliverContainer {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderSliverContainer::new().maybe_child(self.sliver)
    ///     }
    /// }
    /// ```
    fn maybe_child<C: IntoElement>(self, child: Option<C>) -> SliverWithOptionalChild<Self, C>
    where
        Self: crate::render::traits::SliverRender<crate::render::Single>,
    {
        SliverWithOptionalChild {
            render: self,
            child,
        }
    }

    /// Add multiple sliver children
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl View for SliverList {
    ///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///         RenderSliverList::new().children(self.items)
    ///     }
    /// }
    /// ```
    fn children<C: IntoElement>(self, children: Vec<C>) -> SliverWithChildren<Self, C>
    where
        Self: crate::render::traits::SliverRender<crate::render::Variable>,
    {
        SliverWithChildren {
            render: self,
            children,
        }
    }
}

// Blanket implementation for all types
impl<S> SliverExt for S {}

// ============================================================================
// Box Protocol Builder Wrapper Types
// ============================================================================

/// Wrapper for leaf render objects (no children)
///
/// Created by `RenderExt::leaf()`. Implements `IntoElement` to create
/// a `RenderElement` with zero children.
///
/// # Zero-Cost Abstraction
///
/// In release builds, this compiles to the same code as the tuple syntax `(render, ())`.
#[derive(Debug)]
pub struct WithLeaf<R> {
    render: R,
}

/// Wrapper for render objects with a single required child
///
/// Created by `RenderExt::child()`. Implements `IntoElement` to create
/// a `RenderElement` with one child.
///
/// # Zero-Cost Abstraction
///
/// In release builds, this compiles to the same code as the tuple syntax `(render, Some(child))`.
#[derive(Debug)]
pub struct WithChild<R, C> {
    render: R,
    child: C,
}

/// Wrapper for render objects with an optional child
///
/// Created by `RenderExt::maybe_child()`. Implements `IntoElement` to create
/// a `RenderElement` with zero or one child.
///
/// # Zero-Cost Abstraction
///
/// In release builds, this compiles to the same code as the tuple syntax `(render, child)`.
#[derive(Debug)]
pub struct WithOptionalChild<R, C> {
    render: R,
    child: Option<C>,
}

/// Wrapper for render objects with multiple children
///
/// Created by `RenderExt::children()`. Implements `IntoElement` to create
/// a `RenderElement` with variable children.
///
/// # Zero-Cost Abstraction
///
/// In release builds, this compiles to the same code as the tuple syntax `(render, children)`.
#[derive(Debug)]
pub struct WithChildren<R, C> {
    render: R,
    children: Vec<C>,
}

// ============================================================================
// Sliver Protocol Builder Wrapper Types
// ============================================================================

/// Wrapper for leaf sliver render objects (no children)
///
/// Created by `SliverExt::leaf()`. Will implement `IntoElement` to create
/// a sliver element with zero children.
#[derive(Debug)]
#[allow(dead_code)] // Reserved for future sliver migration
pub struct SliverWithLeaf<S> {
    render: S,
}

/// Wrapper for sliver render objects with a single required child
///
/// Created by `SliverExt::child()`. Will implement `IntoElement` to create
/// a sliver element with one child.
#[derive(Debug)]
#[allow(dead_code)] // Reserved for future sliver migration
pub struct SliverWithChild<S, C> {
    render: S,
    child: C,
}

/// Wrapper for sliver render objects with an optional child
///
/// Created by `SliverExt::maybe_child()`. Will implement `IntoElement` to create
/// a sliver element with zero or one child.
#[derive(Debug)]
#[allow(dead_code)] // Reserved for future sliver migration
pub struct SliverWithOptionalChild<S, C> {
    render: S,
    child: Option<C>,
}

/// Wrapper for sliver render objects with multiple children
///
/// Created by `SliverExt::children()`. Will implement `IntoElement` to create
/// a sliver element with variable children.
#[derive(Debug)]
#[allow(dead_code)] // Reserved for future sliver migration
pub struct SliverWithChildren<S, C> {
    render: S,
    children: Vec<C>,
}

// ============================================================================
// IntoElement Implementations - Box Protocol
// ============================================================================

// Implement sealed trait for all wrapper types
impl<R> crate::view::into_element::sealed_into_element::Sealed for WithLeaf<R> where
    R: crate::render::traits::Render<crate::render::Leaf>
{
}

impl<R, C> crate::view::into_element::sealed_into_element::Sealed for WithChild<R, C>
where
    R: crate::render::traits::Render<crate::render::Single>,
    C: IntoElement,
{
}

impl<R, C> crate::view::into_element::sealed_into_element::Sealed for WithOptionalChild<R, C>
where
    R: crate::render::traits::Render<crate::render::Single>,
    C: IntoElement,
{
}

impl<R, C> crate::view::into_element::sealed_into_element::Sealed for WithChildren<R, C>
where
    R: crate::render::traits::Render<crate::render::Variable>,
    C: IntoElement,
{
}

/// Convert `WithLeaf<R>` into `Element`
///
/// Delegates to the tuple syntax `(R, ())` which has the same semantics.
impl<R> IntoElement for WithLeaf<R>
where
    R: crate::render::traits::Render<crate::render::Leaf> + 'static,
{
    fn into_element(self) -> Element {
        Element::Render(RenderElement::box_leaf(self.render))
    }
}

/// Convert `WithChild<R, C>` into `Element`
///
/// Converts the child to `AnyElement` and delegates to the tuple syntax.
impl<R, C> IntoElement for WithChild<R, C>
where
    R: crate::render::traits::Render<crate::render::Single> + 'static,
    C: IntoElement,
{
    fn into_element(self) -> Element {
        use crate::view::into_element::AnyElement;

        // Convert child to AnyElement for type erasure
        let child_any = AnyElement::new(self.child);

        // Delegate to tuple implementation: (R, Option<AnyElement>)
        (self.render, Some(child_any)).into_element()
    }
}

/// Convert `WithOptionalChild<R, C>` into `Element`
///
/// Handles Option<C> and delegates to the tuple syntax.
impl<R, C> IntoElement for WithOptionalChild<R, C>
where
    R: crate::render::traits::Render<crate::render::Single> + 'static,
    C: IntoElement,
{
    fn into_element(self) -> Element {
        use crate::view::into_element::AnyElement;

        // Convert Option<C> to Option<AnyElement>
        let child_any = self.child.map(AnyElement::new);

        // Delegate to tuple implementation: (R, Option<AnyElement>)
        (self.render, child_any).into_element()
    }
}

/// Convert `WithChildren<R, C>` into `Element`
///
/// Converts all children to `AnyElement` and creates a multi-child render element.
impl<R, C> IntoElement for WithChildren<R, C>
where
    R: crate::render::traits::Render<crate::render::Variable> + 'static,
    C: IntoElement,
{
    fn into_element(self) -> Element {
        use crate::element::Element;
        use crate::view::into_element::AnyElement;

        // Convert Vec<C> to Vec<AnyElement>
        let children_any: Vec<AnyElement> =
            self.children.into_iter().map(AnyElement::new).collect();

        // Convert to Vec<Element>
        let child_elements: Vec<Element> =
            children_any.into_iter().map(|c| c.into_element()).collect();

        // Create RenderElement with children
        if child_elements.is_empty() {
            Element::Render(RenderElement::box_variable(self.render))
        } else {
            Element::Render(RenderElement::box_variable_with_children(
                self.render,
                child_elements,
            ))
        }
    }
}

// ============================================================================
// IntoElement Implementations - Sliver Protocol (TODO)
// ============================================================================

// TODO: Implement IntoElement for sliver wrappers after sliver migration completes
//
// The implementations will follow the same pattern as box wrappers, but will:
// 1. Create sliver elements instead of render elements
// 2. Use SliverElement::new() instead of RenderElement::box_*()
// 3. Support sliver-specific child handling

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::render::protocol::BoxLayoutContext;
    use crate::render::protocol::BoxPaintContext;
    use crate::render::traits::Render;
    use crate::render::Leaf;
    use flui_types::Size;

    // Mock render object for testing
    #[derive(Debug)]
    struct MockLeafRender {
        name: &'static str,
    }

    impl Render<Leaf> for MockLeafRender {
        fn layout(&mut self, _ctx: &BoxLayoutContext<Leaf>) -> Size {
            Size::new(100.0, 100.0)
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<Leaf>) {}
    }

    #[test]
    fn test_leaf_builder() {
        let render = MockLeafRender { name: "test" };
        let wrapper = render.leaf();

        // Convert to element
        let element = wrapper.into_element();

        // Verify it's a Render element
        assert!(matches!(element, Element::Render(_)));
    }

    #[test]
    fn test_render_ext_is_available() {
        // This test verifies the trait is in scope and callable
        let render = MockLeafRender { name: "test" };
        let _wrapper = RenderExt::leaf(render);
    }

    #[test]
    fn test_method_chaining_style() {
        // Verify fluent API works
        let render = MockLeafRender { name: "test" };
        let _element = render.leaf().into_element();
    }

    // Note: Tests for child(), maybe_child(), and children() will be added
    // after Render<Single> and Render<Variable> migrations are complete
}
