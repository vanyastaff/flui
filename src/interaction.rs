//! Gesture, focus, keyboard, hit-test, and text-input capabilities for widgets.
//!
//! Ordinary handlers remain on [`crate::widgets::GestureDetector`]. Use these
//! contracts when naming callback payloads or integrating a recognizer with
//! the presentation's [`crate::widgets::GestureArenaScope`]. Lifecycle hooks can
//! also retain the focus, hit-test, and text-input handles provided by
//! [`crate::view::LifecycleContext`], using the callback and result types below.
//! Owners and backend adapter construction remain internal to the runtime.
//!
//! Phase payloads name the callback that delivers them:
//!
//! ```rust
//! use flui::interaction::{
//!     GestureArena, TapGestureRecognizer, TapDownDetails, TapUpDetails,
//!     LongPressGestureRecognizer, LongPressMoveUpdateDetails, LongPressEndDetails,
//! };
//!
//! let arena = GestureArena::new();
//! let tap = TapGestureRecognizer::builder(arena.clone())
//!     .on_tap_down(|details: TapDownDetails| { let _ = details.local_position; })
//!     .on_tap(|details: TapUpDetails| { let _ = details.global_position; })
//!     .build();
//! let press = LongPressGestureRecognizer::builder(arena)
//!     .on_long_press_move_update(|details: LongPressMoveUpdateDetails| {
//!         let _ = details.local_offset_from_origin;
//!     })
//!     .on_long_press_end(|details: LongPressEndDetails| { let _ = details.velocity; })
//!     .build();
//! ```
//!
//! The owning layer's internal runtime bridge is not part of this facade:
//!
//! ```compile_fail,E0432
//! use flui::interaction::__runtime::CloseMode;
//! ```

pub use flui_interaction::arena::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureDisposition, SweepModel,
};
pub use flui_interaction::events::{
    CursorIcon, PointerButtons, PointerEvent, PointerEventExt, PointerKind,
};
pub use flui_interaction::recognizers::double_tap::DoubleTapDetails;
pub use flui_interaction::recognizers::multi_tap::MultiTapDetails;
pub use flui_interaction::recognizers::scale::{
    ScaleEndDetails, ScaleStartDetails, ScaleUpdateDetails,
};
pub use flui_interaction::recognizers::{
    ArenaMembership, BeginContactError, CancelOutcome, ContactId, ContactSnapshot,
    GestureRecognizerState, PrimaryContact, RecognizerSet, cancel_all,
};
pub use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragDownDetails, DragEndDetails, DragGestureRecognizer,
    DragStartDetails, DragUpdateDetails, EagerGestureRecognizer, ForcePressGestureRecognizer,
    GestureEndReason, GestureRecognizer, GestureSettings, GestureSettingsError,
    HorizontalDragGestureRecognizer, LongPressGestureRecognizer, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragUpdateDetails,
    MultiTapGestureRecognizer, PanGestureRecognizer, PointerId, ScaleGestureRecognizer,
    TapAndDragGestureRecognizer, TapDragDownDetails, TapDragEndDetails, TapDragStartDetails,
    TapDragUpDetails, TapDragUpdateDetails, TapGestureRecognizer, VerticalDragGestureRecognizer,
};
pub use flui_interaction::{
    ForcePressDetails, LongPressCallback, LongPressDetails, LongPressDownCallback,
    LongPressDownDetails, LongPressEndCallback, LongPressEndDetails, LongPressMoveUpdateCallback,
    LongPressMoveUpdateDetails, LongPressSimpleCallback, LongPressStartCallback,
    LongPressStartDetails, TapCallback, TapDetails, TapDownCallback, TapDownDetails, TapUpCallback,
    TapUpDetails, Velocity, VelocityEstimate,
};
pub use flui_platform_api::pointer::{
    ButtonChange, CancelReason, DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool,
    PointerButton, PointerInfo, PointerMove, PointerPosition, PointerPress, PointerRelease,
    PointerRole, PointerSample, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit,
};

pub use flui_interaction::events::KeyEvent;
pub use flui_interaction::events::keyboard::{Code, Key, KeyState, Location, Modifiers, NamedKey};
pub use flui_interaction::routing::{
    FocusAttachment, FocusChangeCallback, FocusDetachOutcome, FocusDirection, FocusManager,
    FocusNode, FocusNodeChangeCallback, FocusNodeId, FocusNodeRegistration, FocusRequestOutcome,
    FocusScopeNode, FocusSubscription, FocusTraversalOverrides, FocusTraversalPolicy,
    FocusTreeError, GlobalKeyRegistration, GlobalKeyRegistrationError, HitTestEntry, HitTestHandle,
    HitTestSnapshot, InteractionDispatchError, KeyEventHandler, KeyEventResult, PointerCapture,
    PointerCaptureError, PointerDispatch, ReadingOrderPolicy, RectProvider, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior,
};
pub use flui_interaction::text_input::{
    ClientToken, DetachOutcome, TextInputClient, TextInputError, TextInputHandle,
};
