//! View trait - Core abstraction for reactive UI
//!
//! The View trait is the simplified, unified API for building UI in Flui.
//! It eliminates boilerplate while maintaining the proven three-tree architecture.

use super::build_context::BuildContext;
use super::IntoElement;
use crate::element::Element;
use std::any::Any;

/// View trait - simplified API for reactive UI
///
/// The View trait is FLUI's core abstraction for building user interfaces.
/// It represents a lightweight, immutable description of what the UI should look like.
///
/// # What is a View?
///
/// A View is similar to:
/// - **Flutter**: Widget (declarative UI description)
/// - **React**: Component (returns JSX/elements)
/// - **SwiftUI**: View protocol
///
/// Views are **configuration objects**, not widgets themselves. The framework converts
/// Views into Elements (mutable state) which manage RenderObjects (layout/paint).
///
/// # Three-Tree Architecture
///
/// Views are the first layer in FLUI's architecture:
///
/// ```text
/// View Tree          →    Element Tree    →    Render Tree
/// (immutable)             (mutable)             (layout/paint)
/// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/// View::build()      →    Element state   →    Render::layout()
/// Returns elements        Lifecycle mgmt       Render::paint()
/// ```
///
/// # Design Philosophy (v0.6.0+)
///
/// The View API has been radically simplified:
///
/// - **No GAT State**: Use hooks (`use_signal`, `use_memo`) instead of generic state types
/// - **No GAT Element**: Return `impl IntoElement` instead of associated Element types
/// - **No rebuild()**: Framework handles efficient diffing automatically
/// - **Clone Required**: Views must implement `Clone` for type erasure with `AnyView`
/// - **'static Required**: Views must be `'static` to enable safe storage and lifecycle
/// - **Composable**: Views compose via `IntoElement` trait
///
/// **Result:** 75% less boilerplate compared to the old API!
///
/// # Examples
///
/// ## Simple Composite Widget
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext};
///
/// #[derive(Debug, Clone)]
/// struct Card {
///     title: String,
///     content: String,
/// }
///
/// impl View for Card {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         Column::new()
///             .child(Text::new(self.title).size(24.0))
///             .child(Padding::all(16.0).child(Text::new(self.content)))
///     }
/// }
/// ```
///
/// ## With Hooks (Stateful Widget)
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext, use_signal};
///
/// #[derive(Debug, Clone)]
/// struct Counter;
///
/// impl View for Counter {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Hooks for state management
///         let count = use_signal(ctx, 0);
///
///         Column::new()
///             .child(Text::new(format!("Count: {}", count.get())))
///             .child(
///                 Button::new("Increment")
///                     .on_click(move || count.update(|n| n + 1))
///             )
///     }
/// }
/// ```
///
/// ## Render Widget (Wraps Renderer)
///
/// ```rust,ignore
/// use flui_core::{View, IntoElement, BuildContext};
/// use flui_rendering::RenderPadding;
///
/// #[derive(Debug, Clone)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: Option<Box<dyn AnyView>>,
/// }
///
/// impl View for Padding {
///     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
///         // Tuple syntax for single-child render (shortest!)
///         (RenderPadding::new(self.padding), self.child)
///
///         // Or builder syntax:
///         // RenderPadding::new(self.padding)
///         //     .into_builder()
///         //     .child(self.child)
///     }
/// }
/// ```
pub trait View: Clone + 'static {
    /// Build this view into an element
    ///
    /// This is the only required method for the View trait. It describes what
    /// the UI should look like based on the current view configuration.
    ///
    /// # Return Types
    ///
    /// Returns anything that implements `IntoElement`:
    ///
    /// 1. **Other Views** (composition):
    ///    ```rust,ignore
    ///    fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///        Column::new()
    ///            .child(Text::new(self.title))
    ///            .child(Text::new(self.body))
    ///    }
    ///    ```
    ///
    /// 2. **Tuple of (RenderObject, children)** (wrapping renderers):
    ///    ```rust,ignore
    ///    fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///        // Leaf: (RenderObject, ())
    ///        (RenderText::new(self.text), ())
    ///
    ///        // Single child: (RenderObject, Option<child>)
    ///        (RenderPadding::new(self.padding), self.child)
    ///
    ///        // Multiple children: (RenderObject, Vec<children>)
    ///        (RenderFlex::column(), self.children)
    ///    }
    ///    ```
    ///
    /// # State Management with Hooks
    ///
    /// Use hooks instead of GAT State parameter:
    /// ```rust,ignore
    /// fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///     // Reactive state
    ///     let count = use_signal(ctx, 0);
    ///
    ///     // Derived state (memoized)
    ///     let doubled = use_memo(ctx, move |_| count.get() * 2);
    ///
    ///     // Side effects
    ///     use_effect(ctx, move || {
    ///         println!("Count changed: {}", count.get());
    ///         None  // No cleanup
    ///     });
    ///
    ///     // Build UI using state
    ///     Column::new()
    ///         .child(Text::new(format!("Count: {}", count.get())))
    ///         .child(Button::new("Increment")
    ///             .on_click(move || count.update(|n| n + 1)))
    /// }
    /// ```
    ///
    /// # BuildContext Parameter
    ///
    /// The `ctx: &BuildContext` parameter provides:
    /// - **Hooks API**: `use_signal(ctx, ...)`, `use_memo(ctx, ...)`, `use_effect(ctx, ...)`
    /// - **Tree queries** (rarely needed): `ctx.parent()`, `ctx.size()`
    /// - **Inherited data**: `ctx.depend_on::<Theme>()` (Provider pattern)
    ///
    /// # Performance & Optimization
    ///
    /// The framework automatically handles rebuild optimization:
    /// - Compares views by type ID at runtime
    /// - Only rebuilds when parent marks child as dirty
    /// - No manual `rebuild()` method needed
    ///
    /// For custom optimization, implement `PartialEq`:
    /// ```rust,ignore
    /// #[derive(Clone, PartialEq)]  // ← Enables value-based comparison
    /// struct MyView {
    ///     text: String,
    /// }
    /// // Framework skips rebuild if new view == old view
    /// ```
    ///
    /// # Lifecycle
    ///
    /// Views are **immutable** and **ephemeral**:
    /// - Created fresh every rebuild
    /// - Consumed by `build()` (takes `self` by value)
    /// - Framework manages Element lifecycle (mount, unmount, dirty tracking)
    ///
    /// # Thread-Local BuildContext
    ///
    /// BuildContext is accessed via thread-local storage for ergonomics.
    /// The framework sets up the context before calling `build()` using RAII guards.
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

/// Sealed trait marker - prevents external implementation
///
/// This module contains a sealed trait pattern to prevent users from
/// implementing ViewElement outside of flui-core.
pub(crate) mod sealed {
    /// Sealed trait - only types in flui-core can implement this
    pub trait Sealed {}

    // Implementations for flui-core types
    impl Sealed for crate::element::ComponentElement {}
    impl Sealed for crate::element::RenderElement {}
    impl Sealed for crate::element::ProviderElement {}
    impl Sealed for crate::element::Element {}
}

/// ViewElement trait - bridge between View and Element
///
/// **Internal trait** - This trait is used internally by the framework
/// to bridge between Views and Elements. Users should NOT implement this trait.
///
/// The trait is sealed - you cannot implement it outside of flui-core.
pub trait ViewElement: sealed::Sealed + 'static {
    /// Convert this typed element into the Element enum
    fn into_element(self: Box<Self>) -> Element;

    /// Mark this element as needing rebuild
    fn mark_dirty(&mut self);

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as Any mut for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
