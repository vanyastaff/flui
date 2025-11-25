//! UI-specific notification types for FLUI core
//!
//! This module provides concrete notification types that are specific to UI components
//! and interactions. It builds on the foundation notification system by implementing
//! the base `Notification` trait from `flui-foundation`.
//!
//! # Architecture
//!
//! ```text
//! flui-foundation::notification
//!     ↑ (provides base traits)
//! flui_core::foundation::notification
//!     ↑ (provides UI-specific types)
//! UI Components (ScrollView, TextField, etc.)
//! ```
//!
//! # Built-in UI Notifications
//!
//! This module provides several standard notification types for common UI interactions:
//!
//! ## Layout & Rendering
//! - `LayoutChangedNotification` - Layout changes in elements
//! - `SizeChangedNotification` - Size changes with old/new values
//!
//! ## Scrolling
//! - `ScrollNotification` - Scroll events with position and delta
//!
//! ## Focus Management
//! - `FocusChangedNotification` - Focus gained/lost events
//!
//! ## Lifecycle
//! - `KeepAliveNotification` - Keep-alive requests for lazy lists
//!
//! ## Navigation
//! - `RouteChangedNotification` - Route/navigation changes
//!
//! # Example Usage
//!
//! ```rust
//! use flui_core::foundation::notification::*;
//! use flui_foundation::{Notification, DynNotification, ElementId};
//! use flui_types::Size;
//!
//! // Create a size change notification
//! let element_id = ElementId::new(1).unwrap();
//! let notification = SizeChangedNotification::new(
//!     element_id,
//!     Size::new(100.0, 200.0), // old size
//!     Size::new(150.0, 250.0), // new size
//! );
//!
//! // Use as foundation trait
//! let dyn_notification: &dyn DynNotification = &notification;
//! let should_stop = dyn_notification.visit_ancestor(element_id);
//!
//! // Access UI-specific data
//! assert!(notification.width_changed());
//! assert_eq!(notification.delta().width, 50.0);
//! ```

// Re-export foundation notification traits
pub use flui_foundation::notification::{DynNotification, Notification};
use flui_foundation::ElementId;

// ============================================================================
// UI-Specific Notification Types
// ============================================================================

/// Notification dispatched when scrolling occurs
///
/// This notification bubbles up from scrollable widgets to notify ancestors
/// about scroll events. Used by ScrollView, ListView, GridView, etc.
///
/// # Fields
///
/// - `delta`: Amount scrolled (positive = down/right, negative = up/left)
/// - `position`: Current scroll position (pixels from top/left)
/// - `max_extent`: Maximum scrollable extent (content size - viewport size)
///
/// # Example
///
/// ```rust
/// use flui_core::foundation::notification::ScrollNotification;
///
/// // Create scroll notification
/// let scroll = ScrollNotification::new(10.0, 100.0, 1000.0);
///
/// // Check scroll state
/// assert_eq!(scroll.scroll_percentage(), 0.1); // 10% scrolled
/// assert!(!scroll.is_at_start());
/// assert!(!scroll.is_at_end());
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

    /// Get scrolling direction
    pub fn direction(&self) -> ScrollDirection {
        if self.delta > 0.0 {
            ScrollDirection::Forward
        } else if self.delta < 0.0 {
            ScrollDirection::Backward
        } else {
            ScrollDirection::Idle
        }
    }
}

impl Notification for ScrollNotification {}

/// Direction of scrolling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    /// Scrolling forward (down/right)
    Forward,
    /// Scrolling backward (up/left)
    Backward,
    /// Not scrolling
    Idle,
}

/// Notification dispatched when an element's layout changes
///
/// This allows ancestors to react to layout changes in descendants.
/// Dispatched after layout phase completes.
///
/// # Example
///
/// ```rust
/// use flui_core::foundation::notification::LayoutChangedNotification;
/// use flui_foundation::ElementId;
///
/// let element_id = ElementId::new(1).unwrap();
/// let notification = LayoutChangedNotification::new(element_id);
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
/// Useful for animations, responsive layouts, and size-dependent logic.
///
/// # Example
///
/// ```rust
/// use flui_core::foundation::notification::SizeChangedNotification;
/// use flui_foundation::ElementId;
/// use flui_types::Size;
///
/// let element_id = ElementId::new(1).unwrap();
/// let old_size = Size::new(100.0, 200.0);
/// let new_size = Size::new(150.0, 250.0);
///
/// let notification = SizeChangedNotification::new(element_id, old_size, new_size);
///
/// // Check what changed
/// assert!(notification.width_changed());
/// assert!(notification.height_changed());
///
/// // Get the delta
/// let delta = notification.delta();
/// assert_eq!(delta.width, 50.0);
/// assert_eq!(delta.height, 50.0);
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

    /// Get absolute change in width
    pub fn width_delta_abs(&self) -> f32 {
        (self.new_size.width - self.old_size.width).abs()
    }

    /// Get absolute change in height
    pub fn height_delta_abs(&self) -> f32 {
        (self.new_size.height - self.old_size.height).abs()
    }

    /// Check if this is a significant size change (beyond threshold)
    pub fn is_significant(&self, threshold: f32) -> bool {
        self.width_delta_abs() > threshold || self.height_delta_abs() > threshold
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
/// ```rust
/// use flui_core::foundation::notification::KeepAliveNotification;
/// use flui_foundation::ElementId;
///
/// let element_id = ElementId::new(10).unwrap();
/// let handle = 123;
///
/// let notification = KeepAliveNotification::new(element_id, handle);
///
/// // Later, check if this is the right keep-alive request
/// assert_eq!(notification.handle, 123);
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
/// ```rust
/// use flui_core::foundation::notification::FocusChangedNotification;
/// use flui_foundation::ElementId;
///
/// let element_id = ElementId::new(5).unwrap();
///
/// // Focus gained
/// let focused = FocusChangedNotification::new(element_id, true);
/// assert!(focused.focused());
/// assert!(!focused.unfocused());
///
/// // Focus lost
/// let unfocused = FocusChangedNotification::new(element_id, false);
/// assert!(!unfocused.focused());
/// assert!(unfocused.unfocused());
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

/// Notification for route/navigation changes
///
/// Used by Navigator and routing widgets to notify ancestors about
/// navigation events like push, pop, replace operations.
///
/// # Example
///
/// ```rust
/// use flui_core::foundation::notification::{RouteChangedNotification, RouteChangeType};
/// use flui_foundation::ElementId;
///
/// let navigator_id = ElementId::new(1).unwrap();
/// let notification = RouteChangedNotification::new(
///     navigator_id,
///     RouteChangeType::Push,
///     "/home".to_string(),
/// );
///
/// assert!(notification.is_push());
/// assert_eq!(notification.route_name, "/home");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteChangedNotification {
    /// Navigator element that changed route
    pub element_id: ElementId,

    /// Type of route change
    pub change_type: RouteChangeType,

    /// Name/path of the route
    pub route_name: String,
}

impl RouteChangedNotification {
    /// Create new route changed notification
    pub fn new(element_id: ElementId, change_type: RouteChangeType, route_name: String) -> Self {
        Self {
            element_id,
            change_type,
            route_name,
        }
    }

    /// Check if this is a push operation
    pub fn is_push(&self) -> bool {
        matches!(self.change_type, RouteChangeType::Push)
    }

    /// Check if this is a pop operation
    pub fn is_pop(&self) -> bool {
        matches!(self.change_type, RouteChangeType::Pop)
    }

    /// Check if this is a replace operation
    pub fn is_replace(&self) -> bool {
        matches!(self.change_type, RouteChangeType::Replace)
    }
}

impl Notification for RouteChangedNotification {}

/// Type of route change operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteChangeType {
    /// New route pushed onto navigation stack
    Push,
    /// Route popped from navigation stack
    Pop,
    /// Current route replaced with new route
    Replace,
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

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
        assert_eq!(scroll.direction(), ScrollDirection::Forward);

        // Test at start
        let at_start = ScrollNotification::new(0.0, 0.0, 1000.0);
        assert!(at_start.is_at_start());
        assert_eq!(at_start.direction(), ScrollDirection::Idle);

        // Test at end
        let at_end = ScrollNotification::new(0.0, 1000.0, 1000.0);
        assert!(at_end.is_at_end());

        // Test backward scroll
        let backward = ScrollNotification::new(-5.0, 50.0, 1000.0);
        assert_eq!(backward.direction(), ScrollDirection::Backward);
    }

    #[test]
    fn test_layout_changed_notification() {
        let element_id = ElementId::new(42);
        let notification = LayoutChangedNotification::new(element_id);

        assert_eq!(notification.element_id, element_id);
    }

    #[test]
    fn test_size_changed_notification() {
        let element_id = ElementId::new(1);
        let old_size = Size::new(100.0, 200.0);
        let new_size = Size::new(150.0, 250.0);

        let notification = SizeChangedNotification::new(element_id, old_size, new_size);

        assert_eq!(notification.element_id, element_id);
        assert_eq!(notification.old_size, old_size);
        assert_eq!(notification.new_size, new_size);

        // Test delta
        let delta = notification.delta();
        assert_eq!(delta.width, 50.0);
        assert_eq!(delta.height, 50.0);

        // Test change detection
        assert!(notification.width_changed());
        assert!(notification.height_changed());

        // Test absolute deltas
        assert_eq!(notification.width_delta_abs(), 50.0);
        assert_eq!(notification.height_delta_abs(), 50.0);

        // Test significance
        assert!(notification.is_significant(10.0));
        assert!(!notification.is_significant(100.0));
    }

    #[test]
    fn test_size_changed_no_change() {
        let element_id = ElementId::new(1);
        let size = Size::new(100.0, 200.0);

        let notification = SizeChangedNotification::new(element_id, size, size);

        // Should detect no change
        assert!(!notification.width_changed());
        assert!(!notification.height_changed());

        let delta = notification.delta();
        assert_eq!(delta.width, 0.0);
        assert_eq!(delta.height, 0.0);

        // Should not be significant
        assert!(!notification.is_significant(0.0));
    }

    #[test]
    fn test_keep_alive_notification() {
        let element_id = ElementId::new(10);
        let handle = 123;

        let notification = KeepAliveNotification::new(element_id, handle);

        assert_eq!(notification.element_id, element_id);
        assert_eq!(notification.handle, handle);
    }

    #[test]
    fn test_focus_changed_notification() {
        let element_id = ElementId::new(5);

        // Focus gained
        let focused = FocusChangedNotification::new(element_id, true);
        assert!(focused.focused());
        assert!(!focused.unfocused());
        assert_eq!(focused.has_focus, true);

        // Focus lost
        let unfocused = FocusChangedNotification::new(element_id, false);
        assert!(!unfocused.focused());
        assert!(unfocused.unfocused());
        assert_eq!(unfocused.has_focus, false);
    }

    #[test]
    fn test_route_changed_notification() {
        let navigator_id = ElementId::new(1);
        let route_name = "/home".to_string();

        // Test push
        let push_notif =
            RouteChangedNotification::new(navigator_id, RouteChangeType::Push, route_name.clone());
        assert!(push_notif.is_push());
        assert!(!push_notif.is_pop());
        assert!(!push_notif.is_replace());

        // Test pop
        let pop_notif =
            RouteChangedNotification::new(navigator_id, RouteChangeType::Pop, route_name.clone());
        assert!(!pop_notif.is_push());
        assert!(pop_notif.is_pop());
        assert!(!pop_notif.is_replace());

        // Test replace
        let replace_notif = RouteChangedNotification::new(
            navigator_id,
            RouteChangeType::Replace,
            route_name.clone(),
        );
        assert!(!replace_notif.is_push());
        assert!(!replace_notif.is_pop());
        assert!(replace_notif.is_replace());
    }

    #[test]
    fn test_notification_trait_implementation() {
        use flui_foundation::notification::{DynNotification, Notification};

        let scroll = ScrollNotification::new(5.0, 50.0, 500.0);
        let layout = LayoutChangedNotification::new(ElementId::new(1));

        // Both should implement Notification and DynNotification
        let _: &dyn DynNotification = &scroll;
        let _: &dyn DynNotification = &layout;

        // Should be able to downcast correctly
        let scroll_dyn: &dyn DynNotification = &scroll;
        assert!(scroll_dyn
            .as_any()
            .downcast_ref::<ScrollNotification>()
            .is_some());
        assert!(scroll_dyn
            .as_any()
            .downcast_ref::<LayoutChangedNotification>()
            .is_none());
    }

    #[test]
    fn test_all_notifications_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ScrollNotification>();
        assert_sync::<ScrollNotification>();
        assert_send::<LayoutChangedNotification>();
        assert_sync::<LayoutChangedNotification>();
        assert_send::<SizeChangedNotification>();
        assert_sync::<SizeChangedNotification>();
        assert_send::<KeepAliveNotification>();
        assert_sync::<KeepAliveNotification>();
        assert_send::<FocusChangedNotification>();
        assert_sync::<FocusChangedNotification>();
        assert_send::<RouteChangedNotification>();
        assert_sync::<RouteChangedNotification>();
    }
}
