//! IntoElement trait - universal interface for element conversion
//!
//! This module provides the `IntoElement` trait, which serves as the
//! universal interface for converting any type into an `Element`.
//!
//! # Philosophy
//!
//! Similar to GPUI's `IntoElement` and Xilem's element conversion,
//! this trait unifies all widget types under a single interface.
//!
//! # Design
//!
//! ```text
//! View ────────┐
//!              ├──→ IntoElement ──→ Element
//! RenderObject ┘
//! ```
//!
//! Both Views and RenderObjects implement `IntoElement`,
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
//! // RenderObjects use tuple syntax
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Tuple: (render object, Option<child>)
//!         (RenderPadding::new(self.padding), self.child)
//!     }
//! }
//! ```

use crate::element::Element;

/// IntoElement trait - converts types into Elements
///
/// This is the core trait that enables the simplified View API.
/// Any type that implements `IntoElement` can be used as a widget.
///
/// # Automatic Implementations
///
/// The framework provides automatic implementations for:
/// - All `View` types (via blanket impl)
/// - Tuple syntax for render objects: `(R, ())`, `(R, Option<child>)`, `(R, Vec<children>)`
/// - Convenience wrappers for `Box<dyn AnyView>` types
///
/// # Usage
///
/// You typically don't implement this directly. Instead:
/// 1. Implement `View` for composable widgets
/// 2. Use tuple syntax for render objects: `(render, children)`
/// 3. Use the provided implementations for type conversions
///
/// # Example
///
/// ```rust,ignore
/// // Views automatically impl IntoElement
/// impl View for MyWidget {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Use tuple syntax to combine render object with children
///         (RenderColumn::new(), vec![
///             AnyElement::new(Text::new("Hello")),
///             AnyElement::new(Text::new("Hi")),
///         ])
///     }
/// }
/// ```
///
/// # Not dyn-compatible
///
/// Like GPUI's `IntoElement`, this trait is not object-safe due to
/// `Sized` bound and `impl Trait` in methods. Use `Box<dyn IntoElement>`
/// if you need dynamic dispatch (will be added separately).
pub trait IntoElement: Sized + 'static {
    /// Convert this type into an Element
    ///
    /// This method is called by the framework to build the element tree.
    /// It should:
    /// 1. Convert children recursively
    /// 2. Insert into element tree if needed
    /// 3. Return the final Element
    ///
    /// # Implementation Note
    ///
    /// Most types don't implement this directly. Instead:
    /// - Views use the blanket impl (automatic)
    /// - RenderObjects use tuple syntax: `(render, children)`
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

/// Blanket implementation for all Views
///
/// This enables any View to be used as `impl IntoElement`.
///
/// Uses thread-local BuildContext to call View::build() and convert the result.
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

/// Implementation for Box<dyn AnyView>
///
/// Allows using type-erased views as children:
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

/// Implementation for Option<T: IntoElement>
///
/// Allows optional children:
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

/// Empty render object - returns zero size and empty layer
///
/// Used for Option::None and other cases where a placeholder element is needed.
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

/// AnyElement - type-erased IntoElement for dynamic dispatch
///
/// Since `IntoElement` is not object-safe, we provide this wrapper
/// for cases where you need heterogeneous collections.
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
    /// Create from any IntoElement
    pub fn new(into_element: impl IntoElement) -> Self {
        Self {
            element: into_element.into_element(),
        }
    }

    /// Unwrap into Element
    pub fn into_element_inner(self) -> Element {
        self.element
    }
}

impl IntoElement for AnyElement {
    fn into_element(self) -> Element {
        self.element
    }
}

/// Extension trait for convenient AnyElement creation
pub trait IntoAnyElement: IntoElement {
    /// Convert to AnyElement for type erasure
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
