//! Widget system - immutable configuration for UI elements
//!
//! Based on idea.md Chapter 4 with extensions for StatelessWidget and StatefulWidget.
//!
//! # Widget Types
//!
//! 1. **StatelessWidget** - Builds once, no mutable state
//! 2. **StatefulWidget** - Creates a State object that persists across rebuilds
//! 3. **InheritedWidget** - Propagates data down the tree efficiently
//! 4. **ParentDataWidget** - Attaches metadata to descendant RenderObjects
//! 5. **RenderObjectWidget** - Directly controls layout and painting
//!
//! # Architecture
//!
//! ```text
//! Widget (immutable config)
//!   ├─ StatelessWidget → ComponentElement → build() → child widget
//!   ├─ StatefulWidget → StatefulElement → State → build() → child widget
//!   ├─ InheritedWidget → InheritedElement → (data propagation) → child widget
//!   ├─ ParentDataWidget → ParentDataElement → (attach data) → child widget
//!   └─ RenderObjectWidget → RenderObjectElement<W, A> → RenderObject (layout/paint)
//! ```
//!
//! # Widget Kind System
//!
//! Each widget type has an associated `Kind` marker type to enable multiple blanket
//! implementations without conflicts. See the `kind` module for details.
//!
//! # Module Structure
//!
//! - `kind` - WidgetKind types for type discrimination
//! - `dyn_widget` - Object-safe DynWidget trait for heterogeneous storage
//! - `stateless` - StatelessWidget trait for immutable widgets
//! - `stateful` - StatefulWidget + State traits for stateful widgets
//! - `inherited` - InheritedWidget trait for data propagation
//! - `proxy` - ProxyWidget trait for single-child wrapper widgets
//! - `parent_data_widget` - ParentDataWidget trait for attaching layout metadata
//! - `render_object_widget` - RenderObjectWidget trait for render widgets

// Submodules
pub mod dyn_widget;
pub mod inherited;
pub mod kind;
pub mod parent_data_widget;
pub mod proxy;
pub mod render_object_widget;
pub mod sealed;
pub mod sealed_v2;
pub mod stateful;
pub mod stateful_wrapper;
pub mod stateless;






// Re-exports for convenience
pub use dyn_widget::{DynWidget, BoxedWidget};
pub use kind::{WidgetKind, ComponentKind, StatefulKind, InheritedKind, ParentDataKind, RenderObjectKind};
pub use stateless::StatelessWidget;
pub use stateful::{StatefulWidget, State};
pub use stateful_wrapper::Stateful;  // Zero-cost wrapper for StatefulWidget
pub use inherited::InheritedWidget;
pub use proxy::ProxyWidget;
pub use parent_data_widget::ParentDataWidget;
pub use render_object_widget::RenderObjectWidget;

/// Base Widget trait
///
/// All widgets must be Clone (immutable) and Send + Sync for thread safety.
///
/// This trait is **sealed** - it cannot be implemented directly by downstream crates.
/// Instead, implement one of the higher-level widget traits:
///
/// - `StatelessWidget` for widgets without mutable state (gets Widget automatically)
/// - `StatefulWidget` for widgets with mutable state (use `Stateful(...)` wrapper)
/// - `InheritedWidget` for data propagation (gets Widget automatically)
/// - `ParentDataWidget` for attaching layout metadata (gets Widget automatically)
/// - `RenderObjectWidget` for widgets that directly control rendering (gets Widget automatically)
///
/// # The Sealed Trait Pattern
///
/// This trait extends `sealed::Sealed`, which prevents downstream crates from
/// implementing `Widget` directly. This is a safety feature that:
///
/// - Prevents blanket impl conflicts
/// - Ensures type safety
/// - Makes the library future-proof
///
/// # Automatic Implementations
///
/// Most widget types get automatic `Widget` implementation:
///
/// - **StatelessWidget**: Automatic blanket impl → `ComponentElement`
/// - **StatefulWidget** (via `Stateful` wrapper): Automatic impl → `StatefulElement`
/// - **InheritedWidget**: Automatic blanket impl → `InheritedElement`
/// - **ParentDataWidget**: Automatic blanket impl → `ParentDataElement`
/// - **RenderObjectWidget**: Automatic blanket impl → `RenderObjectElement`
///
/// # Example Usage
///
/// ```rust,ignore
/// use flui_core::{StatelessWidget, BoxedWidget};
///
/// // StatelessWidget gets Widget automatically
/// #[derive(Clone)]
/// struct MyWidget;
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self) -> BoxedWidget {
///         // ...
///     }
/// }
///
/// // ✅ Can use Widget methods automatically!
/// let element = my_widget.into_element();
/// ```
///
/// For StatefulWidget, use the `Stateful` wrapper:
///
/// ```rust,ignore
/// use flui_core::{StatefulWidget, State, Stateful};
///
/// #[derive(Clone)]
/// struct Counter {
///     initial: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     type State = CounterState;
///     fn create_state(&self) -> CounterState { /* ... */ }
/// }
///
/// // Wrap in Stateful to get Widget impl
/// let widget = Stateful(Counter { initial: 0 });
/// let element = widget.into_element(); // ✅ Works!
/// ```
pub trait Widget: sealed::Sealed + DynWidget + Clone + Sized {
    /// Optional key for widget identity
    ///
    /// Keys are used to preserve element state when the widget tree is rebuilt.
    /// If two widgets have different keys, they are considered different even if
    /// they are the same type.
    fn key(&self) -> Option<&str> {
        None
    }

    /// Create an element from this widget
    ///
    /// This method is automatically implemented for all widget types via
    /// the sealed trait. The concrete Element type is determined by
    /// `sealed::Sealed::ElementType`.
    fn into_element(self) -> <Self as sealed::Sealed>::ElementType;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RenderObject, LeafArity, LayoutCx, PaintCx};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    #[derive(Clone, Debug)]
    struct TestWidget {
        value: f32,
    }

    // Widget and DynWidget are automatically implemented via RenderObjectWidget!

    #[derive(Debug)]
    struct TestRender {
        value: f32,
    }

    impl RenderObject for TestRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::ZERO)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    impl RenderObjectWidget for TestWidget {
        type Arity = LeafArity;
        type Render = TestRender;

        fn create_render_object(&self) -> Self::Render {
            TestRender { value: self.value }
        }

        fn update_render_object(&self, render: &mut Self::Render) {
            render.value = self.value;
        }
    }

    #[test]
    fn test_render_object_widget() {
        let widget = TestWidget { value: 42.0 };
        let mut render = widget.create_render_object();

        assert_eq!(render.value, 42.0);

        let updated_widget = TestWidget { value: 100.0 };
        updated_widget.update_render_object(&mut render);

        assert_eq!(render.value, 100.0);
    }
}





