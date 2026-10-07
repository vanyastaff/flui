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
pub mod contact;
pub mod double_tap;
pub mod drag;
pub mod drag_variants;
pub mod eager;
pub mod force_press;
pub mod long_press;
pub mod multi_tap;
pub mod multidrag;
pub mod recognizer;
pub mod scale;
pub mod set;
pub mod tap;
pub mod tap_and_drag;

// Re-export concrete recognizers
pub use contact::{ArenaMembership, BeginContactError, ContactId, ContactSnapshot, PrimaryContact};
pub use double_tap::{DoubleTapDetails, DoubleTapGestureRecognizer, DoubleTapGestureRecognizerBuilder};
pub use drag::{
    DragCancelCallback, DragDownCallback, DragDownDetails, DragEndCallback, DragEndDetails,
    DragGestureRecognizer, DragGestureRecognizerBuilder, DragPointerStrategy, DragStartCallback, DragStartDetails, DragUpdateCallback,
    DragUpdateDetails, GestureEndReason,
};
pub use eager::{EagerGestureRecognizer, EagerGestureRecognizerBuilder};
pub use force_press::{ForcePressGestureRecognizer, ForcePressGestureRecognizerBuilder};
pub use long_press::{LongPressGestureRecognizer, LongPressGestureRecognizerBuilder};
pub use multi_tap::{MultiTapGestureRecognizer, MultiTapGestureRecognizerBuilder};
pub use multidrag::{
    MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragGestureRecognizerBuilder, MultiDragHandle,
    MultiDragStartCallback, MultiDragUpdateDetails,
};
pub use recognizer::{
    CancelOutcome, GestureRecognizer, GestureRecognizerState, cancel_all, constants,
};
pub use scale::{ScaleGestureRecognizer, ScaleGestureRecognizerBuilder};
pub use set::RecognizerSet;
pub use tap::{TapGestureRecognizer, TapGestureRecognizerBuilder};
pub use tap_and_drag::{
    TapAndDragGestureRecognizer, TapAndDragGestureRecognizerBuilder, TapDragDownCallback, TapDragDownDetails, TapDragEndCallback,
    TapDragEndDetails, TapDragStartCallback, TapDragStartDetails, TapDragUpCallback,
    TapDragUpDetails, TapDragUpdateCallback, TapDragUpdateDetails,
};
