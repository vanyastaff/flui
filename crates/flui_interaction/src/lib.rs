//! Event routing, interaction handling, and gesture recognition for FLUI
//!
//! This crate provides the complete event handling and gesture infrastructure for FLUI:
//! - **EventRouter**: Routes pointer/keyboard events via hit testing
//! - **HitTest**: Determines which UI elements are under cursor/touch
//! - **FocusManager**: Manages keyboard focus (global singleton)
//! - **GestureRecognizers**: High-level gesture detection (Tap, Drag, Scale, etc.)
//! - **GestureArena**: Resolves conflicts between competing gesture recognizers
//!
//! # Architecture
//!
//! ```text
//! Platform (winit, Win32, etc.)
//!     ↓
//! PointerEvent/KeyEvent (flui_types)
//!     ↓
//! EventRouter (event routing)
//!     ├─ Hit Testing (spatial)
//!     └─ Focus Management (keyboard)
//!         ↓
//! Handlers (closures in Layers)
//!     ↓
//! GestureRecognizers (gesture recognition)
//!     ├─ GestureArena (conflict resolution)
//!     └─ Individual Recognizers (Tap, Drag, Scale, etc.)
//!         ↓
//! User code (Signal::update, callbacks, etc.)
//! ```
//!
//! # Example: Basic Event Routing
//!
//! ```rust,ignore
//! use flui_interaction::{EventRouter, HitTestable};
//! use flui_types::events::{Event, PointerEvent};
//!
//! let mut router = EventRouter::new();
//!
//! // Register a layer with hit testing
//! let layer = MyLayer { bounds: Rect::from_xywh(0.0, 0.0, 100.0, 100.0) };
//!
//! // Route pointer event
//! let event = PointerEvent::Down { position: Offset::new(50.0, 50.0), ... };
//! router.route_event(&mut layer, &Event::Pointer(event));
//! ```
//!
//! # Example: Keyboard Focus
//!
//! ```rust,ignore
//! use flui_interaction::FocusManager;
//!
//! let focus_id = FocusNodeId::new(1);
//!
//! // Request focus
//! FocusManager::global().request_focus(focus_id);
//!
//! // Check focus
//! if FocusManager::global().has_focus(focus_id) {
//!     println!("We have focus!");
//! }
//! ```
//!
//! # Example: Gesture Recognition
//!
//! ```rust,ignore
//! use flui_interaction::prelude::*;
//!
//! let mut recognizer = TapGestureRecognizer::new();
//! recognizer.on_tap(|| println!("Tapped!"));
//!
//! // Use with GestureDetector widget from flui_widgets
//! ```
//!
//! # Modules
//!
//! ## Event Routing
//! - [`event_router`] - Event dispatch and routing
//! - [`hit_test`] - Spatial hit testing
//! - [`focus_manager`] - Keyboard focus management
//! - [`input`] - Input event types
//!
//! ## Gesture Recognition
//! - [`recognizers`] - Gesture recognizers (Tap, Drag, Scale, etc.)
//! - [`arena`] - Gesture conflict resolution
//!
//! Note: GestureDetector widget is in `flui_widgets::gestures`
//!
//! # Separation from Rendering
//!
//! This crate is deliberately separate from `flui_engine` (rendering):
//! - ✅ Can test event logic without GPU
//! - ✅ Can use rendering without event handling (headless)
//! - ✅ Clear separation of concerns (SOLID principles)
//! - ✅ Smaller compile times and dependencies

// Event routing modules
pub mod event_router;
pub mod focus_manager;
pub mod hit_test;
pub mod input;

// Gesture recognition modules
pub mod arena;
pub mod recognizers;

// Advanced interaction modules
pub mod resampler;
pub mod mouse_tracker;
pub mod signal_resolver;

// Re-export main event routing types
pub use event_router::EventRouter;
pub use focus_manager::{FocusManager, FocusNodeId};
pub use hit_test::{
    ElementId, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
};

// Re-export main gesture types
pub use arena::{GestureArena, GestureArenaMember, GestureDisposition, PointerId};
pub use recognizers::{
    DoubleTapGestureRecognizer, DragGestureRecognizer, GestureRecognizer,
    LongPressGestureRecognizer, MultiTapGestureRecognizer, ScaleGestureRecognizer,
    TapGestureRecognizer,
};

// Re-export advanced interaction types
pub use mouse_tracker::{MouseTracker, MouseTrackerAnnotation};
pub use resampler::PointerEventResampler;
pub use signal_resolver::{PointerSignalResolver, SignalPriority};

// Re-export common types from flui_types for convenience
pub use flui_types::events::{Event, KeyEvent, PointerEvent};
pub use flui_types::geometry::{Offset, Rect};

/// Prelude module with commonly used types and traits
pub mod prelude {
    // Event routing
    pub use crate::event_router::*;
    pub use crate::focus_manager::*;
    pub use crate::hit_test::{
        ElementId, EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
    };

    // Gesture recognition
    #[allow(ambiguous_glob_reexports)]
    pub use crate::arena::*;
    pub use crate::recognizers::{
        double_tap::*, drag::*, long_press::*, multi_tap::*, scale::*, tap::*,
        DoubleTapGestureRecognizer, DragGestureRecognizer, LongPressGestureRecognizer,
        MultiTapGestureRecognizer, ScaleGestureRecognizer, TapGestureRecognizer,
    };

    // Advanced interaction
    pub use crate::mouse_tracker::{MouseTracker, MouseTrackerAnnotation};
    pub use crate::resampler::PointerEventResampler;
    pub use crate::signal_resolver::{PointerSignalResolver, SignalPriority};

    // Common types from flui_types
    pub use flui_types::events::{Event, KeyEvent, PointerEvent};
    pub use flui_types::geometry::{Offset, Rect};
}