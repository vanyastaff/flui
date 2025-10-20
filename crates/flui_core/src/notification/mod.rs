//! Notification system for bubbling events up the widget tree
//!
//! This module provides Flutter's notification system - a mechanism for propagating
//! events **up** through the widget tree, similar to DOM event bubbling.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::notification::*;
//!
//! // Define custom notification
//! #[derive(Debug, Clone)]
//! struct ButtonClicked {
//!     button_id: String,
//! }
//!
//! impl Notification for ButtonClicked {}
//!
//! // Dispatch from child
//! context.dispatch_notification(&ButtonClicked {
//!     button_id: "my_button".to_string(),
//! });
//!
//! // Listen in ancestor
//! NotificationListener::new(
//!     |notification: &ButtonClicked| {
//!         println!("Clicked: {}", notification.button_id);
//!         true // Stop bubbling
//!     },
//!     child,
//! )
//! ```

use std::any::Any;
use std::fmt;

use crate::element::AnyElement;
use crate::ElementId;
use flui_types::Size;

pub mod listener;

pub use listener::NotificationListener;

/// Base trait for notifications that bubble up the widget tree
///
/// Notifications are events that propagate from child to parent through the element tree.
/// Any widget can dispatch a notification, and ancestor widgets can listen for it using
/// `NotificationListener`.
///
/// # Type Safety
///
/// Notifications are type-safe - listeners specify the exact notification type they handle.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyNotification {
///     data: String,
/// }
///
/// impl Notification for MyNotification {}
/// ```
pub trait Notification: Any + Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element during bubbling
    ///
    /// Returns true to stop bubbling, false to continue.
    /// Default implementation continues bubbling.
    fn visit_ancestor(&self, _element: &dyn AnyElement) -> bool {
        false
    }
}

/// Object-safe notification trait for type erasure
///
/// This trait allows storing notifications in collections without knowing their concrete type.
/// Use `Notification` trait for implementing custom notifications.
pub trait AnyNotification: Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element
    fn visit_ancestor(&self, element: &dyn AnyElement) -> bool;

    /// Get notification as Any for downcasting
    fn as_any(&self) -> &dyn Any;
}

/// Blanket implementation of AnyNotification for all Notification types
impl<T: Notification> AnyNotification for T {
    fn visit_ancestor(&self, element: &dyn AnyElement) -> bool {
        Notification::visit_ancestor(self, element)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// Built-in Notification Types
// ============================================================================

/// Notification dispatched when scrolling occurs
///
/// This notification bubbles up from scrollable widgets to notify ancestors
/// about scroll events.
///
/// # Example
///
/// ```rust,ignore
/// context.dispatch_notification(&ScrollNotification {
///     delta: 10.0,
///     position: 100.0,
///     max_extent: 1000.0,
/// });
/// ```
#[derive(Debug, Clone)]
pub struct ScrollNotification {
    /// Scroll delta (positive = scroll down/right, negative = scroll up/left)
    pub delta: f64,

    /// Current scroll position
    pub position: f64,

    /// Maximum scroll extent
    pub max_extent: f64,
}

impl Notification for ScrollNotification {}

/// Notification dispatched when an element's layout changes
///
/// This allows ancestors to react to layout changes in descendants.
#[derive(Debug, Clone)]
pub struct LayoutChangedNotification {
    /// Element that changed layout
    pub element_id: ElementId,
}

impl Notification for LayoutChangedNotification {}

/// Notification dispatched when an element's size changes
///
/// More specific than LayoutChangedNotification, provides old and new sizes.
#[derive(Debug, Clone)]
pub struct SizeChangedLayoutNotification {
    /// Element that changed size
    pub element_id: ElementId,

    /// Previous size
    pub old_size: Size,

    /// New size
    pub new_size: Size,
}

impl Notification for SizeChangedLayoutNotification {}

/// Notification used by AutomaticKeepAlive to request staying alive
///
/// Used in lazy lists to keep items alive even when scrolled out of view.
#[derive(Debug, Clone)]
pub struct KeepAliveNotification {
    /// Element to keep alive
    pub element_id: ElementId,

    /// Keep alive handle (unique identifier)
    pub handle: usize,
}

impl Notification for KeepAliveNotification {}

/// Notification dispatched when focus changes
///
/// Bubbles up to notify ancestors about focus changes.
#[derive(Debug, Clone)]
pub struct FocusChangedNotification {
    /// Element that gained or lost focus
    pub element_id: ElementId,

    /// True if element gained focus, false if lost focus
    pub has_focus: bool,
}

impl Notification for FocusChangedNotification {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestNotification {
        value: i32,
    }

    impl Notification for TestNotification {}

    #[test]
    fn test_notification_trait() {
        let notification = TestNotification { value: 42 };

        // Should implement Debug
        let debug_str = format!("{:?}", notification);
        assert!(debug_str.contains("TestNotification"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_notification_default_visit() {
        let notification = TestNotification { value: 42 };

        // Default implementation should continue bubbling (return false)
        use crate::element::ComponentElement;
        use crate::widget::StatelessWidget;
        use crate::Context;

        #[derive(Debug, Clone)]
        struct DummyWidget;

        impl StatelessWidget for DummyWidget {
            fn build(&self, _context: &Context) -> Box<crate::widget::AnyWidget> {
                Box::new(DummyWidget)
            }
        }

        let element = ComponentElement::new(DummyWidget);
        assert!(!notification.visit_ancestor(&element));
    }

    #[test]
    fn test_any_notification_downcast() {
        let notification = TestNotification { value: 42 };
        let any: &dyn AnyNotification = &notification;

        // Should be able to downcast
        let downcasted = any.as_any().downcast_ref::<TestNotification>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_scroll_notification() {
        let scroll = ScrollNotification {
            delta: 10.0,
            position: 100.0,
            max_extent: 1000.0,
        };

        assert_eq!(scroll.delta, 10.0);
        assert_eq!(scroll.position, 100.0);
        assert_eq!(scroll.max_extent, 1000.0);

        // Should be cloneable
        let cloned = scroll.clone();
        assert_eq!(cloned.delta, 10.0);
    }

    #[test]
    fn test_layout_changed_notification() {
        let element_id = ElementId::new();
        let notification = LayoutChangedNotification { element_id };

        assert_eq!(notification.element_id, element_id);
    }

    #[test]
    fn test_size_changed_notification() {
        let element_id = ElementId::new();
        let old_size = Size::new(100.0, 200.0);
        let new_size = Size::new(150.0, 250.0);

        let notification = SizeChangedLayoutNotification {
            element_id,
            old_size,
            new_size,
        };

        assert_eq!(notification.old_size, old_size);
        assert_eq!(notification.new_size, new_size);
    }

    #[test]
    fn test_keep_alive_notification() {
        let element_id = ElementId::new();
        let notification = KeepAliveNotification {
            element_id,
            handle: 123,
        };

        assert_eq!(notification.handle, 123);
    }

    #[test]
    fn test_focus_changed_notification() {
        let element_id = ElementId::new();
        let notification = FocusChangedNotification {
            element_id,
            has_focus: true,
        };

        assert!(notification.has_focus);
    }

    #[test]
    fn test_multiple_notification_types() {
        // Should be able to have different notification types
        let scroll = ScrollNotification {
            delta: 1.0,
            position: 2.0,
            max_extent: 3.0,
        };

        let layout = LayoutChangedNotification {
            element_id: ElementId::new(),
        };

        // Store in vec of trait objects
        let notifications: Vec<&dyn AnyNotification> = vec![&scroll, &layout];
        assert_eq!(notifications.len(), 2);
    }
}
