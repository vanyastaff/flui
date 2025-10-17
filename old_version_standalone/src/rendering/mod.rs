//! Rendering layer - bridge between widgets and painters
//!
//! This module provides Flutter-like rendering capabilities that egui doesn't
//! support natively, including accessibility, semantics, and advanced text features.
//!
//! Similar to Flutter's rendering library, this layer sits between widgets and
//! the actual painting, handling layout, hit testing, and accessibility.

pub mod accessibility;
pub mod box_constraints;
pub mod mouse_tracker;
pub mod render_object;
pub mod semantics;
pub mod text_selection;



// Re-exports
pub use accessibility::{AccessibilityFeatures, AccessibilityPreferences};
pub use box_constraints::BoxConstraints;
pub use render_object::{RenderObject, RenderBox, RenderProxyBox};
pub use semantics::{SemanticsNode, SemanticsData, SemanticsAction, SemanticsFlag, TextDirection};
pub use text_selection::{TextSelection, TextSelectionHandleType, TextAffinity};
pub use mouse_tracker::{MouseTracker, MouseTrackerAnnotation, MouseCursor, MouseEvent, MouseButton, MouseEventType};


