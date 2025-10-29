//! Notification system for bubbling events up the widget tree
//!
//! This module provides Flutter's notification system - a mechanism for propagating
//! events **up** through the widget tree, similar to DOM event bubbling.
//!
//! # Architecture
//!
//! ```text
//! Child Widget
//!     ↓ dispatch_notification()
//! Parent Widget
//!     ↓ (continues bubbling)
//! Ancestor Widget (NotificationListener)
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
//! ```rust,ignore
//! use flui_core::notification::*;
//!
//! // 1. Define custom notification
//! #[derive(Debug, Clone)]
//! struct ButtonClicked {
//!     button_id: String,
//! }
//!
//! impl Notification for ButtonClicked {}
//!
//! // 2. Dispatch from child
//! context.dispatch_notification(&ButtonClicked {
//!     button_id: "my_button".to_string(),
//! });
//!
//! // 3. Listen in ancestor
//! NotificationListener::new(
//!     |notification: &ButtonClicked| {
//!         println!("Clicked: {}", notification.button_id);
//!         true // Stop bubbling
//!     },
//!     child,
//! )
//! ```
//!
//! # Built-in Notifications
//!
//! FLUI provides several standard notification types:
//! - `ScrollNotification` - Scroll events
//! - `LayoutChangedNotification` - Layout changes
//! - `SizeChangedNotification` - Size changes
//! - `KeepAliveNotification` - Keep-alive requests
//! - `FocusChangedNotification` - Focus changes

use std::any::Any;
use std::fmt;

use crate::element::ElementId;

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
/// # Example
///
/// ```rust
/// use flui_core::foundation::notification::Notification;
///
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
    /// Override this to implement custom bubbling control logic.
    /// For example, a notification might stop bubbling after reaching
    /// a certain ancestor type.
    ///
    /// # Returns
    ///
    /// - `true`: Stop notification from bubbling further
    /// - `false`: Allow notification to continue bubbling (default)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl Notification for MyNotification {
    ///     fn visit_ancestor(&self, element: &Element) -> bool {
    ///         // Stop bubbling at ScrollView
    ///         element.widget().type_name().contains("ScrollView")
    ///     }
    /// }
    /// ```
    fn visit_ancestor(&self, _element_id: ElementId) -> bool {
        false // Default: continue bubbling
    }
}

/// Object-safe notification trait for type erasure
///
/// This trait allows storing notifications in collections without knowing their concrete type.
/// Use `Notification` trait for implementing custom notifications.
///
/// # Design Pattern
///
/// This follows the same pattern as Widget/DynWidget and Render/DynRender:
/// - `Notification` - Has associated types, not object-safe
/// - `DynNotification` - Object-safe for `&dyn DynNotification`
pub trait DynNotification: Send + Sync + fmt::Debug {
    /// Called when visiting an ancestor element
    ///
    /// Returns `true` to stop bubbling, `false` to continue.
    fn visit_ancestor(&self, element_id: ElementId) -> bool;

    /// Get notification as Any for downcasting
    ///
    /// Allows type-safe downcasting to concrete notification type.
    fn as_any(&self) -> &dyn Any;
}

/// Blanket implementation of DynNotification for all Notification types
impl<T: Notification> DynNotification for T {
    fn visit_ancestor(&self, element_id: ElementId) -> bool {
        Notification::visit_ancestor(self, element_id)
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
/// about scroll events. Used by ScrollView, ListView, etc.
///
/// # Fields
///
/// - `delta`: Amount scrolled (positive = down/right, negative = up/left)
/// - `position`: Current scroll position (pixels from top/left)
/// - `max_extent`: Maximum scrollable extent (content size - viewport size)
///
/// # Example
///
/// ```rust,ignore
/// // Dispatch from ScrollView
/// context.dispatch_notification(&ScrollNotification {
///     delta: 10.0,
///     position: 100.0,
///     max_extent: 1000.0,
/// });
///
/// // Listen in ancestor
/// NotificationListener::<ScrollNotification>::new(
///     |scroll| {
///         println!("Scrolled to {}/{}", scroll.position, scroll.max_extent);
///         false // Continue bubbling
///     },
///     child,
/// )
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollNotification {
    /// Scroll delta (positive = scroll down/right, negative = scroll up/left)
    pub delta: f64,

    /// Current scroll position (pixels from top/left edge)
    pub position: f64,

    /// Maximum scroll extent (content_size - viewport_size)
    pub max_extent: f64,
}

impl ScrollNotification {
    /// Create new scroll notification
    pub const fn new(delta: f64, position: f64, max_extent: f64) -> Self {
        Self {
            delta,
            position,
            max_extent,
        }
    }

    /// Get scroll percentage (0.0 to 1.0)
    pub fn scroll_percentage(&self) -> f64 {
        if self.max_extent <= 0.0 {
            0.0
        } else {
            (self.position / self.max_extent).clamp(0.0, 1.0)
        }
    }

    /// Check if scrolled to top/left
    pub fn is_at_start(&self) -> bool {
        self.position <= 0.0
    }

    /// Check if scrolled to bottom/right
    pub fn is_at_end(&self) -> bool {
        self.position >= self.max_extent
    }
}

impl Notification for ScrollNotification {}

/// Notification dispatched when an element's layout changes
///
/// This allows ancestors to react to layout changes in descendants.
/// Dispatched after layout phase completes.
///
/// # Example
///
/// ```rust,ignore
/// // Dispatch after layout
/// context.dispatch_notification(&LayoutChangedNotification {
///     element_id: self_id,
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutChangedNotification {
    /// Element that changed layout
    pub element_id: ElementId,
}

impl LayoutChangedNotification {
    /// Create new layout changed notification
    pub const fn new(element_id: ElementId) -> Self {
        Self { element_id }
    }
}

impl Notification for LayoutChangedNotification {}

/// Notification dispatched when an element's size changes
///
/// More specific than LayoutChangedNotification, provides old and new sizes.
/// Useful for animations, responsive layouts, etc.
///
/// # Example
///
/// ```rust,ignore
/// use flui_types::Size;
///
/// context.dispatch_notification(&SizeChangedNotification {
///     element_id: self_id,
///     old_size: Size::new(100.0, 200.0),
///     new_size: Size::new(150.0, 250.0),
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeChangedNotification {
    /// Element that changed size
    pub element_id: ElementId,

    /// Previous size
    pub old_size: flui_types::Size,

    /// New size
    pub new_size: flui_types::Size,
}

impl SizeChangedNotification {
    /// Create new size changed notification
    pub const fn new(
        element_id: ElementId,
        old_size: flui_types::Size,
        new_size: flui_types::Size,
    ) -> Self {
        Self {
            element_id,
            old_size,
            new_size,
        }
    }

    /// Get size delta (new - old)
    pub fn delta(&self) -> flui_types::Size {
        flui_types::Size::new(
            self.new_size.width - self.old_size.width,
            self.new_size.height - self.old_size.height,
        )
    }

    /// Check if width changed
    pub fn width_changed(&self) -> bool {
        self.old_size.width != self.new_size.width
    }

    /// Check if height changed
    pub fn height_changed(&self) -> bool {
        self.old_size.height != self.new_size.height
    }
}

impl Notification for SizeChangedNotification {}

/// Notification used by AutomaticKeepAlive to request staying alive
///
/// Used in lazy lists to keep items alive even when scrolled out of view.
/// Items send this notification to tell the parent list not to unmount them.
///
/// # Example
///
/// ```rust,ignore
/// // Request keep-alive
/// context.dispatch_notification(&KeepAliveNotification {
///     element_id: self_id,
///     handle: 123,
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeepAliveNotification {
    /// Element to keep alive
    pub element_id: ElementId,

    /// Keep alive handle (unique identifier for this request)
    pub handle: usize,
}

impl KeepAliveNotification {
    /// Create new keep-alive notification
    pub const fn new(element_id: ElementId, handle: usize) -> Self {
        Self { element_id, handle }
    }
}

impl Notification for KeepAliveNotification {}

/// Notification dispatched when focus changes
///
/// Bubbles up to notify ancestors about focus changes.
/// Used by TextField, Button, and other focusable widgets.
///
/// # Example
///
/// ```rust,ignore
/// // Focus gained
/// context.dispatch_notification(&FocusChangedNotification {
///     element_id: self_id,
///     has_focus: true,
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusChangedNotification {
    /// Element that gained or lost focus
    pub element_id: ElementId,

    /// True if element gained focus, false if lost focus
    pub has_focus: bool,
}

impl FocusChangedNotification {
    /// Create new focus changed notification
    pub const fn new(element_id: ElementId, has_focus: bool) -> Self {
        Self {
            element_id,
            has_focus,
        }
    }

    /// Check if focus was gained
    pub fn focused(&self) -> bool {
        self.has_focus
    }

    /// Check if focus was lost
    pub fn unfocused(&self) -> bool {
        !self.has_focus
    }
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
        let element_id = 123;
        assert!(!notification.visit_ancestor(element_id));
    }

    #[test]
    fn test_dyn_notification_downcast() {
        let notification = TestNotification { value: 42 };
        let dyn_notification: &dyn DynNotification = &notification;

        // Should be able to downcast
        let downcasted = dyn_notification.as_any().downcast_ref::<TestNotification>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_scroll_notification() {
        let scroll = ScrollNotification::new(10.0, 100.0, 1000.0);

        assert_eq!(scroll.delta, 10.0);
        assert_eq!(scroll.position, 100.0);
        assert_eq!(scroll.max_extent, 1000.0);

        // Test percentage
        assert_eq!(scroll.scroll_percentage(), 0.1);

        // Test position checks
        assert!(!scroll.is_at_start());
        assert!(!scroll.is_at_end());

        // Should be cloneable
        let cloned = scroll.clone();
        assert_eq!(cloned.delta, 10.0);
    }

    #[test]
    fn test_scroll_notification_at_start() {
        let scroll = ScrollNotification::new(0.0, 0.0, 1000.0);
        assert!(scroll.is_at_start());
        assert!(!scroll.is_at_end());
    }

    #[test]
    fn test_scroll_notification_at_end() {
        let scroll = ScrollNotification::new(0.0, 1000.0, 1000.0);
        assert!(!scroll.is_at_start());
        assert!(scroll.is_at_end());
    }

    #[test]
    fn test_layout_changed_notification() {
        let element_id = 42;
        let notification = LayoutChangedNotification::new(element_id);

        assert_eq!(notification.element_id, element_id);
    }

    #[test]
    fn test_size_changed_notification() {
        use flui_types::Size;

        let element_id = 42;
        let old_size = Size::new(100.0, 200.0);
        let new_size = Size::new(150.0, 250.0);

        let notification = SizeChangedNotification::new(element_id, old_size, new_size);

        assert_eq!(notification.old_size, old_size);
        assert_eq!(notification.new_size, new_size);

        // Test delta
        let delta = notification.delta();
        assert_eq!(delta.width, 50.0);
        assert_eq!(delta.height, 50.0);

        // Test change detection
        assert!(notification.width_changed());
        assert!(notification.height_changed());
    }

    #[test]
    fn test_size_changed_no_change() {
        use flui_types::Size;

        let size = Size::new(100.0, 200.0);
        let notification = SizeChangedNotification::new(42, size, size);

        assert!(!notification.width_changed());
        assert!(!notification.height_changed());
    }

    #[test]
    fn test_keep_alive_notification() {
        let element_id = 42;
        let notification = KeepAliveNotification::new(element_id, 123);

        assert_eq!(notification.element_id, element_id);
        assert_eq!(notification.handle, 123);
    }

    #[test]
    fn test_focus_changed_notification() {
        let element_id = 42;
        let gained = FocusChangedNotification::new(element_id, true);
        let lost = FocusChangedNotification::new(element_id, false);

        assert!(gained.has_focus);
        assert!(gained.focused());
        assert!(!gained.unfocused());

        assert!(!lost.has_focus);
        assert!(!lost.focused());
        assert!(lost.unfocused());
    }

    #[test]
    fn test_multiple_notification_types() {
        use flui_types::Size;

        // Should be able to have different notification types
        let scroll = ScrollNotification::new(1.0, 2.0, 3.0);
        let layout = LayoutChangedNotification::new(42);
        let size_changed =
            SizeChangedNotification::new(43, Size::new(10.0, 20.0), Size::new(30.0, 40.0));

        // Store in vec of trait objects
        let notifications: Vec<&dyn DynNotification> = vec![&scroll, &layout, &size_changed];
        assert_eq!(notifications.len(), 3);
    }

    #[test]
    fn test_notification_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ScrollNotification>();
        assert_sync::<ScrollNotification>();
        assert_send::<LayoutChangedNotification>();
        assert_sync::<LayoutChangedNotification>();
    }
}
