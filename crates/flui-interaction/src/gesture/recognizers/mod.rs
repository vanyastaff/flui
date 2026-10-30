//! Gesture recognizers
//!
//! Recognizers analyze pointer event streams and detect specific gestures.
//!
//! # Architecture
//!
//! ```text
//! GestureArenaMember (trait)
//!     │
//!     └── GestureRecognizer (trait) - add_pointer, handle_event, cancel
//!             │
//!             └── Concrete Recognizers
//!                 ├── TapGestureRecognizer
//!                 ├── LongPressGestureRecognizer
//!                 ├── DoubleTapGestureRecognizer
//!                 ├── DragGestureRecognizer
//!                 ├── ScaleGestureRecognizer
//!                 └── ...
//! ```
//!
//! Arena membership and contact tracking are composed values. Both extension
//! traits use `&self` receivers and support heterogeneous trait objects.
//! Builders configure immutable callbacks before returning an `Rc` owner.
//! Explicit cancellation delivers cancellation and leaves the recognizer reusable;
//! last-owner destruction withdraws contacts silently. Arena membership and
//! listener attachments hold weak references rather than lifetime ownership.
//!
//! # Available Recognizers
//!
//! - [`TapGestureRecognizer`] - Single tap detection
//! - [`DoubleTapGestureRecognizer`] - Double tap detection
//! - [`LongPressGestureRecognizer`] - Long press detection
//! - [`DragGestureRecognizer`] - Drag/pan gesture detection
//! - [`ScaleGestureRecognizer`] - Pinch-to-zoom detection
//! - [`MultiTapGestureRecognizer`] - Multi-finger tap detection
//! - [`ForcePressGestureRecognizer`] - Force/pressure touch detection
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::{GestureArena, TapGestureRecognizer};
//!
//! let arena = GestureArena::new();
//! let recognizer = TapGestureRecognizer::builder(arena)
//!     .on_tap(|details| println!("Tapped at {:?}", details.local_position))
//!     .build();
//! ```

// Concrete recognizers
pub(crate) mod callback_containment;
pub(crate) mod contact;
pub(crate) mod double_tap;
pub(crate) mod drag;
pub(crate) mod drag_variants;
pub(crate) mod force_press;
pub(crate) mod hold;
pub(crate) mod long_press;
pub(crate) mod multi_tap;
pub(crate) mod multidrag;
pub(crate) mod recognizer;
pub(crate) mod scale;
pub(crate) mod set;
pub(crate) mod smart_zoom;
pub(crate) mod swipe;
pub(crate) mod tap;
pub(crate) mod tap_and_drag;

pub use contact::{
    ArenaMembership, BeginContactError, ContactEnd, ContactId, ContactSnapshot, PrimaryContact,
};
pub use double_tap::{
    DoubleTapCallback, DoubleTapDetails, DoubleTapGestureRecognizer,
    DoubleTapGestureRecognizerBuilder,
};
pub use drag::{
    DragCancelCallback, DragDownCallback, DragDownDetails, DragEndCallback, DragEndDetails,
    DragGestureRecognizer, DragGestureRecognizerBuilder, DragPointerStrategy, DragStartBehavior,
    DragStartCallback, DragStartDetails, DragUpdateCallback, DragUpdateDetails, GestureEndReason,
};
pub use drag_variants::{
    HorizontalDragGestureRecognizer, PanGestureRecognizer, VerticalDragGestureRecognizer,
    horizontal_drag, pan, vertical_drag,
};
pub use force_press::{
    FORCE_PRESS_PEAK_PRESSURE, FORCE_PRESS_START_PRESSURE, ForcePressEndCallback,
    ForcePressGestureRecognizer, ForcePressGestureRecognizerBuilder, ForcePressPeakCallback,
    ForcePressStartCallback, ForcePressUpdateCallback,
};
pub use hold::{HoldDetails, HoldEnd, HoldGestureRecognizer, HoldGestureRecognizerBuilder};
pub use long_press::{
    LongPressCallback, LongPressDetails, LongPressDownCallback, LongPressDownDetails,
    LongPressEndCallback, LongPressGestureRecognizer, LongPressGestureRecognizerBuilder,
    LongPressMoveUpdateCallback, LongPressSimpleCallback, LongPressStartCallback,
    LongPressStartDetails,
};
pub use multi_tap::{
    MultiTapCallback, MultiTapCount, MultiTapCountError, MultiTapDetails,
    MultiTapGestureRecognizer, MultiTapGestureRecognizerBuilder,
};
pub use multidrag::{
    MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer,
    MultiDragGestureRecognizerBuilder, MultiDragHandle, MultiDragStartCallback,
    MultiDragUpdateDetails,
};
pub use recognizer::{CancelOutcome, GestureRecognizer, GestureRecognizerState, cancel_all};
pub use scale::{
    PanZoomDisposition, ScaleCancelCallback, ScaleEndCallback, ScaleEndDetails,
    ScaleGestureRecognizer, ScaleGestureRecognizerBuilder, ScaleStartCallback, ScaleStartDetails,
    ScaleStartMode, ScaleUpdateCallback, ScaleUpdateDetails,
};
pub use set::RecognizerSet;
pub use smart_zoom::{SmartZoomDetails, SmartZoomSource};
pub use swipe::{
    PhysicalEdge, SwipeDetails, SwipeGestureRecognizer, SwipeGestureRecognizerBuilder, SwipeStart,
};
pub use tap::{
    TapButton, TapCallback, TapDetails, TapDownCallback, TapGestureRecognizer,
    TapGestureRecognizerBuilder, TapUpCallback,
};
pub use tap_and_drag::{
    TapAndDragGestureRecognizer, TapAndDragGestureRecognizerBuilder, TapDragCancelCallback,
    TapDragDownCallback, TapDragDownDetails, TapDragEndCallback, TapDragEndDetails,
    TapDragStartCallback, TapDragStartDetails, TapDragUpCallback, TapDragUpDetails,
    TapDragUpdateCallback, TapDragUpdateDetails,
};
