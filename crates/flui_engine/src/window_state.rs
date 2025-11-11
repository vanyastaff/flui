//! Window state tracker - Tracks window state (focus, visibility)
//!
//! This module provides window state tracking for focus and visibility.
//! Event routing happens through PipelineOwner::dispatch_pointer_event() which
//! uses ElementTree for hit testing.

use flui_types::Offset;

/// Window state tracker - Tracks window focus and visibility state
///
/// WindowStateTracker tracks window state (focused, visible) to help optimize
/// event processing. Event routing itself happens through PipelineOwner.
///
/// # Example
///
/// ```rust,ignore
/// let mut tracker = WindowStateTracker::new();
///
/// // Update state on window events
/// tracker.on_focus_changed(false); // Window lost focus
/// tracker.on_visibility_changed(false); // Window minimized
/// ```
pub struct WindowStateTracker {
    /// Last known pointer position for hover tracking
    last_pointer_position: Option<Offset>,

    /// Whether the window is currently focused
    /// When unfocused, we reset pointer state as user may have
    /// released buttons outside the window
    is_focused: bool,

    /// Whether the window is currently visible (not minimized/occluded)
    /// When invisible, we can skip event processing for efficiency
    is_visible: bool,
}

impl WindowStateTracker {
    /// Create a new window state tracker
    pub fn new() -> Self {
        Self {
            last_pointer_position: None,
            is_focused: true, // Assume focused on creation
            is_visible: true, // Assume visible on creation
        }
    }

    /// Handle window focus change
    ///
    /// When the window loses focus, we reset pointer state because:
    /// - User may have released mouse buttons outside the window
    /// - Hover state becomes meaningless when unfocused
    ///
    /// # Arguments
    ///
    /// * `focused` - true if window gained focus, false if lost
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In your window event handler
    /// router.on_focus_changed(false); // Window lost focus
    /// ```
    pub fn on_focus_changed(&mut self, focused: bool) {
        tracing::debug!(
            "WindowStateTracker: Focus changed to {}",
            if focused { "focused" } else { "unfocused" }
        );

        self.is_focused = focused;

        if !focused {
            // Lost focus - reset pointer position
            // Hover state becomes meaningless when unfocused
            tracing::debug!("WindowStateTracker: Resetting pointer position due to focus loss");
            self.last_pointer_position = None;
        }
    }

    /// Handle window visibility change
    ///
    /// When the window is minimized or occluded, we can skip event processing
    /// for efficiency. When restored, we resume normal event handling.
    ///
    /// # Arguments
    ///
    /// * `visible` - true if window is visible, false if minimized/occluded
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In your window event handler (WM_SIZE, etc.)
    /// tracker.on_visibility_changed(false); // Window minimized
    /// ```
    pub fn on_visibility_changed(&mut self, visible: bool) {
        tracing::debug!(
            "WindowStateTracker: Visibility changed to {}",
            if visible { "visible" } else { "hidden" }
        );

        self.is_visible = visible;
    }

    /// Check if the window is currently focused
    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    /// Check if the window is currently visible (not minimized)
    pub fn is_visible(&self) -> bool {
        self.is_visible
    }

    // Legacy route_event methods removed - event routing now happens through
    // PipelineOwner::dispatch_pointer_event() which uses ElementTree::hit_test()
    // This is the modern architecture where events are dispatched directly to
    // Elements, not through the legacy Layer tree.

    /// Get the last known pointer position
    pub fn last_pointer_position(&self) -> Option<Offset> {
        self.last_pointer_position
    }
}

impl Default for WindowStateTracker {
    fn default() -> Self {
        Self::new()
    }
}
