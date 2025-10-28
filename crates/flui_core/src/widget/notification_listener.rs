//! NotificationListener widget for catching bubbling notifications
//!
//! Implements a widget that intercepts notification events bubbling up the tree.
//!
//! # Current Status
//!
//! NOTE: This is currently a data structure only. Full Widget integration will be
//! added when notification dispatching is implemented in the Element layer.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::foundation::notification::{Notification, DynNotification};
use crate::widget::BoxedWidget;

/// Widget that listens for notifications of type T bubbling up the tree
///
/// NotificationListener wraps a child and intercepts notifications of a specific type.
/// When a notification bubbles up from descendants, the listener's callback is invoked.
///
/// The callback returns a boolean:
/// - `true`: Stop notification from bubbling further (consumed)
/// - `false`: Allow notification to continue bubbling to ancestors
///
/// # Architecture
///
/// ```text
/// NotificationListener<ScrollNotification>
///     ↓ (wraps)
/// Child Widget Tree
///     ↓ dispatch_notification()
/// ScrollNotification bubbles up
///     ↓ (intercepted)
/// Callback invoked
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{NotificationListener, ScrollNotification};
///
/// NotificationListener::<ScrollNotification>::new(
///     |scroll| {
///         println!("Scrolled: {} pixels", scroll.delta);
///         false // Continue bubbling to parent listeners
///     },
///     Box::new(child_widget),
/// )
/// ```
///
/// # Usage with Custom Notifications
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyNotification {
///     data: String,
/// }
///
/// impl Notification for MyNotification {}
///
/// let listener = NotificationListener::<MyNotification>::new(
///     |notification| {
///         println!("Received: {}", notification.data);
///         true // Stop bubbling
///     },
///     child,
/// );
/// ```
///
/// # Type Parameter
///
/// `T` - The notification type to listen for. Must implement `Notification + Clone`.
///
/// # Implementation Note
///
/// Currently this is just a data structure. Full Widget trait implementation
/// will be added when notification bubbling is implemented in BuildContext.
pub struct NotificationListener<T: Notification + Clone + 'static> {
    /// Callback invoked when notification is received
    ///
    /// Returns `true` to stop bubbling, `false` to continue
    on_notification: Arc<dyn Fn(&T) -> bool + Send + Sync>,

    /// Child widget
    child: BoxedWidget,

    /// Phantom data for type parameter
    _phantom: PhantomData<T>,
}

impl<T: Notification + Clone + 'static> std::fmt::Debug for NotificationListener<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationListener")
            .field("notification_type", &std::any::type_name::<T>())
            .field("has_callback", &true)
            .finish()
    }
}

impl<T: Notification + Clone + 'static> NotificationListener<T> {
    /// Create new notification listener
    ///
    /// # Arguments
    ///
    /// * `on_notification` - Callback invoked when notification of type T is received.
    ///   Returns `true` to stop bubbling, `false` to continue to parent listeners.
    /// * `child` - The child widget to wrap
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::{NotificationListener, ScrollNotification};
    ///
    /// let listener = NotificationListener::<ScrollNotification>::new(
    ///     |scroll| {
    ///         // Handle scroll event
    ///         println!("Scrolled to {}", scroll.position);
    ///         false // Continue bubbling
    ///     },
    ///     Box::new(child),
    /// );
    /// ```
    pub fn new(
        on_notification: impl Fn(&T) -> bool + Send + Sync + 'static,
        child: BoxedWidget,
    ) -> Self {
        Self {
            on_notification: Arc::new(on_notification),
            child,
            _phantom: PhantomData,
        }
    }

    /// Get the notification callback
    ///
    /// Returns a reference to the Arc-wrapped callback function.
    pub fn callback(&self) -> &Arc<dyn Fn(&T) -> bool + Send + Sync> {
        &self.on_notification
    }

    /// Invoke the callback with a notification
    ///
    /// # Returns
    ///
    /// `true` if notification should stop bubbling, `false` to continue
    pub fn handle_notification(&self, notification: &T) -> bool {
        (self.on_notification)(notification)
    }

    /// Try to handle a type-erased notification
    ///
    /// Attempts to downcast the notification to type T and call the callback.
    ///
    /// # Returns
    ///
    /// - `Some(true)` - Notification handled, stop bubbling
    /// - `Some(false)` - Notification handled, continue bubbling
    /// - `None` - Wrong type, this listener doesn't handle it
    pub fn handle_dyn_notification(&self, notification: &dyn DynNotification) -> Option<bool> {
        // Try to downcast to the specific type T
        if let Some(typed_notification) = notification.as_any().downcast_ref::<T>() {
            // Call the callback and return its result
            Some((self.on_notification)(typed_notification))
        } else {
            // Wrong type, this listener doesn't handle it
            None
        }
    }

    /// Get reference to child widget
    pub fn child(&self) -> &BoxedWidget {
        &self.child
    }
}

// NOTE: StatelessWidget implementation will be added when notification
// dispatching is implemented in BuildContext. For now, NotificationListener
// is just a data structure that can be used in tests and will be integrated
// into the widget system later.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundation::notification::{ScrollNotification, FocusChangedNotification};
    use crate::widget::StatelessWidget;
    use crate::BuildContext;

    // Test notification type
    #[derive(Debug, Clone)]
    struct TestNotification {
        message: String,
    }

    impl Notification for TestNotification {}

    // Dummy child widget for testing
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl StatelessWidget for ChildWidget {
        fn build(&self, _context: &BuildContext) -> BoxedWidget {
            Box::new(ChildWidget)
        }
    }

    #[test]
    fn test_notification_listener_creation() {
        let listener = NotificationListener::<TestNotification>::new(
            |_notification| true,
            Box::new(ChildWidget),
        );

        // Callback exists and can be called
        let test = TestNotification { message: "test".to_string() };
        assert!(listener.handle_notification(&test));
    }

    #[test]
    fn test_notification_listener_handle() {
        let listener = NotificationListener::<TestNotification>::new(
            |notification| {
                assert_eq!(notification.message, "test");
                true // Stop bubbling
            },
            Box::new(ChildWidget),
        );

        let notification = TestNotification {
            message: "test".to_string(),
        };

        let should_stop = listener.handle_notification(&notification);
        assert!(should_stop);
    }

    #[test]
    fn test_notification_listener_continue_bubbling() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| false, // Continue bubbling
            Box::new(ChildWidget),
        );

        let notification = TestNotification {
            message: "test".to_string(),
        };

        let should_stop = listener.handle_notification(&notification);
        assert!(!should_stop);
    }

    // Note: Clone test removed because NotificationListener
    // doesn't impl Clone (BoxedWidget is not Clone)

    #[test]
    fn test_notification_listener_debug() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| false,
            Box::new(ChildWidget),
        );

        let debug_str = format!("{:?}", listener);
        assert!(debug_str.contains("NotificationListener"));
        assert!(debug_str.contains("TestNotification"));
    }

    // Note: test_notification_listener_build() removed
    // because BuildContext doesn't have a mock() method yet.
    // This test would require a full Element tree setup.

    #[test]
    fn test_handle_dyn_notification_correct_type() {
        let listener = NotificationListener::<TestNotification>::new(
            |notification| {
                assert_eq!(notification.message, "test");
                true // Stop bubbling
            },
            Box::new(ChildWidget),
        );

        let notification = TestNotification {
            message: "test".to_string(),
        };

        // Should handle notification of correct type
        let result = listener.handle_dyn_notification(&notification);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_handle_dyn_notification_wrong_type() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| true,
            Box::new(ChildWidget),
        );

        let scroll = ScrollNotification::new(10.0, 100.0, 1000.0);

        // Should return None for wrong type
        let result = listener.handle_dyn_notification(&scroll);
        assert_eq!(result, None);
    }

    #[test]
    fn test_handle_dyn_notification_callback_result() {
        // Test with callback returning false (continue bubbling)
        let listener_continue = NotificationListener::<TestNotification>::new(
            |_| false,
            Box::new(ChildWidget),
        );

        let notification = TestNotification {
            message: "test".to_string(),
        };

        assert_eq!(listener_continue.handle_dyn_notification(&notification), Some(false));

        // Test with callback returning true (stop bubbling)
        let listener_stop = NotificationListener::<TestNotification>::new(
            |_| true,
            Box::new(ChildWidget),
        );

        assert_eq!(listener_stop.handle_dyn_notification(&notification), Some(true));
    }

    #[test]
    fn test_notification_listener_with_builtin_types() {
        // Test with ScrollNotification
        let scroll_listener = NotificationListener::<ScrollNotification>::new(
            |scroll| {
                assert_eq!(scroll.position, 100.0);
                false
            },
            Box::new(ChildWidget),
        );

        let scroll = ScrollNotification::new(10.0, 100.0, 1000.0);
        assert!(!scroll_listener.handle_notification(&scroll));

        // Test with FocusChangedNotification
        let focus_listener = NotificationListener::<FocusChangedNotification>::new(
            |focus| {
                assert!(focus.has_focus);
                true
            },
            Box::new(ChildWidget),
        );

        let focus = FocusChangedNotification::new(42, true);
        assert!(focus_listener.handle_notification(&focus));
    }

    #[test]
    fn test_child_accessor() {
        let child = Box::new(ChildWidget);
        let listener = NotificationListener::<TestNotification>::new(
            |_| false,
            child.clone(),
        );

        // Should be able to access child
        let child_ref = listener.child();
        assert!(child_ref.type_name().contains("ChildWidget"));
    }
}
