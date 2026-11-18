//! Input for one FLUI presentation: pointer routing, keyboard focus, text
//! input, gesture recognition, drag and drop and haptics.
//!
//! The crate sits below the element tree and above `flui-platform`, and has
//! no GPU or host edge: everything here runs headless, which is how its tests
//! drive it.
//!
//! - [`GestureBinding`] takes a presentation's pointer stream, hit-tests each
//!   new contact through an [`InteractionLane`](pointer::InteractionLane) and
//!   delivers the sequence along that route.
//! - [`FocusManager`] owns a presentation's focus tree: [`FocusNode`]s attached
//!   under [`FocusScopeNode`]s, with traversal order from a
//!   [`FocusTraversalPolicy`](focus::FocusTraversalPolicy).
//! - The [`gesture`] recognizers compete in a [`GestureArena`](gesture::GestureArena),
//!   which grants each pointer to one winner; a recognizer outside this crate
//!   implements the same traits as the built-in ones.
//!
//! Handlers and recognizers live on the owner thread behind `Rc`; what render
//! storage keeps of a route is data-only identity, resolved back to the
//! handlers when a sequence is delivered (ADR-0027).
//!
//! ```text
//! platform input (PointerEvent, KeyEvent)
//!   ├─ GestureBinding ── hit test ── InteractionLane route ── handlers
//!   │                                      └─ recognizers ── GestureArena
//!   └─ FocusManager ── focused path ── key handlers
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
//! The modules form layers (ADR-0217); a module names only modules below it:
//!
//! 0. [`events`] - The owned input vocabulary; [`ids`] - Type-safe identifiers
//! 1. Close policy and panic containment (private)
//! 2. [`pointer`](mod@pointer) - Hit testing, routing, capture, hover tracking and resampling;
//!    [`focus`] - Keyboard focus and traversal; [`text_input`] - IME and clipboard
//! 3. [`gesture`] - The arena, the recognizers, their settings and details, and
//!    velocity estimation; [`drag`] - Drag-and-drop sessions
//! 4. [`binding`] - [`GestureBinding`], which composes them for one presentation
//! 5. [`haptics`] - A presentation's haptics, on the binding's clock and dispatch
//! 6. `testing` - Synthetic event builders (requires `testing` feature). Gesture
//!    scripting and replay live in `flui-testing`, which owns the virtual clock.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Public items keep their contract documentation at the API boundary.
#![deny(missing_docs)]

// Layer 0: values and identities.
pub mod events;
pub mod ids;

// Layer 1: close policy and containment.
mod close;
mod containment;

// Layer 2: pointer delivery, keyboard focus, text input.
pub mod focus;
pub mod pointer;
pub mod text_input;

// Layer 3: gesture recognition.
pub mod gesture;

// Layer 3: drag-and-drop sessions, over pointer capture and hit testing.
pub mod drag;

// Layer 4: the per-presentation composition root.
pub mod binding;

// Layer 5: a presentation's haptics, on the binding's clock and dispatch.
pub mod haptics;

// Layer 6, above the binding: the runtime's terminal-cleanup seam, and test builders.
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
    GestureDeadlineRegistration, GestureDeadlineUpdate, GestureDisposition, ScrollLatchEnd,
    SweepModel,
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
pub use haptics::{HapticCut, HapticObserver, HapticsHandle, HapticsHost, HapticsHostBuilder};
pub use text_input::{
    ClipboardBackend, ClipboardCapability, ClipboardHandle, PrimaryClaim, PrimaryClaimId,
    PrimaryClaims, PrimarySelectionBackend, PrimarySelectionCapability, PrimarySelectionHandle,
    SelectionText,
};
// The gesture arena's public API takes a `MonotonicClock` from `flui-foundation`.
pub use flui_foundation::{ManualClock, MonotonicClock, SystemClock};
// ============================================================================
// Re-exports: Owned input contracts
// ============================================================================

// Re-export commonly used event types at crate root
pub use events::{CursorIcon, KeyEvent, PointerEvent, PointerEventExt};
// Re-export observability surface — typed event names + span constants.
pub use flui_platform::pointer::{
    ButtonChange, CancelReason, PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool,
    PointerButton, PointerButtons, PointerKind, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, PointerSource, ScrollDelta, ScrollEvent,
    ScrollPhase, ScrollPrecision, ScrollUnit, SmartZoomEvent, SwipeOffset, TouchpadHoldEvent,
    TouchpadHoldPhase, TouchpadSwipeEvent, TouchpadSwipePhase,
};
pub use gesture::{GestureContest, GestureObservation, GestureObserver, ObserverHandle};
pub use gesture::{GestureEvent, SPAN_ARENA, SPAN_RECOGNIZER, pointer_event_kind};
// ============================================================================
// Re-exports: geometry from flui_foundation
// ============================================================================
pub use flui_foundation::geometry::{Offset, Rect};
pub use flui_platform::ImeEvent;
/// The device kinds a recognizer admits ([`GestureRecognizer::allowed_devices`]).
pub use flui_platform::input_capabilities::PointerKindSet;
pub use ids::{
    DeviceId, EmbeddedSurfaceId, FocusNodeId, InputSurface, PointerId, SurfaceIdsExhausted,
};
// ============================================================================
// Re-exports: Input Processing
// ============================================================================
pub use gesture::{
    ArenaMembership, BeginContactError, CancelOutcome, ContactEnd, ContactId, ContactSnapshot,
    DoubleTapDetails, DoubleTapGestureRecognizer, DragCancelCallback, DragDownCallback,
    DragDownDetails, DragEndCallback, DragEndDetails, DragGestureRecognizer, DragPointerStrategy,
    DragStartCallback, DragStartDetails, DragUpdateCallback, DragUpdateDetails,
    ForcePressGestureRecognizer, GestureEndReason, GestureRecognizer, HoldDetails, HoldEnd,
    HoldGestureRecognizer, HoldGestureRecognizerBuilder, LongPressCallback, LongPressDownCallback,
    LongPressEndCallback, LongPressGestureRecognizer, LongPressMoveUpdateCallback,
    LongPressSimpleCallback, LongPressStartCallback, MultiDragAxis, MultiDragEndDetails,
    MultiDragGestureRecognizer, MultiDragHandle, MultiDragStartCallback, MultiDragUpdateDetails,
    MultiTapCount, MultiTapCountError, MultiTapGestureRecognizer, PhysicalEdge, PrimaryContact,
    RecognizerSet, ScaleGestureRecognizer, SmartZoomDetails, SmartZoomSource, SwipeDetails,
    SwipeGestureRecognizer, SwipeGestureRecognizerBuilder, SwipeStart, TapAndDragGestureRecognizer,
    TapCallback, TapDownCallback, TapDragDownCallback, TapDragDownDetails, TapDragEndCallback,
    TapDragEndDetails, TapDragStartCallback, TapDragStartDetails, TapDragUpCallback,
    TapDragUpDetails, TapDragUpdateCallback, TapDragUpdateDetails, TapGestureRecognizer,
    TapUpCallback, cancel_all,
};
pub use gesture::{
    DoubleTapGestureRecognizerBuilder, DragGestureRecognizerBuilder,
    ForcePressGestureRecognizerBuilder, LongPressGestureRecognizerBuilder,
    MultiDragGestureRecognizerBuilder, MultiTapGestureRecognizerBuilder,
    ScaleGestureRecognizerBuilder, TapAndDragGestureRecognizerBuilder, TapGestureRecognizerBuilder,
};
pub use gesture::{Velocity, VelocityEstimate, VelocityTracker};
pub use pointer::PointerEventResampler;
// ============================================================================
// Re-exports: Event Routing
// ============================================================================
pub use focus::{
    EntryTarget, FocusAttachment, FocusCause, FocusChangeCallback, FocusDetachOutcome,
    FocusDirection, FocusManager, FocusNode, FocusNodeChangeCallback, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusTraversalOverrides, FocusTraversalPolicy,
    FocusTreeError, GlobalKeyRegistration, GlobalKeyRegistrationError, KeyCancelHandler,
    KeyEventHandler, KeyEventResult, NodeContext, ReadingOrderPolicy, RectProvider, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior, TraversalEntry,
};
pub use gesture::{
    DEFAULT_DOUBLE_TAP_SLOP, DEFAULT_DOUBLE_TAP_TIMEOUT, DEFAULT_LONG_PRESS_TIMEOUT,
    DEFAULT_MAX_FLING_VELOCITY, DEFAULT_MIN_FLING_VELOCITY, DEFAULT_MOUSE_PAN_SLOP,
    DEFAULT_MOUSE_SLOP, DEFAULT_PAN_SLOP, DEFAULT_PAN_SLOP_HORIZONTAL, DEFAULT_PAN_SLOP_VERTICAL,
    DEFAULT_PEN_SLOP, DEFAULT_SCALE_SLOP, DEFAULT_TOUCH_SLOP, GestureProfiles, GestureSettings,
    GestureSettingsError, GestureSettingsProvider, GestureSettingsSource, GestureValues,
    RemainderHandoff,
};
pub use pointer::{
    EventPropagation, GlobalPointerHandler, HitMetadata, HitTestBehavior, HitTestEntry,
    HitTestHandle, HitTestProbe, HitTestResult, HitTestSnapshot, InteractionDispatchError,
    InteractionDispatchHandle, InteractionLane, LocalPayloadTarget, PathClipTarget, PointerCapture,
    PointerCaptureError, PointerDispatch, PointerRouteHandler, PointerRouter, PointerTarget,
    RenderId, ResolvedRouteToken, RouteResolution, RouteResolutionMiss, ScrollTarget,
    ShaderMaskTarget, SmartZoomTarget, TransformGuard, resolve_local_payload,
    resolve_path_clip_target, resolve_shader_mask_target,
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
    pub use flui_platform::pointer::{PanZoomEvent, PanZoomPhase, PointerKind};
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
    impl AssertSendSync for SmartZoomTarget {}
    impl AssertSendSync for PathClipTarget {}
    impl AssertSendSync for ShaderMaskTarget {}
    impl AssertSendSync for LocalPayloadTarget {}

    impl AssertSendSync for PointerEventResampler {}
}
