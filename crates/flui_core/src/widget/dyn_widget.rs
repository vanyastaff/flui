//! DynWidget - Object-safe trait for heterogeneous widget storage
//!
//! This module provides the `DynWidget` trait, enabling storing different
//! widget types in heterogeneous collections like `Vec<Box<dyn DynWidget>>`.
//!
//! # Design Pattern: Two-Trait Approach
//!
//! FLUI uses a two-level approach for widgets (similar to RenderObject):
//!
//! 1. **Widget** (typed trait) - Zero-cost concrete usage with associated types
//! 2. **DynWidget** (this trait) - Object-safe for `Box<dyn DynWidget>` storage
//!
//! This allows:
//! - **Compile-time safety** when working with concrete Widget types
//! - **Runtime flexibility** for heterogeneous widget collections
//! - **Zero-cost abstractions** where types are known statically
//! - **Dynamic dispatch** only when necessary (e.g., widget trees)
//!
//! # Why DynWidget?
//!
//! The `Widget` trait has associated types and methods, which make it not object-safe.
//! You cannot create `Box<dyn Widget>` or store different Widget types together.
//!
//! `DynWidget` solves this by being object-safe - it doesn't have associated types.
//!
//! # Usage Pattern
//!
//! ```rust,ignore
//! // Concrete types use Widget (zero-cost)
//! #[derive(Clone)]
//! struct MyWidget { text: String }
//!
//! impl Widget for MyWidget {
//!     fn key(&self) -> Option<&str> { None }
//! }
//!
//! impl StatelessWidget for MyWidget {
//!     fn build(&self) -> Box<dyn DynWidget> {
//!         Box::new(Text::new(&self.text))
//!     }
//! }
//!
//! // Heterogeneous storage via DynWidget
//! let widgets: Vec<Box<dyn DynWidget>> = vec![
//!     Box::new(MyWidget { text: "Hello".into() }),
//!     Box::new(Text::new("World")),
//! ];
//! ```

use std::any::Any;
use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};
use dyn_clone::DynClone;

/// Object-safe base trait for all widgets
///
/// This trait is automatically implemented for all types that implement `Widget`.
/// It provides the minimal object-safe interface needed for heterogeneous widget storage.
///
/// # Design Principles
///
/// 1. **Object Safety**: No associated types, no generic methods
/// 2. **Minimal Interface**: Only methods needed for widget tree operations
/// 3. **Downcast Support**: Can convert back to concrete types via `downcast_rs`
///
/// # When to Use Each Trait
///
/// - Use `Widget` when working with concrete types
/// - Use `DynWidget` when storing in heterogeneous collections
/// - Use `downcast_ref/mut` to convert from `DynWidget` back to concrete type
///
/// # Cloning
///
/// This trait extends `DynClone`, which allows `Box<dyn DynWidget>` to be cloned.
/// All widget types must implement `Clone` to satisfy this requirement.
pub trait DynWidget: DynClone + DowncastSync + fmt::Debug + Send + Sync + 'static {
    /// Get the widget's key for identity tracking
    ///
    /// Keys are used to preserve widget state across rebuilds when
    /// widgets are reordered or when you need to uniquely identify a widget.
    ///
    /// Returns `None` if no key is set.
    fn key(&self) -> Option<&str> {
        None
    }

    /// Get the type name for debugging
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Check if this widget can update another widget
    ///
    /// Two widgets can update each other if:
    /// - They have the same type
    /// - They have the same key (or both have no key)
    ///
    /// This is used during rebuild to determine if an element can be reused.
    fn can_update(&self, other: &dyn DynWidget) -> bool {
        // Same type required
        if self.type_id() != other.type_id() {
            return false
        }

        // Check keys
        match (self.key(), other.key()) {
            (Some(k1), Some(k2)) => k1 == k2,
            (None, None) => true,
            _ => false,
        }
    }

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as Any (mutable) for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Enable downcasting for DynWidget trait objects
impl_downcast!(sync DynWidget);

// Enable cloning for DynWidget trait objects
dyn_clone::clone_trait_object!(DynWidget);

/// Boxed Widget trait object
///
/// Commonly used for heterogeneous collections of widgets.
///
/// # Cloning
///
/// `BoxedWidget` can be cloned thanks to `DynClone` trait:
///
/// ```rust,ignore
/// use flui_core::BoxedWidget;
///
/// let widget: BoxedWidget = Box::new(Text::new("Hello"));
/// let cloned = widget.clone(); // Works!
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::BoxedWidget;
///
/// let widgets: Vec<BoxedWidget> = vec![
///     Box::new(Text::new("Hello")),
///     Box::new(Container::new()),
/// ];
///
/// // Can clone the entire vec since BoxedWidget implements Clone
/// let widgets_copy = widgets.clone();
/// ```
pub type BoxedWidget = Box<dyn DynWidget>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Widget;

    #[derive(Debug, Clone)]
    struct TestWidget {
        key: Option<String>,
        value: i32,
    }

    impl Widget for TestWidget {
        type Kind = crate::widget::StatelessKind;

        fn key(&self) -> Option<&str> {
            self.key.as_deref()
        }
    }

    impl crate::widget::StatelessWidget for TestWidget {
        fn build(&self) -> crate::BoxedWidget {
            Box::new(TestWidget {
                key: None,
                value: 0,
            })
        }
    }

    impl DynWidget for TestWidget {
        fn key(&self) -> Option<&str> {
            self.key.as_deref()
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_dyn_widget_downcast() {
        let widget: Box<dyn DynWidget> = Box::new(TestWidget {
            key: None,
            value: 42,
        });

        assert!(widget.downcast_ref::<TestWidget>().is_some());
        assert_eq!(widget.downcast_ref::<TestWidget>().unwrap().value, 42);
    }

    #[test]
    fn test_can_update_same_type_no_key() {
        let w1 = TestWidget { key: None, value: 1 };
        let w2 = TestWidget { key: None, value: 2 };

        assert!(w1.can_update(&w2));
    }

    #[test]
    fn test_can_update_same_key() {
        let w1 = TestWidget { key: Some("key1".into()), value: 1 };
        let w2 = TestWidget { key: Some("key1".into()), value: 2 };

        assert!(w1.can_update(&w2));
    }

    #[test]
    fn test_cannot_update_different_key() {
        let w1 = TestWidget { key: Some("key1".into()), value: 1 };
        let w2 = TestWidget { key: Some("key2".into()), value: 2 };

        assert!(!w1.can_update(&w2));
    }

    #[test]
    fn test_cannot_update_key_mismatch() {
        let w1 = TestWidget { key: Some("key1".into()), value: 1 };
        let w2 = TestWidget { key: None, value: 2 };

        assert!(!w1.can_update(&w2));
    }

    #[test]
    fn test_boxed_widget_clone() {
        // Create a boxed widget
        let widget: BoxedWidget = Box::new(TestWidget {
            key: Some("test".into()),
            value: 42,
        });

        // Clone it!
        let cloned = widget.clone();

        // Both should have the same value
        assert_eq!(
            widget.downcast_ref::<TestWidget>().unwrap().value,
            42
        );
        assert_eq!(
            cloned.downcast_ref::<TestWidget>().unwrap().value,
            42
        );

        // They should be different instances
        assert_ne!(
            widget.as_ref() as *const dyn DynWidget,
            cloned.as_ref() as *const dyn DynWidget
        );
    }

    #[test]
    fn test_vec_boxed_widget_clone() {
        // Create a vec of boxed widgets
        let widgets: Vec<BoxedWidget> = vec![
            Box::new(TestWidget { key: None, value: 1 }),
            Box::new(TestWidget { key: Some("key2".into()), value: 2 }),
            Box::new(TestWidget { key: None, value: 3 }),
        ];

        // Clone the entire vec!
        let cloned_widgets = widgets.clone();

        // Should have same length
        assert_eq!(widgets.len(), cloned_widgets.len());

        // All values should match
        for (original, cloned) in widgets.iter().zip(cloned_widgets.iter()) {
            assert_eq!(
                original.downcast_ref::<TestWidget>().unwrap().value,
                cloned.downcast_ref::<TestWidget>().unwrap().value
            );
        }
    }
}
