//! Gesture, focus, keyboard, hit-test, and text-input capabilities for widgets.
//!
//! Ordinary handlers remain on [`crate::widgets::GestureDetector`]. Use these
//! contracts when naming callback payloads or integrating a recognizer with
//! the presentation's [`crate::widgets::GestureArenaScope`]. Lifecycle hooks can
//! also retain the focus, hit-test, and text-input handles provided by
//! [`crate::view::BuildContext`], using the callback and result types below.
//! Owners and backend adapter construction remain internal to the runtime.

pub use flui_interaction::arena::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureArenaTeam, GestureDisposition,
    PointerSignalResolver, SignalPriority, SweepModel, TeamEntry,
};
pub use flui_interaction::events::{
    CursorIcon, PointerButtons, PointerEvent, PointerEventExt, PointerType,
};
pub use flui_interaction::recognizers::double_tap::DoubleTapDetails;
pub use flui_interaction::recognizers::long_press::{
    LongPressDetails, LongPressDownDetails, LongPressStartDetails,
};
pub use flui_interaction::recognizers::multi_tap::MultiTapDetails;
pub use flui_interaction::recognizers::scale::{
    ScaleEndDetails, ScaleStartDetails, ScaleUpdateDetails,
};
pub use flui_interaction::recognizers::{GestureRecognizerState, RecognizerBase};
pub use flui_interaction::{
    CustomGestureRecognizer, DoubleTapGestureRecognizer, DragAxis, DragDownDetails, DragEndDetails,
    DragGestureRecognizer, DragStartDetails, DragUpdateDetails, EagerGestureRecognizer,
    ForcePressGestureRecognizer, GestureRecognizer, GestureRecognizerExt, GestureSettings,
    HorizontalDragGestureRecognizer, LongPressGestureRecognizer, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragUpdateDetails,
    MultiTapGestureRecognizer, PanGestureRecognizer, PointerId, PointerPanZoomEvent,
    ScaleGestureRecognizer, TapAndDragGestureRecognizer, TapDragDownDetails, TapDragEndDetails,
    TapDragStartDetails, TapDragUpDetails, TapDragUpdateDetails, TapGestureRecognizer,
    VerticalDragGestureRecognizer,
};
pub use flui_types::gestures::{
    ForcePressDetails, LongPressEndDetails, LongPressMoveUpdateDetails, TapDownDetails,
    TapUpDetails, Velocity, VelocityEstimate,
};

pub use flui_interaction::events::KeyEvent;
pub use flui_interaction::events::keyboard::{
    Code, Key, KeyState, KeyboardEvent, Location, Modifiers, NamedKey,
};
pub use flui_interaction::routing::{
    FocusAttachment, FocusChangeCallback, FocusDetachOutcome, FocusManager, FocusNode,
    FocusNodeChangeCallback, FocusNodeId, FocusNodeRegistration, FocusRequestOutcome,
    FocusScopeNode, FocusTraversalPolicy, FocusTreeError, HitTestEntry, HitTestHandle,
    HitTestSnapshot, InteractionDispatchError, KeyEventCallback, KeyEventHandler, KeyEventResult,
    ReadingOrderPolicy, RectProvider, ResolvedStep, TraversalEdgeBehavior,
};
pub use flui_interaction::text_input::{
    ClientToken, DetachOutcome, ImeEventCallback, TextInputError, TextInputHandle,
};
