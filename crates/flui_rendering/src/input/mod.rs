//! Input handling for the rendering layer.
//!
//! This module provides infrastructure for handling pointer input events,
//! including mouse tracking for hover effects.
//!
//! # Key Types
//!
//! - [`MouseTracker`]: Tracks mouse devices and manages hover notifications
//! - [`MouseTrackerAnnotation`]: Marker trait for render objects that want hover events
//! - [`CursorIcon`]: Represents a cursor type (re-exported from flui_interaction)
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `rendering/mouse_tracker.dart`.

mod mouse_tracker;

// Re-export CursorIcon from flui_interaction (W3C CSS compliant)
pub use flui_interaction::CursorIcon;

// Export MouseCursorSession from mouse_tracker (rendering-specific)
pub use mouse_tracker::{
    MouseCursorSession, MouseTracker, MouseTrackerAnnotation, MouseTrackerHitTest,
    PointerEnterEvent, PointerExitEvent, PointerHoverEvent,
};
