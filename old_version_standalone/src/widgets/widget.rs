//! Core Widget trait - the foundation of the widget system
//!
//! This module defines the Widget trait that all widgets must implement.
//! Widgets are immutable descriptions of part of the user interface.

use std::any::Any;
use std::fmt;
use crate::core::Key;
use super::framework::{Element, BuildContext};

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
/// impl Widget for MyWidget {
///     fn create_element(&self) -> Box<dyn Element> {
///         Box::new(ComponentElement::new(self.clone()))
///     }
/// }
/// ```
pub trait Widget: Any + fmt::Debug {
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

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Check if this widget can be updated with another widget
    ///
    /// By default, widgets can update if they have the same type and key.
    fn can_update(&self, other: &dyn Widget) -> bool {
        // Same type
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

/// RenderObjectWidget - widget that directly creates a RenderObject
///
/// Similar to Flutter's RenderObjectWidget. These widgets directly control
/// layout and painting by creating a RenderObject.
pub trait RenderObjectWidget: Widget {
    /// Type of RenderObject this widget creates
    type RenderObject: Any + fmt::Debug;

    /// Create the RenderObject for layout and painting
    fn create_render_object(&self, context: &BuildContext) -> Self::RenderObject;

    /// Update an existing RenderObject with new configuration
    fn update_render_object(&self, context: &BuildContext, render_object: &mut Self::RenderObject);
}

/// LeafRenderObjectWidget - RenderObjectWidget with no children
///
/// Examples: Text, Image, etc.
pub trait LeafRenderObjectWidget: RenderObjectWidget {}

/// SingleChildRenderObjectWidget - RenderObjectWidget with one child
///
/// Examples: Container, Padding, Align, etc.
pub trait SingleChildRenderObjectWidget: RenderObjectWidget {
    /// Get the child widget
    fn child(&self) -> Option<&dyn Widget>;
}

/// MultiChildRenderObjectWidget - RenderObjectWidget with multiple children
///
/// Examples: Row, Column, Stack, etc.
pub trait MultiChildRenderObjectWidget: RenderObjectWidget {
    /// Get all children
    fn children(&self) -> &[Box<dyn Widget>];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::framework::{ComponentElement, StatelessWidget};

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl TestWidget {
        fn new(value: i32) -> Self {
            Self { value }
        }
    }

    impl Widget for TestWidget {
        fn create_element(&self) -> Box<dyn Element> {
            Box::new(ComponentElement::new(self.clone()))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Any> {
            Box::new(format!("Value: {}", self.value))
        }
    }

    #[test]
    fn test_widget_type_name() {
        let widget = TestWidget::new(42);
        assert!(widget.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_widget_can_update_same_type() {
        let widget1 = TestWidget::new(1);
        let widget2 = TestWidget::new(2);

        assert!(widget1.can_update(&widget2));
    }

    #[test]
    fn test_widget_as_any() {
        let widget = TestWidget::new(42);
        let any: &dyn Any = widget.as_any();

        assert!(any.downcast_ref::<TestWidget>().is_some());
    }

    #[test]
    fn test_into_widget() {
        let widget = TestWidget::new(42);
        let boxed: Box<dyn Widget> = widget.into_widget();

        assert_eq!(boxed.type_name().contains("TestWidget"), true);
    }
}
