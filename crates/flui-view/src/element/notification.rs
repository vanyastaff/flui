//! Notification system for bubbling events up the element tree.
//!
//! Notifications provide a way for child elements to communicate with ancestors
//! without explicitly passing callbacks. A notification bubbles up from the
//! dispatch point until it reaches a listener that handles it.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `Notification` class and `NotifiableElementMixin`:
//! - `Notification.dispatch()` → start bubbling
//! - `NotificationListener` → widget that handles notifications
//! - `NotifiableElementMixin` → element mixin for notification handling

use std::any::{Any, TypeId};
use std::sync::Arc;

/// A notification that can bubble up the element tree.
///
/// Notifications are dispatched from a point in the tree and bubble up
/// to ancestors until a listener handles them (returns `true`).
///
/// # Usage
///
/// ```rust,ignore
/// use flui_view::Notification;
///
/// // Define a notification type
/// struct ScrollNotification {
///     offset: f64,
/// }
///
/// impl Notification for ScrollNotification {}
///
/// // Dispatch from a BuildContext
/// let notification = ScrollNotification { offset: 100.0 };
/// notification.dispatch(ctx);
/// ```
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `Notification` abstract class.
pub trait Notification: Send + Sync + 'static {
    /// Get the type ID of this notification for type checking.
    fn notification_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Get this notification as an Any reference for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Dispatch this notification to the element tree.
    ///
    /// The notification will bubble up from the target BuildContext
    /// until a NotifiableElement handles it.
    fn dispatch(&self, target: &dyn crate::context::BuildContext)
    where
        Self: Sized,
    {
        target.dispatch_notification(self);
    }

    /// Add debug information about this notification.
    ///
    /// Override this to provide useful debug output.
    fn debug_fill_description(&self, description: &mut Vec<String>) {
        let _ = description; // default: no extra description
    }
}

/// A boxed notification for dynamic dispatch.
pub type BoxedNotification = Box<dyn Notification>;

/// Callback type for notification listeners.
///
/// Return `true` to stop bubbling (notification handled).
/// Return `false` to continue bubbling to ancestors.
pub type NotificationCallback<T> = Box<dyn Fn(&T) -> bool + Send + Sync>;

/// Trait for elements that can receive notifications.
///
/// Elements that implement this trait can intercept notifications
/// as they bubble up the tree.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `NotifiableElementMixin`.
pub trait NotifiableElement: Send + Sync {
    /// Called when a notification arrives at this element.
    ///
    /// Return `true` to cancel bubbling (notification handled).
    /// Return `false` to allow the notification to continue bubbling.
    ///
    /// # Arguments
    ///
    /// * `notification` - The notification being dispatched
    fn on_notification(&self, notification: &dyn Notification) -> bool;
}

/// A node in the notification tree for efficient dispatch.
///
/// The notification tree is a parallel structure to the element tree,
/// containing only elements that can handle notifications. This enables
/// O(k) dispatch where k is the number of NotifiableElements in the
/// ancestor chain, rather than O(n) where n is the tree depth.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `_NotificationNode`.
pub struct NotificationNode {
    /// Parent node in the notification tree (None for root).
    parent: Option<Arc<NotificationNode>>,
    /// The notifiable element at this node, if any.
    /// Using a function pointer to avoid lifetime issues with trait objects.
    handler: Option<Box<dyn NotificationHandler>>,
}

/// Trait for notification handlers in the notification tree.
pub trait NotificationHandler: Send + Sync {
    /// Handle a notification, returning true if handled.
    fn handle(&self, notification: &dyn Notification) -> bool;
}

impl NotificationNode {
    /// Create a new notification node.
    pub fn new(
        parent: Option<Arc<NotificationNode>>,
        handler: Option<Box<dyn NotificationHandler>>,
    ) -> Self {
        Self { parent, handler }
    }

    /// Create a root notification node (no parent).
    pub fn root() -> Self {
        Self {
            parent: None,
            handler: None,
        }
    }

    /// Dispatch a notification up the tree.
    ///
    /// The notification bubbles up until a handler returns `true`
    /// or the root is reached.
    pub fn dispatch_notification(&self, notification: &dyn Notification) {
        // Try the current handler
        if let Some(ref handler) = self.handler {
            if handler.handle(notification) {
                // Notification was handled, stop bubbling
                return;
            }
        }

        // Continue to parent
        if let Some(ref parent) = self.parent {
            parent.dispatch_notification(notification);
        }
    }

    /// Get the parent node.
    pub fn parent(&self) -> Option<&Arc<NotificationNode>> {
        self.parent.as_ref()
    }
}

impl std::fmt::Debug for NotificationNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationNode")
            .field("has_parent", &self.parent.is_some())
            .field("has_handler", &self.handler.is_some())
            .finish()
    }
}

// ============================================================================
// Common Notification Types
// ============================================================================

/// Notification sent when a layout change occurs.
///
/// Use this to notify ancestors that layout assumptions may be invalid.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `LayoutChangedNotification`.
#[derive(Debug, Clone)]
pub struct LayoutChangedNotification;

impl Notification for LayoutChangedNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Notification sent when size changes.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `SizeChangedLayoutNotification`.
#[derive(Debug, Clone)]
pub struct SizeChangedNotification {
    /// The new size after the change.
    pub size: flui_types::Size,
}

impl Notification for SizeChangedNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn debug_fill_description(&self, description: &mut Vec<String>) {
        description.push(format!("size: {:?}", self.size));
    }
}

/// Notification sent during scrolling.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `ScrollNotification` family.
#[derive(Debug, Clone)]
pub struct ScrollNotification {
    /// The scroll offset.
    pub offset: f64,
    /// The scroll axis.
    pub axis: flui_types::Axis,
}

impl Notification for ScrollNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn debug_fill_description(&self, description: &mut Vec<String>) {
        description.push(format!("offset: {}", self.offset));
        description.push(format!("axis: {:?}", self.axis));
    }
}

/// Notification sent when a drag starts.
#[derive(Debug, Clone)]
pub struct DragStartNotification {
    /// Global position where drag started.
    pub global_position: flui_types::Offset,
}

impl Notification for DragStartNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Notification sent when a drag ends.
#[derive(Debug, Clone)]
pub struct DragEndNotification {
    /// Velocity at drag end.
    pub velocity: flui_types::Offset,
}

impl Notification for DragEndNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Notification sent when focus changes.
#[derive(Debug, Clone)]
pub struct FocusNotification {
    /// Whether the element gained focus.
    pub has_focus: bool,
}

impl Notification for FocusNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Notification sent when a keep-alive status changes.
#[derive(Debug, Clone)]
pub struct KeepAliveNotification {
    /// Whether to keep the element alive.
    pub keep_alive: bool,
}

impl Notification for KeepAliveNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_type_id() {
        let notification = LayoutChangedNotification;
        assert_eq!(
            notification.notification_type_id(),
            TypeId::of::<LayoutChangedNotification>()
        );
    }

    #[test]
    fn test_notification_node_dispatch() {
        use std::sync::atomic::{AtomicBool, Ordering};

        // Create a handler that marks when called
        struct TestHandler {
            called: Arc<AtomicBool>,
        }

        impl NotificationHandler for TestHandler {
            fn handle(&self, _notification: &dyn Notification) -> bool {
                self.called.store(true, Ordering::SeqCst);
                true // handled
            }
        }

        let called = Arc::new(AtomicBool::new(false));
        let handler = Box::new(TestHandler {
            called: Arc::clone(&called),
        });

        let node = NotificationNode::new(None, Some(handler));

        // Dispatch a notification
        node.dispatch_notification(&LayoutChangedNotification);

        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_notification_bubbling() {
        use std::sync::atomic::{AtomicU32, Ordering};

        // Track the order of handler calls
        static CALL_ORDER: AtomicU32 = AtomicU32::new(0);

        struct OrderTracker {
            expected_order: u32,
            handle_it: bool,
        }

        impl NotificationHandler for OrderTracker {
            fn handle(&self, _notification: &dyn Notification) -> bool {
                let order = CALL_ORDER.fetch_add(1, Ordering::SeqCst);
                assert_eq!(order, self.expected_order);
                self.handle_it
            }
        }

        CALL_ORDER.store(0, Ordering::SeqCst);

        // Create a chain: child -> middle -> parent
        let parent = Arc::new(NotificationNode::new(
            None,
            Some(Box::new(OrderTracker {
                expected_order: 2,
                handle_it: true,
            })),
        ));

        let middle = Arc::new(NotificationNode::new(
            Some(Arc::clone(&parent)),
            Some(Box::new(OrderTracker {
                expected_order: 1,
                handle_it: false, // don't handle, bubble up
            })),
        ));

        let child = NotificationNode::new(
            Some(Arc::clone(&middle)),
            Some(Box::new(OrderTracker {
                expected_order: 0,
                handle_it: false, // don't handle, bubble up
            })),
        );

        // Dispatch from child - should bubble through middle to parent
        child.dispatch_notification(&LayoutChangedNotification);

        // Verify all three handlers were called
        assert_eq!(CALL_ORDER.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_scroll_notification_debug() {
        let notification = ScrollNotification {
            offset: 100.0,
            axis: flui_types::Axis::Vertical,
        };

        let mut desc = Vec::new();
        notification.debug_fill_description(&mut desc);

        assert_eq!(desc.len(), 2);
        assert!(desc[0].contains("100"));
        assert!(desc[1].contains("Vertical"));
    }
}
