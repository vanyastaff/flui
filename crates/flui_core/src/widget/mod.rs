//! Widget system - immutable configuration for UI elements
//!
//! This module provides the widget layer of FLUI's three-tree architecture:
//! - **Widget** → Immutable configuration (typed, zero-cost)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **RenderObject** → Layout and painting (optional, for render widgets)
//!
//! # Widget Types
//!
//! 1. **StatelessWidget** - Pure function from config to UI
//! 2. **StatefulWidget** - Creates a State object that persists
//! 3. **InheritedWidget** - Propagates data down the tree
//! 4. **RenderObjectWidget** - Direct control over layout/paint
//! 5. **ParentDataWidget** - Attaches metadata to descendants
//!
//! # Architecture
//!
//! ```text
//! Widget (typed)  ←→  DynWidget (object-safe)
//!   ↑                       ↑
//!   └───── Blanket impl ────┘
//!          (automatic)
//!
//! StatelessWidget  ─┐
//! StatefulWidget   ─┤
//! InheritedWidget  ─┼→ impl Widget → impl DynWidget (automatic!)
//! RenderObjectWidget┤
//! ParentDataWidget ─┘
//! ```
//!
//! # Design Pattern: Two-Trait Approach
//!
//! ## Widget (typed, not object-safe)
//!
//! - Has associated types (`Element`, `State`, `Arity`)
//! - Zero-cost abstractions at compile time
//! - Used when concrete type is known
//!
//! ## DynWidget (object-safe)
//!
//! - No associated types
//! - Enables `Box<dyn DynWidget>` for heterogeneous storage
//! - Used for widget trees with different types
//!
//! ## Blanket Implementation
//!
//! ```rust,ignore
//! impl<W> DynWidget for W
//! where
//!     W: Widget + fmt::Debug + 'static,
//!     W::Element: DynElement,
//! {
//!     // Automatic bridge between Widget and DynWidget
//! }
//! ```
//!
//! # Examples
//!
//! ## Simple Widget
//!
//! ```
//! use flui_core::{Widget, StatelessWidget, BoxedWidget};
//!
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessWidget for Greeting {
//!     fn build(&self) -> BoxedWidget {
//!         Box::new(Text::new(format!("Hello, {}!", self.name)))
//!     }
//! }
//!
//! // Widget and DynWidget are automatic!
//! let widget: BoxedWidget = Box::new(Greeting {
//!     name: "World".into()
//! });
//! ```
//!
//! ## Widget with Key
//!
//! ```
//! use flui_core::{Widget, Key};
//!
//! const HEADER_KEY: Key = Key::from_str("app_header");
//!
//! #[derive(Debug)]
//! struct Header;
//!
//! impl Widget for Header {
//!     type Element = HeaderElement;
//!
//!     fn key(&self) -> Option<Key> {
//!         Some(HEADER_KEY)
//!     }
//! }
//! ```

// Submodules
pub mod dyn_widget;
pub mod inherited;
pub mod notification_listener;
pub mod parent_data_widget;
pub mod render_object_widget;
pub mod stateful;
pub mod stateless;
pub mod widget;


// Re-exports
pub use widget::{Widget, WidgetState};
pub use dyn_widget::{DynWidget, BoxedWidget, SharedWidget};
pub use stateless::{StatelessWidget, KeyedStatelessWidget, with_key};
pub use stateful::{StatefulWidget, State};
pub use inherited::{InheritedWidget, InheritedModel};
pub use render_object_widget::{
    RenderObjectWidget,
    SingleChildRenderObjectWidget,
    MultiChildRenderObjectWidget,
};
pub use parent_data_widget::{ParentDataWidget, ParentData};
pub use notification_listener::NotificationListener;

use std::fmt;
use crate::KeyRef;

// ========== Blanket Implementation: Widget → DynWidget ==========

/// Blanket implementation: All Widget types automatically become DynWidget
///
/// This is the magic that connects the typed `Widget` trait with the
/// object-safe `DynWidget` trait. You implement `Widget`, and you get
/// `DynWidget` for free!
///
/// # Requirements
///
/// For a type to automatically get `DynWidget`, it must:
/// 1. Implement `Widget` (your custom widget logic)
/// 2. Implement `Debug` (for diagnostics)
/// 3. Be `'static` (no borrowed data)
/// 4. Have an `Element` that implements `DynElement`
///
/// # Zero-Cost Abstraction
///
/// This blanket impl has no runtime cost. When you use a widget
/// with its concrete type, the compiler generates direct calls.
/// Dynamic dispatch only happens when you explicitly use `dyn DynWidget`.
///
/// # Examples
///
/// ```
/// use flui_core::{Widget, DynWidget, BoxedWidget};
///
/// #[derive(Debug)]
/// struct MyWidget {
///     data: String,
/// }
///
/// impl Widget for MyWidget {
///     type Element = MyElement;
/// }
///
/// // DynWidget is automatic!
/// let widget: BoxedWidget = Box::new(MyWidget {
///     data: "test".into()
/// });
///
/// // Type-safe operations
/// assert!(widget.is::<MyWidget>());
///
/// // Downcast when needed
/// if let Some(my_widget) = widget.downcast_ref::<MyWidget>() {
///     println!("Data: {}", my_widget.data);
/// }
/// ```
impl<W> DynWidget for W
where
    W: Widget + fmt::Debug + 'static,
{
    #[inline]
    fn key(&self) -> Option<KeyRef> {
        // Convert Widget::key (Key) to DynWidget::key (KeyRef)
        Widget::key(self).map(KeyRef::from)
    }

    // All other methods (type_id, can_update, type_name, as_any)
    // use the default implementations from DynWidget trait
}

// ========== Helper Functions ==========

/// Create a boxed widget from any Widget type
///
/// This is a convenience function for boxing widgets.
///
/// # Examples
///
/// ```
/// use flui_core::{boxed, Text};
///
/// let widget = boxed(Text::new("Hello"));
/// // Same as: Box::new(Text::new("Hello"))
/// ```
#[inline]
pub fn boxed<W: DynWidget + 'static>(widget: W) -> BoxedWidget {
    Box::new(widget)
}

/// Create a shared widget from any Widget type
///
/// This is a convenience function for Arc-wrapping widgets.
///
/// # Examples
///
/// ```
/// use flui_core::{shared, Text};
///
/// let widget = shared(Text::new("Shared"));
/// let clone = widget.clone(); // Cheap Arc clone
/// ```
#[inline]
pub fn shared<W: DynWidget + 'static>(widget: W) -> SharedWidget {
    std::sync::Arc::new(widget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, Element};

    // Mock element for testing
    #[derive(Debug)]
    struct MockElement;

    impl<W: Widget> Element<W> for MockElement {
        fn new(_widget: W) -> Self {
            Self
        }
    }

    impl DynElement for MockElement {
        // Mock implementation
    }

    #[test]
    fn test_blanket_impl() {
        #[derive(Debug)]
        struct TestWidget {
            value: i32,
        }

        impl Widget for TestWidget {
            // Element type determined by framework
        }

        let widget = TestWidget { value: 42 };

        // Widget automatically implements DynWidget
        let _: &dyn DynWidget = &widget;

        // Can box as DynWidget
        let boxed: BoxedWidget = Box::new(widget);
        assert!(boxed.is::<TestWidget>());
    }

    #[test]
    fn test_key_conversion() {
        #[derive(Debug)]
        struct KeyedWidget {
            key: Key,
        }

        impl Widget for KeyedWidget {
            // Element type determined by framework

            fn key(&self) -> Option<Key> {
                Some(self.key)
            }
        }

        let key = Key::new();
        let widget = KeyedWidget { key };

        // Widget::key returns Key
        assert_eq!(Widget::key(&widget), Some(key));

        // DynWidget::key returns KeyRef (automatic conversion)
        assert_eq!(DynWidget::key(&widget), Some(KeyRef::from(key)));
    }

    #[test]
    fn test_boxed_helper() {
        #[derive(Debug)]
        struct SimpleWidget;

        impl Widget for SimpleWidget {
            // Element type determined by framework
        }

        let widget = boxed(SimpleWidget);
        assert!(widget.is::<SimpleWidget>());
    }

    #[test]
    fn test_shared_helper() {
        #[derive(Debug)]
        struct SharedTestWidget {
            data: String,
        }

        impl Widget for SharedTestWidget {
            // Element type determined by framework
        }

        let widget = shared(SharedTestWidget {
            data: "test".into(),
        });

        let clone1 = widget.clone();
        let clone2 = widget.clone();

        // All share the same Arc
        assert!(std::sync::Arc::ptr_eq(&widget, &clone1));
        assert!(std::sync::Arc::ptr_eq(&widget, &clone2));
    }

    #[test]
    fn test_widget_without_clone() {
        // Important: Widget doesn't require Clone!
        #[derive(Debug)]
        struct NonCloneWidget {
            data: Vec<u8>,
        }

        impl Widget for NonCloneWidget {
            // Element type determined by framework
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
        };

        // Can still box it
        let boxed: BoxedWidget = Box::new(widget);
        assert!(boxed.is::<NonCloneWidget>());
    }

    #[test]
    fn test_heterogeneous_storage() {
        #[derive(Debug)]
        struct WidgetA;

        #[derive(Debug)]
        struct WidgetB;

        impl Widget for WidgetA {
            // Element type determined by framework
        }

        impl Widget for WidgetB {
            // Element type determined by framework
        }

        // Different widget types in same vec!
        let widgets: Vec<BoxedWidget> = vec![
            Box::new(WidgetA),
            Box::new(WidgetB),
            Box::new(WidgetA),
        ];

        assert_eq!(widgets.len(), 3);
        assert!(widgets[0].is::<WidgetA>());
        assert!(widgets[1].is::<WidgetB>());
        assert!(widgets[2].is::<WidgetA>());
    }
}