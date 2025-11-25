//! Notification system for bubbling events up the view tree
//!
//! This module provides the fundamental abstractions for Flutter's notification system -
//! a mechanism for propagating events **up** through the view tree, similar to DOM event bubbling.
//!
//! # Architecture
//!
//! ```text
//! Child View
//!     ↓ dispatch_notification()
//! Parent View
//!     ↓ (continues bubbling)
//! Ancestor View (NotificationListener)
//!     ↓ handle notification
//! Stop or Continue bubbling
//! ```
//!
//! # How It Works
//!
//! 1. **Define Notification**: Implement `Notification` trait
//! 2. **Dispatch**: Call `context.dispatch_notification(&notification)`
//! 3. **Listen**: Wrap ancestor with `NotificationListener<T>`
//! 4. **Handle**: Callback receives notification, returns bool to control bubbling
//!
//! # Example
//!
//! ```rust
//! use flui_foundation::notification::*;
//! use flui_foundation::ElementId;
//!
//! // 1. Define custom notification
//! #[derive(Debug, Clone)]
//! struct ButtonClicked {
//!     button_id: String,
//! }
//!
//! impl Notification for ButtonClicked {}
//!
//! // 2. Use with type erasure
//! let notification = ButtonClicked {
//!     button_id: "my_button".to_string(),
//! };
//! let dyn_notification: &dyn DynNotification = &notification;
//!
//! // 3. Control bubbling
//! let element_id = ElementId::new(1);
//! let should_stop = dyn_notification.visit_ancestor(element_id);
//! ```
//!
//! # Design Pattern
//!
//! This follows the same pattern as other FLUI core abstractions:
//! - `Notification` - Primary trait with full type information
//! - `DynNotification` - Object-safe trait for type erasure and storage
//!
//! # Foundation Layer Scope
//!
//! This module contains only the **fundamental abstractions** for notifications.
//! Concrete notification types (ScrollNotification, FocusChangedNotification, etc.)
//! are defined in higher-level crates like `flui_core` or `flui_widgets`.

use std::any::Any;
use std::fmt;

use crate::element_id::ElementId;

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
/// # Bubbling Control
///
/// The `visit_ancestor()` method allows notifications to control their own bubbling behavior.
/// Default implementation continues bubbling (returns false).
///
/// # Thread Safety
///
/// All notifications must be `Send + Sync` to work in FLUI's multi-threaded environment.
///
/// # Example
///
/// ```rust
/// use flui_foundation::notification::Notification;
/// use flui_foundation::ElementId;
///
/// #[derive(Debug, Clone)]
/// struct MyNotification {
///     data: String,
/// }
///
/// impl Notification for MyNotification {
///     fn visit_ancestor(&self, element_id: ElementId) -> bool {
///         // Custom bubbling logic - stop at specific element types
///         // Default implementation just returns false (continue)
///         false
///     }
/// }
/// ```
pub trait Notification: Any + Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element during bubbling
    ///
    /// Override this to implement custom bubbling control logic.
    /// For example, a notification might stop bubbling after reaching
    /// a certain ancestor type, or based on element properties.
    ///
    /// # Parameters
    ///
    /// - `element_id`: The ID of the ancestor element being visited
    ///
    /// # Returns
    ///
    /// - `true`: Stop notification from bubbling further up the tree
    /// - `false`: Allow notification to continue bubbling (default)
    ///
    /// # Default Behavior
    ///
    /// The default implementation always returns `false`, allowing unlimited bubbling.
    /// Most notification types will want to keep this behavior.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_foundation::notification::Notification;
    /// use flui_foundation::ElementId;
    ///
    /// #[derive(Debug, Clone)]
    /// struct StopAtRootNotification {
    ///     root_id: ElementId,
    /// }
    ///
    /// impl Notification for StopAtRootNotification {
    ///     fn visit_ancestor(&self, element_id: ElementId) -> bool {
    ///         // Stop bubbling when we reach the specified root
    ///         element_id == self.root_id
    ///     }
    /// }
    /// ```
    fn visit_ancestor(&self, _element_id: ElementId) -> bool {
        false // Default: continue bubbling
    }
}

/// Object-safe notification trait for type erasure
///
/// This trait allows storing notifications in collections or passing them through
/// APIs without knowing their concrete type. It provides the same interface as
/// `Notification` but is object-safe for dynamic dispatch.
///
/// # Design Pattern
///
/// This follows the same pattern as other FLUI object-safe traits:
/// - `Notification` - Primary trait, not object-safe due to `Any` supertrait
/// - `DynNotification` - Object-safe version for `&dyn DynNotification`
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for all `Notification` types via
/// a blanket implementation. You should never implement this trait directly.
///
/// # Usage
///
/// ```rust
/// use flui_foundation::notification::{Notification, DynNotification};
/// use flui_foundation::ElementId;
///
/// #[derive(Debug, Clone)]
/// struct MyNotification;
/// impl Notification for MyNotification {}
///
/// let notification = MyNotification;
/// let dyn_notification: &dyn DynNotification = &notification;
///
/// // Use object-safe methods
/// let element_id = ElementId::new(1);
/// let should_stop = dyn_notification.visit_ancestor(element_id);
///
/// // Downcast back to concrete type
/// let concrete = dyn_notification.as_any()
///     .downcast_ref::<MyNotification>()
///     .unwrap();
/// ```
pub trait DynNotification: Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element during bubbling
    ///
    /// This is the object-safe version of `Notification::visit_ancestor()`.
    ///
    /// # Returns
    ///
    /// - `true`: Stop notification from bubbling further
    /// - `false`: Allow notification to continue bubbling
    fn visit_ancestor(&self, element_id: ElementId) -> bool;

    /// Get notification as Any for downcasting
    ///
    /// Allows type-safe downcasting to concrete notification type.
    /// This is the primary mechanism for recovering the original type
    /// from a type-erased notification.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_foundation::notification::{Notification, DynNotification};
    ///
    /// #[derive(Debug, Clone)]
    /// struct MyNotification { value: i32 }
    /// impl Notification for MyNotification {}
    ///
    /// let notification = MyNotification { value: 42 };
    /// let dyn_notification: &dyn DynNotification = &notification;
    ///
    /// // Safe downcast
    /// if let Some(concrete) = dyn_notification.as_any()
    ///     .downcast_ref::<MyNotification>() {
    ///     assert_eq!(concrete.value, 42);
    /// }
    /// ```
    fn as_any(&self) -> &dyn Any;
}

/// Blanket implementation of DynNotification for all Notification types
///
/// This implementation automatically provides object-safe versions of
/// notification methods for any type that implements `Notification`.
impl<T: Notification> DynNotification for T {
    fn visit_ancestor(&self, element_id: ElementId) -> bool {
        Notification::visit_ancestor(self, element_id)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestNotification {
        value: i32,
    }

    impl Notification for TestNotification {}

    #[derive(Debug, Clone)]
    struct CustomBubblingNotification {
        stop_at: ElementId,
    }

    impl Notification for CustomBubblingNotification {
        fn visit_ancestor(&self, element_id: ElementId) -> bool {
            element_id == self.stop_at
        }
    }

    #[test]
    fn test_notification_trait() {
        let notification = TestNotification { value: 42 };
        let element_id = ElementId::new(1);

        // Default implementation returns false (continue bubbling)
        assert!(!Notification::visit_ancestor(&notification, element_id));
    }

    #[test]
    fn test_notification_default_visit() {
        let notification = TestNotification { value: 123 };
        let element_id = ElementId::new(5);

        // Should continue bubbling by default
        assert_eq!(
            Notification::visit_ancestor(&notification, element_id),
            false
        );
    }

    #[test]
    fn test_custom_bubbling_logic() {
        let stop_at = ElementId::new(10);
        let other_element = ElementId::new(5);

        let notification = CustomBubblingNotification { stop_at };

        // Should stop at the target element
        assert!(Notification::visit_ancestor(&notification, stop_at));

        // Should continue at other elements
        assert!(!Notification::visit_ancestor(&notification, other_element));
    }

    #[test]
    fn test_dyn_notification_downcast() {
        let notification = TestNotification { value: 42 };
        let dyn_notification: &dyn DynNotification = &notification;

        // Should be able to downcast
        let downcasted = dyn_notification.as_any().downcast_ref::<TestNotification>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);

        // Wrong type should fail
        let wrong_downcast = dyn_notification
            .as_any()
            .downcast_ref::<CustomBubblingNotification>();
        assert!(wrong_downcast.is_none());
    }

    #[test]
    fn test_dyn_notification_visit_ancestor() {
        let stop_at = ElementId::new(7);
        let other_element = ElementId::new(3);

        let notification = CustomBubblingNotification { stop_at };
        let dyn_notification: &dyn DynNotification = &notification;

        // Should work the same as direct trait call
        assert!(dyn_notification.visit_ancestor(stop_at));
        assert!(!dyn_notification.visit_ancestor(other_element));
    }

    #[test]
    fn test_multiple_notification_types() {
        let test_notif = TestNotification { value: 1 };
        let custom_notif = CustomBubblingNotification {
            stop_at: ElementId::new(1),
        };

        // Both should implement DynNotification
        let _: &dyn DynNotification = &test_notif;
        let _: &dyn DynNotification = &custom_notif;

        // Should be able to store in collection
        let notifications: Vec<&dyn DynNotification> = vec![&test_notif, &custom_notif];
        assert_eq!(notifications.len(), 2);
    }

    #[test]
    fn test_notification_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        // All notifications must be Send + Sync
        assert_send::<TestNotification>();
        assert_sync::<TestNotification>();
        assert_send::<CustomBubblingNotification>();
        assert_sync::<CustomBubblingNotification>();
    }

    #[test]
    fn test_notification_debug() {
        let notification = TestNotification { value: 42 };
        let debug_string = format!("{:?}", notification);
        assert!(debug_string.contains("TestNotification"));
        assert!(debug_string.contains("42"));
    }

    #[test]
    fn test_dyn_notification_debug() {
        let notification = TestNotification { value: 42 };
        let dyn_notification: &dyn DynNotification = &notification;
        let debug_string = format!("{:?}", dyn_notification);
        assert!(debug_string.contains("TestNotification"));
    }
}
