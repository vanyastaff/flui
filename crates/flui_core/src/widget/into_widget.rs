//! Helper trait for converting types into Widget trait objects
//!
//! This module provides the IntoWidget trait that simplifies creating
//! boxed Widget trait objects from concrete widget types.

use crate::{DynWidget, Widget};

/// Helper trait for converting types into Widget trait objects
///
/// This trait provides a convenient way to convert concrete widget types
/// into boxed trait objects (`Box<dyn DynWidget>`).
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for all types that implement `Widget`.
/// You don't need to implement it manually.
///
/// # Naming Convention
///
/// The method name `into_widget()` follows Rust API Guidelines (C-CONV):
/// - `into_*` for consuming conversions (takes ownership)
/// - Returns `Box<dyn DynWidget>` (heap allocation)
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
///     fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
///         // ...
///     }
/// }
///
/// // Convert to trait object using into_widget()
/// let widget: Box<dyn DynWidget> = MyWidget { value: 42 }.into_widget();
/// ```
///
/// # Performance Note
///
/// This method performs a heap allocation. For zero-cost operations with
/// concrete types, use `Widget::into_element()` directly instead of going
/// through `Box<dyn DynWidget>`.
pub trait IntoWidget {
    /// Convert this type into a boxed Widget trait object
    ///
    /// This is a consuming conversion that takes ownership of `self`
    /// and returns a heap-allocated trait object.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let widget = MyWidget { value: 42 };
    /// let boxed: Box<dyn DynWidget> = widget.into_widget();
    /// ```
    #[must_use]
    fn into_widget(self) -> Box<dyn DynWidget>;
}

/// Blanket implementation for all Widget types
///
/// This automatically implements `IntoWidget` for any type that implements `Widget`.
impl<T: Widget + 'static> IntoWidget for T {
    #[inline]
    fn into_widget(self) -> Box<dyn DynWidget> {
        Box::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynWidget, Context, StatelessWidget};

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_into_widget() {
        let widget = TestWidget { value: 42 };
        let boxed: Box<dyn DynWidget> = widget.into_widget();

        assert!(boxed.type_name().contains("TestWidget"));
    }

    #[test]
    fn test_into_widget_chain() {
        let boxed = TestWidget { value: 100 }.into_widget();
        assert!(boxed.is::<TestWidget>());
    }

    #[test]
    fn test_into_widget_downcast() {
        let widget = TestWidget { value: 42 };
        let boxed = widget.into_widget();

        let downcasted = boxed.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(downcasted.value, 42);
    }

    #[test]
    fn test_into_widget_moves_ownership() {
        let widget = TestWidget { value: 42 };
        let _boxed = widget.into_widget();

        // widget is moved, can't use it anymore
        // let _ = widget.value; // This would be a compile error
    }

    #[test]
    fn test_into_widget_collection() {
        let widgets: Vec<Box<dyn DynWidget>> = vec![
            TestWidget { value: 1 }.into_widget(),
            TestWidget { value: 2 }.into_widget(),
            TestWidget { value: 3 }.into_widget(),
        ];

        assert_eq!(widgets.len(), 3);

        for (i, widget) in widgets.iter().enumerate() {
            let test_widget = widget.downcast_ref::<TestWidget>().unwrap();
            assert_eq!(test_widget.value, (i + 1) as i32);
        }
    }
}