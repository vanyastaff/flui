//! Universal interface for element conversion.
//!
//! Provides the `IntoElement` trait for converting any type into an `Element`.
//!
//! Similar to GPUI's `IntoElement` and Xilem's element conversion protocols.
//!
//! # Design
//!
//! ```text
//! View ─────────┐
//!               ├──→ IntoElement ──→ Element
//! Renderer ─────┘
//! ```
//!
//! Both Views and renderers (via tuples) implement `IntoElement`,
//! allowing them to be used interchangeably in the widget tree.
//!
//! # Example
//!
//! ```rust,ignore
//! // Views implement IntoElement automatically
//! impl View for Button {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Compose views - Container is also a View
//!         Container::new()
//!             .child(Text::new(self.label))
//!     }
//! }
//!
//! // Renderers use tuple syntax
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Tuple: (renderer, Option<child>)
//!         (RenderPadding::new(self.padding), self.child)
//!     }
//! }
//! ```

use crate::element::Element;

/// Sealed trait module - prevents external implementation of IntoElement
pub(crate) mod sealed_into_element {
    /// Sealed trait - only types in flui-core can implement IntoElement
    pub trait Sealed {}

    // Implement Sealed for all types that have IntoElement implementations

    // All Views can be converted to elements
    impl<V: crate::view::View> Sealed for V {}

    // Type-erased views
    impl Sealed for Box<dyn crate::view::AnyView> {}

    // Optional elements
    impl<T: Sealed> Sealed for Option<T> {}

    // Any element wrapper and element types
    impl Sealed for super::AnyElement {}
    impl Sealed for crate::element::RenderElement {}
    impl Sealed for crate::element::ComponentElement {}
    impl Sealed for crate::element::ProviderElement {}

    // Tuples with render objects and children
    impl<R: crate::render::Render> Sealed for (R, ()) {}
    impl<R: crate::render::Render> Sealed for (R, Option<super::AnyElement>) {}
    impl<R: crate::render::Render> Sealed for (R, Option<Box<dyn crate::view::AnyView>>) {}
    impl<R: crate::render::Render> Sealed for (R, Vec<Box<dyn crate::view::AnyView>>) {}
    impl<R: crate::render::Render> Sealed for (R, Vec<super::AnyElement>) {}
}

/// Universal interface for converting types into Elements.
///
/// Enables FLUI's flexible composition system where Views, RenderObjects,
/// and various helper types can be used interchangeably.
///
/// # Purpose
///
/// Bridges the View tree (immutable configuration) and Element tree (mutable state):
///
/// 1. Automatic conversion: `View → Element`
/// 2. Tuple syntax: `(RenderObject, children) → Element`
/// 3. Type erasure: `Box<dyn AnyView> → Element`
/// 4. Flexible composition: Mix different types in the same tree
///
/// # Sealed Trait
///
/// This trait is sealed. Only flui-core provides implementations.
///
/// To use:
/// - Implement `View` trait for composable widgets
/// - Use tuple syntax for render objects: `(RenderObject, children)`
/// - Framework provides `IntoElement` automatically
///
/// # Automatic Implementations
///
/// The framework provides implementations for:
///
/// | Type | Description | Example |
/// |------|-------------|---------|
/// | `impl View` | All views automatically | `Text::new("Hello")` |
/// | `(R, ())` | Leaf render (no children) | `(RenderBox::new(), ())` |
/// | `(R, Option<child>)` | Single child | `(RenderPadding::new(), child)` |
/// | `(R, Vec<children>)` | Multiple children | `(RenderFlex::column(), children)` |
/// | `Box<dyn AnyView>` | Type-erased view | `box_view` |
/// | `AnyElement` | Type-erased element | `AnyElement::new(view)` |
/// | `Option<T>` | Optional element | `Some(view)` or `None` |
///
/// # Design Rationale
///
/// IntoElement unifies different widget patterns under a single interface:
///
/// ```text
/// View ────────┐
///              ├──→ IntoElement ──→ Element ──→ ElementTree
/// RenderObject ┘
/// ```
///
/// This is similar to:
/// - **GPUI**: `IntoElement` trait for unified element creation
/// - **Xilem**: Element conversion protocols
/// - **React**: JSX transpiles to `React.createElement()`
///
/// # Usage
///
/// ## Views (Automatic)
///
/// ```rust,ignore
/// impl View for MyWidget {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Return another View - IntoElement impl is automatic!
///         Column::new()
///             .child(Text::new(self.title))
///             .child(Text::new(self.body))
///     }
/// }
/// ```
///
/// ## Tuple Syntax (Wrapping RenderObjects)
///
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Tuple: (RenderObject, children) → IntoElement impl is automatic!
///         (RenderPadding::new(self.padding), self.child)
///     }
/// }
/// ```
///
/// # Not Object-Safe
///
/// Like GPUI's `IntoElement`, this trait is not object-safe due to:
/// - `Sized` bound
/// - `impl Trait` return type
///
/// For dynamic dispatch, use `Box<dyn AnyView>` or `AnyElement` instead.
pub trait IntoElement: sealed_into_element::Sealed + Sized + 'static {
    /// Converts this type into an Element.
    ///
    /// Called by the framework to build the element tree. Converts children
    /// recursively and returns the final Element.
    ///
    /// # Implementation
    ///
    /// Most types do not implement this directly:
    /// - Views use the blanket implementation
    /// - Renderers use tuple syntax: `(render, children)`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Automatic via View:
    /// impl<V: View> IntoElement for V {
    ///     fn into_element(self) -> Element {
    ///         let ctx = current_build_context();
    ///         let element_like = self.build(ctx);
    ///         element_like.into_element()
    ///     }
    /// }
    /// ```
    fn into_element(self) -> Element;
}

// ============================================================================
// Automatic implementations
// ============================================================================

/// Blanket implementation for all Views.
///
/// Enables any View to be used as `impl IntoElement`. Uses thread-local
/// BuildContext to call View::build() and convert the result.
impl<V: crate::view::View> IntoElement for V {
    fn into_element(self) -> Element {
        use crate::view::build_context::current_build_context;

        // Get BuildContext from thread-local
        let ctx = current_build_context();

        // Call view's build() method
        let element_like = self.build(ctx);

        // Convert result to Element
        element_like.into_element()
    }
}

/// Implementation for type-erased views.
///
/// Enables using `Box<dyn AnyView>` as children:
///
/// ```rust,ignore
/// struct Padding {
///     child: Option<Box<dyn AnyView>>,
/// }
/// ```
impl IntoElement for Box<dyn crate::view::AnyView> {
    fn into_element(self) -> Element {
        (*self).build_any()
    }
}

/// Implementation for optional elements.
///
/// Enables optional children:
///
/// ```rust,ignore
/// Container::new()
///     .child(self.child)  // child: Option<impl IntoElement>
/// ```
impl<T: IntoElement> IntoElement for Option<T> {
    fn into_element(self) -> Element {
        match self {
            Some(element) => element.into_element(),
            None => {
                // Return empty render element with zero size
                use crate::element::RenderElement;

                Element::Render(RenderElement::new(Box::new(EmptyRender)))
            }
        }
    }
}

/// Empty render object.
///
/// Returns zero size and empty layer. Used for Option::None and placeholder elements.
#[derive(Debug)]
struct EmptyRender;

impl crate::render::Render for EmptyRender {
    fn layout(&mut self, _ctx: &crate::render::LayoutContext) -> flui_types::Size {
        flui_types::Size::ZERO
    }

    fn paint(&self, _ctx: &crate::render::PaintContext) -> flui_engine::BoxedLayer {
        Box::new(flui_engine::ContainerLayer::new())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> crate::render::Arity {
        crate::render::Arity::Exact(0)
    }
}

// ============================================================================
// Helper types
// ============================================================================

/// Type-erased IntoElement for dynamic dispatch.
///
/// Since `IntoElement` is not object-safe, this wrapper enables
/// heterogeneous collections.
///
/// # Example
///
/// ```rust,ignore
/// struct Column {
///     children: Vec<AnyElement>,  // ← Different types
/// }
///
/// Column::new()
///     .child(Text::new("Hello"))    // Different types
///     .child(Button::new("Click"))  // in same collection
/// ```
#[derive(Debug)]
pub struct AnyElement {
    element: Element,
}

impl AnyElement {
    /// Creates from any IntoElement.
    pub fn new(into_element: impl IntoElement) -> Self {
        Self {
            element: into_element.into_element(),
        }
    }

    /// Unwraps into Element.
    pub fn into_element_inner(self) -> Element {
        self.element
    }
}

impl IntoElement for AnyElement {
    fn into_element(self) -> Element {
        self.element
    }
}

/// Extension trait for convenient AnyElement creation.
pub trait IntoAnyElement: IntoElement {
    /// Converts to AnyElement for type erasure.
    fn into_any(self) -> AnyElement {
        AnyElement::new(self)
    }
}

// Blanket implementation
impl<T: IntoElement> IntoAnyElement for T {}

// ============================================================================
// Tuple Syntax for Render Objects
// ============================================================================

/// Extension trait for convenient boxing of Render objects
///
/// Allows writing `.boxed()` instead of `Box::new() as Box<dyn Render>`
pub trait RenderExt: crate::render::Render + Sized {
    /// Box this render object for use with tuple syntax
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// (RenderPadding::new().boxed(), self.child)
    /// ```
    fn boxed(self) -> Box<dyn crate::render::Render> {
        Box::new(self)
    }
}

// Blanket implementation for all Render types
impl<T: crate::render::Render> RenderExt for T {}

/// Tuple implementation for (Render, ()) - leaf render case
///
/// Consistent syntax for leaf renders using tuple:
/// ```rust,ignore
/// impl View for Text {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderParagraph::new(&self.text), ())
///     }
/// }
/// ```
impl<R: crate::render::Render> IntoElement for (R, ()) {
    fn into_element(self) -> Element {
        Element::Render(crate::element::RenderElement::new(Box::new(self.0)))
    }
}

/// Generic tuple implementation for (Render, Option<child>) - single-child syntax
///
/// Works with any Render type directly, no boxing needed:
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderPadding::new(self.padding), self.child)  // No .boxed()!
///     }
/// }
/// ```
impl<R: crate::render::Render> IntoElement for (R, Option<AnyElement>) {
    fn into_element(self) -> Element {
        let (render, child) = self;
        let children: Vec<Element> = child.into_iter().map(|c| c.into_element()).collect();

        let render_element = if children.is_empty() {
            crate::element::RenderElement::new(Box::new(render))
        } else {
            crate::element::RenderElement::new_with_children(Box::new(render), children)
        };

        Element::Render(render_element)
    }
}

/// Convenience implementation for (Render, Option<Box<dyn AnyView>>) - widget child
///
/// Allows passing Option<Box<dyn AnyView>> directly without conversion:
/// ```rust,ignore
/// pub struct Padding {
///     child: Option<Box<dyn AnyView>>,  // Common pattern
/// }
///
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderPadding::new(self.padding), self.child)  // Works directly!
///     }
/// }
/// ```
impl<R: crate::render::Render> IntoElement for (R, Option<Box<dyn crate::view::AnyView>>) {
    fn into_element(self) -> Element {
        let (render, child) = self;
        // Convert Option<Box<dyn AnyView>> to Option<AnyElement>
        let child_element = child.map(AnyElement::new);
        let children: Vec<Element> = child_element.into_iter().map(|c| c.into_element()).collect();

        let render_element = if children.is_empty() {
            crate::element::RenderElement::new(Box::new(render))
        } else {
            crate::element::RenderElement::new_with_children(Box::new(render), children)
        };

        Element::Render(render_element)
    }
}

/// Convenience implementation for (Render, Vec<Box<dyn AnyView>>) - widget children
///
/// Allows passing Vec<Box<dyn AnyView>> directly without conversion:
/// ```rust,ignore
/// pub struct Column {
///     children: Vec<Box<dyn AnyView>>,  // Common pattern
/// }
///
/// impl View for Column {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderFlex::column(), self.children)  // Works directly!
///     }
/// }
/// ```
impl<R: crate::render::Render> IntoElement for (R, Vec<Box<dyn crate::view::AnyView>>) {
    fn into_element(self) -> Element {
        let (render, children) = self;
        // Convert Vec<Box<dyn AnyView>> to Vec<AnyElement>
        let child_elements: Vec<Element> = children
            .into_iter()
            .map(|c| AnyElement::new(c).into_element())
            .collect();

        let render_element = if child_elements.is_empty() {
            crate::element::RenderElement::new(Box::new(render))
        } else {
            crate::element::RenderElement::new_with_children(Box::new(render), child_elements)
        };

        Element::Render(render_element)
    }
}

/// Generic tuple implementation for (Render, Vec<children>) - multi-child syntax
///
/// Works with any Render type directly, no boxing needed:
/// ```rust,ignore
/// impl View for Column {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         (RenderFlex::column(), self.children)  // No .boxed()!
///     }
/// }
/// ```
impl<R: crate::render::Render> IntoElement for (R, Vec<AnyElement>) {
    fn into_element(self) -> Element {
        let (render, children) = self;
        let child_elements: Vec<Element> = children
            .into_iter()
            .map(|c| c.into_element())
            .collect();

        let render_element = if child_elements.is_empty() {
            crate::element::RenderElement::new(Box::new(render))
        } else {
            crate::element::RenderElement::new_with_children(Box::new(render), child_elements)
        };

        Element::Render(render_element)
    }
}
