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
//! Both Views and renderers (via RenderBoxExt) implement `IntoElement`,
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
//! // Renderers use RenderBoxExt API
//! impl View for Padding {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         RenderPadding::new(self.padding).maybe_child(self.child)
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

    // Optional elements
    impl<T: Sealed> Sealed for Option<T> {}

    // Element types
    impl Sealed for crate::element::Element {}
    impl Sealed for crate::element::RenderElement {}
    impl Sealed for crate::element::ComponentElement {}
    impl Sealed for crate::element::ProviderElement {}

    // RenderBoxExt wrapper types
    impl<R: crate::render::RenderBox<crate::render::Leaf>> Sealed for crate::render::WithLeaf<R> {}
    impl<R: crate::render::RenderBox<crate::render::Single>, C: crate::view::IntoElement> Sealed
        for crate::render::WithChild<R, C>
    {
    }
    impl<R: crate::render::RenderBox<crate::render::Single>> Sealed
        for crate::render::WithOptionalChild<R>
    {
    }
    impl<R: crate::render::RenderBox<crate::render::Optional>> Sealed
        for crate::render::WithMaybeChild<R>
    {
    }
    impl<R: crate::render::RenderBox<crate::render::Variable>> Sealed
        for crate::render::WithChildren<R>
    {
    }

    // SliverExt wrapper types
    impl<S: crate::render::SliverRender<crate::render::Leaf>> Sealed
        for crate::render::SliverWithLeaf<S>
    {
    }
    impl<S: crate::render::SliverRender<crate::render::Single>, C: crate::view::IntoElement> Sealed
        for crate::render::SliverWithChild<S, C>
    {
    }
    impl<S: crate::render::SliverRender<crate::render::Single>, C: crate::view::IntoElement> Sealed
        for crate::render::SliverWithOptionalChild<S, C>
    {
    }
    impl<S: crate::render::SliverRender<crate::render::Variable>, C: crate::view::IntoElement>
        Sealed for crate::render::SliverWithChildren<S, C>
    {
    }
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
/// 2. RenderBoxExt API: `RenderObject.leaf()` / `.child()` / `.children()`
/// 3. Flexible composition: Mix different types in the same tree
///
/// # Sealed Trait
///
/// This trait is sealed. Only flui-core provides implementations.
///
/// To use:
/// - Implement `View` trait for composable widgets
/// - Use `RenderBoxExt` for render objects
/// - Framework provides `IntoElement` automatically
///
/// # Automatic Implementations
///
/// The framework provides implementations for:
///
/// | Type | Description | Example |
/// |------|-------------|---------|
/// | `impl View` | All views automatically | `Text::new("Hello")` |
/// | `WithLeaf<R>` | Leaf render (no children) | `render.leaf()` |
/// | `WithChild<R, C>` | Single child | `render.child(child)` |
/// | `WithOptionalChild<R, C>` | Optional child | `render.maybe_child(child)` |
/// | `WithChildren<R, C>` | Multiple children | `render.children(vec![...])` |
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
/// ## RenderBoxExt API
///
/// ```rust,ignore
/// impl View for Padding {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         RenderPadding::new(self.padding).maybe_child(self.child)
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
/// For dynamic dispatch, use `AnyElement` instead.
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
    /// - Renderers use `RenderBoxExt`: `.leaf()`, `.child()`, etc.
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

/// Identity implementation for Element itself.
///
/// Enables Element to be returned directly from build():
///
/// ```rust,ignore
/// fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///     // Can return Element directly
///     some_view.into_element()
/// }
/// ```
impl IntoElement for Element {
    fn into_element(self) -> Element {
        self
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
                use crate::render::{EmptyRender, RenderBoxExt};

                EmptyRender.leaf().into_element()
            }
        }
    }
}
