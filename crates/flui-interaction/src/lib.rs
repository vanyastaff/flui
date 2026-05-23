//! Event routing, interaction handling, and gesture recognition for FLUI
//!
//! This crate provides the complete event handling and gesture infrastructure
//! for FLUI:
//!
//! - **EventRouter**: Routes pointer/keyboard events via hit testing
//! - **HitTest**: Determines which UI elements are under cursor/touch
//! - **FocusManager**: Manages keyboard focus (global singleton)
//! - **FocusScope**: Groups focusable elements for keyboard navigation
//! - **FocusTraversalPolicy**: Determines Tab/Shift+Tab navigation order
//! - **GestureRecognizers**: High-level gesture detection (Tap, Drag, Scale,
//!   etc.)
//! - **GestureArena**: Resolves conflicts between competing gesture recognizers
//!
//! # Type System Features
//!
//! This crate makes extensive use of Rust's advanced type system:
//!
//! - **Sealed traits**: `HitTestable` and `GestureArenaMember` cannot be
//!   implemented outside this crate, allowing API evolution without breaking
//!   changes
//! - **Canonical pointer id**: [`PointerId`] is re-exported from the
//!   `ui-events` crate (`NonZeroU64`-backed). [`FocusNodeId`] and
//!   [`HandlerId`] are crate-local `NonZeroU64` newtypes that prevent
//!   mixing up different ID types at compile time
//! - **Niche optimization**: `Option<FocusNodeId>` is the same size as
//!   `FocusNodeId`
//! - **Extension traits**: Add methods to `PointerEvent` without modifying the
//!   type
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
//! let layer = MyLayer { bounds: Rect::from_xywh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(100.0)) };
//!
//! // Route pointer event
//! let event = PointerEvent::Down { position: Offset::new(Pixels(50.0), Pixels(50.0)), ... };
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
//! let pointer = PointerId::PRIMARY;
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
//! - `testing` - Gesture recording/replay, event builders (requires `testing` feature)
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
// Testing utilities — gated behind `testing` Cargo feature (U29)
// ============================================================================

#[cfg(any(test, feature = "testing"))]
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

// ============================================================================
// Re-exports: Gesture Recognition
// ============================================================================
pub use arena::{
    DEFAULT_DISAMBIGUATION_TIMEOUT, GestureArena, GestureArenaEntry, GestureArenaMember,
    GestureDisposition,
};
// ============================================================================
// Re-exports: Other
// ============================================================================
pub use binding::GestureBinding;
// ============================================================================
// Re-exports: Events (W3C-compliant types)
// ============================================================================

// Re-export commonly used event types at crate root
pub use events::{CursorIcon, KeyboardEvent, PointerEvent};
// ============================================================================
// Re-exports: Geometry from flui_types
// ============================================================================
pub use flui_types::geometry::{Offset, Rect};
pub use ids::{FocusNodeId, HandlerId, PointerId};
pub use mouse_tracker::{CursorChangeCallback, MouseTracker, MouseTrackerAnnotation};
// ============================================================================
// Re-exports: Input Processing
// ============================================================================
pub use processing::{
    InputMode, InputPredictor, PointerEventResampler, PredictedPosition, PredictionConfig,
    RawInputHandler, RawPointerEvent, Velocity, VelocityEstimate, VelocityEstimationStrategy,
    VelocityTracker,
};
pub use recognizers::{
    DoubleTapGestureRecognizer, DragGestureRecognizer, ForcePressGestureRecognizer,
    GestureRecognizer, LongPressGestureRecognizer, MultiTapGestureRecognizer,
    ScaleGestureRecognizer, TapGestureRecognizer,
};
// ============================================================================
// Re-exports: Event Routing
// ============================================================================
pub use routing::{
    EventPropagation, EventRouter, FocusManager, FocusNode, FocusScopeNode, FocusTraversalPolicy,
    GlobalPointerHandler, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
    KeyEventCallback, KeyEventHandler, KeyEventResult, PointerEventHandler, PointerRouteHandler,
    PointerRouter, ReadingOrderPolicy, RenderId, ScrollEventHandler, TransformGuard,
};
pub use sealed::{CustomGestureRecognizer, CustomHitTestable};
pub use settings::{
    DEFAULT_DOUBLE_TAP_SLOP, DEFAULT_DOUBLE_TAP_TIMEOUT, DEFAULT_LONG_PRESS_TIMEOUT,
    DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MIN_FLING_VELOCITY, DEFAULT_MOUSE_SLOP, DEFAULT_PAN_SLOP,
    DEFAULT_PEN_SLOP, DEFAULT_SCALE_SLOP, DEFAULT_TOUCH_SLOP, GestureSettings,
};
pub use signal_resolver::{PointerSignalResolver, SignalPriority};
pub use team::{GestureArenaTeam, TeamEntry};
// ============================================================================
// Re-exports: Testing Utilities (U29 feature-gated)
// ============================================================================
#[cfg(any(test, feature = "testing"))]
pub use testing::{
    GestureBuilder, GesturePlayer, GestureRecorder, GestureRecording, ModifiersBuilder,
    RecordedEvent, RecordedEventType,
};
pub use timer::{GestureTimer, GestureTimerService, TimerId, global_timer_service};
// ============================================================================
// Re-exports: Traits
// ============================================================================
pub use traits::{
    Disposable, DragAxis, GestureCallback, GestureRecognizerExt, HitTestTarget,
    PointerEventExtTrait as PointerEventExt,
};

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
    // Geometry from flui_types
    pub use flui_types::geometry::{Offset, Rect};

    // Gesture recognition
    #[allow(ambiguous_glob_reexports)]
    pub use crate::arena::*;
    // Events (W3C-compliant)
    pub use crate::events::{CursorIcon, KeyboardEvent, PointerEvent};
    // Advanced interaction
    pub use crate::mouse_tracker::{MouseTracker, MouseTrackerAnnotation};
    // Input processing
    pub use crate::processing::{InputPredictor, PointerEventResampler, Velocity, VelocityTracker};
    // Event routing
    pub use crate::routing::{
        EventPropagation, EventRouter, FocusManager, HitTestBehavior, HitTestEntry, HitTestResult,
        HitTestable, PointerEventHandler, PointerRouter, RenderId, TransformGuard,
    };
    // Extension traits for custom types
    pub use crate::sealed::{CustomGestureRecognizer, CustomHitTestable};
    // Testing (U29 feature-gated)
    #[cfg(any(test, feature = "testing"))]
    pub use crate::testing::{GestureBuilder, GesturePlayer, GestureRecorder};
    // Traits
    pub use crate::traits::{
        Disposable, DragAxis, GestureCallback, GestureRecognizerExt, HitTestTarget,
        PointerEventExtTrait as PointerEventExt,
    };
    pub use crate::{
        ids::{DeviceId, FocusNodeId, HandlerId, PointerId, RegionId},
        recognizers::{
            DoubleTapGestureRecognizer, DragGestureRecognizer, ForcePressGestureRecognizer,
            LongPressGestureRecognizer, MultiTapGestureRecognizer, ScaleGestureRecognizer,
            TapGestureRecognizer, double_tap::*, drag::*, force_press::*, long_press::*,
            multi_tap::*, scale::*, tap::*,
        },
        signal_resolver::{PointerSignalResolver, SignalPriority},
    };
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
