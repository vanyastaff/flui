//! NotificationListener widget for catching bubbling notifications
//!
//! NOTE: Simplified stub implementation for Phase 11.
//! Element implementation deferred - needs trait bounds work.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::notification::Notification;
use crate::widget::any_widget::AnyWidget;

/// Widget that listens for notifications of type T bubbling up the tree
///
/// # Example
///
/// ```rust,ignore
/// NotificationListener::<ScrollNotification>::new(
///     |scroll| {
///         println!("Scrolled: {}", scroll.delta);
///         false // Continue bubbling
///     },
///     Box::new(child),
/// )
/// ```
#[derive(Clone)]
pub struct NotificationListener<T: Notification + Clone + 'static> {
    /// Callback invoked when notification is received
    pub on_notification: Arc<dyn Fn(&T) -> bool + Send + Sync>,

    /// Child widget
    pub child: Box<dyn AnyWidget>,

    /// Phantom data for type parameter
    _phantom: PhantomData<T>,
}

impl<T: Notification + Clone + 'static> std::fmt::Debug for NotificationListener<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationListener")
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: Notification + Clone + 'static> NotificationListener<T> {
    /// Create new notification listener
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
}

// NOTE: Element implementation deferred to future work
// Requires ProxyElement trait bounds to be resolved

/// Placeholder element type (not yet implemented)
pub struct NotificationListenerElement<T: Notification> {
    _phantom: PhantomData<T>,
}

impl<T: Notification + Clone + 'static> std::fmt::Debug for NotificationListenerElement<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationListenerElement")
            .finish()
    }
}
