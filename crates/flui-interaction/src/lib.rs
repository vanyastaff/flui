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
//! # Example: Construct a recogniser and register a tap callback
//!
//! ```rust
//! use flui_interaction::arena::GestureArena;
//! use flui_interaction::recognizers::TapGestureRecognizer;
//!
//! // 1. The recogniser set lives behind a single shared `GestureArena`.
//! let arena = GestureArena::new();
//!
//! // 2. Construct the recogniser; the builder returns an `Arc<Self>`.
//! let recognizer = TapGestureRecognizer::new(arena)
//!     .with_on_tap(|details| {
//!         // The user callback fires only after the arena confirms
//!         // this recogniser won (`pending_up` deferral).
//!         let _pos = details.global_position;
//!     });
//! // `recognizer` is now ready to receive pointer events via
//! // `flui_interaction::GestureBinding` at runtime.
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
//! - [`MouseTracker`] — Mouse enter/exit/hover tracking
//! - [`PointerSignalResolver`] — Pointer signal conflict resolution
//!
//! # Separation from Rendering
//!
//! This crate is deliberately separate from `flui_engine` (rendering):
//! - ✅ Can test event logic without GPU
//! - ✅ Can use rendering without event handling (headless)
//! - ✅ Clear separation of concerns (SOLID principles)
//! - ✅ Smaller compile times and dependencies

// Ship bar (wave 2): every public item is documented; keep it that way.
#![deny(missing_docs)]
// ADR-0027: gesture arenas and recognizers are owner-local, but this crate still
// exposes `Arc`-shaped handles at the arena/member seams. Do not restore
// `Send + Sync` to executable callbacks to satisfy this lint; a future focused
// pass can migrate the owner-local handle graph to `Rc`.
#![allow(clippy::arc_with_non_send_sync)]

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
pub mod timer;

// ============================================================================
// Input processing
// ============================================================================

pub mod processing;

// ============================================================================
// Testing utilities — gated behind `testing` Cargo feature
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
pub mod observability;
pub mod pan_zoom;
pub mod settings;

// ============================================================================
// Re-exports: IDs
// ============================================================================

// ============================================================================
// Re-exports: Gesture Recognition
// ============================================================================
pub use arena::{
    DEFAULT_DISAMBIGUATION_TIMEOUT, GestureArena, GestureArenaEntry, GestureArenaMember,
    GestureArenaTeam, GestureDisposition, PointerSignalResolver, SignalPriority, SweepModel,
    TeamEntry, run_pointer_lifecycle,
};
// ============================================================================
// Re-exports: Other
// ============================================================================
pub use binding::GestureBinding;
// The monotonic clock primitive now lives in `flui-foundation`; re-exported here
// because the gesture arena's public API takes a `MonotonicClock` (and tests /
// the headless binding construct `ManualClock`/`SystemClock` against the arena).
pub use flui_foundation::{ManualClock, MonotonicClock, SystemClock};
// ============================================================================
// Re-exports: Events (W3C-compliant types)
// ============================================================================

// Re-export commonly used event types at crate root
pub use events::{CursorIcon, KeyboardEvent, PointerEvent};
// Re-export observability surface — typed event names + span constants.
pub use observability::{GestureEvent, SPAN_ARENA, SPAN_RECOGNIZER, pointer_event_kind};
// Trackpad pan/zoom module — canonical public entry point for the
// Flutter-aligned `PointerPanZoomEvent` type and its W3C conversion helpers
// (`from_w3c_event`, `convert_gesture`). Re-exported at the crate root so
// `use flui_interaction::PointerPanZoomEvent` is the single import path.
pub use pan_zoom::{PointerPanZoomEvent, convert_gesture, from_w3c_event};
// ============================================================================
// Re-exports: Geometry from flui_types
// ============================================================================
pub use flui_types::geometry::{Offset, Rect};
pub use ids::{FocusNodeId, HandlerId, PointerId};
// Back-compat re-exports — `MouseTracker` lives in `routing` now, but was at
// the crate root before the move (PR 163). External crates (`flui-rendering`,
// `flui-app`) import it via `flui_interaction::MouseTracker`; the routing
// path remains the canonical one for new code.
pub use routing::{
    CursorChangeCallback, MouseEnterCallback, MouseExitCallback, MouseHoverCallback, MouseTracker,
    MouseTrackerAnnotation,
};
// ============================================================================
// Re-exports: Input Processing
// ============================================================================
pub use processing::{
    InputMode, InputPredictor, PointerEventResampler, PredictedPosition, PredictionConfig,
    RawInputHandler, RawPointerEvent, Velocity, VelocityEstimate, VelocityTracker,
};
pub use recognizers::{
    DoubleTapGestureRecognizer, DragCancelCallback, DragDownCallback, DragDownDetails,
    DragEndCallback, DragEndDetails, DragGestureRecognizer, DragStartCallback, DragStartDetails,
    DragUpdateCallback, DragUpdateDetails, EagerGestureRecognizer, ForcePressGestureRecognizer,
    GestureRecognizer, LongPressGestureRecognizer, MultiDragAxis, MultiDragEndDetails,
    MultiDragGestureRecognizer, MultiDragHandle, MultiDragStartCallback, MultiDragUpdateDetails,
    MultiTapGestureRecognizer, ScaleGestureRecognizer, TapAndDragGestureRecognizer,
    TapDragDownCallback, TapDragDownDetails, TapDragEndCallback, TapDragEndDetails,
    TapDragStartCallback, TapDragStartDetails, TapDragUpCallback, TapDragUpDetails,
    TapDragUpdateCallback, TapDragUpdateDetails, TapGestureRecognizer,
};
// Re-exports for drag axis sub-recognisers (Flutter parity for
// `VerticalDragGestureRecognizer` / `HorizontalDragGestureRecognizer` /
// `PanGestureRecognizer`). Aliased to `DragGestureRecognizer` so a
// recogniser's axis is fixed at the type level.
pub use recognizers::drag_variants::{
    HorizontalDragGestureRecognizer, PanGestureRecognizer, VerticalDragGestureRecognizer,
};
// ============================================================================
// Re-exports: Event Routing
// ============================================================================
pub use routing::{
    EventPropagation, EventRouter, FocusManager, FocusNode, FocusScopeNode, FocusTraversalPolicy,
    GlobalPointerHandler, HitTestBehavior, HitTestEntry, HitTestResult, HitTestable,
    InteractionDispatchError, InteractionDispatchHandle, InteractionLane, KeyEventCallback,
    KeyEventHandler, KeyEventResult, MouseRegionCallbacks, MouseRegionTarget, PathClipTarget,
    PointerRouteHandler, PointerRouter, PointerTarget, ReadingOrderPolicy, RectProvider, RenderId,
    ResolvedRouteToken, ResolvedStep, RoutePanic, RouteResolution, RouteResolutionMiss,
    ScrollTarget, TransformGuard, TraversalEdgeBehavior, resolve_path_clip_target,
};
pub use sealed::{CustomGestureRecognizer, CustomHitTestable};
pub use settings::{
    DEFAULT_DOUBLE_TAP_SLOP, DEFAULT_DOUBLE_TAP_TIMEOUT, DEFAULT_LONG_PRESS_TIMEOUT,
    DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MIN_FLING_VELOCITY, DEFAULT_MOUSE_SLOP, DEFAULT_PAN_SLOP,
    DEFAULT_PAN_SLOP_HORIZONTAL, DEFAULT_PAN_SLOP_VERTICAL, DEFAULT_PEN_SLOP, DEFAULT_SCALE_SLOP,
    DEFAULT_TOUCH_SLOP, GestureSettings,
};
// ============================================================================
// Re-exports: Testing Utilities (feature-gated)
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
    pub use crate::routing::{MouseTracker, MouseTrackerAnnotation};
    // Input processing
    pub use crate::processing::{InputPredictor, PointerEventResampler, Velocity, VelocityTracker};
    // Event routing
    pub use crate::routing::{
        EventPropagation, EventRouter, FocusManager, HitTestBehavior, HitTestEntry, HitTestResult,
        HitTestable, PointerRouter, RenderId, TransformGuard,
    };
    // Extension traits for custom types
    pub use crate::sealed::{CustomGestureRecognizer, CustomHitTestable};
    // Testing (feature-gated)
    #[cfg(any(test, feature = "testing"))]
    pub use crate::testing::{GestureBuilder, GesturePlayer, GestureRecorder};
    // Traits
    pub use crate::traits::{
        Disposable, DragAxis, GestureCallback, GestureRecognizerExt, HitTestTarget,
        PointerEventExtTrait as PointerEventExt,
    };
    pub use crate::{
        arena::{GestureArenaTeam, PointerSignalResolver, SignalPriority, TeamEntry},
        ids::{DeviceId, FocusNodeId, HandlerId, PointerId, RegionId},
        recognizers::{
            DoubleTapGestureRecognizer, DragGestureRecognizer, ForcePressGestureRecognizer,
            LongPressGestureRecognizer, MultiTapGestureRecognizer, ScaleGestureRecognizer,
            TapGestureRecognizer, double_tap::*, drag::*, force_press::*, long_press::*,
            multi_tap::*, scale::*, tap::*,
        },
    };
}

// ============================================================================
// Static Assertions: Send + Sync (data-plane only)
// ============================================================================

/// Compile-time assertions that identity/data types remain Send + Sync.
///
/// Gesture arenas, recognizers, and focus ownership are intentionally
/// owner-local under ADR-0027; executable gesture callbacks must not regain a
/// thread-safe bound through these assertions.
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
    impl AssertSendSync for ScrollTarget {}
    impl AssertSendSync for PathClipTarget {}

    // Data-path types should be Send + Sync
    impl AssertSendSync for HitTestResult {}
    impl AssertSendSync for HitTestEntry {}
    impl AssertSendSync for PointerEventResampler {}
}
