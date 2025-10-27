//! Core Widget trait - typed, zero-cost widget abstraction
//!
//! This is the primary trait that users implement for custom widgets.
//! It provides compile-time type safety and zero-cost abstractions.
//!
//! # Design Philosophy
//!
//! - **Typed**: Uses associated types for zero-cost abstractions
//! - **Not object-safe**: Allows associated types for better type safety
//! - **Minimal requirements**: Only `'static` bound, no Clone/Send/Sync
//! - **Automatic DynWidget**: Blanket impl provides object-safe version
//!
//! # Examples
//!
//! ```
//! use flui_core::{Widget, Key, Element};
//!
//! #[derive(Debug)]
//! struct Text {
//!     content: String,
//! }
//!
//! impl Widget for Text {
//!     type Element = TextElement;
//!     // State and Arity use defaults
//! }
//!
//! // DynWidget is automatic via blanket impl!
//! let widget: Box<dyn DynWidget> = Box::new(Text {
//!     content: "Hello".into()
//! });
//! ```

use crate::foundation::Key;

/// Core Widget trait - typed widget abstraction
///
/// This is the main trait that users implement for custom widgets.
/// It uses associated types for compile-time type safety and zero-cost abstractions.
///
/// # Key Features
///
/// - **Zero-cost**: Associated types compile to direct calls
/// - **Type-safe**: Element type is compile-time checked
/// - **Flexible**: No forced Clone/Send/Sync requirements
/// - **Automatic DynWidget**: Blanket impl provides dynamic dispatch
///
/// # Associated Types
///
/// ## Element
///
/// The type of element this widget creates. Elements are the mutable
/// state holders in the widget tree.
///
/// Common element types:
/// - `ComponentElement<Self>` for StatelessWidget
/// - `StatefulElement<Self>` for StatefulWidget
/// - `InheritedElement<Self>` for InheritedWidget
/// - `RenderObjectElement<Self>` for RenderObjectWidget
///
/// ## State (default = ())
///
/// The type of state this widget holds. For stateless widgets, this
/// defaults to `()`. For stateful widgets, override this with your
/// state type.
///
/// ## Arity (default = LeafArity)
///
/// The number of children this widget has. Used for compile-time
/// validation and optimization.
///
/// - `LeafArity` - No children (default)
/// - `SingleArity` - Exactly one child
/// - `MultiArity` - Multiple children
///
/// # Implementation Guidelines
///
/// **Don't implement Widget directly!** Instead, implement one of:
///
/// - `StatelessWidget` for simple widgets
/// - `StatefulWidget` for widgets with mutable state
/// - `InheritedWidget` for data propagation
/// - `RenderObjectWidget` for render widgets
/// - `ParentDataWidget` for layout metadata
///
/// These traits automatically implement `Widget` for you.
///
/// # Examples
///
/// ## Simple Widget
///
/// ```
/// use flui_core::{Widget, Key};
///
/// #[derive(Debug)]
/// struct MyWidget {
///     data: String,
/// }
///
/// impl Widget for MyWidget {
///     type Element = MyElement;
///     // State = (), Arity = LeafArity (defaults)
/// }
/// ```
///
/// ## Widget with Key
///
/// ```
/// use flui_core::{Widget, Key};
///
/// const MY_KEY: Key = Key::from_str("my_widget");
///
/// impl Widget for MyWidget {
///     type Element = MyElement;
///
///     fn key(&self) -> Option<Key> {
///         Some(MY_KEY)
///     }
/// }
/// ```
///
/// ## Stateful Widget
///
/// ```
/// use flui_core::{Widget, WidgetState};
///
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: i32,
/// }
///
/// impl Widget for Counter {
///     type Element = StatefulElement<Counter>;
///     type State = CounterState;  // Override default
/// }
/// ```
///
/// ## Widget with Children
///
/// ```
/// use flui_core::{Widget, MultiArity};
///
/// struct Column {
///     children: Vec<BoxedWidget>,
/// }
///
/// impl Widget for Column {
///     type Element = RenderObjectElement<Column>;
///     type Arity = MultiArity;  // Override default
/// }
/// ```
pub trait Widget: 'static {
    // Note: type Element and type Arity removed during enum Element migration
    // Element creation is now handled by DynWidget trait
    // Arity is now part of RenderObject trait only

    /// Optional widget key for identity tracking
    ///
    /// Keys are used to preserve element state when widgets are reordered
    /// or when you need to uniquely identify a widget instance.
    ///
    /// # When to Use Keys
    ///
    /// - List items that can be reordered
    /// - Widgets that need stable identity across rebuilds
    /// - Widgets with state that should persist
    ///
    /// # Key Types
    ///
    /// 1. **Compile-time constant** - `Key::from_str("name")`
    /// 2. **Runtime unique** - `Key::new()`
    /// 3. **Explicit ID** - `Key::from_u64(id)`
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::{Widget, Key};
    ///
    /// // Compile-time constant key
    /// const HEADER_KEY: Key = Key::from_str("app_header");
    ///
    /// impl Widget for Header {
    ///     type Element = HeaderElement;
    ///
    ///     fn key(&self) -> Option<Key> {
    ///         Some(HEADER_KEY)
    ///     }
    /// }
    ///
    /// // Runtime unique key
    /// struct ListItem {
    ///     key: Key,
    ///     data: ItemData,
    /// }
    ///
    /// impl Widget for ListItem {
    ///     type Element = ListItemElement;
    ///
    ///     fn key(&self) -> Option<Key> {
    ///         Some(self.key)
    ///     }
    /// }
    ///
    /// // Explicit key from data
    /// struct UserWidget {
    ///     user_id: u64,
    /// }
    ///
    /// impl Widget for UserWidget {
    ///     type Element = UserElement;
    ///
    ///     fn key(&self) -> Option<Key> {
    ///         Key::from_u64(self.user_id)
    ///     }
    /// }
    /// ```
    fn key(&self) -> Option<Key> {
        None
    }
}

/// Widget state trait
///
/// This trait is implemented by state objects for stateful widgets.
/// For stateless widgets, `()` implements this trait.
pub trait WidgetState<W: Widget>: 'static {
    /// Initialize state from widget
    fn init(widget: &W) -> Self;

    /// Called when widget configuration changes
    fn did_update_widget(&mut self, old_widget: &W, new_widget: &W) {
        let _ = (old_widget, new_widget);
    }

    /// Called when element is disposed
    fn dispose(&mut self) {}
}


#[cfg(test)]
mod tests {
    use super::*;

    // Mock types for testing
    #[derive(Debug)]
    struct TestElement;

    impl<W: Widget> Element<W> for TestElement {
        fn new(_widget: W) -> Self {
            Self
        }
    }

    #[test]
    fn test_widget_with_defaults() {
        #[derive(Debug)]
        struct SimpleWidget;

        impl Widget for SimpleWidget {
            // Element type determined by DynWidget impl
        }

        let widget = SimpleWidget;
        assert!(widget.key().is_none());

        // Check that defaults are used (compile-time check)
        let _: () = <SimpleWidget as Widget>::State::init(&widget);
    }

    #[test]
    fn test_widget_with_key() {
        const TEST_KEY: Key = Key::from_str("test");

        #[derive(Debug)]
        struct KeyedWidget;

        impl Widget for KeyedWidget {
            fn key(&self) -> Option<Key> {
                Some(TEST_KEY)
            }
        }

        let widget = KeyedWidget;
        assert_eq!(widget.key(), Some(TEST_KEY));
    }

    #[test]
    fn test_widget_without_clone() {
        #[derive(Debug)]
        struct NonCloneWidget {
            data: Vec<u8>,
        }

        impl Widget for NonCloneWidget {
            // Element type determined by DynWidget impl
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
        };

        assert!(widget.key().is_none());
        // Widget works without Clone!
    }

    #[test]
    fn test_widget_state() {
        #[derive(Debug)]
        struct StatefulWidget {
            initial: i32,
        }

        struct MyState {
            count: i32,
        }

        impl WidgetState<StatefulWidget> for MyState {
            fn init(widget: &StatefulWidget) -> Self {
                Self {
                    count: widget.initial,
                }
            }

            fn did_update_widget(
                &mut self,
                old: &StatefulWidget,
                new: &StatefulWidget,
            ) {
                if old.initial != new.initial {
                    self.count = new.initial;
                }
            }
        }

        impl Widget for StatefulWidget {
            // Element type determined by DynWidget impl
        }

        let widget = StatefulWidget { initial: 42 };
        let state = MyState::init(&widget);
        assert_eq!(state.count, 42);
    }
}