//! Gesture handling widgets
//!
//! This module provides widgets for handling user interactions like taps, drags, and gestures.

mod detector;

// Re-export gesture types from flui_interaction
pub use flui_interaction::{
    // Gesture recognizers
    DoubleTapGestureRecognizer,
    DragGestureRecognizer,
    // Arena
    GestureArena,
    GestureArenaMember,
    GestureDisposition,
    GestureRecognizer,
    LongPressGestureRecognizer,
    MultiTapGestureRecognizer,
    PointerId,
    ScaleGestureRecognizer,
    TapGestureRecognizer,
};

// Re-export our own GestureDetector widget
pub use detector::GestureDetector;
