//! Core Widget trait - the foundation of the widget system
//!
//! This module defines the Widget trait that all widgets must implement.
//! Widgets are immutable descriptions of part of the user interface.

use std::any::Any;
use std::fmt;

use downcast_rs::{impl_downcast, Downcast, DowncastSync};
use dyn_clone::DynClone;
use flui_foundation::Key;

use crate::context::BuildContext;
use crate::element::{ComponentElement, Element};

pub mod provider;

// Re-export for backward compatibility
pub use provider::{InheritedElement, InheritedWidget};
/// Widget - immutable description of part of the UI
///
/// Similar to Flutter's Widget. Widgets are immutable and lightweight.
/// They describe what the UI should look like, but don't contain mutable state.
///
/// # Three Types of Widgets
///
/// 1. **StatelessWidget** - builds once, no mutable state
/// 2. **StatefulWidget** - creates a State object that persists
/// 3. **RenderObjectWidget** - directly controls layout and painting
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyWidget {
///     title: String,
/// }
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
///         // Build child widgets
///         Box::new(Text::new(self.title.clone()))
///     }
/// }
/// ```
pub trait Widget: DynClone + Downcast + fmt::Debug + Send + Sync {
    /// Create the Element that manages this widget's lifecycle
    ///
    /// This is called when the widget is first inserted into the tree.
    /// The element persists across rebuilds, while the widget is recreated.
    fn create_element(&self) -> Box<dyn Element>;

    /// Optional key for widget identification
    ///
    /// Keys are used to preserve state when widgets move in the tree.
    /// Without keys, widgets are matched by type and position only.
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Type name for debugging
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Check if this widget can be updated with another widget
    ///
    /// By default, widgets can update if they have the same type and key.
    fn can_update(&self, other: &dyn Widget) -> bool {
        // Same type required
        if self.type_id() != other.type_id() {
            return false;
        }

        // Check keys
        match (self.key(), other.key()) {
            (Some(k1), Some(k2)) => k1.id() == k2.id(),
            (None, None) => true,
            _ => false,
        }
    }
}

// Enable cloning for boxed Widget trait objects
dyn_clone::clone_trait_object!(Widget);

// Enable downcasting for Widget trait objects
impl_downcast!(Widget);

/// Helper trait for converting types into Widget trait objects
pub trait IntoWidget {
    /// Convert this type into a boxed Widget trait object
    fn into_widget(self) -> Box<dyn Widget>;
}

impl<T: Widget + 'static> IntoWidget for T {
    fn into_widget(self) -> Box<dyn Widget> {
        Box::new(self)
    }
}

/// StatelessWidget - immutable widget that builds once
///
/// Similar to Flutter's StatelessWidget. Build method creates child widget tree.
/// Stateless widgets don't hold any mutable state - all configuration comes from
/// their fields which are immutable.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessWidget for Greeting {
///     fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
///         Box::new(Text::new(format!("Hello, {}!", self.name)))
///     }
/// }
/// ```
pub trait StatelessWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Build this widget's child widget tree
    ///
    /// Called when the widget is first built or when it needs to rebuild.
    /// Should return the root widget of the child tree.
    fn build(&self, context: &BuildContext) -> Box<dyn Widget>;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn Key> {
        None
    }
}

/// Automatically implement Widget for all StatelessWidgets
impl<T: StatelessWidget> Widget for T {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(ComponentElement::new(self.clone()))
    }

    fn key(&self) -> Option<&dyn Key> {
        StatelessWidget::key(self)
    }
}

/// StatefulWidget - widget with mutable state
///
/// Similar to Flutter's StatefulWidget. Creates a State object that persists across rebuilds.
/// The widget itself is immutable, but the State can be mutated.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Counter {
///     initial_value: i32,
/// }
///
/// impl StatefulWidget for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState {
///             count: self.initial_value,
///         }
///     }
/// }
///
/// #[derive(Debug)]
/// struct CounterState {
///     count: i32,
/// }
///
/// impl State for CounterState {
///     fn build(&mut self, _context: &BuildContext) -> Box<dyn Widget> {
///         Box::new(Text::new(format!("Count: {}", self.count)))
///     }
/// }
/// ```
pub trait StatefulWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Associated State type
    type State: State;

    /// Create the state object
    ///
    /// Called once when the element is first mounted.
    fn create_state(&self) -> Self::State;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn Key> {
        None
    }
}

// TODO: Cannot have blanket impl for both StatelessWidget and StatefulWidget
// because they conflict. Need to use macros or manual implementation.
// For now, users must manually implement Widget for StatefulWidget types.
//
// Example:
// impl Widget for MyStatefulWidget {
//     fn create_element(&self) -> Box<dyn Element> {
//         Box::new(StatefulElement::new(self.clone()))
//     }
// }

/// State - mutable state for StatefulWidget
///
/// Similar to Flutter's State. Holds mutable state and builds widget tree.
/// The state object persists across rebuilds, while the widget is recreated.
///
/// The trait provides downcasting capabilities via the `downcast-rs` crate.
///
/// # Lifecycle
///
/// 1. **init_state()** - Called once when state is created
/// 2. **build()** - Called to build the widget tree
/// 3. **did_update_widget()** - Called when widget configuration changes
/// 4. **dispose()** - Called when state is removed from tree
pub trait State: DowncastSync + fmt::Debug {
    /// Build the widget tree
    ///
    /// Called whenever the state needs to rebuild. Should return the root widget
    /// of the child tree.
    fn build(&mut self, context: &BuildContext) -> Box<dyn Widget>;

    /// Called when state is first created
    ///
    /// Use this for initialization that depends on being in the tree.
    fn init_state(&mut self) {}

    /// Called when widget configuration changes
    ///
    /// The old widget is passed for comparison. Use this to detect changes
    /// and update internal state if needed.
    fn did_update_widget(&mut self, _old_widget: &dyn Any) {}

    /// Called when removed from tree
    ///
    /// Use this for cleanup like canceling timers, unsubscribing from streams, etc.
    fn dispose(&mut self) {}

    /// Request rebuild (like setState in Flutter)
    ///
    /// Marks the element as dirty so it will rebuild on the next frame.
    ///
    /// Note: This method is deprecated. Use `BuildContext::mark_needs_build()` instead.
    /// State objects should call `context.mark_dirty()` after modifying state.
    #[deprecated(note = "Use BuildContext::mark_needs_build() instead")]
    fn mark_needs_build(&mut self) {
        // No-op: Users should call context.mark_dirty() directly
    }
}

// Enable downcasting for State trait objects
impl_downcast!(sync State);

#[cfg(test)]
mod tests {
    use super::*;

    // Test widget for testing
    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
            // Return self for testing purposes
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_widget_type_name() {
        let widget = TestWidget { value: 42 };
        assert!(widget.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_widget_can_update_same_type() {
        let widget1 = TestWidget { value: 1 };
        let widget2 = TestWidget { value: 2 };

        assert!(widget1.can_update(&widget2));
    }

    #[test]
    fn test_widget_downcast() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn Widget> = Box::new(widget);

        // Test downcast_ref
        assert!(boxed.is::<TestWidget>());
        let downcasted = boxed.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(downcasted.value, 42);
    }

    #[test]
    fn test_into_widget() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn Widget> = widget.into_widget();

        assert!(boxed.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_create_element() {
        let widget = TestWidget { value: 42 };
        let element = widget.create_element();

        // Element should be created successfully
        assert!(element.is_dirty());
    }

    // Test StatefulWidget
    #[derive(Debug, Clone)]
    struct CounterWidget {
        initial: i32,
    }

    #[derive(Debug)]
    struct CounterState {
        count: i32,
    }

    impl StatefulWidget for CounterWidget {
        type State = CounterState;

        fn create_state(&self) -> Self::State {
            CounterState {
                count: self.initial,
            }
        }
    }

    impl State for CounterState {
        fn build(&mut self, _context: &BuildContext) -> Box<dyn Widget> {
            Box::new(TestWidget { value: self.count })
        }
    }

    #[test]
    fn test_stateful_widget_create_state() {
        let widget = CounterWidget { initial: 10 };
        let state = widget.create_state();

        assert_eq!(state.count, 10);
    }

    #[test]
    fn test_state_build() {
        let mut state = CounterState { count: 5 };
        let context = BuildContext::empty();

        let child = state.build(&context);
        // Should create a widget
        assert!(child.is::<TestWidget>());
    }

    #[test]
    fn test_widget_clone_box() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn Widget> = Box::new(widget);

        // Clone the boxed trait object using dyn-clone
        let cloned = dyn_clone::clone_box(&*boxed);

        // Both should be TestWidget with same value
        assert!(cloned.is::<TestWidget>());
        let cloned_test = cloned.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(cloned_test.value, 42);
    }

    #[test]
    fn test_widget_vec_clone() {
        let widgets: Vec<Box<dyn Widget>> = vec![
            Box::new(TestWidget { value: 1 }),
            Box::new(TestWidget { value: 2 }),
            Box::new(TestWidget { value: 3 }),
        ];

        // Clone the entire vector of trait objects
        let cloned: Vec<Box<dyn Widget>> = widgets.iter().map(|w| dyn_clone::clone_box(&**w)).collect();

        assert_eq!(cloned.len(), 3);
        for (i, widget) in cloned.iter().enumerate() {
            let test_widget = widget.downcast_ref::<TestWidget>().unwrap();
            assert_eq!(test_widget.value, (i + 1) as i32);
        }
    }

    #[test]
    fn test_widget_downcast_mut() {
        let widget = TestWidget { value: 10 };
        let mut boxed: Box<dyn Widget> = Box::new(widget);

        // Test downcast_mut
        if let Some(downcasted) = boxed.downcast_mut::<TestWidget>() {
            downcasted.value = 20;
        }

        let result = boxed.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(result.value, 20);
    }

    #[test]
    fn test_widget_downcast_owned() {
        let widget = TestWidget { value: 100 };
        let boxed: Box<dyn Widget> = Box::new(widget);

        // Test downcast (owned)
        let downcasted: Box<TestWidget> = boxed.downcast::<TestWidget>().ok().unwrap();
        assert_eq!(downcasted.value, 100);
    }

    #[test]
    fn test_state_downcast() {
        let state = CounterState { count: 42 };
        let boxed: Box<dyn State> = Box::new(state);

        // Test is() check
        assert!(boxed.is::<CounterState>());

        // Test downcast_ref
        let downcasted = boxed.downcast_ref::<CounterState>().unwrap();
        assert_eq!(downcasted.count, 42);
    }

    #[test]
    fn test_state_downcast_mut() {
        let state = CounterState { count: 10 };
        let mut boxed: Box<dyn State> = Box::new(state);

        // Test downcast_mut
        let downcasted = boxed.downcast_mut::<CounterState>().unwrap();
        downcasted.count = 20;

        let result = boxed.downcast_ref::<CounterState>().unwrap();
        assert_eq!(result.count, 20);
    }

    #[test]
    fn test_state_downcast_owned() {
        let state = CounterState { count: 100 };
        let boxed: Box<dyn State> = Box::new(state);

        // Consume and downcast
        let owned: Box<CounterState> = boxed.downcast().ok().unwrap();
        assert_eq!(owned.count, 100);
    }
}

