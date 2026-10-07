//! Gesture, focus, keyboard, hit-test, and text-input capabilities for widgets.
//!
//! Ordinary handlers remain on [`crate::widgets::GestureDetector`]. Use these
//! contracts when naming callback payloads or integrating a recognizer with
//! the presentation's [`crate::widgets::GestureArenaScope`]. Lifecycle hooks can
//! also retain the focus, hit-test, and text-input handles provided by
//! [`crate::view::LifecycleContext`], using the callback and result types below.
//! Owners and backend adapter construction remain internal to the runtime.
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
pub use flui_interaction::recognizers::long_press::{
    LongPressDetails, LongPressDownDetails, LongPressStartDetails,
};
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
    ForcePressDetails, LongPressEndDetails, LongPressMoveUpdateDetails, TapDownDetails,
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
    FocusAttachment, FocusChangeCallback, FocusDetachOutcome, FocusManager, FocusNode,
    FocusNodeChangeCallback, FocusNodeId, FocusNodeRegistration, FocusRequestOutcome,
    FocusScopeNode, FocusTraversalPolicy, FocusTreeError, HitTestEntry, HitTestHandle,
    HitTestSnapshot, InteractionDispatchError, KeyEventCallback, KeyEventHandler, KeyEventResult,
    ReadingOrderPolicy, RectProvider, ResolvedStep, TraversalEdgeBehavior,
};
pub use flui_interaction::text_input::{
    ClientToken, DetachOutcome, TextInputClient, TextInputError, TextInputHandle,
};
