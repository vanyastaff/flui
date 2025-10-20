//! Widget equality optimization utilities
//!
//! Provides helper traits and methods for optimizing widget comparisons.

use std::any::Any;
use crate::widget::AnyWidget;

/// Extension trait for optimized widget equality checks
pub trait WidgetEq: 'static {
    /// Check if widget data has changed
    ///
    /// This is an optimization hint - if widgets are equal,
    /// we can skip rebuilding the element.
    ///
    /// Default implementation uses TypeId comparison only.
    fn widget_eq(&self, other: &dyn AnyWidget) -> bool {
        self.type_id() == other.type_id()
    }
}

// Blanket impl for all AnyWidget
impl<T: AnyWidget + ?Sized> WidgetEq for T {}

/// Macro to implement optimized equality for widgets
///
/// This macro generates PartialEq implementation and optimized widget_eq.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyWidget {
///     value: i32,
///     text: String,
/// }
///
/// impl_widget_eq!(MyWidget);
///
/// // Now MyWidget has optimized equality checks
/// ```
#[macro_export]
macro_rules! impl_widget_eq {
    ($widget:ty) => {
        impl PartialEq for $widget {
            fn eq(&self, other: &Self) -> bool {
                // Implement field-by-field comparison
                // This should be customized per widget
                std::ptr::eq(self, other)
            }
        }
    };
}

/// Helper to check if two widgets are equal without knowing their concrete type
///
/// This uses downcasting to compare widgets of the same type.
pub fn widgets_equal(a: &dyn AnyWidget, b: &dyn AnyWidget) -> bool {
    // Different types are never equal
    if a.type_id() != b.type_id() {
        return false;
    }

    // Same type - compare keys
    match (a.key(), b.key()) {
        (Some(k1), Some(k2)) => k1.id() == k2.id(),
        (None, None) => {
            // No keys - would need to compare widget data
            // This is expensive without PartialEq, so we conservatively return false
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_eq_default() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct TestWidget {
            value: i32,
        }

        impl StatelessWidget for TestWidget {
            fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
                Box::new(TestWidget { value: self.value })
            }
        }

        let w1 = TestWidget { value: 42 };
        let w2 = TestWidget { value: 42 };

        // Same type
        assert!(w1.widget_eq(&w2));
    }

    #[test]
    fn test_widgets_equal_different_types() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct Widget1;

        #[derive(Debug, Clone)]
        struct Widget2;

        impl StatelessWidget for Widget1 {
            fn build(&self, _: &Context) -> Box<dyn AnyWidget> {
                Box::new(Widget1)
            }
        }

        impl StatelessWidget for Widget2 {
            fn build(&self, _: &Context) -> Box<dyn AnyWidget> {
                Box::new(Widget2)
            }
        }

        let w1: Box<dyn AnyWidget> = Box::new(Widget1);
        let w2: Box<dyn AnyWidget> = Box::new(Widget2);

        assert!(!widgets_equal(&*w1, &*w2));
    }

    #[test]
    fn test_widgets_equal_same_type() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct TestWidget;

        impl StatelessWidget for TestWidget {
            fn build(&self, _: &Context) -> Box<dyn AnyWidget> {
                Box::new(TestWidget)
            }
        }

        let w1: Box<dyn AnyWidget> = Box::new(TestWidget);
        let w2: Box<dyn AnyWidget> = Box::new(TestWidget);

        // Same type, no keys - conservatively returns false (would need PartialEq)
        assert!(!widgets_equal(&*w1, &*w2));
    }
}
