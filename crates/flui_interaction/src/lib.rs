//! Event routing, interaction handling, and gesture recognition for FLUI
//!
//! This crate provides the complete event handling and gesture infrastructure for FLUI:
//!
//! - **EventRouter**: Routes pointer/keyboard events via hit testing
//! - **HitTest**: Determines which UI elements are under cursor/touch
//! - **FocusManager**: Manages keyboard focus (global singleton)
//! - **FocusScope**: Groups focusable elements for keyboard navigation
//! - **FocusTraversalPolicy**: Determines Tab/Shift+Tab navigation order
//! - **GestureRecognizers**: High-level gesture detection (Tap, Drag, Scale, etc.)
//! - **GestureArena**: Resolves conflicts between competing gesture recognizers
//!
//! # Type System Features
//!
//! This crate makes extensive use of Rust's advanced type system:
//!
//! - **Sealed traits**: `HitTestable` and `GestureArenaMember` cannot be implemented
//!   outside this crate, allowing API evolution without breaking changes
//! - **Newtype pattern**: Type-safe IDs (`PointerId`, `FocusNodeId`, `HandlerId`)
//!   prevent mixing up different ID types at compile time
//! - **Niche optimization**: `Option<FocusNodeId>` is the same size as `FocusNodeId`
//! - **Extension traits**: Add methods to `PointerEvent` without modifying the type
//! - **RAII guards**: Transform stack management with automatic cleanup
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
//! use crate::events::{Event, PointerEvent};
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
//! use flui_interaction::{FocusManager, FocusNodeId};
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
//! # Example: Type-Safe IDs
//!
//! ```rust,ignore
//! use flui_interaction::ids::{PointerId, FocusNodeId};
//!
//! let pointer = PointerId::new(0);
//! let focus = FocusNodeId::new(42);
//!
//! // These are different types - cannot mix!
//! // fn process(id: PointerId) { ... }
//! // process(focus); // Compile error!
//! ```
//!
//! # Modules
//!
//! ## Core Infrastructure
//! - [`ids`] - Type-safe identifiers (PointerId, FocusNodeId, etc.)
//! - [`traits`] - Core traits and extension traits
//! - [`typestate`] - Typestate pattern implementations
//! - [`sealed`] - Sealed trait infrastructure (internal)
//!
//! ## Event Routing
//! - [`routing`] - Event routing, hit testing, focus management
//!
//! ## Gesture Recognition
//! - [`recognizers`] - Gesture recognizers (Tap, Drag, Scale, etc.)
//! - [`arena`] - Gesture conflict resolution
//!
//! ## Input Processing
//! - [`processing`] - Velocity tracking, prediction, resampling
//!
//! ## Testing Utilities
//! - [`testing`] - Gesture recording/replay, event builders
//!
//! ## Other
//! - [`mouse_tracker`] - Mouse enter/exit/hover tracking
//! - [`signal_resolver`] - Pointer signal conflict resolution
//!
//! # Separation from Rendering
//!
//! This crate is deliberately separate from `flui_engine` (rendering):
//! - ✅ Can test event logic without GPU
//! - ✅ Can use rendering without event handling (headless)
//! - ✅ Clear separation of concerns (SOLID principles)
//! - ✅ Smaller compile times and dependencies

// ============================================================================
// Core infrastructure modules
// ============================================================================

pub mod ids;
pub mod sealed;
pub mod traits;
pub mod typestate;

// ============================================================================
// Event routing
// ============================================================================

pub mod routing;

// ============================================================================
// Gesture recognition
// ============================================================================

pub mod arena;
pub mod recognizers;
pub mod team;
pub mod timer;

// ============================================================================
// Input processing
// ============================================================================

pub mod processing;

// ============================================================================
// Testing utilities
// ============================================================================

pub mod testing;

// ============================================================================
// Events (W3C-compliant types from ui-events and cursor-icon)
// ============================================================================

pub mod events;

// ============================================================================
// Other modules
// ============================================================================

pub mod binding;
pub mod mouse_tracker;
pub mod settings;
pub mod signal_resolver;

// ============================================================================
// Re-exports: IDs
// ============================================================================

pub use ids::{FocusNodeId, HandlerId, PointerId};

// ============================================================================
// Re-exports: Event Routing
// ============================================================================

pub use routing::{
    DirectionalFocusPolicy, EventPropagation, EventRouter, FocusManager, FocusNode, FocusScopeNode,
    FocusTraversalPolicy, GlobalPointerHandler, HitTestBehavior, HitTestEntry, HitTestResult,
    HitTestable, KeyEventCallback, KeyEventHandler, KeyEventResult, OrderedTraversalPolicy,
    PointerEventHandler, PointerRouteHandler, PointerRouter, ReadingOrderPolicy, RenderId,
    ScrollEventHandler, TransformGuard, TraversalDirection,
};

// ============================================================================
// Re-exports: Gesture Recognition
// ============================================================================

pub use arena::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition,
    DEFAULT_DISAMBIGUATION_TIMEOUT,
};

pub use team::{GestureArenaTeam, TeamEntry};

pub use timer::{global_timer_service, GestureTimer, GestureTimerService, TimerId};

pub use recognizers::{
    DoubleTapGestureRecognizer, DragGestureRecognizer, ForcePressGestureRecognizer,
    GestureRecognizer, LongPressGestureRecognizer, MultiTapGestureRecognizer,
    ScaleGestureRecognizer, TapGestureRecognizer,
};

// ============================================================================
// Re-exports: Input Processing
// ============================================================================

pub use processing::{
    InputMode, InputPredictor, PointerEventResampler, PredictedPosition, PredictionConfig,
    RawInputHandler, RawPointerEvent, Velocity, VelocityEstimate, VelocityEstimationStrategy,
    VelocityTracker,
};

// ============================================================================
// Re-exports: Testing Utilities
// ============================================================================

pub use testing::{
    GestureBuilder, GesturePlayer, GestureRecorder, GestureRecording, ModifiersBuilder,
    RecordedEvent, RecordedEventType,
};

// ============================================================================
// Re-exports: Other
// ============================================================================

pub use binding::GestureBinding;
pub use mouse_tracker::{CursorChangeCallback, MouseTracker, MouseTrackerAnnotation};
pub use sealed::{CustomGestureRecognizer, CustomHitTestable};
pub use settings::{
    GestureSettings, DEFAULT_DOUBLE_TAP_SLOP, DEFAULT_DOUBLE_TAP_TIMEOUT,
    DEFAULT_LONG_PRESS_TIMEOUT, DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MIN_FLING_VELOCITY,
    DEFAULT_MOUSE_SLOP, DEFAULT_PAN_SLOP, DEFAULT_PEN_SLOP, DEFAULT_SCALE_SLOP, DEFAULT_TOUCH_SLOP,
};
pub use signal_resolver::{PointerSignalResolver, SignalPriority};

// ============================================================================
// Re-exports: Traits
// ============================================================================

pub use traits::{
    Disposable, DragAxis, GestureCallback, GestureRecognizerExt, HitTestTarget,
    PointerEventExtTrait as PointerEventExt,
};

// ============================================================================
// Re-exports: Geometry from flui_types
// ============================================================================

pub use flui_types::geometry::{Offset, Rect};

// ============================================================================
// Re-exports: Events (W3C-compliant types)
// ============================================================================

// Re-export commonly used event types at crate root
pub use events::{CursorIcon, KeyboardEvent, PointerEvent};

// ============================================================================
// Prelude
// ============================================================================

/// Prelude module with commonly used types and traits.
///
/// # Usage
///
/// ```rust,ignore
/// use flui_interaction::prelude::*;
/// ```
pub mod prelude {
    // IDs
    pub use crate::ids::{DeviceId, FocusNodeId, HandlerId, PointerId, RegionId};

    // Traits
    pub use crate::traits::{
        Disposable, DragAxis, GestureCallback, GestureRecognizerExt, HitTestTarget,
        PointerEventExtTrait as PointerEventExt,
    };

    // Extension traits for custom types
    pub use crate::sealed::{CustomGestureRecognizer, CustomHitTestable};

    // Event routing
    pub use crate::routing::{
        EventPropagation, EventRouter, FocusManager, HitTestBehavior, HitTestEntry, HitTestResult,
        HitTestable, PointerEventHandler, PointerRouter, RenderId, TransformGuard,
    };

    // Gesture recognition
    #[allow(ambiguous_glob_reexports)]
    pub use crate::arena::*;
    pub use crate::recognizers::{
        double_tap::*, drag::*, force_press::*, long_press::*, multi_tap::*, scale::*, tap::*,
        DoubleTapGestureRecognizer, DragGestureRecognizer, ForcePressGestureRecognizer,
        LongPressGestureRecognizer, MultiTapGestureRecognizer, ScaleGestureRecognizer,
        TapGestureRecognizer,
    };

    // Input processing
    pub use crate::processing::{InputPredictor, PointerEventResampler, Velocity, VelocityTracker};

    // Testing
    pub use crate::testing::{GestureBuilder, GesturePlayer, GestureRecorder};

    // Advanced interaction
    pub use crate::mouse_tracker::{MouseTracker, MouseTrackerAnnotation};
    pub use crate::signal_resolver::{PointerSignalResolver, SignalPriority};

    // Events (W3C-compliant)
    pub use crate::events::{CursorIcon, KeyboardEvent, PointerEvent};

    // Geometry from flui_types
    pub use flui_types::geometry::{Offset, Rect};
}

// ============================================================================
// Static Assertions: Send + Sync (C-SEND-SYNC)
// ============================================================================

/// Compile-time assertions that key types are Send + Sync.
/// These ensure thread-safety properties are maintained.
#[cfg(test)]
mod static_assertions {
    use super::*;

    // Helper trait for static assertions
    #[allow(dead_code)]
    trait AssertSendSync: Send + Sync {}

    // IDs should be Send + Sync (they are Copy)
    impl AssertSendSync for PointerId {}
    impl AssertSendSync for FocusNodeId {}
    impl AssertSendSync for HandlerId {}

    // Core types should be Send + Sync
    impl AssertSendSync for FocusManager {}
    impl AssertSendSync for GestureArena {}
    impl AssertSendSync for HitTestResult {}
    impl AssertSendSync for HitTestEntry {}
    impl AssertSendSync for PointerEventResampler {}
    impl AssertSendSync for PointerSignalResolver {}
    impl AssertSendSync for MouseTracker {}

    // Recognizers should be Send + Sync
    impl AssertSendSync for TapGestureRecognizer {}
    impl AssertSendSync for DragGestureRecognizer {}
    impl AssertSendSync for ScaleGestureRecognizer {}
    impl AssertSendSync for LongPressGestureRecognizer {}
    impl AssertSendSync for DoubleTapGestureRecognizer {}
    impl AssertSendSync for MultiTapGestureRecognizer {}
    impl AssertSendSync for ForcePressGestureRecognizer {}
}
