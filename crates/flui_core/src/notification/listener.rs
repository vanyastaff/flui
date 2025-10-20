//! NotificationListener widget for catching bubbling notifications
//!
//! Implements a ProxyWidget that intercepts notification events bubbling up the tree.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::notification::Notification;
use crate::widget::any_widget::AnyWidget;
use crate::widget::proxy::ProxyWidget;

/// Widget that listens for notifications of type T bubbling up the tree
///
/// NotificationListener is a ProxyWidget that wraps a child and intercepts
/// notifications of a specific type. When a notification bubbles up from
/// descendants, the listener's callback is invoked.
///
/// The callback returns a boolean:
/// - `true`: Stop notification from bubbling further (consumed)
/// - `false`: Allow notification to continue bubbling
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::NotificationListener;
///
/// NotificationListener::<ScrollNotification>::new(
///     |scroll| {
///         println!("Scrolled: {} pixels", scroll.delta);
///         false // Continue bubbling
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
#[derive(Clone)]
pub struct NotificationListener<T: Notification + Clone + 'static> {
    /// Callback invoked when notification is received
    ///
    /// Returns `true` to stop bubbling, `false` to continue
    pub on_notification: Arc<dyn Fn(&T) -> bool + Send + Sync>,

    /// Child widget
    pub child: Box<dyn AnyWidget>,

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
    /// * `on_notification` - Callback invoked when notification is received.
    ///   Returns `true` to stop bubbling, `false` to continue.
    /// * `child` - The child widget to wrap
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let listener = NotificationListener::<MyNotification>::new(
    ///     |notification| {
    ///         // Handle notification
    ///         println!("Got: {:?}", notification);
    ///         false // Continue bubbling
    ///     },
    ///     Box::new(child),
    /// );
    /// ```
    pub fn new(
        on_notification: impl Fn(&T) -> bool + Send + Sync + 'static,
        child: Box<dyn AnyWidget>,
    ) -> Self {
        Self {
            on_notification: Arc::new(on_notification),
            child,
            _phantom: PhantomData,
        }
    }

    /// Get the notification callback
    pub fn callback(&self) -> &Arc<dyn Fn(&T) -> bool + Send + Sync> {
        &self.on_notification
    }

    /// Invoke the callback with a notification
    ///
    /// Returns `true` if notification should stop bubbling
    pub fn handle_notification(&self, notification: &T) -> bool {
        (self.on_notification)(notification)
    }
}

// Implement ProxyWidget trait
impl<T: Notification + Clone + 'static> ProxyWidget for NotificationListener<T> {
    fn child(&self) -> &dyn AnyWidget {
        &*self.child
    }

    fn key(&self) -> Option<&dyn crate::foundation::Key> {
        // NotificationListener doesn't support keys by default
        None
    }
}

// Use macro to implement Widget trait automatically
// Note: The macro needs generic parameter support
// For now, we implement Widget manually
impl<T: Notification + Clone + 'static> crate::Widget for NotificationListener<T> {
    type Element = crate::ProxyElement<Self>;

    fn into_element(self) -> Self::Element {
        crate::ProxyElement::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, StatelessWidget, Widget};

    // Test notification type
    #[derive(Debug, Clone)]
    struct TestNotification {
        message: String,
    }

    impl Notification for TestNotification {}

    // Dummy child widget
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl StatelessWidget for ChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(ChildWidget)
        }
    }

    #[test]
    fn test_notification_listener_creation() {
        let listener = NotificationListener::<TestNotification>::new(
            |_notification| false,
            Box::new(ChildWidget),
        );

        assert!(listener.callback().as_ref() as *const _ != std::ptr::null());
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

    #[test]
    fn test_notification_listener_proxy_widget() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| false,
            Box::new(ChildWidget),
        );

        // Should implement ProxyWidget
        let child = listener.child();
        assert!(child.type_name().contains("ChildWidget"));
    }

    #[test]
    fn test_notification_listener_create_element() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| false,
            Box::new(ChildWidget),
        );

        // Should be able to create element
        let element = listener.into_element();
        assert!(format!("{:?}", element).contains("ProxyElement"));
    }

    #[test]
    fn test_notification_listener_clone() {
        let listener = NotificationListener::<TestNotification>::new(
            |_| true,
            Box::new(ChildWidget),
        );

        let cloned = listener.clone();

        // Should share the same callback (Arc)
        assert!(Arc::ptr_eq(&listener.on_notification, &cloned.on_notification));
    }

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
}
