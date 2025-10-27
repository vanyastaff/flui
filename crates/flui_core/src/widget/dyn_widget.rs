//! DynWidget - Object-safe trait for dynamic widget storage
//!
//! This module provides the `DynWidget` trait, enabling heterogeneous
//! widget storage via `Box<dyn DynWidget>`.
//!
//! # Design Pattern
//!
//! FLUI uses a two-level approach for widgets:
//!
//! 1. **Widget** (typed) - Zero-cost with associated types
//! 2. **DynWidget** (this) - Object-safe for dynamic dispatch
//!
//! A blanket implementation connects them automatically:
//! ```text
//! impl<W: Widget> DynWidget for W { }
//! ```
//!
//! # Usage
//!
//! **Don't implement this trait directly!** Implement `Widget` instead.
//! You get `DynWidget` automatically via blanket impl.
//!
//! # Examples
//!
//! ```
//! use flui_core::{Widget, DynWidget, BoxedWidget};
//!
//! #[derive(Debug)]
//! struct Text {
//!     content: String,
//! }
//!
//! impl Widget for Text {
//!     type Element = TextElement;
//! }
//!
//! // DynWidget is automatic!
//! let widget: BoxedWidget = Box::new(Text {
//!     content: "Hello".into()
//! });
//!
//! // Downcast when needed
//! if let Some(text) = widget.downcast_ref::<Text>() {
//!     println!("Text: {}", text.content);
//! }
//! ```

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::Arc;

use crate::KeyRef;

/// Object-safe trait for dynamic widget storage
///
/// This trait enables storing different widget types in heterogeneous
/// collections like `Vec<Box<dyn DynWidget>>`.
///
/// # Do NOT Implement Directly!
///
/// Users should implement `Widget` instead. This trait is automatically
/// implemented via blanket impl:
///
/// ```ignore
/// impl<W: Widget + fmt::Debug> DynWidget for W { }
/// ```
///
/// # Why Separate from Widget?
///
/// - `Widget` has associated types (not object-safe)
/// - `DynWidget` is object-safe (can use `Box<dyn DynWidget>`)
/// - Blanket impl connects them automatically
///
/// # Object Safety
///
/// This trait is carefully designed to be object-safe:
/// - No associated types
/// - No generic methods
/// - No `Self: Sized` bounds
///
/// # Performance
///
/// - Type checks: ~1ns (TypeId comparison)
/// - Key comparisons: ~1ns (u64 comparison)
/// - Downcast: ~5ns (vtable lookup + check)
///
/// # Examples
///
/// ## Heterogeneous Storage
///
/// ```
/// use flui_core::{BoxedWidget, DynWidget};
///
/// let widgets: Vec<BoxedWidget> = vec![
///     Box::new(Text::new("Hello")),
///     Box::new(Container::new()),
///     Box::new(Button::new("Click")),
/// ];
///
/// for widget in &widgets {
///     println!("Type: {}", widget.type_name());
/// }
/// ```
///
/// ## Type Checking
///
/// ```
/// let w1 = Text::new("A");
/// let w2 = Text::new("B");
/// let w3 = Container::new();
///
/// assert!(w1.can_update(&w2));  // Same type
/// assert!(!w1.can_update(&w3)); // Different type
/// ```
///
/// ## Downcasting
///
/// ```
/// let widget: BoxedWidget = Box::new(Text::new("Hello"));
///
/// // Safe downcast
/// if let Some(text) = widget.downcast_ref::<Text>() {
///     println!("Content: {}", text.content);
/// }
/// ```
pub trait DynWidget: fmt::Debug + Any + 'static {
    /// Get widget key for identity tracking
    ///
    /// Keys are used to preserve element state across rebuilds.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::{DynWidget, KeyRef};
    ///
    /// let widget = Text::new("Hello").with_key(MY_KEY);
    /// if let Some(key) = widget.key() {
    ///     println!("Key: {}", key);
    /// }
    /// ```
    fn key(&self) -> Option<KeyRef> {
        None
    }

    /// Get TypeId for fast type comparisons
    ///
    /// This is used internally for `can_update()` checks.
    /// Much faster than string comparisons (~1ns vs ~30ns).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::any::TypeId;
    ///
    /// let w1 = Text::new("A");
    /// let w2 = Text::new("B");
    ///
    /// assert_eq!(w1.type_id(), w2.type_id());
    /// assert_eq!(w1.type_id(), TypeId::of::<Text>());
    /// ```
    #[inline]
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Check if this widget can update another widget
    ///
    /// Two widgets are compatible for update if:
    /// 1. They have the same TypeId (same concrete type)
    /// 2. They have the same key (or both have no key)
    ///
    /// # Performance
    ///
    /// - Same type, no keys: ~1ns
    /// - Same type, same keys: ~2ns
    /// - Different types: ~1ns (early return)
    ///
    /// # Examples
    ///
    /// ```
    /// // Same type, no keys → can update
    /// let w1 = Text::new("A");
    /// let w2 = Text::new("B");
    /// assert!(w1.can_update(&w2));
    ///
    /// // Same type, same key → can update
    /// let k = Key::new();
    /// let w3 = Text::new("C").with_key(k);
    /// let w4 = Text::new("D").with_key(k);
    /// assert!(w3.can_update(&w4));
    ///
    /// // Same type, different keys → cannot update
    /// let w5 = Text::new("E").with_key(Key::new());
    /// let w6 = Text::new("F").with_key(Key::new());
    /// assert!(!w5.can_update(&w6));
    ///
    /// // Different types → cannot update
    /// let w7 = Text::new("G");
    /// let w8 = Container::new();
    /// assert!(!w7.can_update(&w8));
    /// ```
    #[inline]
    fn can_update(&self, other: &dyn DynWidget) -> bool {
        DynWidget::type_id(self) == DynWidget::type_id(other) && self.key() == other.key()
    }

    /// Get type name for debugging
    ///
    /// Returns the full type name including module path.
    /// This is for diagnostics only - do not use for logic!
    ///
    /// # Performance
    ///
    /// This is a compile-time constant, zero runtime cost.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget = Text::new("Hello");
    /// assert!(widget.type_name().contains("Text"));
    /// ```
    #[inline]
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Get as Any for downcasting
    ///
    /// This enables safe downcasting to concrete types.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget: &dyn DynWidget = &Text::new("Hello");
    ///
    /// if let Some(text) = widget.as_any().downcast_ref::<Text>() {
    ///     println!("Text content: {}", text.content);
    /// }
    /// ```
    #[inline]
    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    /// Debug identifier for diagnostics
    ///
    /// Returns a human-readable identifier combining type name and key.
    /// Useful for debugging and error messages.
    ///
    /// # Examples
    ///
    /// ```
    /// let w1 = Text::new("Hello");
    /// println!("{}", w1.debug_id());  // "Text"
    ///
    /// let w2 = Text::new("World").with_key(Key::from_str("greeting"));
    /// println!("{}", w2.debug_id());  // "Text(greeting)"
    /// ```
    fn debug_id(&self) -> String {
        if let Some(key) = self.key() {
            format!("{}({})", self.type_name(), key.as_u64())
        } else {
            self.type_name().to_string()
        }
    }

    /// Build the widget tree (for StatelessWidget and StatefulWidget only)
    ///
    /// This method is called by ComponentElement and StatefulElement to build
    /// the child widget tree. It's only implemented by widgets that have a build phase.
    ///
    /// RenderObjectWidgets return `None` since they don't build - they create render objects.
    ///
    /// # Arguments
    ///
    /// - `context`: BuildContext providing access to inherited widgets and tree structure
    ///
    /// # Returns
    ///
    /// - `Some(BoxedWidget)` for StatelessWidget/StatefulWidget with the built child tree
    /// - `None` for RenderObjectWidget (not applicable)
    ///
    /// # Default Implementation
    ///
    /// The default implementation returns `None`. StatelessWidget and StatefulWidget
    /// override this to call their `build()` method.
    fn build(&self, _context: &crate::element::BuildContext) -> Option<crate::BoxedWidget> {
        None
    }
}

/// Extension methods for dyn DynWidget
///
/// These methods are available on trait objects `&dyn DynWidget`.
impl dyn DynWidget {
    /// Attempt to downcast to concrete type
    ///
    /// Returns `Some(&T)` if the widget is of type `T`, `None` otherwise.
    ///
    /// # Safety
    ///
    /// This is safe - it uses Rust's `Any` trait for type checking.
    ///
    /// # Performance
    ///
    /// ~5ns for the type check and vtable lookup.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget: Box<dyn DynWidget> = Box::new(Text::new("Hello"));
    ///
    /// // Successful downcast
    /// if let Some(text) = widget.downcast_ref::<Text>() {
    ///     println!("Content: {}", text.content);
    /// }
    ///
    /// // Failed downcast
    /// assert!(widget.downcast_ref::<Container>().is_none());
    /// ```
    #[inline]
    pub fn downcast_ref<T: DynWidget>(&self) -> Option<&T> {
        // Cast &dyn DynWidget to &dyn Any, then downcast
        // This works because DynWidget: Any
        (self as &dyn Any).downcast_ref::<T>()
    }

    /// Check if widget is of specific type
    ///
    /// Equivalent to `widget.type_id() == TypeId::of::<T>()` but
    /// more convenient.
    ///
    /// # Examples
    ///
    /// ```
    /// let widget: &dyn DynWidget = &Text::new("Hello");
    ///
    /// assert!(widget.is::<Text>());
    /// assert!(!widget.is::<Container>());
    /// ```
    #[inline]
    pub fn is<T: DynWidget>(&self) -> bool {
        DynWidget::type_id(self) == TypeId::of::<T>()
    }
}

/// Boxed widget trait object
///
/// Commonly used for heterogeneous collections of widgets.
///
/// # Examples
///
/// ```
/// use flui_core::BoxedWidget;
///
/// let widgets: Vec<BoxedWidget> = vec![
///     Box::new(Text::new("Hello")),
///     Box::new(Container::new()),
/// ];
/// ```
pub type BoxedWidget = Box<dyn DynWidget>;

/// Shared widget trait object (reference-counted)
///
/// Use this when you need to share a widget between multiple locations.
/// Cloning a `SharedWidget` only clones the Arc pointer, not the widget.
///
/// # Examples
///
/// ```
/// use flui_core::SharedWidget;
/// use std::sync::Arc;
///
/// let widget: SharedWidget = Arc::new(Text::new("Shared"));
///
/// // Cheap clone - only Arc pointer
/// let clone1 = widget.clone();
/// let clone2 = widget.clone();
///
/// assert!(Arc::ptr_eq(&widget, &clone1));
/// ```
pub type SharedWidget = Arc<dyn DynWidget>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Widget, Key};

    // Mock widget for testing
    #[derive(Debug)]
    struct TestWidget {
        key: Option<Key>,
        value: i32,
    }

    impl Widget for TestWidget {
        // Element type determined at runtime by framework

        fn key(&self) -> Option<Key> {
            self.key
        }
    }

    // DynWidget is automatic via blanket impl in mod.rs

    #[test]
    fn test_type_id() {
        let w1 = TestWidget {
            key: None,
            value: 1,
        };
        let w2 = TestWidget {
            key: None,
            value: 2,
        };

        assert_eq!(w1.type_id(), w2.type_id());
        assert_eq!(w1.type_id(), TypeId::of::<TestWidget>());
    }

    #[test]
    fn test_can_update_same_type_no_key() {
        let w1 = TestWidget {
            key: None,
            value: 1,
        };
        let w2 = TestWidget {
            key: None,
            value: 2,
        };

        assert!(w1.can_update(&w2));
    }

    #[test]
    fn test_can_update_same_key() {
        let key = Key::new();
        let w1 = TestWidget {
            key: Some(key),
            value: 1,
        };
        let w2 = TestWidget {
            key: Some(key),
            value: 2,
        };

        assert!(w1.can_update(&w2));
    }

    #[test]
    fn test_cannot_update_different_key() {
        let w1 = TestWidget {
            key: Some(Key::new()),
            value: 1,
        };
        let w2 = TestWidget {
            key: Some(Key::new()),
            value: 2,
        };

        assert!(!w1.can_update(&w2));
    }

    #[test]
    fn test_downcast() {
        let widget: BoxedWidget = Box::new(TestWidget {
            key: None,
            value: 42,
        });

        // Successful downcast
        let test = widget.downcast_ref::<TestWidget>().unwrap();
        assert_eq!(test.value, 42);
    }

    #[test]
    fn test_is_type() {
        let widget: &dyn DynWidget = &TestWidget {
            key: None,
            value: 1,
        };

        assert!(widget.is::<TestWidget>());
    }

    #[test]
    fn test_debug_id() {
        let w1 = TestWidget {
            key: None,
            value: 1,
        };
        assert!(w1.debug_id().contains("TestWidget"));

        let w2 = TestWidget {
            key: Some(Key::from_u64(42).unwrap()),
            value: 2,
        };
        let debug_id = w2.debug_id();
        assert!(debug_id.contains("TestWidget"));
        assert!(debug_id.contains("42"));
    }

    #[test]
    fn test_shared_widget() {
        let widget: SharedWidget = Arc::new(TestWidget {
            key: None,
            value: 1,
        });

        let clone1 = widget.clone();
        let clone2 = widget.clone();

        // All point to same data
        assert!(Arc::ptr_eq(&widget, &clone1));
        assert!(Arc::ptr_eq(&widget, &clone2));
        assert_eq!(Arc::strong_count(&widget), 3);
    }
}