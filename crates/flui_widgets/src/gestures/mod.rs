//! Gesture handling widgets
//!
//! This module provides widgets for handling user interactions like taps, drags, and gestures.

mod gesture_detector;

pub use gesture_detector::{clear_gesture_handlers, dispatch_gesture_event, GestureDetector};
