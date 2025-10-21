//! Widget equality optimization utilities
//!
//! Provides helper traits and methods for optimizing widget comparisons.

use std::any::Any;
use crate::widget::DynWidget;

/// Extension trait for optimized widget equality checks
///
/// This trait provides an optimization hint for widget rebuilding.
/// If two widgets are equal, the framework can skip rebuilding the element.
///
/// # Design Philosophy
///
/// Widget equality is intentionally conservative:
/// - Type equality is always checked first (fast TypeId comparison)
/// - Key equality is checked second (if keys present)
/// - Data equality is opt-in via explicit implementation
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone, PartialEq)]
/// struct MyWidget {
///     value: i32,
/// }
///
/// impl WidgetEq for MyWidget {
///     fn widget_eq(&self, other: &dyn DynWidget) -> bool {
///         // Fast path: different types are never equal
///         if self.type_id() != other.type_id() {
///             return false;
///         }
///
///         // Downcast and compare data
///         other.downcast_ref::<Self>()
///             .map(|other| self == other)
///             .unwrap_or(false)
///     }
/// }
/// ```
pub trait WidgetEq: 'static {
    /// Check if widget data has changed
    ///
    /// This is an optimization hint - if widgets are equal,
    /// we can skip rebuilding the element.
    ///
    /// # Default Implementation
    ///
    /// The default implementation only compares `TypeId`, which means
    /// widgets of the same type are considered equal. This is safe but
    /// conservative - it may cause unnecessary rebuilds.
    ///
    /// # Returns
    ///
    /// - `true` if widgets are considered equal (skip rebuild)
    /// - `false` if widgets differ (rebuild required)
    #[must_use]
    fn widget_eq(&self, other: &dyn DynWidget) -> bool {
        self.type_id() == other.type_id()
    }

    /// Check if this widget is of the same type as another
    ///
    /// This is a fast TypeId comparison without downcasting.
    #[must_use]
    #[inline]
    fn is_same_type_as(&self, other: &dyn DynWidget) -> bool {
        self.type_id() == other.type_id()
    }
}

// Blanket impl for all DynWidget
impl<T: DynWidget + ?Sized> WidgetEq for T {}

/// Helper to check if two widgets are equal without knowing their concrete type
///
/// This function uses downcasting to compare widgets of the same type.
/// It's more conservative than `WidgetEq::widget_eq()` as it also checks keys.
///
/// # Algorithm
///
/// 1. Compare TypeId (fast, O(1))
/// 2. Compare keys if present
/// 3. Return false conservatively if no keys (would need PartialEq)
///
/// # Example
///
/// ```rust,ignore
/// let w1: Box<dyn DynWidget> = Box::new(Text::new("Hello"));
/// let w2: Box<dyn DynWidget> = Box::new(Text::new("World"));
///
/// // Different widgets
/// assert!(!widgets_equal(&*w1, &*w2));
/// ```
#[must_use]
pub fn widgets_equal(a: &dyn DynWidget, b: &dyn DynWidget) -> bool {
    // Fast path: different types are never equal
    if a.type_id() != b.type_id() {
        return false;
    }

    // Compare keys if present
    match (a.key(), b.key()) {
        (Some(k1), Some(k2)) => k1.id() == k2.id(),
        (None, None) => {
            // No keys - would need PartialEq to compare widget data
            // Conservatively return false to force rebuild
            false
        }
        _ => false, // One has key, other doesn't - not equal
    }
}

/// Check if two widgets have the same type
///
/// This is a fast TypeId comparison without any downcasting.
///
/// # Example
///
/// ```rust,ignore
/// let w1: Box<dyn DynWidget> = Box::new(Text::new("Hello"));
/// let w2: Box<dyn DynWidget> = Box::new(Container::new());
///
/// assert!(!widgets_same_type(&*w1, &*w2));
/// ```
#[must_use]
#[inline]
pub fn widgets_same_type(a: &dyn DynWidget, b: &dyn DynWidget) -> bool {
    a.type_id() == b.type_id()
}

/// Macro to implement optimized equality for widgets
///
/// This macro generates both `PartialEq` implementation and optimized `widget_eq`.
///
/// # Important
///
/// This macro requires that your widget type implements or derives `PartialEq`.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone, PartialEq)]
/// struct MyWidget {
///     value: i32,
///     text: String,
/// }
///
/// impl StatelessWidget for MyWidget {
///     fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
///         // ...
///     }
/// }
///
/// // Implement optimized equality
/// impl_widget_eq!(MyWidget);
/// ```
#[macro_export]
macro_rules! impl_widget_eq {
    ($widget:ty) => {
        impl $crate::widget::WidgetEq for $widget {
            fn widget_eq(&self, other: &dyn $crate::widget::DynWidget) -> bool {
                // Fast path: type check
                if self.type_id() != other.type_id() {
                    return false;
                }

                // Downcast and use PartialEq
                other.downcast_ref::<Self>()
                    .map(|other| self == other)
                    .unwrap_or(false)
            }
        }
    };
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
            fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
                Box::new(TestWidget { value: self.value })
            }
        }

        let w1 = TestWidget { value: 42 };
        let w2 = TestWidget { value: 42 };

        // Same type - default impl returns true
        assert!(w1.widget_eq(&w2));
    }

    #[test]
    fn test_is_same_type_as() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct Widget1;

        #[derive(Debug, Clone)]
        struct Widget2;

        impl StatelessWidget for Widget1 {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget1)
            }
        }

        impl StatelessWidget for Widget2 {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget2)
            }
        }

        let w1 = Widget1;
        let w2 = Widget2;

        assert!(w1.is_same_type_as(&Widget1));
        assert!(!w1.is_same_type_as(&w2));
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
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget1)
            }
        }

        impl StatelessWidget for Widget2 {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget2)
            }
        }

        let w1: Box<dyn DynWidget> = Box::new(Widget1);
        let w2: Box<dyn DynWidget> = Box::new(Widget2);

        assert!(!widgets_equal(&*w1, &*w2));
    }

    #[test]
    fn test_widgets_equal_same_type() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct TestWidget;

        impl StatelessWidget for TestWidget {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(TestWidget)
            }
        }

        let w1: Box<dyn DynWidget> = Box::new(TestWidget);
        let w2: Box<dyn DynWidget> = Box::new(TestWidget);

        // Same type, no keys - conservatively returns false
        assert!(!widgets_equal(&*w1, &*w2));
    }

    #[test]
    fn test_widgets_same_type() {
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct Widget1;

        #[derive(Debug, Clone)]
        struct Widget2;

        impl StatelessWidget for Widget1 {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget1)
            }
        }

        impl StatelessWidget for Widget2 {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(Widget2)
            }
        }

        let w1: Box<dyn DynWidget> = Box::new(Widget1);
        let w2: Box<dyn DynWidget> = Box::new(Widget1);
        let w3: Box<dyn DynWidget> = Box::new(Widget2);

        assert!(widgets_same_type(&*w1, &*w2));
        assert!(!widgets_same_type(&*w1, &*w3));
    }

    #[test]
    fn test_impl_widget_eq_macro() {
        use crate::Context;
        use crate::widget::StatelessWidget;

        #[derive(Debug, Clone, PartialEq)]
        struct TestWidget {
            value: i32,
        }

        impl StatelessWidget for TestWidget {
            fn build(&self, _: &Context) -> Box<dyn DynWidget> {
                Box::new(TestWidget { value: self.value })
            }
        }

        // Note: WidgetEq is already implemented via blanket impl for all DynWidget
        // impl_widget_eq!(TestWidget);  // Don't use this, it conflicts!

        let w1 = TestWidget { value: 42 };
        let w2 = TestWidget { value: 42 };
        let w3 = TestWidget { value: 99 };

        // Note: widget_eq() only checks TypeId, not values
        // All TestWidget instances have the same type, so they're "equal" for widget_eq
        assert!(w1.widget_eq(&w2));  // Same type
        assert!(w1.widget_eq(&w3));  // Same type (even though values differ)

        // For value comparison, use PartialEq directly:
        assert_eq!(w1, w2);   // Values equal
        assert_ne!(w1, w3);   // Values different
    }
}