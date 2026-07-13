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

use std::any::{Any, TypeId};

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
/// body `impl Notification for MyEvent {}` is enough.
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
    /// any method body.
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
