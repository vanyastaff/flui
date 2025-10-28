//! StatelessWidget - pure functional widgets
//!
//! StatelessWidget is the simplest widget type in FLUI. It's a pure function
//! from configuration to UI - no mutable state, just build logic.
//!
//! # When to Use
//!
//! - Widget has no mutable state
//! - Widget is a pure function of its configuration
//! - Widget rebuilds from scratch on each update
//!
//! # Examples
//!
//! ```
//! use flui_core::{StatelessWidget, BoxedWidget, BuildContext};
//!
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessWidget for Greeting {
//!     fn build(&self, context: &BuildContext) -> BoxedWidget {
//!         Box::new(Text::new(format!("Hello, {}!", self.name)))
//!     }
//! }
//!
//! // Widget and DynWidget are automatic!
//! ```

use std::fmt;
use crate::{Widget, DynWidget, BoxedWidget, BuildContext};

/// StatelessWidget - pure functional widget
///
/// This is the most common widget type. It takes configuration and
/// produces a widget tree, with no mutable state.
///
/// # Architecture
///
/// ```text
/// StatelessWidget (immutable config)
///   ↓
/// build() → Widget tree
///   ↓
/// ComponentElement<Self> (holds widget, rebuilds on update)
/// ```
///
/// # Lifecycle
///
/// ```text
/// 1. Widget created: Greeting { name: "Alice" }
/// 2. build() called → Text widget
/// 3. Configuration changes: Greeting { name: "Bob" }
/// 4. build() called again → New Text widget
/// ```
///
/// # Performance
///
/// - **No state overhead** - No state object allocated
/// - **Fast rebuilds** - Just call build() again
/// - **Cache friendly** - Small memory footprint
///
/// # Implementation Rules
///
/// - `build()` must be **pure** - same inputs = same output
/// - `build()` should be **fast** - called on every rebuild
/// - `build()` can access **BuildContext** for inherited widgets
///
/// # Examples
///
/// ## Simple Widget
///
/// ```
/// #[derive(Debug)]
/// struct HelloWorld;
///
/// impl StatelessWidget for HelloWorld {
///     fn build(&self, _context: &BuildContext) -> BoxedWidget {
///         Box::new(Text::new("Hello, World!"))
///     }
/// }
/// ```
///
/// ## Widget with Configuration
///
/// ```
/// #[derive(Debug)]
/// struct UserCard {
///     name: String,
///     age: u32,
///     avatar_url: String,
/// }
///
/// impl StatelessWidget for UserCard {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         Box::new(Column::new(vec![
///             Box::new(Image::network(&self.avatar_url)),
///             Box::new(Text::new(&self.name)),
///             Box::new(Text::new(format!("Age: {}", self.age))),
///         ]))
///     }
/// }
/// ```
///
/// ## Widget with Theme (InheritedWidget)
///
/// ```
/// #[derive(Debug)]
/// struct ThemedButton {
///     label: String,
/// }
///
/// impl StatelessWidget for ThemedButton {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         // Access inherited widget
///         let theme = Theme::of(context);
///
///         Box::new(Container::new()
///             .color(theme.colors.primary)
///             .child(Box::new(Text::new(&self.label))))
///     }
/// }
/// ```
///
/// ## Composition
///
/// ```
/// #[derive(Debug)]
/// struct LoginForm;
///
/// impl StatelessWidget for LoginForm {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         Box::new(Column::new(vec![
///             Box::new(TextField::new("Username")),
///             Box::new(TextField::new("Password")),
///             Box::new(Row::new(vec![
///                 Box::new(TextButton::new("Cancel")),
///                 Box::new(ElevatedButton::new("Login")),
///             ])),
///         ]))
///     }
/// }
/// ```
///
/// ## Conditional Rendering
///
/// ```
/// #[derive(Debug)]
/// struct ConditionalWidget {
///     show_content: bool,
/// }
///
/// impl StatelessWidget for ConditionalWidget {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         if self.show_content {
///             Box::new(Text::new("Content visible"))
///         } else {
///             Box::new(Text::new("Content hidden"))
///         }
///     }
/// }
/// ```
///
/// ## List Mapping
///
/// ```
/// #[derive(Debug)]
/// struct ItemList {
///     items: Vec<String>,
/// }
///
/// impl StatelessWidget for ItemList {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         let children: Vec<BoxedWidget> = self.items
///             .iter()
///             .map(|item| Box::new(ListTile::new(item)) as BoxedWidget)
///             .collect();
///
///         Box::new(ListView::new(children))
///     }
/// }
/// ```
pub trait StatelessWidget: Clone + fmt::Debug + Send + Sync + 'static {
    /// Build the widget tree
    ///
    /// This method is called whenever the widget needs to rebuild.
    /// It should return a new widget tree based on the current configuration.
    ///
    /// # Parameters
    ///
    /// - `context` - BuildContext for accessing inherited widgets
    ///
    /// # Returns
    ///
    /// A boxed widget tree representing the UI
    ///
    /// # Performance
    ///
    /// This method should be fast - it's called on every rebuild.
    /// Avoid expensive operations like:
    /// - Network requests
    /// - Heavy computations
    /// - Large allocations
    ///
    /// If you need expensive initialization, use StatefulWidget instead.
    ///
    /// # Purity
    ///
    /// This method should be pure - same inputs should produce same output.
    /// Don't:
    /// - Modify external state
    /// - Use random numbers (without seed)
    /// - Use current time (unless rebuilding on time change)
    ///
    /// # Examples
    ///
    /// ```
    /// impl StatelessWidget for MyWidget {
    ///     fn build(&self, context: &BuildContext) -> BoxedWidget {
    ///         // Access configuration
    ///         let title = &self.title;
    ///
    ///         // Access inherited widgets
    ///         let theme = Theme::of(context);
    ///
    ///         // Build UI
    ///         Box::new(Container::new()
    ///             .color(theme.primary)
    ///             .child(Box::new(Text::new(title))))
    ///     }
    /// }
    /// ```
    fn build(&self, context: &BuildContext) -> BoxedWidget;
}

/// Automatic Widget implementation for StatelessWidget
///
/// All StatelessWidget types automatically get Widget trait,
/// which in turn automatically get DynWidget via blanket impl.
///
/// # Element Type
///
/// StatelessWidget uses `ComponentElement<Self>` which:
/// - Stores the widget instance
/// - Calls `build()` to get child widget
/// - Rebuilds when widget configuration changes
///
/// # State Type
///
/// Uses default `()` - no state needed for stateless widgets
///
/// # Arity
///
// Widget impl is now generated by #[derive(StatelessWidget)] macro
// This avoids blanket impl conflicts on stable Rust
//
// Use: #[derive(StatelessWidget)] on your widget type

// DynWidget comes automatically via blanket impl in mod.rs!

/// Helper function to create a stateless widget with a key
///
/// # Examples
///
/// ```
/// use flui_core::{StatelessWidget, with_key, Key};
///
/// #[derive(Debug)]
/// struct MyWidget;
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self, _context: &BuildContext) -> BoxedWidget {
///         Box::new(Text::new("Hello"))
///     }
/// }
///
/// // Create with key
/// const MY_KEY: Key = Key::from_str("my_widget");
/// let widget = with_key(MyWidget, MY_KEY);
/// ```
pub fn with_key<W: StatelessWidget>(widget: W, key: crate::Key) -> KeyedStatelessWidget<W> {
    KeyedStatelessWidget { widget, key }
}

/// Wrapper for StatelessWidget with a key
///
/// This is used by the `with_key()` helper function.
#[derive(Debug, Clone)]
pub struct KeyedStatelessWidget<W> {
    widget: W,
    key: crate::Key,
}

// Implement StatelessWidget trait
impl<W: StatelessWidget> StatelessWidget for KeyedStatelessWidget<W> {
    fn build(&self, context: &BuildContext) -> BoxedWidget {
        self.widget.build(context)
    }
}

// Widget comes from blanket impl for StatelessWidget

// Implement Widget::build() for all StatelessWidget types
impl<W> Widget for W
where
    W: StatelessWidget,
{
    // Override build() method to call StatelessWidget::build()
    fn build(&self, context: &BuildContext) -> Option<BoxedWidget> {
        Some(StatelessWidget::build(self, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    // Mock BuildContext for testing
    struct MockBuildContext;
    impl MockBuildContext {
        fn new() -> BuildContext<'static> {
            // In real code, this would be properly constructed
            unsafe { std::mem::zeroed() }
        }
    }

    #[test]
    fn test_simple_stateless_widget() {
        #[derive(Debug, Clone)]
        struct SimpleWidget;

        impl StatelessWidget for SimpleWidget {
            fn build(&self, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        let widget = SimpleWidget;

        // Widget is automatic
        let _: &dyn Widget = &widget;

        // DynWidget is automatic
        let _: &dyn crate::DynWidget = &widget;

        // No key by default
        assert!(widget.key().is_none());
    }

    #[test]
    fn test_stateless_widget_with_config() {
        #[derive(Debug, Clone)]
        struct ConfigWidget {
            value: i32,
            label: String,
        }

        impl StatelessWidget for ConfigWidget {
            fn build(&self, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        let widget = ConfigWidget {
            value: 42,
            label: "Test".into(),
        };

        // Configuration is accessible
        assert_eq!(widget.value, 42);
        assert_eq!(widget.label, "Test");

        // Can box as DynWidget
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.is::<ConfigWidget>());
    }

    #[test]
    fn test_keyed_stateless_widget() {
        #[derive(Debug, Clone)]
        struct TestWidget;

        impl StatelessWidget for TestWidget {
            fn build(&self, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        const TEST_KEY: Key = Key::from_str("test_widget");

        // Create with key
        let widget = with_key(TestWidget, TEST_KEY);

        // Key is present
        assert_eq!(widget.key(), Some(TEST_KEY));

        // Still works as DynWidget
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.key().is_some());
    }

    #[test]
    fn test_stateless_widget_without_clone() {
        // StatelessWidget requires Clone for Widget trait
        #[derive(Debug, Clone)]
        struct NonCloneWidget {
            data: Vec<u8>,
        }

        impl StatelessWidget for NonCloneWidget {
            fn build(&self, _context: &BuildContext) -> BoxedWidget {
                Box::new(MockWidget)
            }
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
        };

        // Can still box it
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.is::<NonCloneWidget>());
    }

    // Mock widget for testing
    #[derive(Debug)]
    struct MockWidget;

    impl Widget for MockWidget {
        // Element type determined by framework
    }

    #[derive(Debug)]
    struct MockElement;

    impl<W: Widget> crate::Element<W> for MockElement {
        fn new(_: W) -> Self {
            Self
        }
    }
}