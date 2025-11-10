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
//!         Container::new()
//!             .child(Text::new(self.label))  // ← Text also IntoElement
//!     }
//! }
//!
//! // RenderObjects use builders that implement IntoElement
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderPadding::new(self.padding)  // ← Returns builder
//!             .with_child(self.child)       // ← Builder impl IntoElement
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
/// - `RenderBuilder<R>` for render objects
/// - Common types like `String`, `&str` (converted to Text)
///
/// # Usage
///
/// You typically don't implement this directly. Instead:
/// 1. Implement `View` for composable widgets
/// 2. Use `RenderBuilder` for render objects
/// 3. Use the provided implementations for primitives
///
/// # Example
///
/// ```rust,ignore
/// // Views automatically impl IntoElement
/// impl View for MyWidget {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child("Hello")          // &str impl IntoElement
///             .child(Text::new("Hi"))  // View impl IntoElement
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
    /// - Views use the blanket impl
    /// - RenderObjects use RenderBuilder
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
