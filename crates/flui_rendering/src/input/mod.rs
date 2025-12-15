//! Input handling for the rendering layer.
//!
//! This module provides infrastructure for handling pointer input events,
//! including mouse tracking for hover effects.
//!
//! # Key Types
//!
//! - [`MouseTracker`]: Tracks mouse devices and manages hover notifications
//! - [`MouseTrackerAnnotation`]: Marker trait for render objects that want hover events
//! - [`MouseCursor`]: Represents a mouse cursor type
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `rendering/mouse_tracker.dart`.

mod mouse_cursor;
mod mouse_tracker;

pub use mouse_cursor::{MouseCursor, MouseCursorSession, SystemMouseCursor};
pub use mouse_tracker::{
    MouseTracker, MouseTrackerAnnotation, MouseTrackerHitTest, PointerEnterEvent, PointerExitEvent,
    PointerHoverEvent,
};
