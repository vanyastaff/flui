//! Interaction RenderObjects (pointer listeners, mouse regions, etc.)
//!
//! This module contains render objects for handling user interaction:
//!
//! - [`RenderAbsorbPointer`] - Absorbs pointer events, preventing them from reaching children
//! - [`RenderIgnorePointer`] - Makes subtree invisible to hit testing (events pass through)
//! - [`RenderMouseRegion`] - Tracks mouse hover state and cursor
//! - [`RenderPointerListener`] - Listens for pointer events (down, up, move, cancel)
//! - [`RenderSemanticsGestureHandler`] - Handles gestures for accessibility
//! - [`RenderTapRegion`] - Detects taps inside or outside its bounds

pub mod absorb_pointer;
pub mod ignore_pointer;
pub mod mouse_region;
pub mod pointer_listener;
pub mod semantics_gesture_handler;
pub mod tap_region;

// Re-exports
pub use absorb_pointer::RenderAbsorbPointer;
pub use ignore_pointer::RenderIgnorePointer;
pub use mouse_region::{MouseCallbacks, MouseCursor, RenderMouseRegion};
pub use pointer_listener::{PointerCallbacks, RenderPointerListener};
pub use semantics_gesture_handler::{RenderSemanticsGestureHandler, SemanticsGestureCallbacks};
pub use tap_region::{RenderTapRegion, TapRegionCallbacks, TapRegionGroupId};
