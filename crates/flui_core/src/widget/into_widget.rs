//! Helper trait for converting types into Widget trait objects
//!
//! This module provides the IntoWidget trait that simplifies creating
//! boxed Widget trait objects from concrete widget types.

use crate::{AnyWidget, Widget};

/// Helper trait for converting types into Widget trait objects
///
/// This trait provides a convenient way to convert concrete widget types
/// into boxed trait objects (`Box<dyn AnyWidget>`).
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for all types that implement `Widget`.
/// You don't need to implement it manually.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{IntoWidget, Widget};
///
/// #[derive(Debug, Clone)]
/// struct MyWidget {
///     value: i32,
/// }
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
///         // ...
///     }
/// }
///
/// // Convert to trait object using into_widget()
/// let widget: Box<dyn AnyWidget> = MyWidget { value: 42 }.into_widget();
/// ```
pub trait IntoWidget {
    /// Convert this type into a boxed Widget trait object
    fn into_widget(self) -> Box<dyn AnyWidget>;
}

impl<T: Widget + 'static> IntoWidget for T {
    fn into_widget(self) -> Box<dyn AnyWidget> {
        Box::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnyWidget, Context, StatelessWidget};

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_into_widget() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn AnyWidget> = widget.into_widget();

        assert!(boxed.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_into_widget_chain() {
        let boxed = TestWidget { value: 100 }.into_widget();
        assert!(boxed.is::<TestWidget>());
    }
}
