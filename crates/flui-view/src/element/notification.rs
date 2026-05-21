//! Notification system for bubbling events up the element tree.
//!
//! Notifications provide a way for child elements to communicate with ancestors
//! without explicitly passing callbacks. A notification bubbles up from the
//! dispatch point until it reaches a listener that handles it.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `Notification` class and
//! `NotifiableElementMixin`:
//! - `Notification.dispatch()` → start bubbling
//! - `NotificationListener` → widget that handles notifications
//! - `NotifiableElementMixin` → element mixin for notification handling

use std::{
    any::{Any, TypeId},
    sync::Arc,
};

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
/// # Plan §D3 decision
///
/// `Notification: Any + Send + Sync + 'static` is the **marker** trait —
/// it is opaque and `Any` is the downcast vehicle. The object-safe
/// `ElementBase::on_notification(TypeId, &dyn Any) -> bool` handler
/// protocol does the runtime-type check + downcast at the dispatch
/// boundary; user-impls don't need to provide any methods, the empty
/// body `impl Notification for MyEvent {}` is enough. Plan U13 / R10.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `Notification` abstract class
/// (`notification_listener.dart:39`).
pub trait Notification: Any + Send + Sync + 'static {
    /// Get the type ID of this notification for type checking.
    ///
    /// Default impl uses `TypeId::of::<Self>()`; override is rarely
    /// needed. Kept for API ergonomics — `dispatch_notification` itself
    /// reads the `TypeId` off the static `N` at the generic call-site
    /// rather than via this virtual call.
    fn notification_type_id(&self) -> TypeId
    where
        Self: Sized,
    {
        TypeId::of::<Self>()
    }

    /// Get this notification as an `Any` reference for downcasting.
    ///
    /// Default impl returns `self` — sound because `Notification: Any`
    /// makes the `&Self` -> `&dyn Any` coercion automatic. User-impls
    /// like `impl Notification for ScrollNotification {}` work without
    /// any method body. Plan U13.
    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

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

/// Opt-in **typed** handler trait for elements that intercept a
/// specific notification type `N`.
///
/// `NotifiableElement<N>` is the ergonomic surface for callers who want
/// a strongly-typed `fn on_notification(&self, &N) -> bool` callback.
/// Internally the dispatcher does NOT walk via `dyn NotifiableElement<N>`
/// — it walks via the object-safe
/// [`ElementBase::on_notification`](crate::view::ElementBase::on_notification)
/// `(TypeId, &dyn Any) -> bool` handler, which translates the typed
/// callback to the object-safe shape at the impl site. This keeps the
/// single-`dyn`-boundary discipline (Constitution Principle 4; plan §D3).
///
/// Default impl returns `false` (no-op), so Elements only need to
/// override when they actually want to intercept notifications of type
/// `N`.
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `NotifiableElementMixin` + the per-listener
/// `_NotificationElement<T extends Notification>`
/// (`notification_listener.dart:127`). Flutter parameterises the
/// listener element on `T`; we mirror that with `N`.
pub trait NotifiableElement<N: Notification>: crate::view::ElementBase {
    /// Called when a notification of type `N` arrives at this element
    /// during bubble dispatch.
    ///
    /// Return `true` to cancel bubbling (notification handled).
    /// Return `false` to allow the notification to continue bubbling.
    ///
    /// Default returns `false`. Override on elements that want typed
    /// handler semantics.
    fn on_notification(&self, _notification: &N) -> bool {
        false
    }
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
        if let Some(ref handler) = self.handler
            && handler.handle(notification)
        {
            // Notification was handled, stop bubbling
            return;
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
