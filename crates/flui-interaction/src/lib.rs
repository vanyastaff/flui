//! Event routing, interaction handling, and gesture recognition for FLUI
//!
//! This crate provides the complete event handling and gesture infrastructure
//! for FLUI:
//!
//! - **GestureBinding**: Routes presentation-owned pointer streams via hit testing
//! - **HitTest**: Determines which UI elements are under cursor/touch
//! - **FocusManager**: Manages keyboard focus for one presentation
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
//! - **Open gesture traits**: external recognizers implement the same
//!   dyn-compatible arbitration and event-delivery contracts as built-ins
//! - **Canonical pointer id**: [`PointerId`] is re-exported from the
//!   `flui-platform-api` crate (`NonZeroU64`-backed). [`FocusNodeId`] is a
//!   crate-local `NonZeroU64` newtype that prevents
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
//! PointerEvent/KeyEvent
//!     ↓
//! GestureBinding (pointers) / FocusManager (keyboard)
//!     ├─ Hit Testing → InteractionLane route
//!     └─ RecognizerSet attachments
//!         ↓
//! Owner-local handlers (render storage keeps data-only route identities)
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
//! use flui_interaction::gesture::{GestureArena, TapGestureRecognizer};
//!
//! // 1. The recogniser set lives behind a single shared `GestureArena`.
//! let arena = GestureArena::new();
//!
//! // 2. Configure callbacks before sharing the recognizer as an `Rc`.
//! let recognizer = TapGestureRecognizer::builder(arena)
//!     .on_tap(|details| {
//!         // The user callback fires only after the arena confirms
//!         // this recogniser won (`pending_up` deferral).
//!         let _pos = details.global_position;
//!     })
//!     .build();
//! // `recognizer` is now ready to receive pointer events via
//! // `flui_interaction::GestureBinding` at runtime.
//! ```
//!
//! # Example: Keyboard Focus
//!
//! ```rust
//! use flui_interaction::{FocusManager, FocusNode};
//!
//! let manager = FocusManager::new();
//! let node = FocusNode::new();
//! let attachment = manager.root_scope().attach_node(&node)?;
//!
//! // Request focus
//! let _ = node.request_focus();
//!
//! // Check focus
//! if node.has_primary_focus() {
//!     println!("We have focus!");
//! }
//! # Ok::<(), flui_interaction::FocusTreeError>(())
//! ```
//!
//! # Example: Type-Safe IDs
//!
//! ```compile_fail
//! use flui_interaction::{PointerId, FocusNode};
//! fn process_pointer(id: PointerId) {}
//! process_pointer(FocusNode::new().id()); // A focus identity cannot name a pointer.
//! ```
//!
//! # Modules
//!
//! The modules form layers (ADR-0217); a module names only modules below it,
//! which `cargo xtask module-dag` checks against the manifest's
//! `[package.metadata.flui.modules]`:
//!
//! 1. [`events`] - The owned input vocabulary; [`ids`] - Type-safe identifiers
//! 2. [`pointer`](mod@pointer) - Hit testing, routing, capture, hover tracking and resampling;
//!    [`focus`] - Keyboard focus and traversal; [`text_input`] - IME and clipboard
//! 3. [`gesture`] - The arena, the recognizers, their settings and details, and
//!    velocity estimation
//! 4. [`binding`] - [`GestureBinding`], which composes them for one presentation
//!
//! `testing` - Synthetic event builders (requires `testing` feature). Gesture
//! scripting and replay live in `flui-testing`, which owns the virtual clock.
//!
//! # Separation from Rendering
//!
//! This crate is deliberately separate from `flui_engine` (rendering):
//! - ✅ Can test event logic without GPU
//! - ✅ Can use rendering without event handling (headless)
//! - ✅ Clear separation of concerns (SOLID principles)
//! - ✅ Smaller compile times and dependencies

// Public items keep their contract documentation at the API boundary.
#![deny(missing_docs)]

// Layer 0: values, identities and containment retain values.
pub mod events;
pub mod ids;
mod retain;

// Layer 1: close policy and containment.
mod close;
mod containment;

// Layer 2: pointer delivery, keyboard focus, text input.
pub mod focus;
pub mod pointer;
pub mod text_input;

// Layer 3: gesture recognition.
pub mod gesture;

// Layer 4: the per-presentation composition root.
pub mod binding;

// Layer 5, above the binding: the runtime's terminal-cleanup seam, and test builders.
#[doc(hidden)]
pub mod __runtime;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use pointer::{WheelPreferencesProvider, WheelPreferencesSource};

// ============================================================================
// Re-exports: Gesture Recognition
// ============================================================================
pub use gesture::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureDeadlineError,
    GestureDeadlineRegistration, GestureDeadlineUpdate, GestureDisposition, SweepModel,
};
// ============================================================================
// Re-exports: Other
// ============================================================================
pub use binding::{
    ContactlessSequence, ContactlessSource, DispatchOutcome, GestureBinding, InvalidSamplingWindow,
    ResamplingModeChangeError,
};
pub use flui_foundation::{IdExhausted, InputSequenceId};
pub use gesture::{
    ForcePressDetails, LongPressDetails, LongPressDownDetails, LongPressEndDetails,
    LongPressMoveUpdateDetails, LongPressStartDetails, TapDetails, TapDownDetails, TapUpDetails,
};
pub use text_input::ClipboardHandle;
// The monotonic clock primitive now lives in `flui-foundation`; re-exported here
// because the gesture arena's public API takes a `MonotonicClock` (and tests /
// the headless binding construct `ManualClock`/`SystemClock` against the arena).
pub use flui_foundation::{ManualClock, MonotonicClock, SystemClock};
// ============================================================================
// Re-exports: Owned input contracts
// ============================================================================

// Re-export commonly used event types at crate root
pub use events::{CursorIcon, KeyEvent, PointerEvent, PointerEventExt};
// Re-export observability surface — typed event names + span constants.
pub use flui_platform_api::pointer::{
    ButtonChange, CancelReason, PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool,
    PointerButton, PointerButtons, PointerInfo, PointerKind, PointerMove, PointerPosition,
    PointerPress, PointerRelease, PointerRole, PointerSample, ScrollDelta, ScrollEvent,
    ScrollPhase, ScrollPrecision, ScrollUnit,
};
pub use gesture::{GestureEvent, SPAN_ARENA, SPAN_RECOGNIZER, pointer_event_kind};
pub use gesture::{GestureObservation, GestureObserver, ObserverHandle};
// ============================================================================
// Re-exports: geometry from flui_foundation
// ============================================================================
pub use flui_foundation::geometry::{Offset, Rect};
pub use flui_platform_api::ImeEvent;
/// The device kinds a recognizer admits ([`GestureRecognizer::allowed_devices`]).
pub use flui_platform_api::input_capabilities::PointerKindSet;
pub use ids::{
    DeviceId, EmbeddedSurfaceId, FocusNodeId, InputSurface, PointerId, SurfaceIdsExhausted,
};
// ============================================================================
// Re-exports: Input Processing
// ============================================================================
pub use gesture::{
    ArenaMembership, BeginContactError, CancelOutcome, ContactId, ContactSnapshot,
    DoubleTapDetails, DoubleTapGestureRecognizer, DragCancelCallback, DragDownCallback,
    DragDownDetails, DragEndCallback, DragEndDetails, DragGestureRecognizer, DragPointerStrategy,
    DragStartCallback, DragStartDetails, DragUpdateCallback, DragUpdateDetails,
    EagerGestureRecognizer, ForcePressGestureRecognizer, GestureEndReason, GestureRecognizer,
    HoldDetails, HoldEnd, HoldGestureRecognizer, HoldGestureRecognizerBuilder, LongPressCallback,
    LongPressDownCallback, LongPressEndCallback, LongPressGestureRecognizer,
    LongPressMoveUpdateCallback, LongPressSimpleCallback, LongPressStartCallback, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragStartCallback,
    MultiDragUpdateDetails, MultiTapGestureRecognizer, PhysicalEdge, PrimaryContact, RecognizerSet,
    ScaleGestureRecognizer, SwipeDetails, SwipeGestureRecognizer, SwipeGestureRecognizerBuilder,
    SwipeStart, TapAndDragGestureRecognizer, TapCallback, TapDownCallback, TapDragDownCallback,
    TapDragDownDetails, TapDragEndCallback, TapDragEndDetails, TapDragStartCallback,
    TapDragStartDetails, TapDragUpCallback, TapDragUpDetails, TapDragUpdateCallback,
    TapDragUpdateDetails, TapGestureRecognizer, TapUpCallback, cancel_all,
};
pub use gesture::{
    DoubleTapGestureRecognizerBuilder, DragGestureRecognizerBuilder, EagerGestureRecognizerBuilder,
    ForcePressGestureRecognizerBuilder, LongPressGestureRecognizerBuilder,
    MultiDragGestureRecognizerBuilder, MultiTapGestureRecognizerBuilder,
    ScaleGestureRecognizerBuilder, TapAndDragGestureRecognizerBuilder, TapGestureRecognizerBuilder,
};
pub use gesture::{Velocity, VelocityEstimate, VelocityTracker};
pub use pointer::PointerEventResampler;
// Re-exports for the drag axis sub-recognisers (vertical, horizontal, pan).
// Aliased to `DragGestureRecognizer` so a recogniser's axis is fixed at the
// type level.
pub use gesture::{
    HorizontalDragGestureRecognizer, PanGestureRecognizer, VerticalDragGestureRecognizer,
};
// ============================================================================
// Re-exports: Event Routing
// ============================================================================
pub use focus::{
    EntryTarget, FocusAttachment, FocusCause, FocusChangeCallback, FocusDetachOutcome,
    FocusDirection, FocusManager, FocusNode, FocusNodeChangeCallback, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusSubscription, FocusTraversalOverrides,
    FocusTraversalPolicy, FocusTreeError, GlobalKeyRegistration, GlobalKeyRegistrationError,
    KeyEventHandler, KeyEventResult, NodeContext, ReadingOrderPolicy, RectProvider, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior, TraversalEntry,
};
pub use gesture::{
    DEFAULT_DOUBLE_TAP_SLOP, DEFAULT_DOUBLE_TAP_TIMEOUT, DEFAULT_LONG_PRESS_TIMEOUT,
    DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MIN_FLING_VELOCITY, DEFAULT_MOUSE_PAN_SLOP,
    DEFAULT_MOUSE_SLOP, DEFAULT_PAN_SLOP, DEFAULT_PAN_SLOP_HORIZONTAL, DEFAULT_PAN_SLOP_VERTICAL,
    DEFAULT_PEN_SLOP, DEFAULT_SCALE_SLOP, DEFAULT_TOUCH_SLOP, GestureProfiles, GestureSettings,
    GestureSettingsError, GestureSettingsProvider, GestureSettingsSource, GestureValues,
    PointerClass, RemainderHandoff,
};
pub use pointer::{
    EventPropagation, GlobalPointerHandler, HitTestBehavior, HitTestEntry, HitTestHandle,
    HitTestProbe, HitTestResult, HitTestSnapshot, InteractionDispatchError,
    InteractionDispatchHandle, InteractionLane, LocalPayloadTarget, PathClipTarget, PointerCapture,
    PointerCaptureError, PointerDispatch, PointerRouteHandler, PointerRouter, PointerTarget,
    RenderId, ResolvedRouteToken, RouteResolution, RouteResolutionMiss, ScrollTarget,
    ShaderMaskTarget, TransformGuard, resolve_local_payload, resolve_path_clip_target,
    resolve_shader_mask_target,
};
pub use text_input::{
    ClientToken, DetachOutcome, TextInputBackend, TextInputClient, TextInputError, TextInputHandle,
    TextInputOwner,
};
// ============================================================================
// Re-exports: Testing Utilities (feature-gated)
// ============================================================================
#[cfg(any(test, feature = "testing"))]
pub use testing::ModifiersBuilder;
// ============================================================================
// Re-exports: Traits
// ============================================================================
pub use gesture::{AxisLock, DragAxis, TransformLock};

// ============================================================================
// Prelude
// ============================================================================

/// Prelude module with commonly used types and traits.
///
/// # Usage
///
/// ```rust
/// use flui_interaction::prelude::*;
/// ```
pub mod prelude {
    // IDs
    // Geometry from flui_foundation
    pub use flui_foundation::geometry::{Offset, Rect};

    // Gesture recognition: the arena, recognizers, details, velocity
    pub use crate::gesture::*;
    // Owned input contracts
    pub use crate::events::{CursorIcon, KeyEvent, PointerEvent, PointerEventExt};
    pub use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase, PointerKind};
    // Pointer delivery and hover tracking
    pub use crate::pointer::{
        EventPropagation, HitTestBehavior, HitTestEntry, HitTestResult, MouseTracker,
        MouseTrackerAnnotation, PointerEventResampler, PointerMotionKind, PointerRouter, RenderId,
        TransformGuard,
    };
    // Keyboard focus
    pub use crate::focus::FocusManager;
    // Testing (feature-gated)
    pub use crate::ids::{DeviceId, FocusNodeId, PointerId, RegionId};
    #[cfg(any(test, feature = "testing"))]
    pub use crate::testing::ModifiersBuilder;
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
    #[expect(dead_code)]
    trait AssertSendSync: Send + Sync {}

    // IDs should be Send + Sync (they are Copy)
    impl AssertSendSync for PointerId {}
    impl AssertSendSync for FocusNodeId {}
    impl AssertSendSync for ScrollTarget {}
    impl AssertSendSync for PathClipTarget {}
    impl AssertSendSync for ShaderMaskTarget {}
    impl AssertSendSync for LocalPayloadTarget {}

    // Data-path types should be Send + Sync
    impl AssertSendSync for HitTestResult {}
    impl AssertSendSync for HitTestEntry {}
    impl AssertSendSync for PointerEventResampler {}
}
