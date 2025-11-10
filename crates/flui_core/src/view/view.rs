//! Core abstraction for reactive UI components.
//!
//! Provides a simplified, unified API for building UI components while
//! maintaining the three-tree architecture.

use super::build_context::BuildContext;
use super::IntoElement;
use crate::element::Element;
use std::any::Any;

/// Core abstraction for building reactive UI components.
///
/// Represents a lightweight, immutable description of UI state. Views are
/// configuration objects that the framework converts into Elements (mutable state)
/// which manage RenderObjects (layout/paint).
///
/// Similar to Widget in Flutter, Component in React, or View in SwiftUI.
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
/// # Design (v0.6.0+)
///
/// The View API design:
///
/// - No GAT State: Use hooks (`use_signal`, `use_memo`) instead
/// - No GAT Element: Return `impl IntoElement`
/// - No rebuild(): Framework handles diffing automatically
/// - Clone Required: Enables type erasure with `AnyView`
/// - 'static Required: Enables safe storage and lifecycle management
/// - Composable: Views compose via `IntoElement` trait
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
    /// Builds this view into an element.
    ///
    /// Describes the UI structure based on the current view configuration.
    ///
    /// # Return Types
    ///
    /// Returns any type implementing `IntoElement`:
    ///
    /// 1. Other Views (composition):
    ///    ```rust,ignore
    ///    fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///        Column::new()
    ///            .child(Text::new(self.title))
    ///            .child(Text::new(self.body))
    ///    }
    ///    ```
    ///
    /// 2. Tuple of (RenderObject, children):
    ///    ```rust,ignore
    ///    fn build(self, ctx: &BuildContext) -> impl IntoElement {
    ///        // Leaf
    ///        (RenderText::new(self.text), ())
    ///
    ///        // Single child
    ///        (RenderPadding::new(self.padding), self.child)
    ///
    ///        // Multiple children
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
    /// # BuildContext
    ///
    /// Provides access to:
    /// - Hooks API: `use_signal`, `use_memo`, `use_effect`
    /// - Tree queries: `ctx.parent()`, `ctx.size()`
    /// - Inherited data: `ctx.depend_on::<Theme>()`
    ///
    /// # Performance
    ///
    /// The framework handles rebuild optimization automatically:
    /// - Compares views by type ID at runtime
    /// - Only rebuilds when parent marks child as dirty
    /// - No manual `rebuild()` method required
    ///
    /// Implement `PartialEq` for value-based optimization:
    ///
    /// ```rust,ignore
    /// #[derive(Clone, PartialEq)]
    /// struct MyView {
    ///     text: String,
    /// }
    /// // Framework skips rebuild if new view == old view
    /// ```
    ///
    /// # Lifecycle
    ///
    /// Views are immutable and ephemeral:
    /// - Created fresh on each rebuild
    /// - Consumed by `build()` (takes ownership)
    /// - Framework manages Element lifecycle
    ///
    /// BuildContext is accessed via thread-local storage. The framework
    /// sets up the context before calling `build()` using RAII guards.
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

/// Sealed trait marker.
///
/// Prevents external implementation of ViewElement outside flui-core.
pub(crate) mod sealed {
    /// Sealed trait. Only types in flui-core can implement this.
    pub trait Sealed {}

    // Implementations for flui-core types
    impl Sealed for crate::element::ComponentElement {}
    impl Sealed for crate::element::RenderElement {}
    impl Sealed for crate::element::ProviderElement {}
    impl Sealed for crate::element::Element {}
}

/// Internal bridge between View and Element.
///
/// This trait is sealed and cannot be implemented outside flui-core.
/// Used internally by the framework to convert Views into Elements.
pub trait ViewElement: sealed::Sealed + 'static {
    /// Converts this typed element into the Element enum.
    fn into_element(self: Box<Self>) -> Element;

    /// Marks this element as needing rebuild.
    fn mark_dirty(&mut self);

    /// Returns reference as Any for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns mutable reference as Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
